//! Scoped generic signatures, call-site inference and bounded instantiation.
use super::*;
use crate::{
    ast::{GenericParameter, TypeAnnotation},
    types::{DefinitionId, TypeConstraint, TypeKind, TypeParameter},
};
use std::{collections::VecDeque, sync::Arc};

const MAX_INSTANCES: usize = 256;
const MAX_EXPANDED_SOURCE_BYTES: usize = 4 * 1024 * 1024;
const MAX_EXPANDED_PLAN_NODES: usize = 250_000;

fn definition(span: Span, message: impl Into<String>) -> Diagnostic {
    Diagnostic::new(DiagnosticKind::GenericDefinition, span, message)
}

pub(super) fn arguments(span: Span, message: impl Into<String>) -> Diagnostic {
    Diagnostic::new(DiagnosticKind::GenericArguments, span, message)
        .with_help("supply matching arguments or specify the types with name::<type>(...)")
}

fn limit(span: Span, message: impl Into<String>) -> Diagnostic {
    Diagnostic::new(DiagnosticKind::GenericLimit, span, message)
}

pub(super) fn declare(
    declaration: usize,
    parameters: &[GenericParameter],
    registry: &mut TypeRegistryBuilder,
    span: Span,
) -> Result<Vec<SourceType>, Diagnostic> {
    if parameters.len() > crate::parser::MAX_TYPE_PARAMETERS {
        return Err(limit(
            span,
            "a function can declare at most 64 type parameters",
        ));
    }
    let mut names = HashMap::new();
    let binder = DefinitionId {
        module: u32::MAX - 1,
        declaration: declaration as u32,
    };
    parameters
        .iter()
        .enumerate()
        .map(|(index, parameter)| {
            if let Some(previous) = names.insert(&parameter.name, parameter.span) {
                return Err(definition(
                    parameter.span,
                    format!("duplicate type parameter {:?}", parameter.name),
                )
                .with_related(previous, "the first declaration is here"));
            }
            let id = registry
                .intern(TypeKind::Parameter {
                    binder,
                    index: index as u32,
                })
                .map_err(|e| type_info::failure(e, parameter.span))?;
            Ok(SourceType::Parameter(Arc::new(TypeParameter {
                id: Some(id),
                index,
                name: parameter.name.as_str().into(),
                constraint: parameter.constraint,
            })))
        })
        .collect()
}

pub(super) fn resolve_type(
    layouts: &nominal::Layouts,
    kind: &SourceType,
    bindings: &[SourceType],
    span: Span,
) -> Result<SourceType, Diagnostic> {
    let kind = nominal::resolve_type(layouts, kind, span)?;
    if kind.has_parameter() {
        substitute(&kind, bindings, span)
    } else {
        Ok(kind)
    }
}

pub(super) fn substitute(
    kind: &SourceType,
    bindings: &[SourceType],
    span: Span,
) -> Result<SourceType, Diagnostic> {
    kind.map_parameters(
        &mut |p| {
            bindings
                .get(p.index)
                .cloned()
                .ok_or_else(|| definition(span, format!("unknown type parameter {:?}", p.name)))
        },
        &|| limit(span, "substituted type exceeds the composite type limits"),
    )
}

pub(super) struct Inference<'a> {
    function: &'a FunctionInfo,
    inferred: Vec<Option<(SourceType, Span)>>,
    explicit: bool,
}

impl<'a> Inference<'a> {
    pub(super) fn new(
        function: &'a FunctionInfo,
        explicit: &[TypeAnnotation],
        resolver: &Resolver<'_>,
        span: Span,
    ) -> Result<Self, Diagnostic> {
        if !explicit.is_empty() && explicit.len() != function.type_parameters.len() {
            return Err(arguments(
                span,
                format!(
                    "{:?} expects {} type arguments, found {}",
                    function.name,
                    function.type_parameters.len(),
                    explicit.len()
                ),
            )
            .with_related(function.span, "the function is declared here"));
        }
        let inferred = if explicit.is_empty() {
            vec![None; function.type_parameters.len()]
        } else {
            explicit
                .iter()
                .map(|annotation| {
                    resolver
                        .annotation(&annotation.kind, annotation.span)
                        .map(|kind| Some((kind, annotation.span)))
                })
                .collect::<Result<Vec<_>, _>>()?
        };
        Ok(Self {
            function,
            inferred,
            explicit: !explicit.is_empty(),
        })
    }

    pub(super) fn infer_argument(
        &mut self,
        parameter: &ParameterInfo,
        kind: &SourceType,
        span: Span,
    ) -> Result<(), Diagnostic> {
        if self.explicit {
            return Ok(());
        }
        self.unify(&parameter.kind, kind, span, false)
            .map_err(|e| e.with_related(parameter.span, "the parameter is declared here"))
    }

