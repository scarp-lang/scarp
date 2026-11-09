# Sampled calculus — published in v0.0.6-alpha.4

**Status:** `sample_area`, `sample_slopes` and `sample_cumulative_area` are
available in the [published experimental Scarp v0.0.6-alpha.4 prerelease](https://github.com/scarp-lang/scarp/releases/tag/v0.0.6-alpha.4),
from exact source `f04be0758cbce405ddcdb229699b0ff83c09d207` (8 November 2026).
The changes originally arrived via
[compiler PR #153](https://github.com/scarp-lang/scarp/pull/153) and
[compiler PR #155](https://github.com/scarp-lang/scarp/pull/155) after alpha.3.
**The earlier alpha.3 binaries do not contain these functions.**
The features are bounded numerical kernels, not symbolic calculus or a general library.

Scarp currently represents finite binary64 measurements as immutable
one-dimensional `vector` values. Until named standard-library namespaces are
supported, the numerical runtime exposes three pure, typed calls:

| Function | Type | Meaning |
| --- | --- | --- |
| `sample_area(x, y)` | `(vector, vector) -> float` | Composite trapezoidal integral of values `y` sampled at coordinates `x` |
| `sample_slopes(x, y)` | `(vector, vector) -> vector` | Secant derivative on each consecutive interval |
| `sample_cumulative_area(x, y)` | `(vector, vector) -> vector` | Trapezoidal integral from the initial sample to every coordinate |

`sample_cumulative_area` returns exactly N elements: the first is `0.0`,
and the last matches `sample_area(x, y)` when finite and representable.
It allocates a fresh vector of 8 × N logical payload bytes and prepays
4 × (N − 1) work steps, as do the other sample kernels.
It uses the same ordered compensated accumulation and validated grid;
an unrepresentable intermediate prefix fails rather than becoming infinity.

Both inputs must have the same length of **2–65,536** and coordinate values
must be **strictly increasing**. Readings may be negative. The output of
`sample_slopes` has one fewer element than its inputs; each result is attached
to an *interval*, not a coordinate. These calls do not claim a higher-order
derivative, spline fit or arbitrary continuous integration.

## Executable Scarp example

```scarp
bind time = [0.0, 1.0, 3.0];
bind velocity = [0.0, 2.0, 6.0];
print(sample_area(time, velocity)); // 9.0
print(sample_slopes(time, velocity)); // [2.0, 2.0]
```

This calculates displacement from velocity samples using the trapezoidal
rule, and acceleration estimates over each interval. The real, tracked
example is `examples/sampled_calculus.srp` with an exact `.stdout` fixture.

## Numerical and resource contract

For consecutive pairs `(x[i], y[i])`, the integration contribution is
`(x[i+1] - x[i]) * (y[i] + y[i+1]) / 2`. When the interval width is at least one, Scarp halves the width before
multiplication; for smaller widths it halves the ordinate sum. This
preserves finite integrals that would otherwise underflow from halving
a subnormal reading or coordinate width too early. If the ordinate sum
overflows, it halves both finite ordinates individually before adding.
Contributions are combined with ordered Neumaier compensation. This
is **not** exact integration and does not guarantee correct rounding.
Cancellation can amplify the relative error of rounded individual
interval contributions; independent reference tolerances must account
for their absolute magnitudes, not only the final answer's ULP.

The slope on each interval is `(y[i+1] - y[i]) / (x[i+1] - x[i])`.
If the ordinate subtraction overflows, the kernel tries dividing each
ordinate by the width first, which can recover a finite quotient.
It rejects a slope that cannot be represented as a finite float.

The kernels reject:

- Incompatible vector lengths: **E0032**, not silent truncation.
- Fewer than two samples or duplicate/descending coordinates: **E0034**.
- Non-finite coordinate widths, contributions, slopes, or running sum:
  **E0031**. All input elements themselves must already be finite.

Numerical operations reject intermediate overflow even when some
real-number regrouping might produce a finite answer. Integration has
the usual trapezoidal discretization error. Secant slopes are interval
averages, not exact point derivatives.

The kernels scan only visible vector windows and never mutate input data.
Each prepays **4 × (N − 1)** numerical work steps. `sample_area`
does not allocate a numeric output buffer. `sample_slopes` allocates
`8 × (N − 1)` new payload bytes, charged against
`ExecutionOptions.max_vector_bytes`. N is bounded to 65,536; neither
algorithm uses recursion, threads, GPU execution or external dependencies.

## Verification

```sh
cargo test --locked --offline --test sampled_calculus
cargo test --locked --offline --test examples
python3 scripts/test_sampled_calculus.py target/debug/scarp
cargo bench --locked --offline --bench numerics -- --quick
sh scripts/check.sh --offline
```

The test suite covers analytic linear integrals, nonuniform grids,
large finite differences, subnormal arithmetic, signed cancellation,
shape/domain/overflow diagnostics, exact work/payload limits, shared
vector windows, maximum-length execution, static types, and atomic output.

Future `scarp.science`/numerical module organization is a *proposal*;
this change does not introduce module namespaces, automatic differentiation,
simulation solvers, vectorized syntax, GPU execution or native compilation.
