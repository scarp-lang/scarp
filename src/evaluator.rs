//! Primitive evaluation with checked integers and finite binary64 arithmetic.

use crate::{
    ast::{BinaryOperator, Expression, ExpressionKind, LogicalOperator},
    diagnostics::{Diagnostic, DiagnosticKind, Span},
    limits::Budget,
    numeric::{self, Builtin},
    resolver::{self, ResolvedCall, ResolvedExpression},
    value::{finite_number, type_mismatch, Value, ValueType},
};

/// Evaluate a standalone integer expression, preserving the v0.0.1 return type.
/// Use `evaluate_value` when any value result is allowed.
pub fn evaluate(expression: &Expression) -> Result<i64, Diagnostic> {
    evaluate_value(expression)?.require_integer(expression.nodes()[expression.root()].span)
}

/// Evaluate a standalone expression without a variable environment. This legacy
/// helper has an unbounded work allowance; capped program/native APIs are
/// preferable for controlled execution of numerical workloads.
pub fn evaluate_value(expression: &Expression) -> Result<Value, Diagnostic> {
    Workspace::default().evaluate(
        &resolver::resolve_standalone(expression)?,
        &[],
        &mut Budget::new(u64::MAX),
    )
}

/// Scratch storage belongs to one execution, never to the immutable program.
#[derive(Default)]
pub(crate) struct Workspace {
    values: Vec<Value>,
    tasks: Vec<Task>,
}

enum Task {
    BitNot(Span),
    Vector(usize, bool, Span),
    Index(Span),
    Builtin(Builtin, usize, Span),
    UserCall(usize, usize, Span),
    Visit(usize),
    Negate(Span),
    Not(Span),
    Boolean(Span),
    Binary(BinaryOperator, Span),
    Logical(LogicalOperator, usize, Span),
}

pub(crate) enum Evaluation {
    Complete(Value),
    Call {
        node: usize,
        arguments: Vec<Value>,
        span: Span,
    },
}

impl Workspace {
    pub(crate) fn clear(&mut self) {
        self.values.clear();
        self.tasks.clear();
    }
    pub(crate) fn evaluate(
        &mut self,
        expression: &ResolvedExpression,
        environment: &[Value],
        budget: &mut Budget,
    ) -> Result<Value, Diagnostic> {
        self.start(expression);
        match self.resume(expression, environment, budget)? {
            Evaluation::Complete(value) => Ok(value),
            Evaluation::Call { span, .. } => Err(Diagnostic::new(
                DiagnosticKind::FunctionUnsupported,
                span,
                "user calls require a program execution frame",
            )),
        }
    }

    pub(crate) fn start(&mut self, expression: &ResolvedExpression) {
        self.clear();
        self.tasks.push(Task::Visit(expression.syntax.root()));
    }

    pub(crate) fn accept(&mut self, value: Value) {
        self.values.push(value);
    }

