//! Explicit vector reverse-mode primitives and immutable gradient updates.
//! These functions do not record a graph or differentiate user functions.

use super::{buffer, compensated, domain, length_error, vector, Builtin};
use crate::{
    diagnostics::{Diagnostic, Span},
    limits::Budget,
    value::{finite_number, NumericVector, Value},
};

fn sigmoid(value: f64) -> f64 {
    if value >= 0.0 {
        1.0 / (1.0 + (-value).exp())
    } else {
        let e = value.exp();
        e / (1.0 + e)
    }
}

fn derivative(function: Builtin, x: f64) -> f64 {
    match function {
        Builtin::ReluBackward => {
            if x > 0.0 {
                1.0
            } else {
                0.0
            }
        }
        Builtin::SigmoidBackward => {
            // Unlike sigmoid(x) * (1 - sigmoid(x)), this retains the
            // positive tail after sigmoid(x) rounds to one.
            let e = (-x.abs()).exp();
            let denom = 1.0 + e;
            e / (denom * denom)
        }
        Builtin::TanhBackward => {
            // sech^2(x), with the factor four inside exp so subnormal
            // results do not vanish prematurely. An overflowing negative
            // exponent is benign and represents an underflow to zero.
            let exponent = -2.0 * x.abs();
            let denom = 1.0 + exponent.exp();
            (exponent + std::f64::consts::LN_2 * 2.0).exp() / (denom * denom)
        }
        Builtin::SoftplusBackward => sigmoid(x),
        _ => unreachable!("activation derivative dispatch"),
    }
}

fn softmax_backward(
    logits: &NumericVector,
    gradient: &NumericVector,
    budget: &mut Budget,
    span: Span,
) -> Result<Value, Diagnostic> {
    if logits.is_empty() {
        return Err(domain(span, "softmax_backward requires nonempty logits"));
    }
    let count = logits.len();
    // Maximum, weight sum, constant check, gradient maximum, weighted
    // reduction, optional scaled retry and final pass are all prepaid.
    budget.charge(12 * count as u64, span)?;
    let mut output = buffer(count, budget, span)?;
    let first = gradient.as_slice()[0].get();
    if gradient.as_slice().iter().all(|g| g.get() == first) {
        // The softmax Jacobian annihilates constant upstream gradients.
        output.resize(count, finite_number(0.0, span)?);
        return vector(output, span);
    }

    let maximum = logits
        .as_slice()
        .iter()
        .map(|v| v.get())
        .fold(f64::NEG_INFINITY, f64::max);
    let weight = |value: f64| (value - maximum).exp();
    let total = compensated(logits.as_slice().iter().map(|v| Ok(weight(v.get()))), span)?;
    let probability = |value: f64| weight(value) / total;
    let pairs = || logits.as_slice().iter().zip(gradient.as_slice());
    let center = match compensated(
        pairs().map(|(x, g)| Ok(probability(x.get()) * g.get())),
        span,
    ) {
        Ok(value) => value,
        Err(_) => {
            // Weighted sums are mathematically within the gradient range.
            // Scale when intermediate accumulation would overflow.
            let scale = gradient
                .as_slice()
                .iter()
                .map(|g| g.get().abs())
                .fold(0.0, f64::max);
            let normalized = compensated(
                pairs().map(|(x, g)| Ok(probability(x.get()) * (g.get() / scale))),
                span,
            )?;
            normalized.clamp(-1.0, 1.0) * scale
        }
    };
    for (x, g) in pairs() {
        let p = probability(x.get());
        let difference = g.get() - center;
        // A finite weighted difference can exist even when the unweighted
        // difference overflows; scale each term before subtracting then.
        let value = if difference.is_finite() {
            p * difference
        } else {
            p * g.get() - p * center
        };
        output.push(finite_number(value, span)?);
    }
    vector(output, span)
}

pub(super) fn evaluate(
    function: Builtin,
    args: &[Value],
    budget: &mut Budget,
    span: Span,
    at: impl Fn(usize) -> Span,
) -> Result<Value, Diagnostic> {
    let values = args[0].require_vector(at(0))?;
    let gradient = args[1].require_vector(at(1))?;
    if values.len() != gradient.len() {
        return Err(length_error(span));
    }
    if function == Builtin::SoftmaxBackward {
        return softmax_backward(values, gradient, budget, span);
    }
    let rate = if function == Builtin::SgdStep {
        let rate = args[2].require_float(at(2))?;
        if rate < 0.0 {
            return Err(domain(
                at(2),
                "sgd_step requires a nonnegative learning rate",
            ));
        }
        rate
    } else {
        0.0
    };
    let work = match function {
        Builtin::ReluBackward => 1,
        Builtin::SgdStep => 2,
        _ => 6,
    };
    budget.charge((work * values.len() as u64).max(1), span)?;
    let mut output = buffer(values.len(), budget, span)?;
    for (x, g) in values.as_slice().iter().zip(gradient.as_slice()) {
        let x = x.get();
        let g = g.get();
        let value = if function == Builtin::SgdStep {
            // Match separate scale/subtract semantics: the intermediate
            // product and final subtraction must each be finite.
            let delta = finite_number(rate * g, span)?.get();
            x - delta
        } else {
            g * derivative(function, x)
        };
        output.push(finite_number(value, span)?);
    }
    vector(output, span)
}
