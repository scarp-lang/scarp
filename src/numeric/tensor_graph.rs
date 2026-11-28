//! Bounded source tensor reverse mode over the existing forward and VJP kernels.

use std::collections::{BTreeMap, BTreeSet};

use super::{
    source_tensor::{self, TensorCall},
    tensor::{self, TensorBinary, TensorUnary},
};
use crate::{
    ast::{CallArgument, Expression, ExpressionKind},
    diagnostics::{Diagnostic, DiagnosticKind, Span},
    limits::Budget,
    resolver::ResolvedCall,
    types::SourceType,
    value::{FiniteFloat, NumericTensor, Value, ValueType},
};

pub(crate) const VALUE_GRAD: &str = "tensor_value_grad";
const MAX_GRAPH_NODES: usize = 1024;

#[derive(Debug)]
pub(crate) struct TensorGraph {
    nodes: Box<[Node]>,
    output: usize,
    parameter: usize,
}

#[derive(Debug)]
struct Node {
    op: Operation,
    kind: SourceType,
    span: Span,
    active: bool,
}

#[derive(Debug, Clone, Copy)]
enum Operation {
    Input(usize),
    Alias(usize),
    Binary(TensorBinary, usize, usize),
    Matmul(usize, usize),
    Unary(TensorUnary, usize),
    Reshape(usize),
    Broadcast(usize),
    Transpose(usize),
    SumAxis(usize, usize),
}

impl Operation {
    fn map(self, mut index: impl FnMut(usize) -> usize) -> Self {
        match self {
            Self::Input(_) => self,
            Self::Alias(a) => Self::Alias(index(a)),
            Self::Binary(op, a, b) => Self::Binary(op, index(a), index(b)),
            Self::Matmul(a, b) => Self::Matmul(index(a), index(b)),
            Self::Unary(op, a) => Self::Unary(op, index(a)),
            Self::Reshape(a) => Self::Reshape(index(a)),
            Self::Broadcast(a) => Self::Broadcast(index(a)),
            Self::Transpose(a) => Self::Transpose(index(a)),
            Self::SumAxis(a, axis) => Self::SumAxis(index(a), axis),
        }
    }
}

impl TensorGraph {
    pub(crate) fn compile(
        syntax: &Expression,
        arguments: &[CallArgument],
        types: &[SourceType],
        slots: &[Option<usize>],
        calls: &[Option<ResolvedCall>],
        span: Span,
    ) -> Result<(Self, SourceType), Diagnostic> {
        let positional = arguments.iter().all(|arg| arg.label.is_none());
        let [parameter, loss] = arguments else {
            return Err(arity(span));
        };
        if !positional {
            return Err(arity(span));
        }
        let parameter = parameter.expression;
        let parameter_slot = slots[parameter]
            .filter(|_| types[parameter].as_tensor().is_some())
            .ok_or_else(|| {
                error(
                    syntax.nodes()[parameter].span,
                    "first argument must be a named tensor binding",
                )
            })?;
        let loss = ungrouped(syntax, loss.expression);
        let loss_node = &syntax.nodes()[loss];
        let root = match (&calls[loss], &loss_node.kind) {
            (
                Some(ResolvedCall::Tensor(TensorCall::Sum)),
                ExpressionKind::UserCall { arguments, .. },
            ) => arguments[0].expression,
            _ => {
                return Err(error(
                    loss_node.span,
                    "tensor loss must be written as tensor_sum(expression)",
                ))
            }
        };

        let mut pending = vec![root];
        let mut checked = BTreeSet::new();
        let mut operations = BTreeMap::new();
        while let Some(index) = pending.pop() {
            if !checked.insert(index) {
                continue;
            }
            let node = &syntax.nodes()[index];
            if checked.len() >= MAX_GRAPH_NODES {
                return Err(error(
                    node.span,
                    "tensor gradient graph exceeds 1024 syntax nodes",
                ));
            }
            let op = match &node.kind {
                ExpressionKind::Name(_) => {
                    Operation::Input(slots[index].expect("resolved tensor binding"))
                }
                ExpressionKind::Group { expression } => Operation::Alias(*expression),
                ExpressionKind::Binary { left, right, .. } => {
                    let Some(ResolvedCall::Tensor(TensorCall::Binary(operation))) = calls[index]
                    else {
                        return Err(unsupported(node.span));
                    };
                    Operation::Binary(operation, *left, *right)
                }
                ExpressionKind::UserCall { arguments, .. } => {
                    let Some(ResolvedCall::Tensor(operation)) = calls[index] else {
                        return Err(unsupported(node.span));
                    };
                    let first = arguments[0].expression;
                    match operation {
                        TensorCall::Matmul => Operation::Matmul(first, arguments[1].expression),
                        TensorCall::Unary(op) => Operation::Unary(op, first),
                        TensorCall::Reshape => Operation::Reshape(first),
                        TensorCall::Broadcast => Operation::Broadcast(first),
                        TensorCall::Transpose => Operation::Transpose(first),
                        TensorCall::SumAxis => Operation::SumAxis(
                            first,
                            source_tensor::static_axis(syntax, arguments[1].expression)?,
                        ),
                        _ => return Err(unsupported(node.span)),
                    }
                }
                _ => return Err(unsupported(node.span)),
            };
            op.map(|child| {
                pending.push(child);
                child
            });
            operations.insert(index, op);
        }
        // Intern each binding once, so two edges to the same value accumulate
        // in the same adjoint. The parameter also has an entry if disconnected.
        let mut nodes = vec![Node {
            op: Operation::Input(parameter_slot),
            span: syntax.nodes()[parameter].span,
            kind: types[parameter].clone(),
            active: true,
        }];
        let mut bindings = BTreeMap::from([(parameter_slot, 0)]);
        let mut indices = BTreeMap::new();
        for (index, op) in operations {
            let id = match op {
                Operation::Alias(operand) => indices[&operand],
                Operation::Input(slot) => *bindings.entry(slot).or_insert_with(|| {
                    let id = nodes.len();
                    nodes.push(Node {
                        op,
                        span: syntax.nodes()[index].span,
                        kind: types[index].clone(),
                        active: false,
                    });
                    id
                }),
                _ => {
                    let mut active = false;
                    let lowered = op.map(|child| {
                        let id = indices[&child];
                        active |= nodes[id].active;
                        id
                    });
                    let id = nodes.len();
                    nodes.push(Node {
                        op: lowered,
                        span: syntax.nodes()[index].span,
                        kind: types[index].clone(),
                        active,
                    });
                    id
                }
            };
            indices.insert(index, id);
        }
        let graph = Self {
            nodes: nodes.into(),
            output: indices[&root],
            parameter: 0,
        };
        let pair = SourceType::tuple(vec![ValueType::Float.into(), types[parameter].clone()])
            .expect("float and tensor pair type");
        Ok((graph, pair))
    }

