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
            } else {
                // Dividing first avoids intermediate signed overflow. A zero
                // GCD means both inputs are zero, so their LCM is also zero.
                // Magnitudes are at most 2^63: the product fits in u128.
                let multiple = match left.unsigned_abs().checked_div(a) {
                    Some(quotient) => u128::from(quotient) * u128::from(right.unsigned_abs()),
                    None => 0,
                };
                i64::try_from(multiple).map_err(|_| overflow(span, "integer_lcm"))?
            }
        }
        _ => unreachable!("checked fixed-width integer scalar dispatch"),
    };
    Ok(Value::Integer(result))
}
