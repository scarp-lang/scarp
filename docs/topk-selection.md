# Deterministic Top-K score selection

> **Published in Scarp `v0.0.6-alpha.3`.** Download the verified
> [GitHub prerelease](https://github.com/scarp-lang/scarp/releases/tag/v0.0.6-alpha.3)
> for Windows, Linux and macOS. These functions are absent from earlier
> `v0.0.6-alpha.2` binaries.

Scarp's pure numerical built-ins now support selecting several candidates
from a finite numerical score vector, without a general tensor runtime or
random sampling. This is useful for simple classifiers, ranking systems,
and decoding candidate scores from inference pipelines.

| Built-in | Typed signature | Result |
| --- | --- | --- |
| `topk(scores, count)` | `vector, integer -> integers` | Selected **zero-based original indices** in descending score order |
| `topk_values(scores, count)` | `vector, integer -> vector` | Selected original finite **scores** in the same order |

## Example

```scarp
bind scores = [1.0, 5.0, 5.0, -2.0, 3.0];
print(topk(scores, 3));         // [integer: 1, 2, 4]
print(topk_values(scores, 3));  // [5.0, 5.0, 3.0]
print(topk(softmax(scores), 2)); // [integer: 1, 2]
```

Run the executable source example at
[`examples/topk_scores.srp`](../examples/topk_scores.srp).

For equal scores, the lower original index comes first. This includes
`-0.0` versus `0.0`: numerical zeros compare equal, but `topk_values`
preserves the original signed-zero bit pattern. Index results are
**relative to the visible vector**: `topk(window(scores, 1, 3), 2)`
returns indices within that three-element window.

A count of **zero is valid**, even for an empty input: `topk([], 0)`
returns an empty integer collection and `topk_values([], 0)` returns
an empty numerical vector. A count equal to the vector length sorts every
element. Negative counts or counts larger than the visible length yield
**E0033** at the count argument. Wrong types give **E0015**, wrong arity
**E0024**. The count is an integer, not a floating-point probability.

## Deterministic resource use

Both built-ins maintain only a `count`-element, worst-first heap instead
of allocating a full-length ranking array. The heap takes **8 × count**
bytes, charged to `--max-vector-bytes`; `topk_values` additionally
allocates **8 × count** bytes for the returned numerical vector.
`topk` reuses its heap buffer as the returned immutable integer collection.

Work is prepaid, independent of actual score values or tie outcomes:
`(length + count) × (floor(log2(count)) + 1)` steps for positive counts,
and **zero** for count zero. Work failure reports **E0017**; a cumulative
vector allocation shortfall reports **E0036**. Input literals also incur
their normal separate work and allocation charges. All checks run before
publishing output, and no partially selected values escape on failure.

The ranked candidates can be composed with
[temperature-controlled softmax](temperature-softmax.md) to change probability
concentration before selecting a fixed number of candidates.

For probability-mass rather than fixed-count selection, the independently
developed [Top-P nucleus selection](nucleus-selection.md) returns ranked
indices/values without randomness. It is **not included** in the frozen
v0.0.6-alpha.4 source candidate or previously published binaries.

The implementation uses only finite binary64 values, bounded Scarp vectors,
and safe Rust. It does not add a sampling/RNG API, top-p/nucleus decoding,
automatic differentiation, or a native object compiler.