    pub(crate) fn resume(
        &mut self,
        expression: &ResolvedExpression,
        environment: &[Value],
        budget: &mut Budget,
    ) -> Result<Evaluation, Diagnostic> {
        let Self { values, tasks } = self;
        while let Some(task) = tasks.pop() {
            let span = match &task {
                Task::Visit(index) => expression.syntax.nodes()[*index].span,
                Task::UserCall(_, _, span)
                | Task::Vector(_, _, span)
                | Task::BitNot(span)
                | Task::Index(span)
                | Task::Builtin(_, _, span)
                | Task::Negate(span)
                | Task::Not(span)
                | Task::Boolean(span)
                | Task::Binary(_, span)
                | Task::Logical(_, _, span) => *span,
            };
            budget.charge(1, span)?;
            match task {
                Task::Visit(index) => {
                    let node = &expression.syntax.nodes()[index];
                    match &node.kind {
                        ExpressionKind::UserCall { arguments, .. } => {
                            tasks.push(match &expression.calls[index] {
                                Some(ResolvedCall::Builtin(builtin)) => {
                                    Task::Builtin(*builtin, index, node.span)
                                }
                                _ => Task::UserCall(index, arguments.len(), node.span),
                            });
                            for argument in arguments.iter().rev() {
                                tasks.push(Task::Visit(argument.expression));
                            }
                        }
                        ExpressionKind::Vector { elements }
                        | ExpressionKind::Integers { elements } => {
                            tasks.push(Task::Vector(
                                elements.len(),
                                matches!(&node.kind, ExpressionKind::Integers { .. }),
                                node.span,
                            ));
                            for &element in elements.iter().rev() {
                                tasks.push(Task::Visit(element));
                            }
                        }
                        ExpressionKind::Index {
                            vector,
                            index,
                            operator_span,
                        } => {
                            tasks.push(Task::Index(*operator_span));
                            tasks.push(Task::Visit(*index));
                            tasks.push(Task::Visit(*vector));
                        }
                        ExpressionKind::Integer(value) => values.push(Value::Integer(*value)),
                        ExpressionKind::Float(value) => values.push(Value::Float(*value)),
                        ExpressionKind::Boolean(value) => values.push(Value::Boolean(*value)),
                        ExpressionKind::String(value) => {
                            budget.charge_text_allocation(value.len(), node.span)?;
                            values.push(Value::from(value.as_str()));
                        }
                        ExpressionKind::Name(name) => {
                            let value = expression.slots[index]
                                .and_then(|slot| environment.get(slot))
                                .cloned()
                                .ok_or_else(|| resolver::unknown_name(name, node.span))?;
                            values.push(value);
                        }
                        ExpressionKind::Group { expression } => {
                            tasks.push(Task::Visit(*expression))
                        }
                        ExpressionKind::BitNot {
                            operand,
                            operator_span,
                        } => {
                            tasks.push(Task::BitNot(*operator_span));
                            tasks.push(Task::Visit(*operand));
                        }
                        ExpressionKind::Negate {
                            operand,
                            operator_span,
                        } => {
                            tasks.push(Task::Negate(*operator_span));
                            tasks.push(Task::Visit(*operand));
                        }
                        ExpressionKind::Not {
                            operand,
                            operator_span,
                        } => {
                            tasks.push(Task::Not(*operator_span));
                            tasks.push(Task::Visit(*operand));
                        }
                        ExpressionKind::Binary {
                            left,
                            operator,
                            right,
                            operator_span,
                        } => {
                            tasks.push(Task::Binary(*operator, *operator_span));
                            tasks.push(Task::Visit(*right));
                            tasks.push(Task::Visit(*left));
                        }
                        ExpressionKind::Logical {
                            left,
                            operator,
                            right,
                            operator_span,
                        } => {
                            tasks.push(Task::Logical(*operator, *right, *operator_span));
                            tasks.push(Task::Visit(*left));
                        }
                    }
                }
                Task::Vector(count, integers, span) => {
                    budget.charge(count as u64, span)?;
                    let start = values.len() - count;
                    if integers {
                        let mut elements = numeric::buffer(count, budget, span)?;
                        for value in &values[start..] {
                            elements.push(value.require_integer(span)?);
                        }
                        values.truncate(start);
                        values.push(Value::Integers(
                            crate::value::IntegerVector::from_values(elements)
                                .expect("bounded collection"),
                        ));
                        continue;
                    }
                    let mut elements = numeric::buffer(count, budget, span)?;
                    for value in &values[start..] {
                        elements.push(finite_number(value.require_float(span)?, span)?);
                    }
                    values.truncate(start);
                    values.push(numeric::vector(elements, span)?);
                }
                Task::Index(span) => {
                    let index = values.pop().unwrap().require_integer(span)?;
                    let value = values.pop().unwrap();
                    if let Some(array) = value.as_integers() {
                        let element = usize::try_from(index)
                            .ok()
                            .and_then(|index| array.as_slice().get(index))
                            .ok_or_else(|| {
                                Diagnostic::new(
                                    DiagnosticKind::VectorIndex,
                                    span,
                                    format!(
                                        "index {index} is outside integer collection length {}",
                                        array.len()
                                    ),
                                )
                            })?;
                        values.push(Value::Integer(*element));
                        continue;
                    }
                    let vector = value.require_vector(span)?;
                    let element = usize::try_from(index)
                        .ok()
                        .and_then(|i| vector.as_slice().get(i))
                        .ok_or_else(|| {
                            Diagnostic::new(
                                DiagnosticKind::VectorIndex,
                                span,
                                format!("index {index} is outside vector length {}", vector.len()),
                            )
                        })?;
                    values.push(Value::Float(*element));
                }
                Task::Builtin(builtin, node, span) => {
                    let ExpressionKind::UserCall { arguments, .. } =
                        &expression.syntax.nodes()[node].kind
                    else {
                        unreachable!("resolved builtin call")
                    };
                    let count = arguments.len();
                    let start = values.len() - count;
                    let value = numeric::evaluate_with_spans(
                        builtin,
                        &values[start..],
                        budget,
                        span,
                        |i| expression.syntax.nodes()[arguments[i].expression].span,
                    )?;
                    values.truncate(start);
                    values.push(value);
                }
                Task::UserCall(node, count, span) => {
                    let arguments = values.split_off(values.len() - count);
                    return Ok(Evaluation::Call {
                        node,
                        arguments,
                        span,
                    });
                }
                Task::BitNot(span) => {
                    let value = values.pop().unwrap().require_integer(span)?;
                    values.push(Value::Integer(!value));
                }
                Task::Negate(span) => {
                    let value = values.pop().unwrap();
                    values.push(match value {
                        Value::Float(value) => finite(-value.get(), span)?,
                        Value::BigInt(value) => Value::BigInt(value.negated()),
                        _ => Value::Integer(
                            value
                                .require_integer(span)?
                                .checked_neg()
                                .ok_or_else(|| overflow(span))?,
                        ),
                    });
                }
                Task::Not(span) => {
                    let value = values.pop().unwrap().require_boolean(span)?;
                    values.push(Value::Boolean(!value));
                }
                Task::Boolean(span) => {
                    let value = values.pop().unwrap().require_boolean(span)?;
                    values.push(Value::Boolean(value));
                }
                Task::Binary(operator, span) => {
                    let right = values.pop().unwrap();
                    let left = values.pop().unwrap();
                    if let (Some(a), Some(b)) = (left.as_vector(), right.as_vector()) {
                        budget.charge(a.len().max(b.len()) as u64, span)?;
                    }
                    if let (Some(a), Some(b)) = (left.as_tile(), right.as_tile()) {
                        budget.charge(a.len().max(b.len()) as u64, span)?;
                    }
                    if let (Some(a), Some(b)) = (left.as_integers(), right.as_integers()) {
                        budget.charge(a.len().max(b.len()) as u64, span)?;
                    }
                    values.push(apply_binary_with_budget(
                        operator, &left, &right, budget, span,
                    )?);
                }
                Task::Logical(operator, right, span) => {
                    let left = values.pop().unwrap().require_boolean(span)?;
                    if (operator == LogicalOperator::And && !left)
                        || (operator == LogicalOperator::Or && left)
                    {
                        values.push(Value::Boolean(left));
                    } else {
                        tasks.push(Task::Boolean(span));
                        tasks.push(Task::Visit(right));
                    }
                }
            }
        }
        Ok(Evaluation::Complete(
            values
                .pop()
                .expect("a resolved expression produces one value"),
        ))
    }
}

