# Stable temperature-controlled softmax

> **Development main only.** These functions are not part of the published
> `v0.0.6-alpha.2` prerelease. Build Scarp from current source to use them.

Two pure, fully typed built-ins extend Scarp's finite numerical vector support
for model inference and log-probability evaluation:

| Built-in | Signature | Meaning |
| --- | --- | --- |
| `softmax_temp(scores, temperature)` | `vector, float -> vector` | Probabilities proportional to `exp(score / temperature)` |
| `logsoftmax_temp(scores, temperature)` | `vector, float -> vector` | Natural-log probabilities for the same distribution |

`temperature` is a finite **strictly positive float**. At `1.0`, these
correspond to existing `softmax` and `logsoftmax`. Smaller positive values
concentrate probability on higher scores; larger values flatten the
distribution. These are deterministic transformations, **not** RNG sampling
or a model runtime. A separate future-development
[Top-P nucleus selection API](nucleus-selection.md) can rank candidates
by cumulative probability but is not part of the pinned alpha.3 release.

## Runnable example

```scarp
print(softmax_temp([2.0, 2.0], 0.5)); // [0.5, 0.5]
print(logsoftmax_temp([5.0], 2.0)); // [0.0]

bind logits = [1.0, 4.0, 4.0, 2.0];
print(topk(softmax_temp(logits, 0.5), 3)); // [integer: 1, 2, 3]
```

The complete [example](../examples/temperature_softmax.srp) is included
in both Unix and Windows compiler checks. It composes naturally with
[deterministic Top-K ranking](topk-selection.md).

## Numerical stability and diagnostics

The implementation subtracts the greatest original score **before**
temperature scaling, and uses compensated accumulation for the exponential
normalizer. When subtraction of opposite-sign binary64 extremes would overflow,
it instead scales the finite operands before subtracting them. This can recover
representable differences for large temperatures and avoids treating a
legitimate finite probability as zero.

Only representable finite values are stored. Very low temperatures can
legitimately underflow some *probabilities* to zero; if a corresponding
log-probability cannot be represented as finite binary64, `logsoftmax_temp`
returns **E0031** instead of storing infinity. Empty score vectors and
nonpositive temperatures return **E0034**, with invalid temperatures attributed
to the source's temperature argument. Type mismatches return **E0015** and
incorrect arity returns **E0024**.

Each invocation prepays exactly **4 × vector length** numerical work steps
(E0017 if exceeded), and allocates exactly **8 × length bytes** for a new
immutable result vector (cumulative vector budget E0036 if exceeded).
Input vectors are never modified; immutable visible windows work without
copying their backing storage. All output remains transactional on failure.

No new dependencies, public native compilation API, package-release tag or
language-syntax changes are part of this development feature.
