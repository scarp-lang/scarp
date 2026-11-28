//! Shape checking and dispatch for source tensor calls.

use super::{
    tensor::{self, TensorBinary, TensorUnary},
    Builtin,
};
use crate::{
    ast::{BinaryOperator, CallArgument, Expression, ExpressionKind},
    diagnostics::{Diagnostic, DiagnosticKind, Span},
    limits::Budget,
    types::SourceType,
    value::{NumericTensor, TensorValue, Value, ValueType, MAX_TENSOR_RANK},
};

#[derive(Debug, Clone, Copy)]
pub(crate) enum TensorCall {
    Binary(TensorBinary),
    BinaryBackward(TensorBinary),
    Matmul,
    MatmulBackward,
    Unary(TensorUnary),
    UnaryBackward(TensorUnary),
    Reshape,
    Broadcast,
    Transpose,
    Sum,
    SumAxis,
    SumAxisBackward,
    Item,
}

impl TensorCall {
    pub(crate) fn named(name: &str) -> Option<Self> {
        let (operation, backward) = name
            .strip_suffix("_backward")
            .map_or((name, false), |name| (name, true));
        let unary = match operation {
            "tensor_neg" => Some(TensorUnary::Negate),
            "tensor_square" => Some(TensorUnary::Square),
            "tensor_exp" => Some(TensorUnary::Exp),
            "tensor_log" => Some(TensorUnary::Log),
            "tensor_relu" => Some(TensorUnary::Relu),
            "tensor_sigmoid" => Some(TensorUnary::Sigmoid),
            "tensor_tanh" => Some(TensorUnary::Tanh),
            "tensor_softplus" => Some(TensorUnary::Softplus),
            _ => None,
        };
        if let Some(operation) = unary {
            return Some(if backward {
                Self::UnaryBackward(operation)
            } else {
                Self::Unary(operation)
            });
        }
        Some(match (operation, backward) {
            ("tensor_add", true) => Self::BinaryBackward(TensorBinary::Add),
            ("tensor_sub", true) => Self::BinaryBackward(TensorBinary::Subtract),
            ("tensor_mul", true) => Self::BinaryBackward(TensorBinary::Multiply),
            ("tensor_div", true) => Self::BinaryBackward(TensorBinary::Divide),
            ("tensor_matmul", true) => Self::MatmulBackward,
            ("tensor_sum_axis", true) => Self::SumAxisBackward,
            ("tensor_reshape", false) => Self::Reshape,
            ("tensor_broadcast", false) => Self::Broadcast,
            ("tensor_transpose", false) => Self::Transpose,
            ("tensor_sum", false) => Self::Sum,
            ("tensor_sum_axis", false) => Self::SumAxis,
            ("tensor_item", false) => Self::Item,
            _ => return None,
        })
    }

    pub(crate) fn overloaded(builtin: Builtin) -> Option<Self> {
        match builtin {
            Builtin::MatMul => Some(Self::Matmul),
            _ => None,
        }
    }

    pub(crate) fn operator(operator: BinaryOperator) -> Option<Self> {
        match operator {
            BinaryOperator::Add => Some(Self::Binary(TensorBinary::Add)),
            BinaryOperator::Subtract => Some(Self::Binary(TensorBinary::Subtract)),
            BinaryOperator::Multiply => Some(Self::Binary(TensorBinary::Multiply)),
            BinaryOperator::Divide => Some(Self::Binary(TensorBinary::Divide)),
            _ => None,
        }
    }

