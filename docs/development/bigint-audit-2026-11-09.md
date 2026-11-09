# BigInt development audit — 9 November 2026

## Verified starting point

| Item | Observed state |
| --- | --- |
| Repository | `scarp-lang/scarp` (current canonical compiler repository) |
| Default branch/source | Protected `main`, `d222996be74ad3150417e084c7f59f67713eec9e` |
| Cargo version | `0.0.6-alpha.5`, unpublished source candidate |
| Latest public prerelease | `v0.0.6-alpha.4`, source `f04be0758cbce405ddcdb229699b0ff83c09d207` |
| Dependency/toolchain policy | No third-party dependencies, Rust 1.74, forbidden unsafe code |
| Previous numeric model | Checked signed i64, finite binary64, immutable numeric vectors/tiles |
| Existing runtime controls | Per-execution work, calls, frames, output snapshots and cumulative vector/text payload |
| Related work | Draft #171 adds order statistics after alpha.5; this work does not reuse its branch or modify it |
| Related issues | BigInt/arbitrary-precision searches found no existing tracking issue |
| Protection | Active main ruleset requires PRs, forbids deletion/non-fast-forward changes and requires resolved review threads |

Open and recently merged PRs, public releases, main/source tree, type/value
representation, parser/resolver/evaluator, module constants, output capture,
budget code, CI, package workflow and guarded publication process were reviewed.
The existing alpha.5 candidate already includes Top-P, Pearson correlation and
limited multi-binding ELF64 kernels. BigInt is a distinct later development
milestone, not a retroactive change to the frozen candidate's scope.

## Implementation delivered

Branch: `feature/bounded-bigint-post-alpha5`, based on the exact source above.
The accompanying PR identifies its actual commit SHA, diff and CI checks.

| Area | Files / resulting behavior |
| --- | --- |
| Exact engine | `src/bigint.rs`: immutable signed base-10^9 u32 limbs, canonical zero, exact decimal parse/display, comparison, add/subtract/multiply/divide/remainder, negation/absolute value and checked i64 conversion |
| Runtime/type integration | `src/value.rs`, `src/types.rs`, `src/resolver.rs`, `src/evaluator.rs`, `src/interpreter/machine.rs`, `src/lib.rs`: separate primitive/value, strict same-kind operations, annotations, bindings, compound updates, functions and output accounting |
| Pure built-ins | `src/numeric.rs`, `src/numeric/bigints.rs`: `bigint`, `bigint_from_integer`, `bigint_to_integer`, `bigint_abs`, plus host `evaluate_with_options` |
| Resource controls | `src/limits.rs`, `src/main.rs`: configurable digit/cumulative limb quotas, prepaid arithmetic/comparison/format work, scratch accounting and CLI flags |
| Diagnostics | `src/diagnostics.rs`, parser help and annotation fixture: E0051/E0052/E0053 plus existing zero-divisor, work and exact-conversion errors |
| Tests | `tests/bigints.rs`, `tests/bigint_modules_cli.rs`, `scripts/test_bigint.py`; registry/option/diagnostic compatibility expectations updated |
| Runnable examples | `examples/bigint_arithmetic`, `bigint_sequences`, `bigint_module` and their exact stdout fixtures; exported helper `examples/modules/bigint_math.srp` |
| Documentation | `docs/bigint.md`, `docs/proposals/bounded-bigint.md`, Hindi tutorial and linked type/value/CLI/budget/embedding references |
| Benchmark/CI | `benches/bigint.rs`, benchmark snapshot, Cargo bench target, shell/PowerShell example checks and bounded Python/quick benchmark CI gates |

The [design](../proposals/bounded-bigint.md) explains the representation choice,
signed semantics, explicit conversions, shared immutable storage, compiler
constant boundary and separate future-decimal proposal. The [reference](../bigint.md)
contains exact work formulas, allocation policy, CLI usage and error semantics.

## Executed local verification

Final validation used separate fresh Cargo target directories for stable and
minimum toolchains. Stable was Rust 1.99.1; minimum was Rust 1.74.0.

