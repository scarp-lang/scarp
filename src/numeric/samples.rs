//! Sampled calculus on finite, immutable vectors.
//!
//! Both kernels operate on the *visible* coordinate/value slices, never on the
//! backing allocation. They use no global state and have no side effects.

use super::*;

/// A midpoint-free trapezoidal rule and interval (secant) derivatives.
///
/// Strictly increasing sample coordinates are intentional: they reject
/// ambiguous duplicate points and accidental reordering rather than silently
/// turning an integration into a signed walk through the data.
pub(super) fn evaluate(
    function: Builtin,
    args: &[Value],
    budget: &mut Budget,
    span: Span,
    at: impl Fn(usize) -> Span,
) -> Result<Value, Diagnostic> {
    let coordinates = args[0].require_vector(at(0))?;
    let readings = args[1].require_vector(at(1))?;
    if coordinates.len() != readings.len() {
        return Err(
            length_error(at(1)).with_help("provide one reading for every sample coordinate")
        );
    }
    if coordinates.len() < 2 {
        return Err(domain(
            at(0),
            "sampled calculus requires at least two coordinate/reading pairs",
        ));
    }

    let intervals = coordinates.len() - 1;
    // Prepay for both grid validation and computation. An invalid sample grid
    // must not allow an unbounded scan under an exhausted work budget.
    budget.charge(intervals as u64 * 4, span)?;

    let xs = coordinates.as_slice();
    let ys = readings.as_slice();
    // Check coordinates before any output allocation. Difference overflow is
    // not silently converted to an infinite step width.
    for pair in xs.windows(2) {
        if pair[1].get() <= pair[0].get() {
            return Err(domain(
                at(0),
                "sample coordinates must be strictly increasing",
            ));
        }
        finite_number(pair[1].get() - pair[0].get(), at(0))?;
    }

    match function {
        Builtin::SampleArea => {
            let areas = xs.windows(2).zip(ys.windows(2)).map(|(x, y)| {
                let width = x[1].get() - x[0].get();
                let first = y[0].get();
                let second = y[1].get();
                // Summing and then halving preserves tiny subnormals. When the
                // sum overflows, halving first avoids a spurious infinity.
                let sum = first + second;
                let average = if sum.is_finite() {
                    sum * 0.5
                } else {
                    first * 0.5 + second * 0.5
                };
                finite_number(width * average, span).map(FiniteFloat::get)
            });
            compensated(areas, span)
                .and_then(|result| finite_number(result, span))
                .map(Value::Float)
        }
        Builtin::SampleSlopes => {
            let mut slopes = buffer(intervals, budget, span)?;
            for (x, y) in xs.windows(2).zip(ys.windows(2)) {
                let width = x[1].get() - x[0].get();
                let difference = y[1].get() - y[0].get();
                // An extreme ordinate difference can overflow even though its
                // quotient by a large width is finite. Divide each ordinate
                // first in that case; final outputs must still be finite.
                let slope = if difference.is_finite() {
                    difference / width
                } else {
                    y[1].get() / width - y[0].get() / width
                };
                slopes.push(finite_number(slope, span)?);
            }
            vector(slopes, span)
        }
        _ => unreachable!("sampled calculus dispatch accepts only sample kernels"),
    }
}
