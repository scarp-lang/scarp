//! Resolve record and variant declarations in one bounded, acyclic type graph.
use super::*;
use crate::{
    ast::{RecordDeclaration, TypeAnnotation, VariantDeclaration},
    types::{
        DefinitionId, NominalKind, TypeKind, MAX_COMPOSITE_DEPTH, MAX_RECORD_FIELDS,
        MAX_VARIANT_CASES,
    },
};
use std::sync::Arc;

pub(super) type Layouts = HashMap<String, SourceType>;

#[derive(Clone, Copy)]
enum Declaration<'a> {
    Record(&'a RecordDeclaration),
    Variant(&'a VariantDeclaration),
}
impl<'a> Declaration<'a> {
    fn from_statement(statement: &'a Statement) -> Option<Self> {
        match statement {
            Statement::Record(record) => Some(Self::Record(record)),
            Statement::Variant(variant) => Some(Self::Variant(variant)),
            _ => None,
        }
    }

    fn header(self) -> (NominalKind, &'a str, Span) {
        match self {
            Self::Record(record) => (NominalKind::Record, &record.name, record.name_span),
            Self::Variant(variant) => (NominalKind::Variant, &variant.name, variant.name_span),
        }
    }

    fn members(self) -> Option<impl Iterator<Item = (&'a str, Span, Option<&'a TypeAnnotation>)>> {
        let (fields, cases) = match self {
            Self::Record(record) if record.fields.len() <= MAX_RECORD_FIELDS => {
                (record.fields.as_slice(), &[][..])
            }
            Self::Variant(variant)
                if !variant.cases.is_empty() && variant.cases.len() <= MAX_VARIANT_CASES =>
            {
                (&[][..], variant.cases.as_slice())
            }
            _ => return None,
        };
        Some(
            fields
                .iter()
                .map(|(name, span, ty)| (name.as_str(), *span, Some(ty)))
                .chain(
                    cases
                        .iter()
                        .map(|case| (case.name.as_str(), case.span, case.payload.as_ref())),
                ),
        )
    }
}

pub(super) fn definition_error(
    kind: NominalKind,
    span: Span,
    message: impl Into<String>,
) -> Diagnostic {
    let diagnostic = match kind {
        NominalKind::Record => DiagnosticKind::RecordDefinition,
        NominalKind::Variant => DiagnosticKind::VariantDefinition,
    };
    Diagnostic::new(diagnostic, span, message)
}

fn shape_error(kind: NominalKind, span: Span) -> Diagnostic {
    definition_error(
        kind,
        span,
        format!("{} layout exceeds composite type limits", kind.name()),
    )
}

fn unknown(kind: NominalKind, name: &str, span: Span) -> Diagnostic {
    definition_error(
        kind,
        span,
        format!("unknown {} {:?}", kind.name(), display_name(name)),
    )
}

pub(super) fn lookup(
    layouts: &Layouts,
    kind: NominalKind,
    name: &str,
    span: Span,
) -> Result<SourceType, Diagnostic> {
    layouts
        .get(name)
        .filter(|ty| match kind {
            NominalKind::Record => ty.as_record().is_some(),
            NominalKind::Variant => ty.as_variant().is_some(),
        })
        .cloned()
        .ok_or_else(|| unknown(kind, name, span))
}

pub(super) fn resolve_type(
    layouts: &Layouts,
    kind: &SourceType,
    span: Span,
) -> Result<SourceType, Diagnostic> {
    kind.map_nominal_refs(
        &mut |kind, name, _| lookup(layouts, kind, name, span),
        &|| {
            definition_error(
                NominalKind::Record,
                span,
                "nominal type exceeds composite type limits",
            )
        },
    )
}

pub(super) fn collect(
    statements: &[Statement],
    registry: &mut TypeRegistryBuilder,
) -> Result<Layouts, Diagnostic> {
    let items = || statements.iter().filter_map(Declaration::from_statement);
    let mut declarations = HashMap::new();
    for declaration in items() {
        let (kind, name, span) = declaration.header();
        check_name(name, span)?;
        if declarations.len() >= 4096 {
            return Err(definition_error(
                kind,
                span,
                "program exceeds 4096 nominal declarations",
            ));
        }
        let index = declarations.len() as u32;
        if let Some((previous, _)) = declarations.insert(name, (declaration, index)) {
            return Err(definition_error(
                kind,
                span,
                format!("type {:?} is already declared", display_name(name)),
            )
            .with_related(previous.header().2, "the first declaration is here"));
        }
    }
    let mut builder = Builder {
        declarations,
        layouts: Layouts::new(),
        active: Vec::new(),
        registry,
    };
    for declaration in items() {
        let (kind, name, span) = declaration.header();
        builder.resolve(kind, name, span, 0)?;
    }
    Ok(builder.layouts)
}

struct Builder<'a> {
    declarations: HashMap<&'a str, (Declaration<'a>, u32)>,
    layouts: Layouts,
    active: Vec<&'a str>,
    registry: &'a mut TypeRegistryBuilder,
}
impl Builder<'_> {
    fn resolve(
        &mut self,
        kind: NominalKind,
        name: &str,
        span: Span,
        depth: usize,
    ) -> Result<SourceType, Diagnostic> {
        if depth >= MAX_COMPOSITE_DEPTH {
            return Err(shape_error(kind, span));
        }
        if self.layouts.contains_key(name) {
            return lookup(&self.layouts, kind, name, span);
        }
        let &(declaration, index) = self
            .declarations
            .get(name)
            .ok_or_else(|| unknown(kind, name, span))?;
        let (actual, declared_name, declaration_span) = declaration.header();
        if actual != kind {
            return Err(unknown(kind, name, span));
        }
        if self.active.contains(&declared_name) {
            return Err(definition_error(
                kind,
                span,
                format!(
                    "recursive {} layout for {:?}",
                    kind.name(),
                    display_name(name)
                ),
            )
            .with_related(declaration_span, "the cycle revisits this declaration"));
        }
        self.active.push(declared_name);
        let mut seen = HashMap::new();
        let mut members = Vec::new();
        let mut type_ids = Vec::new();
        for (member, member_span, annotation) in declaration
            .members()
            .ok_or_else(|| shape_error(kind, span))?
        {
            if kind == NominalKind::Variant && member == "_" {
                return Err(variants::case_error(
                    member_span,
                    "the wildcard cannot name a variant case",
                ));
            }
            if let Some(earlier) = seen.insert(member, member_span) {
                let err = match kind {
                    NominalKind::Record => {
                        records::field_error(member_span, format!("duplicate field {member:?}"))
                    }
                    NominalKind::Variant => {
                        variants::case_error(member_span, format!("duplicate case {member:?}"))
                    }
                };
                return Err(err.with_related(earlier, "the first declaration is here"));
            }
            let payload = annotation
                .map(|annotation| {
                    let ty = annotation.kind.map_nominal_refs(
                        &mut |kind, name, within| {
                            self.resolve(kind, name, annotation.span, depth + within + 1)
                        },
                        &|| shape_error(kind, annotation.span),
                    )?;
                    require_value(&ty, annotation.span)?;
                    Ok(ty)
                })
                .transpose()?;
            type_ids.push(
                payload
                    .as_ref()
                    .map(|ty| {
                        let at = annotation.expect("payload has a type").span;
                        ty.intern(self.registry)
                            .map_err(|error| type_info::failure(error, at))
                    })
                    .transpose()?,
            );
            members.push((member.to_owned(), payload));
        }
        let module = name
            .strip_prefix("$nominal:")
            .and_then(|name| name.split_once(':'))
            .and_then(|(module, _)| module.parse().ok())
            .unwrap_or(0);
        let definition = DefinitionId {
            module,
            declaration: index,
        };
        let descriptor = match kind {
            NominalKind::Record => TypeKind::Record {
                definition,
                fields: type_ids.into_iter().map(Option::unwrap).collect(),
            },
            NominalKind::Variant => {
                // Compiler-generated concrete option/result specializations
                // are structural applications of reserved type constructors.
                // Unlike source 'variant' declarations, identical type
                // arguments across modules must produce the same TypeId.
                // The reserved module cannot collide with any source module.
                let short = display_name(declared_name);
                let signature = if short.starts_with("option<") && short.ends_with('>') {
                    if members.len() != 2
                        || members[0].0 != "None"
                        || members[0].1.is_some()
                        || members[1].0 != "Some"
                        || members[1].1.is_none()
                    {
                        return Err(definition_error(
                            kind,
                            declaration_span,
                            "invalid compiler-generated option case layout",
                        ));
                    }
                    Some((0, vec![type_ids[1].expect("checked Some payload")]))
                } else if short.starts_with("result<") && short.ends_with('>') {
                    if members.len() != 2
                        || members[0].0 != "Ok"
                        || members[0].1.is_none()
                        || members[1].0 != "Err"
                        || members[1].1.is_none()
                    {
                        return Err(definition_error(
                            kind,
                            declaration_span,
                            "invalid compiler-generated result case layout",
                        ));
                    }
                    Some((
                        1,
                        vec![
                            type_ids[0].expect("checked Ok payload"),
                            type_ids[1].expect("checked Err payload"),
                        ],
                    ))
                } else {
                    None
                };
                match signature {
                    Some((constructor, arguments)) => TypeKind::Applied {
                        constructor: DefinitionId {
                            module: u32::MAX,
                            declaration: constructor,
                        },
                        arguments: arguments.into_boxed_slice(),
                    },
                    None => TypeKind::Variant {
                        definition,
                        cases: type_ids.into_boxed_slice(),
                    },
                }
            },
        };
        let id = self
            .registry
            .intern(descriptor)
            .map_err(|error| type_info::failure(error, declaration_span))?;
        let short = Arc::from(display_name(name));
        let layout = match kind {
            NominalKind::Record => SourceType::record(
                id,
                declaration_span,
                short,
                members
                    .into_iter()
                    .map(|(member, payload)| (member, payload.unwrap()))
                    .collect(),
            ),
            NominalKind::Variant => SourceType::variant(id, declaration_span, short, members),
        }
        .ok_or_else(|| shape_error(kind, span))?;
        self.active.pop();
        self.layouts
            .insert(declared_name.to_owned(), layout.clone());
        Ok(layout)
    }
}

fn display_name(name: &str) -> &str {
    name.rsplit(':').next().unwrap_or(name)
}