| Command/check | Result |
| --- | --- |
| `SCARP_REQUIRE_C=1 sh scripts/check.sh --offline` on stable | Passed formatting, Clippy with warnings denied, 895 debug tests, 895 release tests, docs/builds and executable examples |
| `cargo +1.74.0 test --locked --offline` | 895 passed, zero failures/ignored |
| `cargo +1.74.0 test --locked --offline --release` | 895 passed, zero failures/ignored |
| BigInt-focused Rust suites | 25 new tests, including 250 seeded i128-reference programs, signed quotient/remainder identities, exact conversion boundaries, budgets, modules, CLI statuses, Unicode source ownership, concurrent/independent executions and atomic streams |
| Python oracle against final stable binary | 304 operand pairs, 3,344 independent arithmetic/comparison results; exec/evaluating-check parity and silent static verify |
| Python oracle `--large` | Passed generated 19/100/1k/10k/100k/1m-digit linear-operation fixtures, with controlled Python conversion limit |
| BigInt quick benchmark on stable/minimum | Passed; reproducible expanded stable measurements recorded separately |
| Existing Python inspection/diagnostic/symbol/RMS/calculus tools | 27 tests passed in total |
| Existing release helper suites | 97 passed |
| Existing branch-pruning tests | 5 passed |
| `git diff --check` | Passed |

The full check script was rerun after updating the type-registry example's old
fixed primitive-count assertions. Parser diagnostic fixtures were updated to
list BigInt. Earlier local attempts also encountered non-executable generated
test files and inconsistent cached diagnostics in the original shared target
directory; final fresh builds passed. These failed attempts are not counted as
successful validation. Clippy on Rust 1.74 additionally reported unrelated
pre-existing test lints; CI's actual Clippy gate uses stable, which passed.
After adding an explicit C11 BigInt-rejection assertion, the focused suite was
rechecked successfully. No physical out-of-memory condition was injected.

Cross-platform packaging, macOS and Windows results must be read from this
PR's actual GitHub checks; local Linux results are not a claim of owner-performed
Windows testing. No manual Windows verification is claimed.

## Completeness and limits

Implemented: exact signed runtime arithmetic, construction/conversion, strict
types, immutable/mutable bindings, comparisons, control flow, typed pure
functions/defaults/contracts, module-function handoff, printing, CLI execution/
static verification and deterministic per-run resource accounting.

Defaults are 10,000 input/result digits and 8 MiB cumulative new limb payload.
The configurable implementation ceiling is 1,000,000 digits. Controlled tests
verify large linear operations; schoolbook multiplication and binary-search
long division remain quadratic and can exhaust work limits. The
[benchmark snapshot](../benchmarks/2026-11-09-bigint/README.md) records measured
timings and whole-process peak RSS; it makes no isolated allocator, worst-case
latency or million-digit quadratic-performance guarantee. Logical quotas do not
cover every compiler/allocator byte or recover all process-wide allocation
failures. No third-party numerical runtime or unsafe code was added.

Unsupported: BigInt module constants/constant folding, bitwise/shift operators,
implicit numeric mixing, BigInt-to-float, literal suffixes, BigInt vectors,
native C11/ELF64 execution and arbitrary exponentiation. Decimal arithmetic is
only a distinct future design. Existing integer overflow behavior is retained.

Rust embedding compatibility: original primitive IDs and two-field
`ExecutionLimits` remain stable. Exhaustive `Value`/`PrimitiveType` matches and
complete `ExecutionOptions` literals need the new variant/fields or struct
update defaults; this source migration is explicitly documented.

## GitHub, manual actions and release readiness

The feature is prepared for technical review as a **draft PR after alpha.5**.
It should not be merged into the frozen alpha.5 candidate without a deliberate
new release-scope review. PR creation/CI are automated; no issue comment is
needed, and none was posted. There is no required manual action merely to
continue reviewing this branch. Existing tags, versions, release metadata,
release request, issue discussions and other Scarp repositories are preserved.

No stable release or prerelease was published. This milestone is reviewable
runtime functionality, not a production-readiness or publication assertion.
For a future release, first select/integrate scope and an unused version, then
verify final-main Rust and package jobs, native archives, source records,
checksums and notes. Only then obtain explicit authorization naming that
version and its full source SHA and follow the separate guarded request in
[the release process](../releasing.md). The current broad development request
does not satisfy that source-specific publication requirement.

Next priorities: review custom arithmetic/resource boundaries and all platform
checks; resolve post-alpha.5 integration sequencing with other active work;
design compiler-budgeted constants separately; profile/optimize multiplication
and division before larger quadratic workloads. Update external docs/examples/
website/editor support only against a verified integrated compiler revision.
