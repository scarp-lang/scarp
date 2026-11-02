use super::*;
use crate::limits::text_construction;

fn reserve(bytes: usize, budget: &mut Budget, span: Span) -> Result<String, Diagnostic> {
    budget.charge_text_allocation(bytes, span)?;
    budget.charge(u64::try_from(bytes).unwrap_or(u64::MAX).max(1), span)?;
    let mut result = String::new();
    result
        .try_reserve_exact(bytes)
        .map_err(|_| text_construction(span, "could not reserve string result storage"))?;
    Ok(result)
}
fn size_error(span: Span) -> Diagnostic {
    text_construction(span, "string result size is too large")
}
fn slice_error(span: Span) -> Diagnostic {
    Diagnostic::new(DiagnosticKind::TextIndex, span, "text slice is outside the string")
        .with_help("use nonnegative scalar start/count with start + count <= text_len(text); an empty slice at the end is valid")
}

pub(super) fn evaluate(
    function: Builtin,
    args: &[Value],
    budget: &mut Budget,
    span: Span,
    argument_span: impl Fn(usize) -> Span,
) -> Result<Value, Diagnostic> {
    let text = args[0].as_str().expect("checked string argument");
    let result = match function {
        Builtin::TextConcat => {
            let right = args[1].as_str().expect("checked string argument");
            let bytes = text
                .len()
                .checked_add(right.len())
                .ok_or_else(|| size_error(span))?;
            let mut result = reserve(bytes, budget, span)?;
            result.push_str(text);
            result.push_str(right);
            result
        }
        Builtin::TextRepeat => {
            let count = args[1].require_integer(argument_span(1))?;
            if count < 0 {
                return Err(text_construction(
                    argument_span(1),
                    "text repetition count must be nonnegative",
                ));
            }
            if text.is_empty() || count == 0 {
                reserve(0, budget, span)?
            } else {
                let count = usize::try_from(count).map_err(|_| size_error(argument_span(1)))?;
                let bytes = text
                    .len()
                    .checked_mul(count)
                    .ok_or_else(|| size_error(span))?;
                let mut result = reserve(bytes, budget, span)?;
                for _ in 0..count {
                    result.push_str(text);
                }
                result
            }
        }
        Builtin::TextSlice => {
            let start = args[1].require_integer(argument_span(1))?;
            let count = args[2].require_integer(argument_span(2))?;
            let start = usize::try_from(start).map_err(|_| slice_error(argument_span(1)))?;
            let count = usize::try_from(count).map_err(|_| slice_error(argument_span(2)))?;
            let end = start
                .checked_add(count)
                .ok_or_else(|| slice_error(argument_span(2)))?;
            budget.charge(u64::try_from(text.len()).unwrap_or(u64::MAX).max(1), span)?;
            let mut begin = None;
            let mut finish = None;
            // Include EOF so start == length and count == 0 are well-defined.
            for (index, offset) in text
                .char_indices()
                .map(|(byte, _)| byte)
                .chain(std::iter::once(text.len()))
                .enumerate()
            {
                if index == start {
                    begin = Some(offset);
                }
                if index == end {
                    finish = Some(offset);
                    break;
                }
            }
            let begin = begin.ok_or_else(|| slice_error(argument_span(1)))?;
            let finish = finish.ok_or_else(|| slice_error(argument_span(2)))?;
            let slice = &text[begin..finish];
            let mut result = reserve(slice.len(), budget, span)?;
            result.push_str(slice);
            result
        }
        _ => unreachable!("checked text construction dispatch"),
    };
    Ok(Value::from(result.as_str()))
}