    // Only variables in the callee's template are solved. A caller's type
    // parameter is a rigid type, even for a recursive call to the same binder.
    fn unify(
        &mut self,
        template: &SourceType,
        actual: &SourceType,
        span: Span,
        from_context: bool,
    ) -> Result<(), Diagnostic> {
        if !template.has_parameter() {
            return Ok(());
        }
        match (template, actual) {
            (SourceType::Parameter(p), _) => {
                let binding = &mut self.inferred[p.index];
                if let Some((first, first_span)) = binding {
                    if first != actual && !from_context {
                        return Err(arguments(
                            span,
                            format!(
                                "conflicting types for {}: inferred {first}, found {actual}",
                                p.name
                            ),
                        )
                        .with_related(*first_span, "the earlier inference is here"));
                    }
                } else {
                    *binding = Some((actual.clone(), span));
                }
            }
            (SourceType::Tuple(pattern), SourceType::Tuple(value))
                if pattern.elements().len() == value.elements().len() =>
            {
                for (pattern, actual) in pattern.elements().iter().zip(value.elements()) {
                    self.unify(pattern, actual, span, from_context)?;
                }
            }
            (SourceType::Array(pattern), SourceType::Array(value))
                if pattern.len() == value.len() =>
            {
                self.unify(pattern.element(), value.element(), span, from_context)?;
            }
            (SourceType::List(pattern), SourceType::List(value)) => {
                self.unify(pattern.element(), value.element(), span, from_context)?;
            }
            (SourceType::Map(pattern), SourceType::Map(value)) if pattern.key() == value.key() => {
                self.unify(pattern.value(), value.value(), span, from_context)?;
            }
            _ if from_context => {}
            _ => {
                return Err(arguments(
                    span,
                    format!("cannot infer type arguments by matching {actual} against {template}"),
                ))
            }
        }
        Ok(())
    }

    pub(super) fn finish(
        mut self,
        expected: Option<&SourceType>,
        span: Span,
    ) -> Result<Vec<SourceType>, Diagnostic> {
        if let Some(expected) = expected.filter(|_| !self.explicit) {
            self.unify(&self.function.result, expected, span, true)?;
        }
        self.function
            .type_parameters
            .iter()
            .zip(self.inferred)
            .map(|(parameter, inferred)| {
                let (kind, argument_span) = inferred.ok_or_else(|| {
                    arguments(
                        span,
                        format!("cannot infer type parameter {}", parameter.name),
                    )
                    .with_related(parameter.span, "the type parameter is declared here")
                })?;
                let allowed = match parameter.constraint {
                    Some(TypeConstraint::Numeric) => kind.is_numeric(),
                    Some(TypeConstraint::Comparable) => kind.is_ordered(),
                    None => kind != ValueType::Unit,
                };
                if !allowed {
                    let requirement = parameter.constraint.map_or_else(
                        || "a non-unit value type".into(),
                        |constraint| constraint.to_string(),
                    );
                    return Err(arguments(
                        argument_span,
                        format!(
                            "type argument {} requires {requirement}, found {kind}",
                            parameter.name
                        ),
                    )
                    .with_related(parameter.span, "the type parameter is declared here"));
                }
                Ok(kind)
            })
            .collect()
    }
}

pub(super) struct Instance {
    pub(super) declaration: usize,
    pub(super) bindings: Vec<SourceType>,
    pub(super) span: Span,
}

pub(super) struct Instances {
    source_count: usize,
    ids: HashMap<(usize, Vec<TypeId>), usize>,
    pending: VecDeque<Instance>,
    source_bytes_left: usize,
    plan_nodes_left: usize,
}

impl Instances {
    pub(super) fn new(source_count: usize) -> Self {
        Self {
            source_count,
            ids: HashMap::new(),
            pending: VecDeque::new(),
            source_bytes_left: MAX_EXPANDED_SOURCE_BYTES,
            plan_nodes_left: MAX_EXPANDED_PLAN_NODES,
        }
    }

    pub(super) fn next(&mut self) -> Option<Instance> {
        self.pending.pop_front()
    }

    pub(super) fn request(
        &mut self,
        declaration: usize,
        function: &FunctionInfo,
        bindings: Vec<SourceType>,
        registry: &mut TypeRegistryBuilder,
        span: Span,
    ) -> Result<usize, Diagnostic> {
        let key = (
            declaration,
            bindings
                .iter()
                .map(|t| t.intern(registry).map_err(|e| type_info::failure(e, span)))
                .collect::<Result<Vec<_>, _>>()?,
        );
        if let Some(id) = self.ids.get(&key) {
            return Ok(*id);
        }
        if self.ids.len() >= MAX_INSTANCES {
            return Err(
                limit(span, "program exceeds 256 distinct generic specializations").with_related(
                    function.span,
                    "this generic function would require another specialization",
                ),
            );
        }
        let id = self.source_count + self.ids.len();
        self.ids.insert(key, id);
        self.pending.push_back(Instance {
            declaration,
            bindings,
            span,
        });
        Ok(id)
    }

