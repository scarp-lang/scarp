//! Bounded deterministic nucleus selection for finite probability vectors.
//!
//! Unlike random top-p sampling, these pure built-ins return the minimal
//! highest-probability prefix meeting a specified cumulative probability.
//! Sorting compares the original visible indices on ties, including +/-0.0.

use super::*;
use std::cmp::Ordering;

fn invalid_mass(span: Span) -> Diagnostic {
    Diagnostic::new(
        DiagnosticKind::NumericDomain,
        span,
        "top_p cumulative mass must be greater than 0.0 and at most 1.0",
    )
    .with_help("pass a finite float probability in (0.0, 1.0]")
}

fn invalid_probabilities(span: Span) -> Diagnostic {
    Diagnostic::new(
        DiagnosticKind::NumericDomain,
        span,
        "top_p requires a nonempty vector of nonnegative probabilities summing to 1.0",
    )
    .with_help("pass softmax(scores) or softmax_temp(scores, temperature) as the first argument")
}

/// Both built-ins share a stable ranking and quota model. The full bounded
/// index buffer is required to rank an arbitrary visible probability vector.
/// The selected prefix reuses that buffer for top_p; top_p_values allocates
/// one additional final score vector of exactly the selected length.
pub(super) fn evaluate(
    function: Builtin,
    args: &[Value],
    budget: &mut Budget,
    span: Span,
    argument_span: impl Fn(usize) -> Span,
) -> Result<Value, Diagnostic> {
    let input = args[0].require_vector(argument_span(0))?;
    let mass = args[1].require_float(argument_span(1))?;
    if mass <= 0.0 || mass > 1.0 {
        return Err(invalid_mass(argument_span(1)));
    }
    if input.is_empty() {
        return Err(invalid_probabilities(argument_span(0)));
    }
    let scores = input.as_slice();

    // Comparisons, sorting and summation have a finite input-independent
    // abstract cost. Include the validation pass before materialization.
    let levels = u64::from(usize::BITS - input.len().leading_zeros());
    budget.charge(input.len() as u64 * (levels + 4), span)?;

    // Do not silently normalize arbitrary weights or treat negative scores
    // as probabilities. Small floating-point summation error is tolerated;
    // the domain check never permits a materially unnormalized distribution.
    // A normalized probability cannot exceed one. Reject any such entry
    // before summation: even a vector of f64::MAX inputs must diagnose the
    // probability argument, never overflow into a generic float error.
    // Neumaier compensation is used without allocating temporary vectors.
    let mut sum: f64 = 0.0;
    let mut correction: f64 = 0.0;
    for score in scores {
        let value = score.get();
        if !(0.0..=1.0).contains(&value) {
            return Err(invalid_probabilities(argument_span(0)));
        }
        let next = sum + value;
        let delta = if sum >= value {
            (sum - next) + value
        } else {
            (value - next) + sum
        };
        correction += delta;
        sum = next;
    }
    let total = sum + correction;
    if (total - 1.0).abs() > 1.0e-12 {
        return Err(invalid_probabilities(argument_span(0)));
    }

    let mut ranked = buffer::<i64>(input.len(), budget, span)?;
    ranked.extend((0..scores.len()).map(|index| index as i64));
    ranked.sort_unstable_by(|&left, &right| {
        scores[right as usize]
            .get()
            .partial_cmp(&scores[left as usize].get())
            .unwrap_or(Ordering::Equal)
            .then_with(|| left.cmp(&right))
    });

    let mut running = 0.0;
    let mut correction = 0.0;
    let mut selected = 0usize;
    for &index in &ranked {
        let probability = scores[index as usize].get();
        let next = running + probability;
        let delta = if running.abs() >= probability.abs() {
            (running - next) + probability
        } else {
            (probability - next) + running
        };
        correction += delta;
        running = next;
        selected += 1;
        // An exactly full nucleus deliberately includes every visible
        // candidate, even entries assigned a probability of zero.
        if mass < 1.0 && running + correction >= mass {
            break;
        }
    }

    match function {
        Builtin::TopP => {
            ranked.truncate(selected);
            crate::value::IntegerVector::from_values(ranked)
                .map(Value::Integers)
                .ok_or_else(|| length_error(span))
        }
        Builtin::TopPValues => {
            let mut values = buffer::<FiniteFloat>(selected, budget, span)?;
            for index in ranked.into_iter().take(selected) {
                values.push(scores[index as usize]);
            }
            vector(values, span)
        }
        _ => unreachable!("nucleus dispatch accepts only Top-P built-ins"),
    }
}
