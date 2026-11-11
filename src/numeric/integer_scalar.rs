//! Checked exact scalar arithmetic for the signed 64-bit Scarp `integer` type.
//! All magnitudes use unsigned arithmetic so i64::MIN is never negated.

use super::*;

fn overflow(span: Span, operation: &str) -> Diagnostic {
    Diagnostic::new(
        DiagnosticKind::ArithmeticOverflow,
        span,
        format!("{operation} result exceeds the signed 64-bit integer range"),
    )
    .with_help(
        "use an operation whose exact result fits -9223372036854775808..=9223372036854775807",
    )
}

pub(super) fn evaluate(
    function: Builtin,
    args: &[Value],
    budget: &mut Budget,
    span: Span,
    at: impl Fn(usize) -> Span,
) -> Result<Value, Diagnostic> {
    // All paths pay for the invocation, even gcd(0, 0) and lcm(0, x).
    budget.charge(1, span)?;
    let left = args[0].require_integer(at(0))?;
    let result = match function {
        Builtin::IntegerAbs => left
            .checked_abs()
            .ok_or_else(|| overflow(at(0), "integer_abs"))?,
        Builtin::IntegerSign => left.signum(),
        Builtin::IntegerGcd | Builtin::IntegerLcm => {
            let right = args[1].require_integer(at(1))?;
            let (mut a, mut b) = (left.unsigned_abs(), right.unsigned_abs());

            // Euclid's algorithm has a logarithmic bound on u64 magnitudes.
            // Charge each remainder before performing it; no allocation occurs.
            while b != 0 {
                budget.charge(1, span)?;
                let remainder = a % b;
                a = b;
                b = remainder;
            }

            if function == Builtin::IntegerGcd {
                i64::try_from(a).map_err(|_| overflow(span, "integer_gcd"))?
            } else if a == 0 {
                // lcm(0, 0) and lcm(0, x) are zero.
                0
            } else {
                // Dividing first avoids the intermediate signed overflow.
                // Both magnitudes are at most 2^63, so this u128 product
                // is at most 2^126 and cannot overflow.
                let multiple =
                    u128::from(left.unsigned_abs() / a) * u128::from(right.unsigned_abs());
                i64::try_from(multiple).map_err(|_| overflow(span, "integer_lcm"))?
            }
        }
        _ => unreachable!("checked fixed-width integer scalar dispatch"),
    };
    Ok(Value::Integer(result))
}
