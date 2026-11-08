# Deterministic nucleus (Top-P) candidate selection

> **Post-alpha.4 development only.** This feature is not in the published
> `v0.0.6-alpha.3` binaries or in the frozen `v0.0.6-alpha.4` source
> candidate (commit `f04be0758cbce405ddcdb229699b0ff83c09d207`).
> This work must not be merged into that release source or advertised as
> shipped. It requires new exact-source release review.

Nucleus selection identifies the smallest descending-probability prefix
that reaches a requested cumulative mass. Scarp exposes the selected
candidates **without sampling or any randomness**.

| Built-in | Type | Result |
| --- | --- | --- |
| `top_p(probabilities, mass)` | `vector, float -> integers` | Indices in descending probability order |
| `top_p_values(probabilities, mass)` | `vector, float -> vector` | Original probability values in the same order |

## Example

```scarp
bind p = [0.5, 0.25, 0.25];
print(top_p(p, 0.75));         // [integer: 0, 1]
print(top_p_values(p, 0.75));  // [0.5, 0.25]
print(top_p(p, 1.0));          // [integer: 0, 1, 2]

bind logits = [1.0, 4.0, 4.0, 2.0];
print(top_p(softmax_temp(logits, 0.5), 0.9));
// [integer: 1, 2]
```

Source and exact output are in
[`examples/nucleus_selection.srp`](../examples/nucleus_selection.srp).

## Determinism, validation, and edge cases

- Input must be **nonempty, finite, nonnegative probabilities summing to 1.0**.
  Compensated summation permits at most `1e-12` absolute numerical error
  around 1.0; arbitrary unnormalized scores are rejected, not silently
  rescaled. Compute probabilities with `softmax` or `softmax_temp` first.
- Cumulative mass must be a finite float in **(0, 1]**.
  For any value less than 1.0, at least one candidate is returned, and
  selection stops as soon as the prefix reaches the threshold.
- A threshold of exactly **1.0 retains every visible candidate**, including
  zero-probability entries. Equal probabilities choose the lowest original
  zero-based index first; `+0.0` and `-0.0` compare equal. The latter's
  original signed-zero bits are preserved in `top_p_values`.
- Immutable `window(...)` inputs yield indices **relative to the window**,
  not its original backing buffer; the caller's input is never modified.
- Empty/negative/unnormalized input gives **E0034** at the vector argument;
  zero, negative or greater-than-one threshold gives **E0034** at the mass
  argument. Wrong types yield **E0015**, wrong arity **E0024**.

For distributions with one probability greater than 1.0, rejection occurs
immediately as **E0034** at the input vector argument; this includes huge
finite values and avoids an incidental floating-point overflow diagnostic.

## Bounded execution and allocation

Because the requested result length depends on the input probability
distribution, both operations rank the entire visible input using a
bounded **N-index** array (`N <= 65536`). Sorting is worst-case
`O(N log N)`, with stable tie behavior defined by the original index.
The selection scan uses ordered compensated accumulation.

A call **prepays** `N × (floor(log2(N)) + 5)` abstract work steps before
ranking; an exhausted work budget raises **E0017**. The index array costs
`8N` newly charged vector payload bytes; `top_p_values` additionally
allocates `8K` bytes for its `K` chosen probabilities. An exhausted
cumulative vector allocation budget raises **E0036**. `top_p` reuses
its rank buffer for the immutable integer result, with no secondary output
allocation. The input vector's previously incurred cost is separate.

Failures do not commit partial outputs. This API does not add random
sampling, an RNG/seed, top-p temperature control (compose with
`softmax_temp` instead), an AI runtime or a public native compiler
CLI command. Scarp's independent experimental ELF64 object writer
remains restricted to a tiny integer-expression subset.