    pub(crate) fn infer(
        self,
        syntax: &Expression,
        arguments: &[CallArgument],
        types: &[SourceType],
        span: Span,
    ) -> Result<SourceType, Diagnostic> {
        let exact = match self {
            Self::Reshape | Self::Broadcast => 1..=1 + MAX_TENSOR_RANK,
            Self::Unary(_) | Self::Transpose | Self::Sum | Self::Item => 1..=1,
            Self::Binary(_) | Self::Matmul | Self::UnaryBackward(_) | Self::SumAxis => 2..=2,
            Self::BinaryBackward(_) | Self::MatmulBackward | Self::SumAxisBackward => 3..=3,
        };
        if !exact.contains(&arguments.len()) || arguments.iter().any(|arg| arg.label.is_some()) {
            let min = exact.start();
            let max = exact.end();
            let count = if min == max {
                min.to_string()
            } else {
                format!("{min} through {max}")
            };
            return Err(Diagnostic::new(
                DiagnosticKind::CallArguments,
                span,
                format!("tensor operation requires {count} positional arguments"),
            ));
        }
        let operand = |i: usize| {
            let node = arguments[i].expression;
            tensor_shape(&types[node], syntax.nodes()[node].span)
        };
        let shape = operand(0)?;
        match self {
            Self::Binary(_) | Self::Matmul => self.infer_binary(shape, operand(1)?, span),
            Self::BinaryBackward(_) | Self::MatmulBackward => {
                let other = operand(1)?;
                let forward = if matches!(self, Self::MatmulBackward) {
                    Self::Matmul
                } else {
                    Self::Binary(TensorBinary::Add)
                };
                let result = forward.infer_binary(shape, other, span)?;
                same_shape(result.as_tensor().unwrap().dimensions(), operand(2)?, span)?;
                Ok(SourceType::tuple(vec![
                    types[arguments[0].expression].clone(),
                    types[arguments[1].expression].clone(),
                ])
                .expect("two tensor types fit the source tuple limits"))
            }
            Self::Unary(_) => Ok(types[arguments[0].expression].clone()),
            Self::UnaryBackward(_) => {
                same_shape(shape, operand(1)?, span)?;
                Ok(types[arguments[0].expression].clone())
            }
            Self::Sum => Ok(ValueType::Float.into()),
            Self::Item if shape.is_empty() => Ok(ValueType::Float.into()),
            Self::Item => Err(shape_error(span, "tensor_item requires a rank-zero tensor")),
            Self::Transpose => {
                let axes = transpose_axes(shape.len(), span)?;
                let mut target = [0; MAX_TENSOR_RANK];
                for (axis, &from) in axes[..shape.len()].iter().enumerate() {
                    target[axis] = shape[from];
                }
                source_shape(&target[..shape.len()], span)
            }
            Self::Reshape | Self::Broadcast => {
                let mut target = [0; MAX_TENSOR_RANK];
                for (axis, arg) in arguments[1..].iter().enumerate() {
                    target[axis] = static_axis(syntax, arg.expression)?;
                }
                let target = &target[..arguments.len() - 1];
                let result = source_shape(target, span)?;
                if matches!(self, Self::Reshape) {
                    if crate::value::tensor_element_count(shape)
                        != crate::value::tensor_element_count(target)
                    {
                        return Err(shape_error(
                            span,
                            "tensor reshape requires the same cell count",
                        ));
                    }
                } else {
                    let computed =
                        Self::Binary(TensorBinary::Add).infer_binary(shape, target, span)?;
                    same_shape(target, computed.as_tensor().unwrap().dimensions(), span)?;
                }
                Ok(result)
            }
            Self::SumAxis | Self::SumAxisBackward => {
                let axis = static_axis(syntax, arguments[1].expression)?;
                let target = reduced_shape(shape, axis, span)?;
                let result = source_shape(&target[..shape.len() - 1], span)?;
                if matches!(self, Self::SumAxisBackward) {
                    same_shape(result.as_tensor().unwrap().dimensions(), operand(2)?, span)?;
                    Ok(types[arguments[0].expression].clone())
                } else {
                    Ok(result)
                }
            }
        }
    }

    pub(crate) fn infer_binary(
        self,
        left: &[usize],
        right: &[usize],
        span: Span,
    ) -> Result<SourceType, Diagnostic> {
        let mut shape = [0; MAX_TENSOR_RANK];
        let rank = if matches!(self, Self::Matmul) {
            tensor::matmul_shape(left, right, &mut shape)
        } else {
            tensor::broadcast_shape(left, right, &mut shape)
        }
        .map_err(|error| shape_error(span, error.message))?;
        source_shape(&shape[..rank], span)
    }

