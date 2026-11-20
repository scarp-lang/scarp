//! Deterministic scalar bit kernels for the fixed-width Scarp `integer` type.
//!
//! These operations reinterpret the i64 value as an unsigned 64-bit pattern.
//! Returned patterns are converted back to i64 without overflow or coercion.
//! They are distinct from arbitrary-precision BigInt operations.

use super::*;

fn invalid_rotation(span: Span, count: i64) -> Diagnostic {
    Diagnostic::new(
        DiagnosticKind::InvalidShift,
        span,
        format!("bit rotation count {count} is outside the 0..=63 range"),
    )
    .with_help("use a nonnegative integer rotation count between 0 and 63")
}

pub(super) fn evaluate(
    function: Builtin,
    args: &[Value],
    budget: &mut Budget,
    span: Span,
    at: impl Fn(usize) -> Span,
) -> Result<Value, Diagnostic> {
    // The central builtin registry has already checked argument arity/types.
    // Charge exactly one deterministic operation before any computation.
    budget.charge(1, span)?;
    let pattern = args[0].require_integer(at(0))? as u64;
    let result = match function {
        Builtin::BitsCountOnes => i64::from(pattern.count_ones()),
        Builtin::BitsLeadingZeros => i64::from(pattern.leading_zeros()),
        Builtin::BitsTrailingZeros => i64::from(pattern.trailing_zeros()),
        Builtin::BitsReverse => pattern.reverse_bits() as i64,
        Builtin::BitsSwapBytes => pattern.swap_bytes() as i64,
        Builtin::BitsRotateLeft | Builtin::BitsRotateRight => {
            let count = args[1].require_integer(at(1))?;
            let shift = u32::try_from(count)
                .ok()
                .filter(|&count| count < u64::BITS)
                .ok_or_else(|| invalid_rotation(at(1), count))?;
            if function == Builtin::BitsRotateLeft {
                pattern.rotate_left(shift) as i64
            } else {
                pattern.rotate_right(shift) as i64
            }
        }
        _ => unreachable!("checked fixed-width bit dispatch"),
    };
    Ok(Value::Integer(result))
}
