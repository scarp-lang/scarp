use super::*;
use crate::value::IntegerVector;

fn owned(values: Vec<i64>, span: Span) -> Result<Value, Diagnostic> {
    IntegerVector::from_values(values)
        .map(Value::Integers)
        .ok_or_else(|| length_error(span))
}
fn bounds(span: Span) -> Diagnostic {
    Diagnostic::new(
        DiagnosticKind::VectorIndex,
        span,
        "integer collection index or window is outside its visible range",
    )
    .with_help("use nonnegative indices and a window fully contained within ints_len(values)")
}
fn overflow(span: Span) -> Diagnostic {
    Diagnostic::new(
        DiagnosticKind::ArithmeticOverflow,
        span,
        "integer collection arithmetic overflow",
    )
    .with_help("every sum or range element must fit a signed 64-bit integer")
}
pub(super) fn evaluate(
    function: Builtin,
    args: &[Value],
    budget: &mut Budget,
    span: Span,
    at: impl Fn(usize) -> Span,
) -> Result<Value, Diagnostic> {
    if function == Builtin::IntsRange {
        let start = args[0].require_integer(at(0))?;
        let count = args[1].require_integer(at(1))?;
        let step = args[2].require_integer(at(2))?;
        let count = usize::try_from(count)
            .ok()
            .filter(|count| *count <= MAX_VECTOR_LENGTH)
            .ok_or_else(|| length_error(at(1)))?;
        budget.charge((count as u64).max(1), span)?;
        let mut values = buffer(count, budget, span)?;
        let mut value = start;
        for index in 0..count {
            values.push(value);
            if index + 1 < count {
                value = value.checked_add(step).ok_or_else(|| overflow(at(2)))?;
            }
        }
        return owned(values, span);
    }
    if function == Builtin::IntsFromVector {
        let source = args[0].require_vector(at(0))?;
        budget.charge((source.len() as u64).max(1), span)?;
        let mut values = buffer(source.len(), budget, span)?;
        for (index, value) in source.as_slice().iter().enumerate() {
            values.push(
                scalar_text::exact_integer(value.get(), at(0)).map_err(|mut error| {
                    error.message = format!("element {index}: {}", error.message);
                    error
                })?,
            );
        }
        return owned(values, span);
    }
    let source = args[0].require_integers(at(0))?;
    match function {
        Builtin::IntsLength => {
            budget.charge(1, span)?;
            Ok(Value::Integer(source.len() as i64))
        }
        Builtin::IntsSum => {
            budget.charge((source.len() as u64).max(1), span)?;
            source
                .as_slice()
                .iter()
                .try_fold(0i64, |sum, value| {
                    sum.checked_add(*value).ok_or_else(|| overflow(at(0)))
                })
                .map(Value::Integer)
        }
        Builtin::IntsWindow => {
            let start =
                usize::try_from(args[1].require_integer(at(1))?).map_err(|_| bounds(at(1)))?;
            let count = usize::try_from(args[2].require_integer(at(2))?)
                .map_err(|_| length_error(at(2)))?;
            budget.charge(1, span)?;
            source
                .window(start, count)
                .map(Value::Integers)
                .ok_or_else(|| bounds(at(1)))
        }
        Builtin::IntsJoin => {
            let right = args[1].require_integers(at(1))?;
            let count = source
                .len()
                .checked_add(right.len())
                .filter(|count| *count <= MAX_VECTOR_LENGTH)
                .ok_or_else(|| length_error(span))?;
            budget.charge((count as u64).max(1), span)?;
            let mut values = buffer(count, budget, span)?;
            values.extend_from_slice(source.as_slice());
            values.extend_from_slice(right.as_slice());
            owned(values, span)
        }
        Builtin::IntsSet => {
            let index = usize::try_from(args[1].require_integer(at(1))?)
                .ok()
                .filter(|index| *index < source.len())
                .ok_or_else(|| bounds(at(1)))?;
            budget.charge((source.len() as u64).max(1), span)?;
            let mut values = buffer(source.len(), budget, span)?;
            values.extend_from_slice(source.as_slice());
            values[index] = args[2].require_integer(at(2))?;
            owned(values, span)
        }
        Builtin::IntsToVector => {
            budget.charge((source.len() as u64).max(1), span)?;
            let mut values = buffer(source.len(), budget, span)?;
            for (index, value) in source.as_slice().iter().enumerate() {
                values.push(
                    scalar_text::exact_float(*value, at(0)).map_err(|mut error| {
                        error.message = format!("element {index}: {}", error.message);
                        error
                    })?,
                );
            }
            vector(values, span)
        }
        _ => unreachable!("checked integer collection dispatch"),
    }
}
