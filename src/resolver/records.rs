//! Resolve source record declarations and lower named fields to layout offsets.
use super::*;
use crate::{
    ast::{CallArgument, RecordDeclaration},
    types::{DefinitionId, TypeKind, MAX_COMPOSITE_DEPTH, MAX_RECORD_FIELDS},
};
use std::sync::Arc;

pub(super) type Layouts = HashMap<String, SourceType>;

#[derive(Debug, Clone)]
pub(crate) enum RecordOperation {
    Construct {
        kind: SourceType,
        /// One source argument index for each field in declared order.
        order: Box<[usize]>,
    },
    Field(usize),
}

pub(super) fn definition_error(span: Span, message: impl Into<String>) -> Diagnostic {
    Diagnostic::new(DiagnosticKind::RecordDefinition, span, message)
}

fn shape_error(span: Span) -> Diagnostic {
    definition_error(span, "record layout exceeds composite type limits")
}

pub(super) fn lookup(layouts: &Layouts, name: &str, span: Span) -> Result<SourceType, Diagnostic> {
    layouts
        .get(name)
        .cloned()
        .ok_or_else(|| definition_error(span, format!("unknown record {name:?}")))
}

pub(super) fn resolve_type(
    layouts: &Layouts,
    kind: &SourceType,
    span: Span,
) -> Result<SourceType, Diagnostic> {
    kind.map_record_refs(&mut |name, _| lookup(layouts, name, span), &|| {
        shape_error(span)
    })
}

pub(super) fn collect(
    statements: &[Statement],
    registry: &mut TypeRegistryBuilder,
) -> Result<Layouts, Diagnostic> {
    let mut declarations = HashMap::new();
    for statement in statements {
        if let Statement::Record(record) = statement {
            check_name(&record.name, record.name_span)?;
            if declarations.len() >= 4096 {
                return Err(definition_error(
                    record.name_span,
                    "program exceeds 4096 record declarations",
                ));
            }
            let index = declarations.len() as u32;
            if let Some((previous, _)) =
                declarations.insert(record.name.as_str(), (record.as_ref(), index))
            {
                return Err(definition_error(
                    record.name_span,
                    format!("record {:?} is already declared", record.name),
                )
                .with_related(previous.name_span, "the first declaration is here"));
            }
        }
    }
    let mut builder = Builder {
        declarations,
        layouts: Layouts::new(),
        active: Vec::new(),
        registry,
    };
    // Resolve in source order, including declarations with no construction sites.
    for statement in statements {
        if let Statement::Record(record) = statement {
            builder.resolve(&record.name, record.name_span, 0)?;
        }
    }
    Ok(builder.layouts)
}

struct Builder<'a> {
    declarations: HashMap<&'a str, (&'a RecordDeclaration, u32)>,
    layouts: Layouts,
    active: Vec<&'a str>,
    registry: &'a mut TypeRegistryBuilder,
}

impl Builder<'_> {
    fn resolve(&mut self, name: &str, span: Span, depth: usize) -> Result<SourceType, Diagnostic> {
        if depth >= MAX_COMPOSITE_DEPTH {
            return Err(shape_error(span));
        }
        if let Some(kind) = self.layouts.get(name) {
            return Ok(kind.clone());
        }
        let &(record, declaration) = self
            .declarations
            .get(name)
            .ok_or_else(|| definition_error(span, format!("unknown record {name:?}")))?;
        if self.active.contains(&record.name.as_str()) {
            return Err(definition_error(
                span,
                format!("recursive record layout for {:?}", display_name(name)),
            )
            .with_related(record.name_span, "the cycle revisits this declaration"));
        }
        if record.fields.len() > MAX_RECORD_FIELDS {
            return Err(shape_error(span));
        }
        self.active.push(&record.name);
        let mut seen = HashMap::new();
        let mut fields = Vec::with_capacity(record.fields.len());
        let mut field_ids = Vec::with_capacity(record.fields.len());
        for (name, name_span, annotation) in &record.fields {
            if let Some(first) = seen.insert(name, *name_span) {
                return Err(field_error(*name_span, format!("duplicate field {name:?}"))
                    .with_related(first, "the first declaration is here"));
            }
            let kind = annotation.kind.map_record_refs(
                &mut |name, within| self.resolve(name, annotation.span, depth + within + 1),
                &|| shape_error(annotation.span),
            )?;
            require_value(&kind, annotation.span)?;
            field_ids.push(
                kind.intern(self.registry)
                    .map_err(|e| type_info::failure(e, annotation.span))?,
            );
            fields.push((name.clone(), kind));
        }
        let module = record
            .name
            .strip_prefix("$record:")
            .and_then(|name| name.split_once(':'))
            .and_then(|(module, _)| module.parse().ok())
            .unwrap_or(0);
        let id = self
            .registry
            .intern(TypeKind::Record {
                definition: DefinitionId {
                    module,
                    declaration,
                },
                fields: field_ids.into_boxed_slice(),
            })
            .map_err(|e| type_info::failure(e, record.name_span))?;
        let kind = SourceType::record(id, record.name_span, Arc::from(display_name(name)), fields)
            .ok_or_else(|| shape_error(span))?;
        self.active.pop();
        self.layouts.insert(record.name.clone(), kind.clone());
        Ok(kind)
    }
}

pub(super) fn field_error(span: Span, message: impl Into<String>) -> Diagnostic {
    Diagnostic::new(DiagnosticKind::RecordField, span, message)
}

fn display_name(name: &str) -> &str {
    name.rsplit(':').next().unwrap_or(name)
}

pub(super) fn constructor(
    kind: &SourceType,
    fields: &[CallArgument],
    types: &[SourceType],
    syntax: &Expression,
    span: Span,
) -> Result<RecordOperation, Diagnostic> {
    let layout = kind.as_record().expect("record lookup returns a layout");
    let mut order = vec![None; layout.fields().len()];
    for (input, field) in fields.iter().enumerate() {
        let (name, field_span) = field.label.as_ref().expect("constructor fields have names");
        let (index, (_, expected)) = layout
            .fields()
            .iter()
            .enumerate()
            .find(|(_, (s, _))| s == name)
            .ok_or_else(|| {
                field_error(
                    *field_span,
                    format!("{} has no field {name:?}", layout.name()),
                )
            })?;
        if let Some(first) = order[index].replace(input) {
            let (_, first_span) = fields[first].label.as_ref().expect("field name");
            return Err(
                field_error(*field_span, format!("field {name:?} is initialized twice"))
                    .with_related(*first_span, "the first initializer is here"),
            );
        }
        require_type(
            &types[field.expression],
            expected,
            syntax.nodes()[field.expression].span,
        )?;
    }
    let order = order
        .into_iter()
        .zip(layout.fields())
        .map(|(position, (name, _))| {
            position.ok_or_else(|| field_error(span, format!("missing field {name:?}")))
        })
        .collect::<Result<Box<[_]>, _>>()?;
    Ok(RecordOperation::Construct {
        kind: kind.clone(),
        order,
    })
}
