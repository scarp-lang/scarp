//! Exact decimal intrinsics with explicit types, rounding, and bounded work.
//! No decimal operation ever coerces through binary64.

use super::*;
use crate::{
    ast::BinaryOperator,
    decimal::{Decimal, DecimalError, RoundingMode, MAX_DECIMAL_SCALE, MAX_DECIMAL_TEXT_BYTES},
};

pub(crate) fn diagnostic(error: DecimalError, span: Span) -> Diagnostic {
    let (kind, message) = match error {
        DecimalError::InvalidSyntax => {
            (DiagnosticKind::InvalidDecimal, "invalid exact decimal text")
        }
        DecimalError::ScaleExceeded => (
            DiagnosticKind::DecimalLimit,
            "decimal exceeds 18 fractional places",
        ),
        DecimalError::PrecisionExceeded => (
            DiagnosticKind::DecimalLimit,
            "decimal exceeds the 28-digit coefficient limit",
        ),
        DecimalError::DivisionByZero => {
            (DiagnosticKind::DivisionByZero, "decimal division by zero")
        }
        DecimalError::Inexact => (
            DiagnosticKind::DecimalInexact,
            "exact decimal operation requires explicit rounding",
        ),
    };
    Diagnostic::new(kind, span, message)
}

fn rounding_mode(value: &str, span: Span) -> Result<RoundingMode, Diagnostic> {
    match value {
        "half_even" => Ok(RoundingMode::HalfEven),
        "half_up" => Ok(RoundingMode::HalfUp),
        "toward_zero" => Ok(RoundingMode::TowardZero),
        "away_from_zero" => Ok(RoundingMode::AwayFromZero),
        _ => Err(Diagnostic::new(
            DiagnosticKind::InvalidDecimal,
            span,
            "rounding mode must be half_even, half_up, toward_zero or away_from_zero",
        )),
    }
}

fn places(value: i64, span: Span) -> Result<u8, Diagnostic> {
    u8::try_from(value)
        .ok()
        .filter(|places| *places <= MAX_DECIMAL_SCALE)
        .ok_or_else(|| {
            Diagnostic::new(
                DiagnosticKind::DecimalLimit,
                span,
                "decimal places must be an integer from 0 through 18",
            )
        })
}

fn allocated_text(value: String, budget: &mut Budget, span: Span) -> Result<Value, Diagnostic> {
    budget.charge(value.len() as u64, span)?;
    budget.charge_text_allocation(value.len(), span)?;
    Ok(Value::from(value))
}

pub(super) fn evaluate(
    function: Builtin,
    args: &[Value],
    budget: &mut Budget,
    span: Span,
    argument_span: impl Fn(usize) -> Span,
) -> Result<Value, Diagnostic> {
    match function {
        Builtin::Decimal => {
            let text = args[0].as_str().expect("type-checked decimal constructor");
            // The host parser checks length before scanning; even a malicious
            // multi-megabyte string incurs bounded work here before rejection.
            budget.charge((text.len().min(MAX_DECIMAL_TEXT_BYTES) + 1) as u64, span)?;
            text.parse::<Decimal>()
                .map(Value::Decimal)
                .map_err(|error| diagnostic(error, argument_span(0)))
        }
        Builtin::DecimalFromInteger => {
            budget.charge(1, span)?;
            Ok(Value::Decimal(Decimal::from_integer(
                args[0].as_integer().expect("typed integer"),
            )))
        }
        Builtin::DecimalToInteger => {
            budget.charge(1, span)?;
            args[0]
                .as_decimal()
                .expect("typed decimal")
                .to_integer_exact()
                .map(Value::Integer)
                .map_err(|error| diagnostic(error, argument_span(0)))
        }
        Builtin::DecimalToText => {
            let decimal = args[0].as_decimal().expect("typed decimal");
            allocated_text(decimal.to_string(), budget, span)
        }
        Builtin::DecimalAbs => {
            budget.charge(1, span)?;
            let decimal = args[0].as_decimal().expect("typed decimal");
            Decimal::new(decimal.coefficient().abs(), decimal.scale())
                .map(Value::Decimal)
                .map_err(|error| diagnostic(error, span))
        }
        Builtin::DecimalRound => {
            budget.charge(24, span)?;
            let scale = places(
                args[1].as_integer().expect("typed places"),
                argument_span(1),
            )?;
            let mode = rounding_mode(args[2].as_str().expect("typed mode"), argument_span(2))?;
            args[0]
                .as_decimal()
                .expect("typed decimal")
                .round_to(scale, mode)
                .map(Value::Decimal)
                .map_err(|error| diagnostic(error, span))
        }
        Builtin::DecimalFormat => {
            budget.charge(24, span)?;
            let scale = places(
                args[1].as_integer().expect("typed places"),
                argument_span(1),
            )?;
            let mode = rounding_mode(args[2].as_str().expect("typed mode"), argument_span(2))?;
            let value = args[0]
                .as_decimal()
                .expect("typed decimal")
                .format_fixed(scale, mode)
                .map_err(|error| diagnostic(error, span))?;
            allocated_text(value, budget, span)
        }
        Builtin::DecimalDivRounded => {
            budget.charge(64, span)?;
            let scale = places(
                args[2].as_integer().expect("typed places"),
                argument_span(2),
            )?;
            let mode = rounding_mode(args[3].as_str().expect("typed mode"), argument_span(3))?;
            args[0]
                .as_decimal()
                .expect("typed numerator")
                .div_rounded(
                    args[1].as_decimal().expect("typed denominator"),
                    scale,
                    mode,
                )
                .map(Value::Decimal)
                .map_err(|error| diagnostic(error, argument_span(1)))
        }
        _ => unreachable!("resolved decimal builtin"),
    }
}

/// Interpreter arithmetic is exact. The / operator accepts only quotients
/// representable within the bounded decimal format; repeating quotients use
/// the explicit decimal_div_rounded builtin instead.
pub(crate) fn binary(
    operator: BinaryOperator,
    left: Decimal,
    right: Decimal,
    span: Span,
) -> Result<Value, Diagnostic> {
    let result = match operator {
        BinaryOperator::Add => left.checked_add(right),
        BinaryOperator::Subtract => left.checked_sub(right),
        BinaryOperator::Multiply => left.checked_mul(right),
        BinaryOperator::Divide => left.checked_div_exact(right),
        BinaryOperator::Equal => return Ok(Value::Boolean(left == right)),
        BinaryOperator::NotEqual => return Ok(Value::Boolean(left != right)),
        BinaryOperator::Less => return Ok(Value::Boolean(left < right)),
        BinaryOperator::LessEqual => return Ok(Value::Boolean(left <= right)),
        BinaryOperator::Greater => return Ok(Value::Boolean(left > right)),
        BinaryOperator::GreaterEqual => return Ok(Value::Boolean(left >= right)),
        _ => {
            return Err(crate::value::type_mismatch(
                span,
                ValueType::Integer,
                ValueType::Decimal,
            ))
        }
    };
    result
        .map(Value::Decimal)
        .map_err(|error| diagnostic(error, span))
}