    pub(crate) fn evaluate(
        &self,
        environment: &[Value],
        budget: &mut Budget,
        span: Span,
    ) -> Result<Value, Diagnostic> {
        let count = self.nodes.len();
        budget.charge(4 * count as u64, span)?;
        let mut values: Vec<Option<NumericTensor>> = super::buffer(count, budget, span)?;
        let mut gradients: Vec<Option<NumericTensor>> = super::buffer(count, budget, span)?;
        gradients.resize_with(count, || None);
        for node in self.nodes.iter() {
            let input = |id: usize| values[id].as_ref().expect("earlier graph node");
            let shape = node
                .kind
                .as_tensor()
                .expect("typed graph node")
                .dimensions();
            let value = match node.op {
                Operation::Input(slot) => Ok(environment[slot]
                    .as_tensor()
                    .expect("typed graph input")
                    .clone()),
                Operation::Binary(op, a, b) => {
                    tensor::binary_with_budget(op, input(a), input(b), budget)
                }
                Operation::Matmul(a, b) => tensor::matmul_with_budget(input(a), input(b), budget),
                Operation::Unary(op, a) => tensor::unary_with_budget(op, input(a), budget),
                Operation::Reshape(a) => Ok(input(a).reshape(shape).expect("checked reshape")),
                Operation::Broadcast(a) => {
                    tensor::broadcast_to_with_budget(input(a), shape, budget)
                }
                Operation::Transpose(a) => {
                    tensor::transpose_with_budget(input(a), budget, node.span)
                }
                Operation::SumAxis(a, axis) => {
                    tensor::sum_axis_with_budget(input(a), axis, false, budget)
                }
                Operation::Alias(_) => unreachable!("aliases share an existing node"),
            }
            .map_err(|err| at(err, node.span))?;
            values.push(Some(value));
        }
        let output = values[self.output].as_ref().expect("graph output");
        let loss =
            tensor::sum_to_shape_with_budget(output, &[], budget).map_err(|err| at(err, span))?;
        if self.nodes[self.output].active {
            gradients[self.output] = Some(fill(output, 1.0, budget, span)?);
        }
        for (id, node) in self.nodes.iter().enumerate().rev() {
            if id == self.parameter {
                continue;
            }
            if let Some(upstream) = gradients[id].take() {
                self.backward(node, &upstream, &values, &mut gradients, budget)
                    .map_err(|err| at(err, node.span))?;
            }
            // All consumers are later in the topological order and have run.
            // Release references while retaining earlier primals for their VJPs.
            values[id] = None;
        }
        let gradient = match gradients[self.parameter].take() {
            Some(value) => value,
            None => fill(
                values[self.parameter].as_ref().expect("graph parameter"),
                0.0,
                budget,
                span,
            )?,
        };
        crate::evaluator::make_tuple(
            [Value::Float(loss.cells()[0]), source_tensor::wrap(gradient)],
            budget,
            span,
        )
    }

