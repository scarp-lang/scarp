# Proposal 0038 — Scientific computing, simulation and AI ecosystem

Status: **architecture proposal / partly prototyped** (7 November 2026).
No statements here grant unimplemented compiler features or public API stability.

## Verified starting point

The existing Rust compiler has a lexer, AST, typed resolver, bounded
interpreter, relative source modules, a crate-private verified scalar IR, and
an experimental *library-only* C11 scalar emitter. No general native object
writer or public native executable compiler exists.

Numerical values are finite binary64 scalars, immutable one-axis vectors
(up to 65,536 elements), immutable two-axis tiles, and bounded integer
collections. Storage sharing uses Rust-managed immutable references and
visible vector windows. Kernels are typed by `Builtin::parameter_types`
and `result_type`; `numeric::evaluate_with_spans` owns shared validation
and dispatch. Work and fresh numeric payload use explicit independent budgets.
The published v0.0.6-alpha.3 includes stable selection and softmax kernels.
Sampled integration and interval slopes are **post-alpha.3 development work**,
not released features.

The compiler's public Rust library and CLI already run these kernels without
any generated C, shader compiler, Python bridge, BLAS binding or native object
writer. That is a valuable narrow baseline, not yet an HPC platform.

## Language/library boundary

Prefer **Scarp libraries** for mathematical algorithms, input validation,
dataset transformations, probability distributions, solvers and simulations.
The compiler should provide only primitives requiring representation,
type/shape information, controlled execution or verified lowering.

Proposed eventual namespaces, all **unavailable as namespaces today**:

| Namespace | Primary responsibility | Dependency |
| --- | --- | --- |
| `scarp.math` | Elementary/transcendental scalar math with explicit domains | Finite float contract and portable math validation |
| `scarp.stats` | Stable reductions, moments, distributions, sampling policies | Numerical kernels, error and random-state design |
| `scarp.science` | Sampled derivatives/integrals, root finding, quadrature, interpolation | Safe vector operations, function arguments or callable adapter |
| `scarp.linalg` | Dense linear algebra, factorizations, direct/iterative solvers | Matrix shape types and stable storage |
| `scarp.tensor` | Typed shapes, strides/views, broadcasting, immutable/mutable policy | Composite value representation + lifetime/budget invariants |
| `scarp.optimize` | Objective evaluation, gradients, constrained optimizers | Callable functions, AD and numerical tolerances |
| `scarp.simulate` | ODE stepping, event handling, physics models, reproducible trajectories | Solver interfaces and bounded execution |
| `scarp.units` | Quantity dimensions and checked conversion | Dimension algebra in a reviewed type-system design |
| `scarp.ai` | Models, layers, loss and training loops | Tensors, gradients, optimizer/checkpoint contracts |
| `scarp.signal` | Filtering and transforms | Linear algebra and documented precision |
| `scarp.viz` | Data plotting and export | Typed data serialization and capability-safe I/O |

Names and package/module syntax require a language-design gate; existing
relative .srp modules do **not** magically support these imports. Do not
reserve names or emulate a module by pretending a built-in is a package.

## Milestone A — Reliable sampled numerical analysis (current vertical slice)

`sample_area(vector, vector) -> float` applies a bounded trapezoidal rule.
`sample_slopes(vector, vector) -> vector` returns secant slopes over
consecutive, strictly increasing coordinates. A validated, shared finite
vector storage API and prepaid work/memory budgets make these useful today.

Validation: exact affine integrals, nonuniform spacing, reference arithmetic,
subnormals, cancellation, finite extremes and overflow, duplicate/descending
coordinates, window-only views, typed calls, byte-for-byte stdout fixtures,
runtime errors and resource boundaries on all CI target platforms.

This deliberately avoids a new parser rule, arbitrary callbacks, matrix
broadcasting or inaccurate claims of high-order differentiability.

## Milestone B — Libraries and type soundness

