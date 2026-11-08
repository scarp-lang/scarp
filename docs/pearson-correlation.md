# Pearson correlation — post-alpha.4 development feature

> **Unpublished feature.** This API is not in the published
> `v0.0.6-alpha.3` binaries or the frozen `v0.0.6-alpha.4` candidate.
> It may be considered for a separately reviewed later prerelease.

`pearson_correlation(left: vector, right: vector) -> float` computes the
**centered** Pearson product–moment correlation coefficient of two paired
finite numerical vectors, returning a finite result in [-1.0, 1.0].

```scarp
bind x = [1.0, 2.0, 3.0];
print(pearson_correlation(x, [10.0, 20.0, 30.0])); // 1.0
print(pearson_correlation(x, [30.0, 20.0, 10.0])); // -1.0
print(pearson_correlation([-1.0, 0.0, 1.0], [1.0, -2.0, 1.0])); // 0.0
```

This is **not** `cosine_similarity`: Pearson correlation subtracts each
axis's mean before measuring their relationship, so identical constant-offset
trends retain correlation 1 while orthogonal centered trends give 0.

## Domain and diagnostics

- Both vectors must have exactly the same visible length (E0032 at the
  right argument); at least **two observations** are required (E0034).
- Neither vector may be constant; zero variance is E0034 at the corresponding
  argument, even if the other vector varies.
- Input elements must be finite binary64 values, matching the existing
  `vector` type. Wrong types and wrong arity follow E0015/E0024.
- Immutable windows use only visible observations and are never modified.
- Failed runtime validation is transactional: no earlier program output
  becomes visible after a numerical failure.

## Stable numerical computation

Each axis subtracts its own first sample *before scaling*. If a raw
difference between finite values would overflow, the kernel subtracts the
halved original values instead. This preserves adjacent large representable
values when possible and handles opposite signed maximal inputs safely.

Each axis's deviations are divided by the maximum absolute deviation and
then centered using compensated mean reduction. Squared centered differences
and cross-products are accumulated with ordered Neumaier compensation.
The centered terms remain bounded by 2 in magnitude, avoiding overflow of
uncentered squared norms and products even when the original inputs are near
the largest finite float. The result is clipped to [-1,1] only to correct
rounding outside the mathematical interval.

This is still finite precision: it does not guarantee exact results for all
ill-conditioned data, and some tiny relative contributions may underflow.

## Resource contract

- Work is prepaid at **12 × N abstract kernel steps**, regardless of data,
  where N is the visible vector length (maximum 65,536).
- **Zero** new numerical vector payload bytes are allocated.
- There are no new language keywords, tensor engine, GPU backend, dynamic
  numerical dependencies or native-object lowering claims.
- Pure and deterministic on supported targets; requires Rust 1.74 or later.

The runnable source and exact output are
[`examples/pearson_correlation.srp`](../examples/pearson_correlation.srp)
and `examples/pearson_correlation.stdout`.
Rust regression tests include exact integer centered-moment oracles, extreme
finite inputs, visible windows, maximum length, error spans, transactionality
and exact budget boundaries.

```sh
cargo test --locked --test pearson_correlation
sh scripts/check.sh --offline
```