pub(crate) fn apply_binary_with_budget(
    operator: BinaryOperator,
    left: &Value,
    right: &Value,
    budget: &mut Budget,
    span: Span,
) -> Result<Value, Diagnostic> {
    if let (Some(left), Some(right)) = (left.as_bigint(), right.as_bigint()) {
        return crate::bigint::binary(operator, left, right, budget, span);
    }
    apply_binary(operator, left, right, span)
}

pub(crate) fn apply_binary(
    operator: BinaryOperator,
    left: &Value,
    right: &Value,
    span: Span,
) -> Result<Value, Diagnostic> {
    if matches!(operator, BinaryOperator::Equal | BinaryOperator::NotEqual) {
        if left.kind() != right.kind() {
            return Err(type_mismatch(span, left.kind(), right.kind()));
        }
        return Ok(Value::Boolean(if operator == BinaryOperator::Equal {
            left == right
        } else {
            left != right
        }));
    }
    if left.kind() == ValueType::Float || right.kind() == ValueType::Float {
        let left = left.require_float(span)?;
        let right = right.require_float(span)?;
        return match operator {
            BinaryOperator::Add => finite(left + right, span),
            BinaryOperator::Subtract => finite(left - right, span),
            BinaryOperator::Multiply => finite(left * right, span),
            BinaryOperator::Divide => {
                if right == 0.0 {
                    return Err(Diagnostic::new(
                        DiagnosticKind::DivisionByZero,
                        span,
                        "division by zero",
                    )
                    .with_help("the float divisor must be nonzero, including signed zero"));
                }
                finite(left / right, span)
            }
            BinaryOperator::Less => Ok(Value::Boolean(left < right)),
            BinaryOperator::LessEqual => Ok(Value::Boolean(left <= right)),
            BinaryOperator::Greater => Ok(Value::Boolean(left > right)),
            BinaryOperator::GreaterEqual => Ok(Value::Boolean(left >= right)),
            _ => Err(type_mismatch(span, ValueType::Integer, ValueType::Float)),
        };
    }
    let left = left.require_integer(span)?;
    let right = right.require_integer(span)?;
    let result = match operator {
        BinaryOperator::BitAnd => return Ok(Value::Integer(left & right)),
        BinaryOperator::BitOr => return Ok(Value::Integer(left | right)),
        BinaryOperator::BitXor => return Ok(Value::Integer(left ^ right)),
        BinaryOperator::ShiftLeft | BinaryOperator::ShiftRight | BinaryOperator::UnsignedRight => {
            if !(0..64).contains(&right) {
                return Err(Diagnostic::new(
                    DiagnosticKind::InvalidShift,
                    span,
                    "shift count must be between 0 and 63",
                ).with_help("supply an integer count from 0 through 63; shifts use a fixed 64-bit bit pattern"));
            }
            return Ok(Value::Integer(match operator {
                BinaryOperator::ShiftLeft => left.wrapping_shl(right as u32),
                BinaryOperator::ShiftRight => left >> right,
                _ => ((left as u64) >> right) as i64,
            }));
        }
        BinaryOperator::Add => left.checked_add(right),
        BinaryOperator::Subtract => left.checked_sub(right),
        BinaryOperator::Multiply => left.checked_mul(right),
        BinaryOperator::Divide | BinaryOperator::Remainder => {
            if right == 0 {
                return Err(Diagnostic::new(
                    DiagnosticKind::DivisionByZero,
                    span,
                    "division by zero",
                )
                .with_help("the divisor must evaluate to a nonzero integer"));
            }
            if operator == BinaryOperator::Divide {
                left.checked_div(right)
            } else {
                left.checked_rem(right)
            }
        }
        BinaryOperator::Less => return Ok(Value::Boolean(left < right)),
        BinaryOperator::LessEqual => return Ok(Value::Boolean(left <= right)),
        BinaryOperator::Greater => return Ok(Value::Boolean(left > right)),
        BinaryOperator::GreaterEqual => return Ok(Value::Boolean(left >= right)),
        BinaryOperator::Equal | BinaryOperator::NotEqual => unreachable!("equality handled above"),
    };
    result.map(Value::Integer).ok_or_else(|| overflow(span))
}

fn overflow(span: Span) -> Diagnostic {
    Diagnostic::new(
        DiagnosticKind::ArithmeticOverflow,
        span,
        "integer arithmetic overflow",
    )
    .with_help("each result must fit in a signed 64-bit integer")
}

pub(crate) fn finite(value: f64, span: Span) -> Result<Value, Diagnostic> {
    finite_number(value, span).map(Value::Float)
}
