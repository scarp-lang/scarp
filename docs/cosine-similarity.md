# Cosine similarity (development branch)

> **Unreleased development feature.** This addition is proposed for a future
> Scarp release and is **not** included in the published `v0.0.6-alpha.3`
> binaries. Do not claim release availability until the new PR is merged and
> a separately authorized release is published.

`cosine_similarity(left, right) -> float` measures the orientation of two
finite vectors, independently of their lengths. It is a useful foundational
operation for embedding comparisons and scientific analysis, but **not** a
machine-learning model or an approximate-nearest-neighbor search engine.

```scarp
print(cosine_similarity([3.0, 4.0], [6.0, 8.0])); // 1.0
print(cosine_similarity([1.0, 0.0], [0.0, 1.0])); // 0.0
print(cosine_similarity([3.0], [-4.0]));          // -1.0
```

It computes the dot product divided by the product of Euclidean lengths,
with a result in `[-1.0, 1.0]`. Both arguments are typed `vector`; neither
argument is mutated. Immutable `window(...)` inputs operate only on their
visible elements. There is no hidden normalization in an output buffer.

## Domain, numerical behavior, and limits

- Both vectors must have **equal, nonzero length**. Length mismatch is E0032;
  empty input is E0034.
- Neither vector may be all zero, including signed zero. A zero operand is
  rejected with E0034 at its argument source span, not silently treated as
  having zero similarity.
- Finite inputs stay finite even when their raw dot product or raw squared
  norms would overflow. The kernel scales each vector by its own maximum
  absolute element, reduces normalized squares and products with compensated
  summation, and divides by the product of normalized lengths.
- The result is rounded binary64, **not an exact real-number answer**. Tiny
  contributions may underflow in normalized arithmetic. Roundoff marginally
  outside the mathematical range is clipped to `[-1, 1]`.
- The runtime prepays **6 × N abstract numerical steps** for N elements and
  allocates **zero new numerical payload bytes**. This excludes language-level
  parsing, dispatch and argument construction. N remains bounded at 65,536.
- A failing call publishes no partial output. All operations remain pure,
  deterministic, CPU-only, and compatible with Rust 1.74.

The runnable fixture is `examples/cosine_similarity.srp` and its exact
`.stdout` file. The Rust tests cover geometry, independent scale extremes,
subnormals, malformed arguments, resource limits, visible windows, the maximum
length, and a deterministic integer-dot oracle.

```sh
cargo test --locked --test cosine_similarity
sh scripts/check.sh --offline
```

This work does not introduce GPU support, tensors, vector indices/search,
approximate nearest neighbors, a native object writer, or new CLI commands.
