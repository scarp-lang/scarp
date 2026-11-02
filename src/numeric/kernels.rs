use super::*;

// Center before scaling to retain small spreads around large offsets. When a
// deviation overflows, scale the original values first instead. Neither path
// squares an unscaled deviation or allocates a temporary vector.
fn population_spread(values: &NumericVector, span: Span) -> Result<(f64, f64), Diagnostic> {
    let center = values.as_slice()[0].get();
    let floats = || values.as_slice().iter().map(|value| value.get());
    let scale = floats()
        .map(|value| (value - center).abs())
        .fold(0.0, f64::max);
    if scale == 0.0 {
        return Ok((0.0, 0.0));
    }
    let count = values.len() as f64;
    if scale.is_finite() {
        // Compute the mean of shifted, scaled deviations. This keeps adjacent
        // large input floats distinct even when their mean is unrepresentable.
        let residual = compensated(
            floats().map(|value| Ok(((value - center) / scale) / count)),
            span,
        )?;
        let squares = floats().map(|value| {
            let deviation = (value - center) / scale - residual;
            Ok(deviation * deviation)
        });
        return Ok((scale, (compensated(squares, span)? / count).clamp(0.0, 1.0)));
    }
    let scale = floats().map(f64::abs).fold(0.0, f64::max);
    let center = compensated(floats().map(|value| Ok((value / scale) / count)), span)?;
    let squares = floats().map(|value| {
        let deviation = value / scale - center;
        Ok(deviation * deviation)
    });
    Ok((scale, (compensated(squares, span)? / count).clamp(0.0, 1.0)))
}

pub(super) fn evaluate(
    function: Builtin,
    args: &[Value],
    budget: &mut Budget,
    span: Span,
    at: impl Fn(usize) -> Span,
) -> Result<Value, Diagnostic> {
    let values = args[0].require_vector(at(0))?;
    let statistics = matches!(function, Builtin::Variance | Builtin::StdDev);
    if statistics && values.is_empty() {
        return Err(domain(
            at(0),
            "population variance and standard deviation require a nonempty vector",
        ));
    }
    let work = if function == Builtin::Relu { 1 } else { 4 };
    budget.charge((values.len() as u64 * work).max(1), span)?;
    if statistics {
        let (scale, normalized) = population_spread(values, span)?;
        let result = if function == Builtin::StdDev {
            scale * normalized.sqrt()
        } else {
            (scale * normalized) * scale
        };
        return finite_number(result, span).map(Value::Float);
    }
    let mut result = buffer(values.len(), budget, span)?;
    for value in values.as_slice() {
        let value = value.get();
        let mapped = match function {
            // Choose an exponent with a nonpositive argument, so even the
            // largest finite logits cannot overflow exp into infinity.
            Builtin::Sigmoid if value >= 0.0 => 1.0 / (1.0 + (-value).exp()),
            Builtin::Sigmoid => {
                let exp = value.exp();
                exp / (1.0 + exp)
            }
            Builtin::Relu => {
                if value > 0.0 {
                    value
                } else {
                    0.0
                }
            }
            Builtin::Tanh => value.tanh(),
            _ => unreachable!("checked activation dispatch"),
        };
        result.push(finite_number(mapped, span)?);
    }
    vector(result, span)
}
