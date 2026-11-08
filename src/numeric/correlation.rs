//! Pearson correlation for immutable finite visible vectors.
//!
//! Subtract the first observed value before scaling, preserving small spreads
//! around huge offsets. If a finite subtraction overflows, subtract halved
//! inputs instead. Each axis is centered and scaled independently. Squared
//! deviations/products are bounded by 4, and all reductions are compensated.
//! There is no allocation of a numerical vector or global state.

use super::*;

struct Centered<'a> {
    values: &'a [FiniteFloat],
    anchor: f64,
    half: bool,
    scale: f64,
    mean: f64,
}

impl<'a> Centered<'a> {
    fn new(values: &'a [FiniteFloat], span: Span) -> Result<Option<Self>, Diagnostic> {
        let anchor = values[0].get();
        let half = values
            .iter()
            .any(|value| (value.get() - anchor).is_infinite());
        let deviation = |value: f64| {
            if half {
                value * 0.5 - anchor * 0.5
            } else {
                value - anchor
            }
        };
        let scale = values
            .iter()
            .map(|value| deviation(value.get()).abs())
            .fold(0.0_f64, f64::max);
        if scale == 0.0 {
            return Ok(None);
        }
        let count = values.len() as f64;
        // Dividing each normalized term before summing prevents spurious
        // intermediate overflow and preserves small cancellation terms.
        let mean = compensated(
            values
                .iter()
                .map(|value| Ok((deviation(value.get()) / scale) / count)),
            span,
        )?;
        Ok(Some(Self {
            values,
            anchor,
            half,
            scale,
            mean,
        }))
    }

    fn centered(&self, value: FiniteFloat) -> f64 {
        let difference = if self.half {
            value.get() * 0.5 - self.anchor * 0.5
        } else {
            value.get() - self.anchor
        };
        difference / self.scale - self.mean
    }

    fn squares(&self, span: Span) -> Result<f64, Diagnostic> {
        compensated(
            self.values.iter().map(|value| {
                let centered = self.centered(*value);
                Ok(centered * centered)
            }),
            span,
        )
    }
}

pub(super) fn evaluate(
    args: &[Value],
    budget: &mut Budget,
    span: Span,
    at: impl Fn(usize) -> Span,
) -> Result<Value, Diagnostic> {
    let left = args[0].require_vector(at(0))?;
    let right = args[1].require_vector(at(1))?;
    if left.len() != right.len() {
        return Err(length_error(at(1)).with_help("supply equally long visible vectors"));
    }
    if left.len() < 2 {
        return Err(domain(
            at(0),
            "pearson_correlation requires at least two paired observations",
        ));
    }
    // A conservative fixed charge for overflow detection, independent pivot
    // scans, normalized means, centered squared norms and cross-products.
    // No allocations; early domain failures cannot leak partial stdout.
    budget.charge(left.len() as u64 * 12, span)?;

    let x = Centered::new(left.as_slice(), span)?.ok_or_else(|| {
        domain(
            at(0),
            "pearson_correlation requires nonconstant left observations",
        )
    })?;
    let y = Centered::new(right.as_slice(), span)?.ok_or_else(|| {
        domain(
            at(1),
            "pearson_correlation requires nonconstant right observations",
        )
    })?;

    let xx = x.squares(span)?;
    let yy = y.squares(span)?;
    if xx == 0.0 {
        return Err(domain(
            at(0),
            "pearson_correlation requires nonconstant left observations",
        ));
    }
    if yy == 0.0 {
        return Err(domain(
            at(1),
            "pearson_correlation requires nonconstant right observations",
        ));
    }

    // Centered values are in [-2, 2], so the numerator and both squared
    // norms are finite under the fixed 65536-element source-vector limit.
    let xy = compensated(
        x.values
            .iter()
            .zip(y.values)
            .map(|(left, right)| Ok(x.centered(*left) * y.centered(*right))),
        span,
    )?;
    let result = (xy / (xx.sqrt() * yy.sqrt())).clamp(-1.0, 1.0);
    finite_number(result, span).map(Value::Float)
}
