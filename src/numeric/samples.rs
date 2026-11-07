//! Sampled calculus on finite, immutable vectors.
//!
//! Both kernels operate on the *visible* coordinate/value slices, never on the
//! backing allocation. They use no global state and have no side effects.

use super::*;

// Use the same checked interval arithmetic for the total and prefix
// integrals so their last values have consistent rounding and diagnostics.
fn interval_area(x: &[FiniteFloat], y: &[FiniteFloat], span: Span) -> Result<f64, Diagnostic> {
    let width = x[1].get() - x[0].get();
    let first = y[0].get();
    let second = y[1].get();
    // Multiplication order matters at binary64 extremes: large widths must
    // be halved before subnormal ordinates; tiny widths are multiplied after
    // halving the ordinate sum. Avoid overflow in a finite pair's sum.
    let sum = first + second;
    let area = if !sum.is_finite() {
        width * (first * 0.5 + second * 0.5)
    } else if width >= 1.0 {
        (width * 0.5) * sum
    } else {
        width * (sum * 0.5)
    };
    finite_number(area, span).map(FiniteFloat::get)
}

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
            let areas = xs
                .windows(2)
                .zip(ys.windows(2))
                .map(|(x, y)| interval_area(x, y, span));
            compensated(areas, span)
                .and_then(|result| finite_number(result, span))
                .map(Value::Float)
        }
        Builtin::SampleCumulativeArea => {
            // Every output i is the trapezoidal area from x[0] through x[i].
            // A fresh bounded vector is required, including the initial zero.
            let mut output = buffer(coordinates.len(), budget, span)?;
            output.push(finite_number(0.0, span)?);
            let mut sum = 0.0_f64;
            let mut correction = 0.0_f64;
            for (x, y) in xs.windows(2).zip(ys.windows(2)) {
                let area = interval_area(x, y, span)?;
                // Ordered Neumaier compensation, identical to the total-area
                // reduction. Fail closed if a running prefix is nonfinite.
                let next = finite_number(sum + area, span)?.get();
                let delta = if sum.abs() >= area.abs() {
                    (sum - next) + area
                } else {
                    (area - next) + sum
                };
                correction = finite_number(correction + delta, span)?.get();
                sum = next;
                output.push(finite_number(sum + correction, span)?);
            }
            vector(output, span)
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
