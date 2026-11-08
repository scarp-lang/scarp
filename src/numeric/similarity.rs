//! Deterministic, bounded cosine similarity on finite, visible vectors.
//!
//! Scale each operand independently before multiplying, avoiding overflow
//! from large finite inputs and underflow from small but nonzero vectors.
//! No temporary numerical vector or global state is required.

use super::*;

pub(super) fn evaluate(
    args: &[Value],
    budget: &mut Budget,
    span: Span,
    at: impl Fn(usize) -> Span,
) -> Result<Value, Diagnostic> {
    let left = args[0].require_vector(at(0))?;
    let right = args[1].require_vector(at(1))?;
    if left.len() != right.len() {
        return Err(
            length_error(at(1)).with_help("provide vectors of equal visible length")
        );
    }
    if left.is_empty() {
        return Err(domain(at(0), "cosine_similarity requires nonempty vectors"));
    }

    // Two maximum-magnitude scans, two compensated squared-norm scans and
    // one compensated normalized dot scan, with a conservative spare pass.
    // All work is paid before scanning, independent of values or early errors.
    budget.charge(left.len() as u64 * 6, span)?;

    let (left_scale, left_norm) = scaled_l2(left, span)?;
    if left_scale == 0.0 {
        return Err(domain(at(0), "cosine_similarity requires a nonzero left vector"));
    }
    let (right_scale, right_norm) = scaled_l2(right, span)?;
    if right_scale == 0.0 {
        return Err(domain(at(1), "cosine_similarity requires a nonzero right vector"));
    }

    // Each product is in [-1,1] and the compensated sum is at most N in
    // magnitude, even when the unscaled dot product cannot be represented.
    let dot = compensated(
        left.as_slice().iter().zip(right.as_slice()).map(|(a, b)| {
            Ok((a.get() / left_scale) * (b.get() / right_scale))
        }),
        span,
    )?;
    // The normalized lengths are <= sqrt(N); their product is bounded by N.
    // Clip only floating-point roundoff beyond the exact mathematical range.
    let result = (dot / (left_norm * right_norm)).clamp(-1.0, 1.0);
    finite_number(result, span).map(Value::Float)
}
