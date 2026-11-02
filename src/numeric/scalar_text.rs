//! Pure scalar/text intrinsics, sharing the existing typed built-in boundary.
use super::*;

fn conversion(span: Span, message: &str) -> Diagnostic {
    Diagnostic::new(DiagnosticKind::NumericConversion, span, message)
        .with_help("conversions must be exact: use an integral in-range float or an exactly representable integer")
}
fn index_error(span: Span, index: i64) -> Diagnostic {
    Diagnostic::new(DiagnosticKind::TextIndex, span, format!("text scalar index {index} is outside the string"))
        .with_help("use a nonnegative index below text_len(text); indices count Unicode scalar values, not UTF-8 bytes")
}
fn integer(value: usize, span: Span) -> Result<Value, Diagnostic> {
    i64::try_from(value).map(Value::Integer).map_err(|_| {
        Diagnostic::new(
            DiagnosticKind::ArithmeticOverflow,
            span,
            "text result exceeds the signed 64-bit integer range",
        )
        .with_help("reduce the text result to fit a signed 64-bit integer")
    })
}
fn bytes(value: usize) -> u64 {
    u64::try_from(value).unwrap_or(u64::MAX)
}

pub(super) fn exact_float(value: i64, span: Span) -> Result<FiniteFloat, Diagnostic> {
    let number = value as f64;
    // A saturating i64 round trip would incorrectly accept i64::MAX.
    if number as i128 != i128::from(value) {
        return Err(conversion(
            span,
            "integer cannot be represented exactly as a float",
        ));
    }
    finite_number(number, span)
}
pub(super) fn exact_integer(value: f64, span: Span) -> Result<i64, Diagnostic> {
    if value.fract() != 0.0 || value < i64::MIN as f64 || value >= -(i64::MIN as f64) {
        return Err(conversion(
            span,
            "float must be integral and within the signed 64-bit integer range",
        ));
    }
    Ok(value as i64)
}

pub(super) fn evaluate(
    function: Builtin,
    args: &[Value],
    budget: &mut Budget,
    span: Span,
    argument_span: impl Fn(usize) -> Span,
) -> Result<Value, Diagnostic> {
    if function == Builtin::ToFloat {
        budget.charge(1, span)?;
        let value = args[0].require_integer(argument_span(0))?;
        return exact_float(value, argument_span(0)).map(Value::Float);
    }
    if function == Builtin::ToInteger {
        budget.charge(1, span)?;
        let value = args[0].require_float(argument_span(0))?;
        return exact_integer(value, argument_span(0)).map(Value::Integer);
    }
    let text = args[0].as_str().expect("checked string argument");
    match function {
        Builtin::TextBytes => {
            budget.charge(1, span)?;
            integer(text.len(), span)
        }
        Builtin::TextLength => {
            budget.charge(bytes(text.len()).max(1), span)?;
            integer(text.chars().count(), span)
        }
        Builtin::TextCodepoint => {
            let index = args[1].require_integer(argument_span(1))?;
            let selected =
                usize::try_from(index).map_err(|_| index_error(argument_span(1), index))?;
            budget.charge(bytes(text.len()).max(1), span)?;
            text.chars()
                .nth(selected)
                .map(|value| Value::Integer(i64::from(u32::from(value))))
                .ok_or_else(|| index_error(argument_span(1), index))
        }
        Builtin::TextFind
        | Builtin::TextContains
        | Builtin::TextStartsWith
        | Builtin::TextEndsWith => {
            let needle = args[1].as_str().expect("checked string argument");
            let work = if matches!(function, Builtin::TextStartsWith | Builtin::TextEndsWith) {
                bytes(needle.len()).max(1)
            } else if needle.is_empty() {
                1
            } else {
                bytes(text.len())
                    .saturating_mul(bytes(needle.len()))
                    .saturating_add(bytes(text.len()))
                    .saturating_add(bytes(needle.len()))
                    .max(1)
            };
            budget.charge(work, span)?;
            match function {
                Builtin::TextStartsWith => Ok(Value::Boolean(text.starts_with(needle))),
                Builtin::TextEndsWith => Ok(Value::Boolean(text.ends_with(needle))),
                Builtin::TextContains => Ok(Value::Boolean(text.contains(needle))),
                _ => match text.find(needle) {
                    Some(offset) => integer(text[..offset].chars().count(), span),
                    None => Ok(Value::Integer(-1)),
                },
            }
        }
        _ => unreachable!("checked scalar/text dispatch"),
    }
}
