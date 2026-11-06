# Changelog

## 0.0.6-alpha.2 — prerelease candidate, 2026-11-06

- Add conservative, bounded module-graph invalidation planning and exact
  source-graph comparisons for trusted loaders and import retargeting (#121).
- Add caller-owned exact-input whole-graph semantic compilation cache with
  checked compiler budgets, revisions, source identities, and FIFO eviction (#122).
- Add source-relative lexical token caching for unchanged individual modules,
  with source-span rebasing and independent logical resource limits (#123).
- Add source-relative parsed AST caching with exhaustive nested span
  rebasing, independent FIFO quotas and cold/warm diagnostics/index tests (#124).
- Add opt-in combined front-end/semantic cache entry points and compiler
  benchmark workloads that check correctness without asserting speedups.
- Extend native archive requirements and damage-test the alpha.2 release
  metadata and cache contracts across all three supported targets.
- **Candidate source only:** not tagged, not a GitHub prerelease, not stable.
  Publication requires exact-main CI and separately approved source SHA.
  The latest public prerelease remains v0.0.6-alpha.1.

## 0.0.6-alpha.1 — prerelease, 2026-11-05

- Add opt-in, bounded source symbol/reference snapshots with go-to-definition and
  find-references Rust APIs; retain original cross-file and folded constant sites.
- Add static `inspect symbols` / `inspect references` text and versioned JSON views,
  original UTF-8 locations, named-argument and assignment references, and E0050.
- Add independent JSON/source-span checks, exact fixtures and runnable examples.
  Existing module/diagnostic JSON schemas and execution behavior are preserved.
- Add pure `rms(vector) -> float` with scaled compensated reduction, finite
  extreme-magnitude results, exact 4n native work and no new element allocation.
  Cover windows, modules, contracts, empty domains and a high-precision oracle.
- Require GCC and Clang UBSan differential checks on Linux; make the C host
  fixture's size formatting portable to Windows GCC without weakening warnings.
- Published as a **GitHub prerelease**, not stable, on 5 November 2026 from
  source `aa329a22cf2bf92d2c9814a26ff9c8b67a72e6e8`.
  Four Rust and three native package checks passed on the exact source and
  separate authorization control revision. Seven checksum-referenced assets
  are available. See the
  [alpha.6 publication record](docs/releases/v0.0.6-alpha.1-validation.md).

## 0.0.5-alpha.5 — prerelease, 2026-11-04

- Add project-aware module inspection of an explicit `Scarp.project` directory,
  reusing the existing module JSON schema and path-traversal protections.
- Integrate opt-in freestanding C11 scalar source emission with checked integer
  and Boolean expressions, branches, `while` and counted `repeat` loops.
- Match interpreter negative-count, overflow, invalid-shift, work-budget and
  nearest-loop `break`/`continue` behavior with differential C tests.
- Add E0049 for unsupported experimental backend forms. Generated C does not
  imply a `scarp compile` command, a native executable or a stable ABI.
- Strengthen required-C, freestanding ELF and cross-platform native checks;
  avoid zero-length host callbacks and test checked output/repeated execution.
- Include runnable `native_scalar_repeat.srp` with an exact stdout fixture,
  release-metadata validation, compatibility updates and candidate notes.
- Published as a **prerelease**, not stable, from exact source
  `2eeed0aa52ccb59523307841ad8ffbd83f8911a8` on 4 November 2026.
  Four Rust and three native package jobs passed for both the source and the
  separate authorization control revision. The guarded publisher verified
  all seven public downloads. See the
  [alpha.5 publication record](docs/releases/v0.0.5-alpha.5-validation.md).

## 0.0.5-alpha.4 — prerelease, 2026-11-03

- Extend static-only `scarp verify` to accept an explicit project directory,
  reusing the bounded `Scarp.project` manifest validation from `launch`
  without executing runtime statements or changing file-level verification.
- Add CLI regressions for runtime-only errors, malformed manifests, Unicode
  and symlink escaping. Correct stale reference documentation: bitwise shifts,
  remainder and binary/hexadecimal literals already ship in alpha.3.
- Add interpreter-backed `scarp create DIRECTORY` and `scarp launch DIRECTORY`
  with a bounded, strictly validated `Scarp.project` manifest and starter code.
- Test create → verify → launch on supported platforms, including Unicode,
  no-overwrite behavior, malicious manifests, symlink escapes and runtime budgets.
  No native build, package registry or stable manifest semantics are implied.
- Documentation: reconcile the already-published 0.0.5-alpha.3 release status
  across current guides and add an offline release-document consistency check.
  No compiler behavior, existing tag or release asset changes.
- Published as a prerelease from exact source
  `79141329176f5dce51457e90a6652902f6432135`, with Windows x64,
  Linux x64 and Apple Silicon macOS archives, three sidecar hashes and a
  combined `SHA256SUMS`. All seven public downloads passed verification.
  See the [alpha.4 publication record](docs/releases/v0.0.5-alpha.4-validation.md).

## 0.0.5-alpha.3 — prerelease, 2026-11-02

- Add `scarp inspect ast FILE` using the real lexer/parser for an entry-file
  development syntax tree without executing or loading imports. Retain
  schema-v1 JSON *diagnostics* for malformed source, but do not promise
  a stable AST JSON format or create an unintended top-level command alias.
- Test AST parity against the embedding parser, parse errors, missing imports,
  unsupported flags, Unicode paths, and dash-leading filenames.

- Add a read-only `scarp diagnose [--json]` command that reports actual
  compiler package version, host OS/architecture and available CLI capabilities
  without network access or pretending external toolchains/registries exist.
  Cover text, JSON, help and invalid arguments in integration tests.

- Introduce Scarp-first `exec`, `verify`, `inspect tokens`, `inspect modules`,
  `guide`, and `version` CLI routes on a separate development branch. These
  reuse the actual interpreter, static module resolver, lexer, module inspection
  and version backends. `verify` performs real non-executing checks and cannot
  detect runtime-only failures. New entrypoints forward schema-v1 JSON diagnostics
  from current compiler main without changing the JSON output format.
- Preserve old `run/check/tokens/modules` behavior temporarily while documenting
  a future reviewed migration; `check` is not renamed to static-only `verify`.
  No native compiler backend or project workflow is implied by these commands.
- Add CLI integration coverage for contextual help, versions, backend equivalence,
  JSON stability, invalid arguments, and Unicode/spaced filenames.

## 0.0.5-alpha.2 — prerelease, 2026-11-02

- Add streaming schema-v1 module JSON inspection, preserving ordering, source-local
  byte spans, original signatures and constant references without runtime execution.
- Add source-free diagnostic explanations backed by one code catalogue, plus a
  correctness-checked compiler/module/JSON benchmark and independent JSON validation.

- Add lossless to_float/to_integer and seven pure Unicode text queries, sharing
  strict built-in typing, argument locations and deterministic execution budgets.
  Preserve scalar-vs-byte/grapheme distinctions and reject invalid indices/rounding.
- Add boundary/reference-model/host/CLI tests, exact E0044/E0045 fixtures and two
  runnable examples; no string-result allocation or ambient external effects.

- Integrate verified module inspection and constant expressions in dependency order.
- Add optional primitive declaration annotations with exact inferred-type matching,
  required initialization and source-aware errors; preserve lexical scope and budgets.
- Record a comprehensive feature matrix and document the additive declaration AST
  schema migration. Add annotation API/CLI fixtures and a runnable numerical example.

- Compile bounded scalar module constant expressions with declaration ordering,
  checked arithmetic/logic, imported public references and iterative cycle checks.
  Preserve ordinary entry-script runtime bindings and literal-only programs.
- Add independent ConstantLimits and immutable source-located constant-reference
  metadata without changing existing ModuleLimits or compile signatures.
- Reuse the existing expression type checker and checked evaluator; improve operand,
  argument and return-type notes, and add API/CLI/golden/boundary regressions and a
  runnable three-file constant example.

- Add `scarp modules` for statically checked source inventories without executing
  statements, defaults or contracts. Show import sites, dependencies, visibility
  and typed original function signatures; preserve existing command behavior.
- Add immutable import records, direct/transitive forward/reverse dependency
  queries, borrowed per-module function details and validated source locations.
- Validate complete import lists before dependency callbacks, remove repeated
  namespace scans, locate the first exceeding import and report actual cycle
  edges. Offer bounded deterministic spelling repairs for public names.
- Add API/CLI/diagnostic/boundary regressions, a runnable example and reviewed
  inventory, documentation and native inspection smoke checks. Existing older
  release tags and asset contents remain immutable.

## 0.0.5-alpha.1 — prerelease, 2026-11-01

- Add explicit relative source imports, qualified aliases, public/private
  functions and immutable scalar literal constants, with declaration-only
  libraries and deterministic initialization-free linking.
- Add filesystem/in-memory source loaders, independent graph/expansion limits,
  canonical deduplication, cycle detection and source-aware E0037–E0043 errors.
- Preserve function typing, local shadowing, defaults, purity, contracts,
  numerical budgets, ordinary single-file diagnostics and atomic output.
- Add cross-file type locations, owned source maps, public typed symbol metadata,
  reusable compiled module programs and separate load/resolve/check/execute APIs.
- Add API/CLI/path/limit regressions, exact diagnostic fixtures and a three-file
  numerical example. Refresh introductory docs and require new/inherited package
  contents. Publish the explicitly authorized source through the guarded
  exact-source publisher. See the [verification record](docs/releases/v0.0.5-alpha.1-validation.md).

## 0.0.4-alpha.4 — prerelease, 2026-11-01

- Integrate immutable shape-aware tiles and typed tile parameters/results from
  PR #53 with visible vector windows and the cumulative allocation ledger.
- Add `matvec`, `matmul`, `tileadd` and `tilescale`, checked shapes, compensated
  products, finite intermediates and prepaid work/payload limits.
- Integrate scope retirement from PR #51 across lexical exits, loop transfers,
  function completion and shared snapshots. Strengthen invalid-plan tests for
  postcondition parameter lifetimes, slots, defaults and control flow.
- Add precise dimension/index spans, contextual tile argument errors, reviewed
  diagnostics, independent matrix reference tests and seven runnable examples.
- Include the post-alpha.3 window/stitch, log-space, selection and classification
  kernels already integrated on main. Preserve existing language/CLI behavior.
- Retire completed publication requests. Publish alpha.4 from the separately
  authorized source after exact-source CI, native-package, checksum, draft and
  public-download gates. See the
  [verification record](docs/releases/v0.0.4-alpha.4-validation.md).

## 0.0.4-alpha.3 — prerelease candidate, 2026-10-31

- Add pure, overflow-resistant `norm(v)` and `normalize(v)` kernels with
  scaled numerical reductions, checked finite results and regression coverage.
- Reuse the current `funcra` frame on eligible direct terminal self-returns
  without bypassing total-call/step budgets or needs/default contracts.
  Postcondition functions and nonterminal calls retain ordinary frames.
- Introduce cumulative numerical payload allocation quota
  `--max-vector-bytes`/`ExecutionOptions.max_vector_bytes` (default 8 MiB),
  E0036 errors, exact-byte host/kernel/CLI tests and atomic-output validation.
  `normalize` participates in this same quota.
- Add three exact-output examples and a memory-control specification.
- Define minimal branch naming, auto-prune only merged inactive branches,
  and retain open PR heads or divergent experiments.
- Require exact-main multi-OS CI, native assets, checksum/source verification,
  and explicitly requested gated publication. See
  [release notes](docs/releases/v0.0.4-alpha.3.md).


## 0.0.4-alpha.2 — scheduled prerelease, 2026-10-31

- Immutable finite vectors, literals/indexing, whole-value assignment and typed
  funcra defaults/arguments/results/contracts with shared snapshots.
- Ten pure kernels with compensated reductions, scaled mean/MSE and shifted
  softmax. Preserve user-defined kernel names; fill uses value then count.
- Prepay native work and payload allocations; account vector equality, output
  preparation and snapshot bytes under the unchanged six execution controls.
- Add E0032–E0035 without changing established codes, independent numerical
  references, adversarial/function/CLI tests and three exact-output examples.
- Record numerical benchmark results and require native vector smoke tests.
- Schedule the authorized alpha.2 publication for 31 October at 10:00 AM IST,
  with an explicit UTC start/expiry window and unchanged source/artifact gates.
  Add numerical tutorials, error repairs and checkpoint evidence. See
  [release notes](docs/releases/v0.0.4-alpha.2.md).

## 0.0.4-alpha.1 — 2026-10-30 — prerelease

- Finite binary64 float scalars with decimal/scientific literals, same-type
  arithmetic/comparisons, checked nonfinite results and signed-zero divisors.
- Float parameters/defaults/results use existing funcra frames, purity/contracts,
  all six budgets, atomic streams and value snapshots without implicit coercion.
- Append E0030/E0031 without changing existing codes; add numeric examples and
  boundary, rounding, source-span, concurrency and native-package regressions.
- No vectors, native backend, CLI rename, or third-party dependencies.
- Require float guides/example fixtures in native archives and retire the
  completed beta.3 recovery request. Publication uses a fresh, explicitly
  authorized request and exact-main gates. See [release notes](docs/releases/v0.0.4-alpha.1.md).

## 0.0.3-beta.3 — 2026-10-29 — prerelease

- Adapt typed file-level `funcra` functions onto beta.2 main without importing
  the experimental numeric, native, or CLI stacks. Forward calls, recursion,
  named/default arguments, unit returns, purity, and needs/ensures contracts.
- Heap call frames, additive resource options, bounded traces, immutable metadata,
  and function-aware repeat counts preserve atomic stdout/stderr preparation.
- New function codes E0020–E0029 preserve existing E0001–E0019. Unknown call names
  now report E0023; unknown non-call statements retain E0005.
- Expose call depth/count, live slots, and output snapshot caps as CLI options
  for both run and check, with exact-boundary tests and actionable diagnostics.
- Verify bundled documentation/examples against the exact Git tree; require all
  function guides/examples and smoke-test every resource flag in native archives.
- Add a separately gated tag publisher using successful final-main checks and
  native packages. See [release notes](docs/releases/v0.0.3-beta.3.md).

## 0.0.3-beta.2 — 2026-10-28 — prerelease

- Contextual repairs for incomplete bind/var declarations and assignments,
  preserving precise expression errors and output atomicity.

- Canonical `print`, raw `write`, and explicit stderr `eprint`/`ewrite`, with
  zero/multiple arguments and optional trailing commas. `println` remains an
  exact alias. Output events preserve snapshots, budgets, and atomic diagnostics.
- Additive dual-writer execution APIs; the CLI routes program stderr explicitly.
  Single-writer hosts receive an actionable error before stdout when a program
  requires stderr. Current examples and guides use `print` by default.

- Immutable `bind` and mutable `var` declarations replace `let`/`let mut`.
  Legacy declarations report E0005 with migration help; Rust syntax uses
  `Statement::Declaration` instead of `Statement::Let`.
- Current examples and guides use the new declarations; the v0.0.2 source
  examples remain preserved as exact historical compatibility fixtures.

## 0.0.3-beta.1 — 2026-10-27 — prerelease

- Actionable loop/count/index diagnostics, configured limits in budget errors,
  contextual run/check/tokens help, and iteration/budget guides.
- Tab `\t` and carriage-return `\r` string escapes, with exact-byte fixture checks.
- Counted `repeat count as index` with a one-time count snapshot, immutable
  zero-based index, fixed storage, and nearest-loop control shared with while.
- Scoped while loops, nearest-loop break/continue, per-call work/print budgets,
  and CLI/embedding controls with atomic budget diagnostics.


## 0.0.2 — 2026-10-26 — prerelease

- Execution-local expression work buffers, immediate disposal of validation print
  values, and reproducible stage benchmarks with verified workloads.

- Conditional `if`/`elif`/`else` statements with boolean guards, scoped bodies,
  static checking of all paths, and execution of the selected path only.

- Boolean values, same-type equality, integer ordering, and short-circuit `and`,
  `or`, and `not` expressions.

- Immutable integer bindings and names in expressions.
- Explicit mutable bindings, assignment, and checked `+=`, `-=`, `*=`, `/=`.
- Immutable-assignment diagnostics and value snapshots for deferred output.
- Lexical block scopes, shadowing, and bounded block parsing/resolution.
- Name resolution before evaluation, with fresh storage for every execution.
- Unknown, duplicate, and reserved-name diagnostics with related declarations.
- A separate parsed-syntax API and immutable resolved-program API.
- Silent `check` validation of syntax, names, mutability, scopes, and arithmetic.
- Sequential token locations without repeated source-prefix scans.
- Generated-program conformance cases and malformed source/token mutation corpora.
- Package version `0.0.2`, native archives, and verified source/checksum records.
- String variables, primitive type inference,
  same-type mutable assignment, and immutable string value snapshots.

The variable milestone is integrated into main. Loops, floating-point/vector
arithmetic, bit operations, and freestanding C emission remain in separate PRs
and are excluded from this prerelease.

## 0.0.1 — 2026-10-25 — prerelease

The first experimental Scarp interpreter. Syntax and library APIs may change.

### Language

- UTF-8 `.srp` files with whitespace, line comments, and escaped string literals.
- `println` statements accepting strings or signed 64-bit integer expressions.
- Arithmetic precedence, grouping, unary minus, and left-associative operators.
- Checked arithmetic, literal range checks, and division-by-zero diagnostics.
- Full validation and evaluation before program output is written.
- Source byte spans, line/column locations, carets, error codes, and help.
- A nesting limit with diagnostics and flat expression storage for long chains.

### Tools and distribution

- `scarp run`, `scarp tokens`, help, and version commands.
- A reusable Rust library with explicit output destinations and error handling.
- Debug/release tests, cross-platform CI, and minimum Rust 1.74.0 checks.
- Runnable examples, a language reference, and contributor documentation.
- Native binary archives with SHA-256 checksums and recorded build information.
- Packaging smoke tests performed on the extracted deliverables.

### Known limits

No bindings, user-defined functions, floating-point values, type checker, modules,
loops, package manager, or native code generation. Source and intermediate values
are held in memory. Only the first diagnostic is reported. Expression grouping
is limited to 64 levels. The prerelease includes Windows x64, Linux x64 with
GNU libc, and Apple Silicon macOS builds; other platforms can build from source
where a compatible Rust toolchain is available.
