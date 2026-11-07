//! Deterministic, bounded Top-K decoding for finite model scores.
//!
//! A worst-first heap retains only K candidate indices. Ties use the
//! original zero-based index, not host sort implementation details; signed
//! zero compares equal, and the retained float keeps its original bits.

use super::*;
use std::cmp::Ordering;

fn better(scores: &[FiniteFloat], candidate: i64, incumbent: i64) -> bool {
    let candidate_value = scores[candidate as usize].get();
    let incumbent_value = scores[incumbent as usize].get();
    candidate_value > incumbent_value
        || (candidate_value == incumbent_value && candidate < incumbent)
}

fn worse(scores: &[FiniteFloat], candidate: i64, incumbent: i64) -> bool {
    better(scores, incumbent, candidate)
}

fn sift_up(heap: &mut [i64], scores: &[FiniteFloat]) {
    let mut child = heap.len() - 1;
    while child > 0 {
        let parent = (child - 1) / 2;
        if !worse(scores, heap[child], heap[parent]) {
            break;
        }
        heap.swap(child, parent);
        child = parent;
    }
}

fn sift_down(heap: &mut [i64], scores: &[FiniteFloat]) {
    let mut parent = 0;
    loop {
        let left = parent * 2 + 1;
        if left >= heap.len() {
            break;
        }
        let right = left + 1;
        let worst = if right < heap.len() && worse(scores, heap[right], heap[left]) {
            right
        } else {
            left
        };
        if !worse(scores, heap[worst], heap[parent]) {
            break;
        }
        heap.swap(parent, worst);
        parent = worst;
    }
}

fn count_error(span: Span, count: i64, length: usize) -> Diagnostic {
    Diagnostic::new(
        DiagnosticKind::VectorIndex,
        span,
        format!("topk count {count} is outside 0..={length}"),
    )
    .with_help("choose a nonnegative count no greater than the visible vector length")
}

/// Both operations share the same stable ranking order, including original
/// signed-zero payloads. No intermediate full-length ranking array is built.
pub(super) fn evaluate(
    function: Builtin,
    args: &[Value],
    budget: &mut Budget,
    span: Span,
    argument_span: impl Fn(usize) -> Span,
) -> Result<Value, Diagnostic> {
    let input = args[0].require_vector(argument_span(0))?;
    let requested = args[1].require_integer(argument_span(1))?;
    let count = usize::try_from(requested)
        .ok()
        .filter(|count| *count <= input.len())
        .ok_or_else(|| count_error(argument_span(1), requested, input.len()))?;
    let scores = input.as_slice();

    // Bounded abstract cost for the O(n log(k+1) + k log(k+1))
    // comparison/heap algorithm, independent of the actual data or ties.
    // A zero result requires neither a scan nor numerical element storage.
    let levels = u64::from(usize::BITS - count.leading_zeros());
    let work = (input.len() as u64 + count as u64) * levels;
    budget.charge(work, span)?;

    // This is the only auxiliary heap. Each element is the final integer
    // index, so topk does not allocate and copy a second index buffer.
    let mut heap = buffer::<i64>(count, budget, span)?;
    for index in 0..scores.len() {
        if heap.len() < count {
            heap.push(index as i64);
            sift_up(&mut heap, scores);
        } else if count > 0 && better(scores, index as i64, heap[0]) {
            heap[0] = index as i64;
            sift_down(&mut heap, scores);
        }
    }

    // Always rank greatest score first, and earliest original index first
    // for equal values. Numerical equality intentionally merges +/-0.0.
    heap.sort_unstable_by(|&left, &right| {
        scores[right as usize]
            .get()
            .partial_cmp(&scores[left as usize].get())
            .unwrap_or(Ordering::Equal)
            .then_with(|| left.cmp(&right))
    });
    match function {
        Builtin::TopK => crate::value::IntegerVector::from_values(heap)
            .map(Value::Integers)
            .ok_or_else(|| length_error(span)),
        Builtin::TopKValues => {
            let mut values = buffer::<FiniteFloat>(count, budget, span)?;
            for index in heap {
                values.push(scores[index as usize]);
            }
            vector(values, span)
        }
        _ => unreachable!("topk dispatch accepts only the two Top-K built-ins"),
    }
}
