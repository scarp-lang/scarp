//! Stable finite-temperature score normalization for model inference.
//!
//! The score maximum is subtracted before scaling by temperature. If that
//! subtraction overflows, scaling the two finite operands first recovers
//! representable differences for large temperatures.

use super::*;

fn shifted(value: f64, maximum: f64, temperature: f64) -> f64 {
    let difference = value - maximum;
    if difference.is_finite() {
        difference / temperature
    } else {
        // Opposite-sign binary64 extremes overflow their difference, even
        // though the temperature-scaled difference may be representable.
        value / temperature - maximum / temperature
    }
}

fn exponential(value: f64) -> f64 {
    if value.is_finite() {
        value.exp()
    } else {
        // A nonfinite shifted score can only be negative infinity:
        // source scores and temperature are finite and temperature > 0.
        0.0
    }
}

pub(super) fn evaluate(
    function: Builtin,
    args: &[Value],
    budget: &mut Budget,
    span: Span,
    argument_span: impl Fn(usize) -> Span,
) -> Result<Value, Diagnostic> {
    let scores = args[0].require_vector(argument_span(0))?;
    let temperature = args[1].require_float(argument_span(1))?;
    if temperature <= 0.0 {
        return Err(
            domain(argument_span(1), "temperature must be greater than zero")
                .with_help("pass a finite, positive float temperature such as 1.0 or 0.5"),
        );
    }
    if scores.is_empty() {
        return Err(domain(
            argument_span(0),
            "temperature normalization requires a nonempty vector",
        ));
    }

    // Two bounded scans and one result vector: charge deterministically
    // before starting work and before any result payload is allocated.
    budget.charge(scores.len() as u64 * 4, span)?;
    let maximum = scores
        .as_slice()
        .iter()
        .map(|value| value.get())
        .fold(f64::NEG_INFINITY, f64::max);
    let normalizer = compensated(
        scores.as_slice().iter().map(|value| {
            finite_number(
                exponential(shifted(value.get(), maximum, temperature)),
                span,
            )
            .map(FiniteFloat::get)
        }),
        span,
    )?;
    let log_normalizer = normalizer.ln();
    let mut output = buffer::<FiniteFloat>(scores.len(), budget, span)?;

    for score in scores.as_slice() {
        let difference = shifted(score.get(), maximum, temperature);
        let result = match function {
            Builtin::SoftmaxTemp => exponential(difference) / normalizer,
            Builtin::LogSoftmaxTemp => difference - log_normalizer,
            _ => unreachable!("temperature dispatch accepts only normalization built-ins"),
        };
        output.push(finite_number(result, span)?);
    }
    vector(output, span)
}