    fn backward(
        &self,
        node: &Node,
        upstream: &NumericTensor,
        values: &[Option<NumericTensor>],
        gradients: &mut [Option<NumericTensor>],
        budget: &mut Budget,
    ) -> Result<(), Diagnostic> {
        let input = |id: usize| values[id].as_ref().expect("retained graph operand");
        let add = |gradients: &mut [Option<NumericTensor>],
                   id: usize,
                   contribution,
                   budget: &mut Budget| {
            if self.nodes[id].active {
                gradients[id] = Some(match gradients[id].take() {
                    Some(previous) => tensor::binary_with_budget(
                        TensorBinary::Add,
                        &previous,
                        &contribution,
                        budget,
                    )?,
                    None => contribution,
                });
            }
            Ok::<(), Diagnostic>(())
        };
        match node.op {
            Operation::Input(_) => Ok(()),
            Operation::Binary(_, a, b) | Operation::Matmul(a, b) => {
                let (left, right) = match node.op {
                    Operation::Binary(op, _, _) => tensor::binary_backward_with_budget(
                        op,
                        input(a),
                        input(b),
                        upstream,
                        budget,
                    )?,
                    _ => tensor::matmul_backward_with_budget(input(a), input(b), upstream, budget)?,
                };
                add(gradients, a, left, budget)?;
                add(gradients, b, right, budget)
            }
            Operation::Unary(_, a)
            | Operation::Reshape(a)
            | Operation::Broadcast(a)
            | Operation::Transpose(a)
            | Operation::SumAxis(a, _) => {
                let contribution = match node.op {
                    Operation::Unary(op, _) => {
                        tensor::unary_backward_with_budget(op, input(a), upstream, budget)?
                    }
                    Operation::Reshape(_) => upstream
                        .reshape(input(a).shape())
                        .expect("checked reverse reshape"),
                    Operation::Broadcast(_) => {
                        tensor::sum_to_shape_with_budget(upstream, input(a).shape(), budget)?
                    }
                    Operation::Transpose(_) => {
                        tensor::transpose_with_budget(upstream, budget, node.span)?
                    }
                    Operation::SumAxis(_, axis) => tensor::sum_axis_backward_with_budget(
                        input(a).shape(),
                        axis,
                        false,
                        upstream,
                        budget,
                    )?,
                    _ => unreachable!("checked unary operation"),
                };
                add(gradients, a, contribution, budget)
            }
            Operation::Alias(_) => unreachable!("aliases share an existing node"),
        }
    }
}

fn fill(
    input: &NumericTensor,
    value: f64,
    budget: &mut Budget,
    span: Span,
) -> Result<NumericTensor, Diagnostic> {
    budget.charge(input.len() as u64, span)?;
    let mut cells = super::buffer(input.len(), budget, span)?;
    cells.resize(
        input.len(),
        FiniteFloat::new(value).expect("finite gradient seed"),
    );
    tensor::finish(cells, input.shape()).map_err(|err| at(err, span))
}

fn ungrouped(syntax: &Expression, mut index: usize) -> usize {
    while let ExpressionKind::Group { expression } = syntax.nodes()[index].kind {
        index = expression;
    }
    index
}

fn error(span: Span, message: &str) -> Diagnostic {
    Diagnostic::new(DiagnosticKind::AutoDiffExpression, span, message)
}

fn arity(span: Span) -> Diagnostic {
    Diagnostic::new(
        DiagnosticKind::CallArguments,
        span,
        "tensor_value_grad requires a named tensor and a scalar tensor_sum loss",
    )
}

fn unsupported(span: Span) -> Diagnostic {
    error(span, "unsupported tensor gradient operation; use named tensors, arithmetic, matmul, unary activations, reshape, broadcast, transpose or tensor_sum_axis")
}

fn at(mut error: Diagnostic, span: Span) -> Diagnostic {
    if error.span == Span::default() {
        error.span = span;
    }
    error
}
