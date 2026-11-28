//! Checked source lowering to the reusable scalar differentiation engine.

use std::collections::{BTreeMap, BTreeSet};

use super::autodiff::{self, Binary, Graph, NodeId, Unary};
use crate::{
    ast::{BinaryOperator, CallArgument, Expression, ExpressionKind},
    diagnostics::{Diagnostic, DiagnosticKind, Span},
    limits::Budget,
    resolver::ResolvedCall,
    types::SourceType,
    value::{finite_number, FiniteFloat, Value, ValueType},
};

pub(crate) const VALUE_GRAD: &str = "value_grad";
const MAX_SOURCE_GRAPH_NODES: usize = 4096;

pub(crate) fn named_unary(name: &str) -> Option<Unary> {
    Some(match name {
        "scalar_square" => Unary::Square,
        "scalar_exp" => Unary::Exp,
        "scalar_log" => Unary::Log,
        "scalar_relu" => Unary::Relu,
        "scalar_sigmoid" => Unary::Sigmoid,
        "scalar_tanh" => Unary::Tanh,
        _ => return None,
    })
}

pub(crate) fn unary(
    operation: Unary,
    input: &Value,
    budget: &mut Budget,
    span: Span,
) -> Result<Value, Diagnostic> {
    budget.charge(8, span)?;
    autodiff::unary_value(operation, input.require_float(span)?)
        .map(Value::Float)
        .map_err(|mut error| {
            error.span = span;
            error
        })
}

enum Operation {
    Input(usize),
    Constant(FiniteFloat),
    Alias(usize),
    Unary(Unary, usize),
    Binary(Binary, usize, usize),
}

#[derive(Debug)]
pub(crate) struct ScalarGraph {
    graph: Graph,
    output: NodeId,
    parameter: NodeId,
    input_slots: Box<[usize]>,
}

impl ScalarGraph {
    pub(crate) fn compile(
        syntax: &Expression,
        arguments: &[CallArgument],
        types: &[SourceType],
        slots: &[Option<usize>],
        calls: &[Option<ResolvedCall>],
        span: Span,
    ) -> Result<Self, Diagnostic> {
        if arguments.len() != 2 || arguments.iter().any(|arg| arg.label.is_some()) {
            return Err(Diagnostic::new(
                DiagnosticKind::CallArguments,
                span,
                "value_grad requires a float binding and a float expression",
            ));
        }
        for argument in arguments {
            let index = argument.expression;
            if types[index] != ValueType::Float {
                return Err(crate::value::type_mismatch(
                    syntax.nodes()[index].span,
                    ValueType::Float,
                    &types[index],
                ));
            }
        }
        let parameter_index = arguments[0].expression;
        let parameter_slot = slots[parameter_index].ok_or_else(|| {
            diagnostic(
                syntax.nodes()[parameter_index].span,
                "value_grad needs a named float binding as its first argument",
            )
        })?;
        let root = arguments[1].expression;
        let mut pending = vec![root];
        let mut checked = BTreeSet::new();
        let mut operations = BTreeMap::new();
        while let Some(index) = pending.pop() {
            if !checked.insert(index) {
                continue;
            }
            let node = &syntax.nodes()[index];
            if checked.len() >= MAX_SOURCE_GRAPH_NODES {
                return Err(diagnostic(
                    node.span,
                    "value_grad graph exceeds 4096 syntax nodes",
                ));
            }
            let operation = match &node.kind {
                ExpressionKind::Float(value) => Operation::Constant(*value),
                ExpressionKind::Name(_) => {
                    Operation::Input(slots[index].expect("resolved float binding"))
                }
                ExpressionKind::Group { expression } => {
                    pending.push(*expression);
                    Operation::Alias(*expression)
                }
                ExpressionKind::Negate { operand, .. } => {
                    pending.push(*operand);
                    Operation::Unary(Unary::Negate, *operand)
                }
                ExpressionKind::Binary {
                    left,
                    operator,
                    right,
                    ..
                } => {
                    let operation = match operator {
                        BinaryOperator::Add => Binary::Add,
                        BinaryOperator::Subtract => Binary::Subtract,
                        BinaryOperator::Multiply => Binary::Multiply,
                        BinaryOperator::Divide => Binary::Divide,
                        _ => return Err(unsupported(node.span)),
                    };
                    pending.extend([*right, *left]);
                    Operation::Binary(operation, *left, *right)
                }
                ExpressionKind::UserCall { arguments, .. } => {
                    let Some(ResolvedCall::ScalarUnary(operation)) = calls[index] else {
                        return Err(unsupported(node.span));
                    };
                    let operand = arguments[0].expression;
                    pending.push(operand);
                    Operation::Unary(operation, operand)
                }
                _ => return Err(unsupported(node.span)),
            };
            operations.insert(index, operation);
        }

        let mut graph = Graph::new(operations.len() + 1)?;
        let parameter = graph.input()?;
        let mut slot_nodes = BTreeMap::from([(parameter_slot, parameter.clone())]);
        let mut bindings = vec![parameter_slot];
        let mut lowered = BTreeMap::<usize, NodeId>::new();
        for (index, operation) in operations {
            let node = match operation {
                Operation::Input(slot) => {
                    let entry = match slot_nodes.entry(slot) {
                        std::collections::btree_map::Entry::Occupied(entry) => entry.into_mut(),
                        std::collections::btree_map::Entry::Vacant(entry) => {
                            bindings.push(slot);
                            entry.insert(graph.input()?)
                        }
                    };
                    entry.clone()
                }
                Operation::Constant(value) => graph.constant(value.get())?,
                Operation::Alias(operand) => lowered[&operand].clone(),
                Operation::Unary(op, operand) => graph.unary(op, &lowered[&operand])?,
                Operation::Binary(op, left, right) => {
                    graph.binary(op, &lowered[&left], &lowered[&right])?
                }
            };
            lowered.insert(index, node);
        }
        Ok(Self {
            graph,
            output: lowered.remove(&root).expect("lowered output"),
            parameter,
            input_slots: bindings.into(),
        })
    }

    pub(crate) fn evaluate(
        &self,
        environment: &[Value],
        budget: &mut Budget,
        span: Span,
    ) -> Result<Value, Diagnostic> {
        let mut inputs = super::buffer(self.input_slots.len(), budget, span)?;
        for &slot in self.input_slots.iter() {
            inputs.push(environment[slot].require_float(span)?);
        }
        let result = self
            .graph
            .evaluate_with_budget(&self.output, &inputs, 1.0, budget)
            .map_err(|mut error| {
                error.span = span;
                error
            })?;
        let output = Value::Float(finite_number(result.output(), span)?);
        let gradient = Value::Float(finite_number(
            result.gradient(&self.parameter).expect("graph parameter"),
            span,
        )?);
        crate::evaluator::make_tuple([output, gradient], budget, span)
    }
}

fn unsupported(span: Span) -> Diagnostic {
    diagnostic(span, "value_grad expression supports float bindings, float literals, +, -, *, / and scalar activation calls")
}

fn diagnostic(span: Span, message: &str) -> Diagnostic {
    Diagnostic::new(DiagnosticKind::AutoDiffExpression, span, message)
}