1. Formalize public callable interfaces (including purity/effects) so Scarp
   functions can be passed to numerical solvers without host-language code.
2. Specify generic numeric containers independently from primitive tokens,
   including rank, dimension and total element caps; distinguish views from
   fresh allocations. Consider separate scalar, integer and float element kinds.
3. Design private typed library namespaces/import resolution and version
   compatibility before introducing `scarp.*` syntax. A regular .srp file
   should eventually use the same resolver as the user's application.
4. Extend diagnostics for dimensions, partial linear-algebra rank, algebraic
   singularity and convergence failure, with spans at faulting arguments.
5. Decide how deterministic work/payload accounting applies to shared
   computation graphs and heap-backed tensor views.

## Milestone C — Scientific algorithms (safe CPU)

- Sampled integration: higher-order composite methods when spacing permits,
  without promoting a result as exact; adaptive quadrature needs callable
  functions and explicit evaluation budgets.
- Root finding: bracketed bisection first, then safeguarded Newton/Brent
  approaches with documented stop conditions and error estimates.
- Linear algebra: stable LU/QR/SVD with pivot strategy, rank/singularity
  diagnostics and residual tests. Avoid `inverse` as the default solver.
- ODE simulation: explicit RK integrators with bounded steps, stiffness
  guidance, local error tracking and event handling.
- Statistics: covariance, correlations, regression, quantiles, reproducible
  pseudorandom streams and distribution functions; numerically validate at
  extreme magnitudes before exposing convenience APIs.
- Units: dimensionality tracked by compiler-verified quantity types, rather
  than unconstrained strings or implicit conversions.

## Milestone D — Differentiation, optimization and models

Automatic differentiation should begin with a *tested CPU scalar/vector
reverse-mode tape* after ownership, graph reuse, lifetime and work accounting
are settled. Validate against analytic gradients and independently computed
finite differences on well-conditioned functions, not finite differences alone.
Next: optimizer iterations with explicit state and checkpoint schemas, stable
batching, deterministic initialization, and neural layers implemented in Scarp.
Do not claim general training is complete because specialized loss-gradient
kernels already exist.

## Milestone E — Performance and devices

- Establish CPU baselines: fixed numerical workloads, same input payload,
  precision and diagnostics; report parsing/validation/runtime/native-kernel
  costs separately and multiple repeated samples.
- Introduce scalar/native execution only when private typed IR verifier,
  ownership and differential interpreter tests pass. Scalar object writing
  does not imply vectors/tiles can run natively.
- Inspect memory layout and release/borrow contracts before SIMD, threads or
  BLAS. An unsafe/FFI backend belongs in an isolated, reviewed optional crate,
  never behind unreviewed `unsafe` in the current `#![forbid(unsafe_code)]`
  discipline.
- For GPU: explicit device/host tensor ownership, backend availability,
  transfer errors, kernel result comparison, deterministic fallback policy and
  capability permission model. **No GPU support is implemented.**
- For distributed computing: data serialization, scheduling and replay rules
  must precede network execution. No silent nondeterministic reduction order.

## Benchmarks, release gates and evidence

Every numerical kernel must have a host-call correctness suite and executable
Scarp fixture. Check Linux, Windows, macOS, minimum supported Rust, short and
large vectors, signed zero, subnormals, nonfinite intermediates, exact step
and allocation limits, and interpreter error parity. Record the source SHA,
compiler version, CPU/OS, command, sample count, summary statistic and
reference implementation for each performance claim.

Rust-only native-kernel times are *not* equivalent to whole-program Scarp
times or NumPy vectorized timings. Only compare same numerical algorithm,
precision, input lengths and allocation scope. Regressions need repeatable
measurements, not one noisy timing.

Ship feature flags or prerelease warnings only when attached to actual
tested behavior. Keep compiler, documentation, examples, website and extension
advertisements behind a verified merge/release source identity. Prefer small
reviewable PRs; an experimental feature should not force a stable release.
