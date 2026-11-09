//! Explicit scalar/text boundaries. Parsing never introduces numeric coercion.
use super::*;
use std::fmt::{self, Write};

// Finite scalar formatting is bounded independently of input text length.
// Use stack storage before charging/reserving the exact new string payload.
const FORMAT_WORK: u64 = 64;
struct ScalarBuffer {
    bytes: [u8; 64],
    len: usize,
}
impl fmt::Write for ScalarBuffer {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        let end = self.len.checked_add(text.len()).ok_or(fmt::Error)?;
        let destination = self.bytes.get_mut(self.len..end).ok_or(fmt::Error)?;
        destination.copy_from_slice(text.as_bytes());
        self.len = end;
        Ok(())
    }
}

fn invalid(span: Span, message: &str) -> Diagnostic {
    Diagnostic::new(DiagnosticKind::NumericConversion, span, message)
        .with_help("use ASCII decimal text without whitespace, separators or radix prefixes; trim explicitly with text_trim; floats must remain finite")
}
/// Decimal grammar shared in meaning with source literals, with an optional
/// constructor sign and integer-shaped input accepted by explicit float parse.
/// No temporary tokens or source program are allocated/evaluated here.
fn decimal(text: &str, float: bool) -> bool {
    let bytes = text.as_bytes();
    let mut i = usize::from(matches!(bytes.first(), Some(b'+' | b'-')));
    let digits = |index: &mut usize| {
        let start = *index;
        while bytes.get(*index).is_some_and(u8::is_ascii_digit) {
            *index += 1;
        }
        *index > start
    };
    if !digits(&mut i) {
        return false;
    }
    if float && bytes.get(i) == Some(&b'.') {
        i += 1;
        if !digits(&mut i) {
            return false;
        }
    }
    if float && matches!(bytes.get(i), Some(b'e' | b'E')) {
        i += 1;
        if matches!(bytes.get(i), Some(b'+' | b'-')) {
            i += 1;
        }
        if !digits(&mut i) {
            return false;
        }
    }
    i == bytes.len()
}

pub(super) fn evaluate(
    function: Builtin,
    args: &[Value],
    budget: &mut Budget,
    span: Span,
    argument_span: impl Fn(usize) -> Span,
) -> Result<Value, Diagnostic> {
    if matches!(function, Builtin::TextToInteger | Builtin::TextToFloat) {
        let text = args[0].as_str().expect("typed string argument");
        budget.charge(u64::try_from(text.len()).unwrap_or(u64::MAX).max(1), span)?;
        let site = argument_span(0);
        if !decimal(text, function == Builtin::TextToFloat) {
            return Err(invalid(site, "invalid decimal text for numeric conversion"));
        }
        return if function == Builtin::TextToInteger {
            text.parse::<i64>().map(Value::Integer).map_err(|_| {
                invalid(
                    site,
                    "decimal text is outside the signed 64-bit integer range",
                )
            })
        } else {
            text.parse::<f64>()
                .ok()
                .and_then(FiniteFloat::new)
                .map(Value::Float)
                .ok_or_else(|| invalid(site, "decimal text is outside the finite binary64 range"))
        };
    }
    budget.charge(FORMAT_WORK, span)?;
    let mut buffer = ScalarBuffer {
        bytes: [0; 64],
        len: 0,
    };
    let result = match function {
        Builtin::IntegerToText => write!(
            &mut buffer,
            "{}",
            args[0].require_integer(argument_span(0))?
        ),
        Builtin::FloatToText => write!(
            &mut buffer,
            "{:?}",
            args[0].require_float(argument_span(0))?
        ),
        _ => unreachable!("checked scalar/text conversion dispatch"),
    };
    result.map_err(|_| {
        crate::limits::text_construction(span, "scalar formatting exceeded bounded scratch storage")
    })?;
    let mut text = text::reserve(buffer.len, budget, span)?;
    // Both formatters produce ASCII; Rust's formatter supplies valid UTF-8.
    text.push_str(std::str::from_utf8(&buffer.bytes[..buffer.len]).expect("formatted UTF-8"));
    Ok(Value::from(text))
}