    pub(super) fn charge(
        &mut self,
        function: &FunctionInfo,
        template: &ResolvedFunction,
        span: Span,
    ) -> Result<(), Diagnostic> {
        let expanded_source = function.span.end.saturating_sub(function.span.start);
        self.source_bytes_left = self
            .source_bytes_left
            .checked_sub(expanded_source)
            .ok_or_else(|| {
                limit(
                    span,
                    "generic specializations exceed the 4 MiB expansion limit",
                )
            })?;
        let expressions = template
            .defaults
            .iter()
            .flatten()
            .chain(template.contracts.iter().map(|c| &c.expression))
            .chain(
                template
                    .statements
                    .iter()
                    .flat_map(type_info::operation_expressions),
            );
        let plan_nodes = expressions.fold(
            template.slots.saturating_add(template.statements.len()),
            |nodes, e| nodes.saturating_add(e.syntax.nodes().len()),
        );
        self.plan_nodes_left = self
            .plan_nodes_left
            .checked_sub(plan_nodes)
            .ok_or_else(|| {
                limit(
                    span,
                    "generic specializations exceed 250000 expanded plan nodes",
                )
            })?;
        Ok(())
    }
}

pub(super) fn abstract_call_signature(
    function: &FunctionInfo,
    bindings: &[SourceType],
    result: &SourceType,
    registry: &mut TypeRegistryBuilder,
    span: Span,
) -> Result<TypeId, Diagnostic> {
    use crate::types::{FunctionEffect, TypeKind};

    let parameters = function
        .parameters
        .iter()
        .map(|p| {
            substitute(&p.kind, bindings, span)?
                .intern(registry)
                .map_err(|e| type_info::failure(e, span))
        })
        .collect::<Result<_, _>>()?;
    let result = result
        .intern(registry)
        .map_err(|e| type_info::failure(e, span))?;
    registry
        .intern(TypeKind::Function {
            parameters,
            result,
            effect: if function.pure {
                FunctionEffect::Pure
            } else {
                FunctionEffect::Impure
            },
        })
        .map_err(|e| type_info::failure(e, span))
}

pub(super) fn operation(
    parameter: &TypeParameter,
    constraint: TypeConstraint,
    span: Span,
) -> Diagnostic {
    Diagnostic::new(
        DiagnosticKind::TypeMismatch,
        span,
        format!("operation requires {constraint}, found {}", parameter.name),
    )
    .with_help(format!(
        "declare {}: {constraint} to use this operation",
        parameter.name
    ))
}

/// Pass an expected result through nodes that preserve an independently
/// known type. Leave operands of overloaded operators and call arguments to
/// ordinary bottom-up inference.
pub(super) fn call_expectations<'a>(
    syntax: &Expression,
    expected: &'a SourceType,
) -> std::collections::BTreeMap<usize, &'a SourceType> {
    let mut calls = std::collections::BTreeMap::new();
    let mut pending = vec![(syntax.root(), expected)];
    while let Some((index, kind)) = pending.pop() {
        match (&syntax.nodes()[index].kind, kind) {
            (ExpressionKind::UserCall { .. }, _) => {
                calls.insert(index, kind);
            }
            (ExpressionKind::Group { expression }, _) => pending.push((*expression, kind)),
            (
                ExpressionKind::Conditional {
                    branches,
                    otherwise,
                },
                _,
            ) => {
                pending.extend(branches.iter().map(|b| (b.result, kind)));
                pending.push((*otherwise, kind));
            }
            (ExpressionKind::Tuple { elements }, SourceType::Tuple(tuple))
                if elements.len() == tuple.elements().len() =>
            {
                pending.extend(elements.iter().copied().zip(tuple.elements()));
            }
            (ExpressionKind::List { elements, .. }, SourceType::List(list)) => {
                pending.extend(elements.iter().map(|&i| (i, list.element())));
            }
            (ExpressionKind::Array { elements, .. }, SourceType::Array(array))
                if elements.len() == array.len() =>
            {
                pending.extend(elements.iter().map(|&i| (i, array.element())));
            }
            (ExpressionKind::Map { entries, .. }, SourceType::Map(map)) => {
                for &(key, value) in entries {
                    pending.push((key, map.key()));
                    pending.push((value, map.value()));
                }
            }
            _ => {}
        }
    }
    calls
}