    fn run(self, args: &[Value], budget: &mut Budget, span: Span) -> Result<Value, Diagnostic> {
        let input = args[0].as_tensor().expect("typed tensor argument");
        let tensor_arg = |index: usize| args[index].as_tensor().expect("typed tensor argument");
        let axis_arg = |index: usize| {
            usize::try_from(args[index].as_integer().expect("typed axis")).expect("checked axis")
        };
        let result = match self {
            Self::Binary(op) => tensor::binary_with_budget(op, input, tensor_arg(1), budget)?,
            Self::Matmul => tensor::matmul_with_budget(input, tensor_arg(1), budget)?,
            Self::Unary(op) => tensor::unary_with_budget(op, input, budget)?,
            Self::UnaryBackward(op) => {
                tensor::unary_backward_with_budget(op, input, tensor_arg(1), budget)?
            }
            Self::Reshape | Self::Broadcast => {
                let mut shape = [0; MAX_TENSOR_RANK];
                for (axis, extent) in shape[..args.len() - 1].iter_mut().enumerate() {
                    *extent = axis_arg(axis + 1);
                }
                let shape = &shape[..args.len() - 1];
                if matches!(self, Self::Reshape) {
                    input.reshape(shape).expect("statically verified reshape")
                } else {
                    tensor::broadcast_to_with_budget(input, shape, budget)?
                }
            }
            Self::Transpose => tensor::transpose_with_budget(input, budget, span)?,
            Self::Sum => {
                let scalar = tensor::sum_to_shape_with_budget(input, &[], budget)?;
                return Ok(Value::Float(scalar.cells()[0]));
            }
            Self::Item => return Ok(Value::Float(input.cells()[0])),
            Self::SumAxis => tensor::sum_axis_with_budget(input, axis_arg(1), false, budget)?,
            Self::SumAxisBackward => tensor::sum_axis_backward_with_budget(
                input.shape(),
                axis_arg(1),
                false,
                tensor_arg(2),
                budget,
            )?,
            Self::BinaryBackward(_) | Self::MatmulBackward => {
                let (left, right) = match self {
                    Self::BinaryBackward(op) => tensor::binary_backward_with_budget(
                        op,
                        input,
                        tensor_arg(1),
                        tensor_arg(2),
                        budget,
                    )?,
                    _ => tensor::matmul_backward_with_budget(
                        input,
                        tensor_arg(1),
                        tensor_arg(2),
                        budget,
                    )?,
                };
                return crate::evaluator::make_tuple([wrap(left), wrap(right)], budget, span);
            }
        };
        Ok(wrap(result))
    }

    pub(crate) fn evaluate(
        self,
        args: &[Value],
        budget: &mut Budget,
        span: Span,
    ) -> Result<Value, Diagnostic> {
        self.run(args, budget, span).map_err(|mut error| {
            if error.span == Span::default() {
                error.span = span;
            }
            error
        })
    }
}

pub(crate) fn wrap(tensor: NumericTensor) -> Value {
    Value::Tensor(TensorValue::new(tensor))
}

pub(crate) fn tensor_shape(kind: &SourceType, span: Span) -> Result<&[usize], Diagnostic> {
    kind.as_tensor().map(|t| t.dimensions()).ok_or_else(|| {
        Diagnostic::new(
            DiagnosticKind::TypeMismatch,
            span,
            format!("expected tensor, found {kind}"),
        )
    })
}

pub(crate) fn source_shape(shape: &[usize], span: Span) -> Result<SourceType, Diagnostic> {
    SourceType::tensor(shape)
        .ok_or_else(|| shape_error(span, "tensor shape exceeds rank, axis, or cell limits"))
}

fn shape_error(span: Span, message: impl Into<String>) -> Diagnostic {
    Diagnostic::new(DiagnosticKind::TensorShape, span, message)
}

fn same_shape(expected: &[usize], actual: &[usize], span: Span) -> Result<(), Diagnostic> {
    if expected == actual {
        Ok(())
    } else {
        Err(shape_error(
            span,
            format!("expected tensor shape {expected:?}, found {actual:?}"),
        ))
    }
}

pub(crate) fn static_axis(syntax: &Expression, mut index: usize) -> Result<usize, Diagnostic> {
    let span = syntax.nodes()[index].span;
    loop {
        match &syntax.nodes()[index].kind {
            ExpressionKind::Group { expression } => index = *expression,
            ExpressionKind::Integer(value) => {
                if let Ok(axis) = usize::try_from(*value) {
                    return Ok(axis);
                }
                break;
            }
            _ => break,
        }
    }
    Err(shape_error(
        span,
        "tensor shape or axis requires a nonnegative integer literal",
    ))
}

fn transpose_axes(rank: usize, span: Span) -> Result<[usize; MAX_TENSOR_RANK], Diagnostic> {
    if rank < 2 {
        return Err(shape_error(
            span,
            "tensor transpose requires at least two axes",
        ));
    }
    let mut axes = std::array::from_fn(|i| i);
    axes.swap(rank - 2, rank - 1);
    Ok(axes)
}

fn reduced_shape(
    shape: &[usize],
    axis: usize,
    span: Span,
) -> Result<[usize; MAX_TENSOR_RANK], Diagnostic> {
    if axis >= shape.len() {
        return Err(shape_error(
            span,
            "tensor sum axis is outside the input rank",
        ));
    }
    let mut target = [0; MAX_TENSOR_RANK];
    target[..axis].copy_from_slice(&shape[..axis]);
    target[axis..shape.len() - 1].copy_from_slice(&shape[axis + 1..]);
    Ok(target)
}
