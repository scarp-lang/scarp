use super::*;
use crate::limits::text_construction;

pub(super) fn reserve(bytes: usize, budget: &mut Budget, span: Span) -> Result<String, Diagnostic> {
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
    if function == Builtin::TextFromCodepoint {
        let codepoint = args[0].require_integer(argument_span(0))?;
        // Unicode scalar values exclude surrogates and values beyond U+10FFFF.
        // Reject before allocating or publishing a partial result.
        let scalar = u32::try_from(codepoint)
            .ok()
            .and_then(char::from_u32)
            .ok_or_else(|| {
                Diagnostic::new(
                    DiagnosticKind::NumericDomain,
                    argument_span(0),
                    "text_from_codepoint requires a valid Unicode scalar value",
                )
                .with_help("use an integer in 0..=0x10FFFF, excluding 0xD800..=0xDFFF (surrogates)")
            })?;
        budget.charge(1, span)?;
        let mut result = reserve(scalar.len_utf8(), budget, span)?;
        result.push(scalar);
        return Ok(Value::from(result));
    }

    let text = args[0].as_str().expect("checked string argument");
    let result = match function {
        Builtin::TextTrim => {
            budget.charge(u64::try_from(text.len()).unwrap_or(u64::MAX).max(1), span)?;
            let trimmed = text.trim();
            if trimmed.len() == text.len() {
                return Ok(args[0].clone());
            }
            let mut result = reserve(trimmed.len(), budget, span)?;
            result.push_str(trimmed);
            result
        }
        Builtin::TextReplace => {
            let pattern = args[1].as_str().expect("typed pattern");
            let replacement = args[2].as_str().expect("typed replacement");
            if pattern.is_empty() {
                return Err(Diagnostic::new(
                    DiagnosticKind::NumericDomain,
                    argument_span(1),
                    "text_replace requires a nonempty search pattern",
                )
                .with_help("supply a nonempty exact UTF-8 substring; replacement may be empty"));
            }
            let work = u64::try_from(text.len())
                .unwrap_or(u64::MAX)
                .saturating_mul(u64::try_from(pattern.len()).unwrap_or(u64::MAX))
                .saturating_add(text.len() as u64)
                .saturating_add(pattern.len() as u64)
                .saturating_mul(2)
                .max(1);
            budget.charge(work, span)?;
            let count = text.matches(pattern).count();
            if count == 0 || pattern == replacement {
                return Ok(args[0].clone());
            }
            let removed = count
                .checked_mul(pattern.len())
                .ok_or_else(|| size_error(span))?;
            let inserted = count
                .checked_mul(replacement.len())
                .ok_or_else(|| size_error(span))?;
            let bytes = text
                .len()
                .checked_sub(removed)
                .and_then(|n| n.checked_add(inserted))
                .ok_or_else(|| size_error(span))?;
            let mut result = reserve(bytes, budget, span)?;
            let mut start = 0;
            for (offset, matched) in text.match_indices(pattern) {
                result.push_str(&text[start..offset]);
                result.push_str(replacement);
                start = offset + matched.len();
            }
            result.push_str(&text[start..]);
            result
        }
        Builtin::TextLower | Builtin::TextUpper => {
            // Unicode scalar case mappings may expand (e.g. U+0130 lowercases
            // to two scalars). Count exact UTF-8 bytes before reserving storage.
            // The two full traversals are charged independently of output bytes.
            budget.charge(
                u64::try_from(text.len())
                    .unwrap_or(u64::MAX)
                    .saturating_mul(2)
                    .max(1),
                span,
            )?;
            let mut bytes = 0usize;
            for scalar in text.chars() {
                let mapped_bytes: usize = if function == Builtin::TextLower {
                    scalar.to_lowercase().map(char::len_utf8).sum()
                } else {
                    scalar.to_uppercase().map(char::len_utf8).sum()
                };
                bytes = bytes
                    .checked_add(mapped_bytes)
                    .ok_or_else(|| size_error(span))?;
            }
            let mut result = reserve(bytes, budget, span)?;
            for scalar in text.chars() {
                if function == Builtin::TextLower {
                    result.extend(scalar.to_lowercase());
                } else {
                    result.extend(scalar.to_uppercase());
                }
            }
            result
        }
        Builtin::TextReverse => {
            // Reverse Unicode scalar values, not raw UTF-8 bytes or grapheme
            // clusters. The operation preserves each scalar's UTF-8 encoding.
            budget.charge(u64::try_from(text.len()).unwrap_or(u64::MAX).max(1), span)?;
            let mut result = reserve(text.len(), budget, span)?;
            for scalar in text.chars().rev() {
                result.push(scalar);
            }
            result
        }
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
