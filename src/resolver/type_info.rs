use super::*;
use crate::types::{
    FunctionEffect, TypeKind, TypeRegistry, TypeRegistryBuilder, TypeRegistryError,
};
use std::collections::BTreeMap;

#[derive(Debug, Default, Clone)]
pub(super) struct ProgramTypes {
    registry: TypeRegistry,
    bindings: Vec<TypeId>,
    functions: Vec<TypeId>,
    builtins: BTreeMap<Builtin, TypeId>,
    divmod: Option<TypeId>,
    input: Option<TypeId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExpressionContext {
    Entry,
    FunctionBody {
        function: usize,
    },
    ParameterDefault {
        function: usize,
        parameter: usize,
    },
    Contract {
        function: usize,
        contract: usize,
        postcondition: bool,
    },
}

#[derive(Clone, Copy)]
pub struct TypedExpression<'a> {
    expression: &'a ResolvedExpression,
    types: &'a ProgramTypes,
    context: ExpressionContext,
}
#[derive(Debug, Clone, Copy)]
pub struct TypedNode<'a> {
    pub index: usize,
    pub syntax: &'a ExpressionKind,
    pub span: Span,
    pub kind: TypeId,
    pub call_signature: Option<TypeId>,
}
impl TypedExpression<'_> {
    pub fn context(&self) -> ExpressionContext {
        self.context
    }
    pub fn span(&self) -> Span {
        self.expression.syntax.nodes()[self.expression.syntax.root()].span
    }
    pub fn root_type(&self) -> TypeId {
        self.expression.types[self.expression.syntax.root()]
    }
    pub fn nodes(&self) -> impl Iterator<Item = TypedNode<'_>> {
        self.expression
            .syntax
            .nodes()
            .iter()
            .enumerate()
            .map(|(index, node)| {
                let call_signature = match self.expression.calls[index] {
                    Some(ResolvedCall::User { function, .. }) => {
                        self.types.functions.get(function).copied()
                    }
                    Some(ResolvedCall::Builtin(builtin)) => {
                        self.types.builtins.get(&builtin).copied()
                    }
                    Some(ResolvedCall::ReadLine) => self.types.input,
                    Some(ResolvedCall::BigIntDivMod) => self.types.divmod,
                    None => None,
                };
                TypedNode {
                    index,
                    syntax: &node.kind,
                    span: node.span,
                    kind: self.expression.types[index],
                    call_signature,
                }
            })
    }
}

fn operation_expressions(
    operation: &ResolvedOperation,
) -> impl Iterator<Item = &ResolvedExpression> {
    let (single, arguments): (Option<&ResolvedExpression>, &[ResolvedArgument]) =
        match &operation.kind {
            ResolvedStatement::Return { value } => (value.as_ref(), &[]),
            ResolvedStatement::Call { expression } => (Some(expression), &[]),
            ResolvedStatement::RepeatInit { count, .. } => (Some(count), &[]),
            ResolvedStatement::JumpIfFalse { condition, .. } => (Some(condition), &[]),
            ResolvedStatement::Store { value, .. } | ResolvedStatement::Update { value, .. } => {
                (Some(value), &[])
            }
            ResolvedStatement::Output { arguments, .. } => (None, arguments),
            _ => (None, &[]),
        };
    single
        .into_iter()
        .chain(arguments.iter().filter_map(|argument| match argument {
            ResolvedArgument::Expression(expression) => Some(expression),
            _ => None,
        }))
}

fn expressions(
    program: &ResolvedProgram,
) -> impl Iterator<Item = (ExpressionContext, &ResolvedExpression)> {
    let entry = program
        .statements
        .iter()
        .flat_map(operation_expressions)
        .map(|expression| (ExpressionContext::Entry, expression));
    entry.chain(
        program
            .functions
            .iter()
            .enumerate()
            .flat_map(|(function, body)| {
                let defaults =
                    body.defaults
                        .iter()
                        .enumerate()
                        .filter_map(move |(parameter, expression)| {
                            expression.as_ref().map(|expression| {
                                (
                                    ExpressionContext::ParameterDefault {
                                        function,
                                        parameter,
                                    },
                                    expression,
                                )
                            })
                        });
                let contracts = body
                    .contracts
                    .iter()
                    .enumerate()
                    .map(move |(contract, value)| {
                        (
                            ExpressionContext::Contract {
                                function,
                                contract,
                                postcondition: value.postcondition,
                            },
                            &value.expression,
                        )
                    });
                let statements =
                    body.statements
                        .iter()
                        .flat_map(operation_expressions)
                        .map(move |expression| {
                            (ExpressionContext::FunctionBody { function }, expression)
                        });
                defaults.chain(contracts).chain(statements)
            }),
    )
}

