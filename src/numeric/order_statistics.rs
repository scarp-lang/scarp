//! Bounded order statistics for finite immutable visible vector windows.
//!
//! Sorting a single charged copy makes the result independent of the input
//! layout, avoids mutating Arc-backed vectors, and keeps quantile calculation
//! deterministic across repeated calls. Only binary64 scalar outputs escape.

use super::*;

fn invalid_quantile(span: Span) -> Diagnostic {
    domain(
        span,
        "quantile fraction must be between 0.0 and 1.0 inclusive",
    )
    .with_help("pass a finite fraction in [0.0, 1.0]")
}

// Avoid overflow in the difference for opposite-signed finite endpoints.
// For like signs the difference itself is finite, avoiding overflow in the
// weighted sum. Exact endpoints retain their original signed-zero bits.
fn interpolate(lower: f64, upper: f64, fraction: f64) -> f64 {
    if fraction == 0.0 || lower.to_bits() == upper.to_bits() {
        lower
    } else if fraction == 1.0 {
        upper
    } else if lower.is_sign_negative() == upper.is_sign_negative() {
        lower + (upper - lower) * fraction
    } else {
        lower * (1.0 - fraction) + upper * fraction
    }
}

/// Median uses the same type-7 linear rank interpolation as quantile(0.5).
/// Work is prepaid as N * (floor(log2(N)) + 5) for a nonempty input.
/// One N-element f64 copy is charged to the vector allocation budget.
pub(super) fn evaluate(
    function: Builtin,
    args: &[Value],
    budget: &mut Budget,
    span: Span,
    at: impl Fn(usize) -> Span,
) -> Result<Value, Diagnostic> {
    let input = args[0].require_vector(at(0))?;
    let fraction = if function == Builtin::Quantile {
        let fraction = args[1].require_float(at(1))?;
        if !(0.0..=1.0).contains(&fraction) {
            return Err(invalid_quantile(at(1)));
        }
        fraction
    } else {
        0.5
    };
    if input.is_empty() {
        return Err(domain(
            at(0),
            "median and quantile require a nonempty vector",
        ));
    }
    let len = input.len();
    let levels = u64::from(usize::BITS - len.leading_zeros());
    budget.charge(len as u64 * (levels + 4), span)?;
    let mut ordered = buffer::<f64>(len, budget, span)?;
    ordered.extend(input.as_slice().iter().map(|value| value.get()));
    // total_cmp gives a reproducible order for signed zero (negative first)
    // without relying on host sort stability. Every input is finite.
    ordered.sort_unstable_by(f64::total_cmp);

    let position = (len - 1) as f64 * fraction;
    let lower = position.floor() as usize;
    let upper = (lower + 1).min(len - 1);
    let result = interpolate(ordered[lower], ordered[upper], position - lower as f64);
    finite_number(result, span).map(Value::Float)
}
