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

// Euclid's algorithm is bounded on u64 magnitudes; charge each remainder.
fn gcd(mut a: u64, mut b: u64, budget: &mut Budget, span: Span) -> Result<u64, Diagnostic> {
    while b != 0 {
        budget.charge(1, span)?;
        (a, b) = (b, a % b);
    }
    Ok(a)
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
            // Preserve the binary work contract and charge every additional operand,
            // including zero inputs that need no remainder operations.
            budget.charge((args.len() - 2) as u64, span)?;
            let mut accumulator = left.unsigned_abs();
            if function == Builtin::IntegerGcd {
                // Intermediate magnitudes may be 2^63 even if the final GCD fits.
                for (index, value) in args.iter().enumerate().skip(1) {
                    let right = value.require_integer(at(index))?.unsigned_abs();
                    accumulator = gcd(accumulator, right, budget, span)?;
                }
                i64::try_from(accumulator).map_err(|_| overflow(span, "integer_gcd"))?
            } else {
                // A zero anywhere makes the exact LCM zero. Check before folding:
                // an overflowing prefix must not reject a valid final zero result.
                // Binary calls keep their historical remainder accounting.
                if args.len() > 2 && args.iter().any(|v| matches!(v, Value::Integer(0))) {
                    return Ok(Value::Integer(0));
                }
                for (index, value) in args.iter().enumerate().skip(1) {
                    let right = value.require_integer(at(index))?.unsigned_abs();
                    let divisor = gcd(accumulator, right, budget, span)?;
                    let multiple = match accumulator.checked_div(divisor) {
                        Some(quotient) => u128::from(quotient) * u128::from(right),
                        None => 0,
                    };
                    // With no zero remaining, LCM cannot decrease. Reject an
                    // unrepresentable prefix before multiplying again.
                    if multiple > i64::MAX as u128 {
                        return Err(overflow(span, "integer_lcm"));
                    }
                    accumulator = multiple as u64;
                }
                i64::try_from(accumulator).map_err(|_| overflow(span, "integer_lcm"))?
            }
        }
        _ => unreachable!("checked fixed-width integer scalar dispatch"),
    };
    Ok(Value::Integer(result))
}