impl ResolvedProgram {
    pub fn type_registry(&self) -> &TypeRegistry {
        &self.type_info.registry
    }
    pub fn binding_type_ids(&self) -> &[TypeId] {
        &self.type_info.bindings
    }
    pub fn function_type_id(&self, function: usize) -> Option<TypeId> {
        self.type_info.functions.get(function).copied()
    }
    pub fn function_binding_type_ids(&self, function: usize) -> Option<&[TypeId]> {
        self.functions
            .get(function)
            .map(|body| body.slot_types.as_slice())
    }
    pub fn builtin_type_id(&self, builtin: Builtin) -> Option<TypeId> {
        self.type_info.builtins.get(&builtin).copied()
    }
    // Plan order: entry operations, then each function's defaults, contracts and
    // body. Node indices are local postorder indices within one expression.
    pub fn typed_expressions(&self) -> impl Iterator<Item = TypedExpression<'_>> {
        expressions(self).map(|(context, expression)| TypedExpression {
            expression,
            types: &self.type_info,
            context,
        })
    }
}

pub(super) fn failure(error: TypeRegistryError, span: Span) -> Diagnostic {
    Diagnostic::new(DiagnosticKind::TypeComplexity, span, error.to_string()).with_help(
        "reduce semantic type/signature complexity or adjust TypeLimits in the embedding compiler",
    )
}

pub(super) fn build(
    program: &ResolvedProgram,
    mut registry: TypeRegistryBuilder,
) -> Result<ProgramTypes, Diagnostic> {
    let mut functions = Vec::with_capacity(program.metadata.len());
    for function in &program.metadata {
        let signature = TypeKind::Function {
            parameters: function
                .parameters
                .iter()
                .map(|parameter| {
                    parameter
                        .kind
                        .intern(&mut registry)
                        .map_err(|e| failure(e, parameter.span))
                })
                .collect::<Result<Vec<_>, _>>()?
                .into(),
            result: function
                .result
                .intern(&mut registry)
                .map_err(|e| failure(e, function.span))?,
            effect: if function.pure {
                FunctionEffect::Pure
            } else {
                FunctionEffect::Impure
            },
        };
        let id = registry
            .intern(signature)
            .map_err(|error| failure(error, function.span))?;
        functions.push(id);
    }
    let mut builtins = BTreeMap::new();
    let mut divmod = None;
    let mut input = None;
    for (_, expression) in expressions(program) {
        for (index, call) in expression.calls.iter().enumerate() {
            if matches!(call, Some(ResolvedCall::ReadLine)) && input.is_none() {
                input = Some(
                    registry
                        .intern(TypeKind::Function {
                            parameters: Vec::new().into(),
                            result: ValueType::String.id(),
                            effect: FunctionEffect::Impure,
                        })
                        .map_err(|error| failure(error, expression.syntax.nodes()[index].span))?,
                );
            }
            if matches!(call, Some(ResolvedCall::BigIntDivMod)) && divmod.is_none() {
                let span = expression.syntax.nodes()[index].span;
                let result = crate::types::SourceType::tuple(vec![
                    ValueType::BigInt.into(),
                    ValueType::BigInt.into(),
                ])
                .expect("bounded two-element BigInt tuple")
                .intern(&mut registry)
                .map_err(|error| failure(error, span))?;
                divmod = Some(
                    registry
                        .intern(TypeKind::Function {
                            parameters: vec![
                                ValueType::BigInt.id(),
                                ValueType::BigInt.id(),
                            ]
                            .into(),
                            result,
                            effect: FunctionEffect::Pure,
                        })
                        .map_err(|error| failure(error, span))?,
                );
            }
            if let Some(ResolvedCall::Builtin(builtin)) = call {
                if let std::collections::btree_map::Entry::Vacant(entry) = builtins.entry(*builtin)
                {
                    let id = builtin
                        .intern_signature(&mut registry)
                        .map_err(|error| failure(error, expression.syntax.nodes()[index].span))?;
                    entry.insert(id);
                }
            }
        }
    }
    let bindings = program
        .slot_types
        .iter()
        .map(|kind| {
            kind.intern(&mut registry)
                .map_err(|e| failure(e, Span::default()))
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(ProgramTypes {
        registry: registry.finish(),
        bindings,
        functions,
        builtins,
        divmod,
        input,
    })
}
