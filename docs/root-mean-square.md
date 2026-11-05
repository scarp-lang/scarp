# Root mean square (unreleased v0.0.6 work)

`rms(values: vector) -> float` measures the magnitude of a nonempty finite
binary64 vector: `sqrt(sum(x * x) / len(values))`. This is a pure positional
builtin, available in the development source and absent from published
v0.0.5-alpha.5 binaries. It composes with functions, defaults, contracts and
module calls. No implicit integer-collection conversion is performed.

```scarp
print(rms([2.0, -2.0]));                    // 2.0
print(rms([1e308, -1e308]) == 1e308);        // true
```

The implementation divides by the maximum absolute element, uses the existing
compensated scaled sum of squares, divides its square root by the square root
of the count, and only then restores the scale. It never forms the unscaled
squared sum or L2 norm. A final fraction is clamped to [0, 1] to preserve the
mathematical bound at binary64 rounding boundaries. Thus finite nonempty input
stays finite even if its norm would overflow. All-zero input returns +0.0;
a singleton returns its absolute magnitude, including subnormals. Underflow to
zero remains permitted. Results are rounded binary64 approximations; use a
tolerance for nontrivial values rather than relying on exact decimal text.

`rms([])` reports existing E0034 at the argument span, with help to provide a
nonempty vector. Argument type/arity errors retain E0015/E0024. Runtime failure
discards both pending program streams. `verify` and symbol/module inspection
type-check a call without evaluating its runtime numerical domain.

For n visible elements, the kernel prepays exactly **4n native work steps**.
Ordinary expression/statement costs are additional. RMS allocates **no new
numerical payload**, reads only a window's visible range and never modifies
input storage. Constructing that input still consumes the existing allocation
quota; n remains bounded by 65,536. These counters do not measure process RAM.

Rust hosts can call `numeric::evaluate(Builtin::Rms, ...)` or its allocation-
limited variant. The public `Builtin` enum gains `Rms`; exhaustive host matches
need an arm. The canonical signature is the same vector-to-float signature as
`norm`. Existing Value/ValueType layouts and diagnostic codes are unchanged.
The experimental scalar C11 emitter rejects RMS/vectors with E0049.

Run the [exact-output example](../examples/root_mean_square.srp) with
`scarp exec examples/root_mean_square.srp`. `tests/root_mean_square.rs` covers
embedding, windows, extreme magnitudes, subnormals, budgets, modules, defaults,
contracts, types and output atomicity. An independent standard-library oracle
uses 200-digit Decimal arithmetic on the exact binary64 inputs:

```sh
python scripts/test_rms.py target/debug/scarp
```
