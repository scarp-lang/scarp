# Changelog

## 0.0.7-alpha.5 — development

- Add typed source tensors, broadcasting, batched matmul and checked gradients.
- Train with scalar and tensor reverse-mode differentiation, SGD, Adam and AdamW.
- Add explicit fixed-width integer types and a standalone Linux x86-64 backend.
- Reuse tensor differentiation workspace while retiring immutable snapshots.

## 0.0.7-alpha.4 — published GitHub prerelease, 2026-11-28

Published from [`e620bdcb`](https://github.com/scarp-lang/scarp/commit/e620bdcb512fbe909b08f1b4cc625865e4cdf339).
[Release and downloads](https://github.com/scarp-lang/scarp/releases/tag/v0.0.7-alpha.4)

- Add opt-in native self and mutual recursion for ELF64 integer entries,
  with checked stack, frame-depth, and shared work limits.
- Lower nested fixed arrays, tuples, and records as verified private stack lanes.
- Infer nested generic calls from partial result context across modules.
- Add bounded file-backed exact integer arithmetic and checked output staging.
- Add exact BigInt utilities, sequences, wide exponents, and chunked formatting.

## 0.0.7-alpha.3 — published GitHub prerelease, 2026-11-27

Published from [`44a3bb0`](https://github.com/scarp-lang/scarp/commit/44a3bb0a91e67570e46ce058a3f6211f8e5db0f5).
[Release and downloads](https://github.com/scarp-lang/scarp/releases/tag/v0.0.7-alpha.3)

- Add bounded generic function specialization, contextual inference, and
  generic Option and Result payloads.
- Compile concrete generic integer calls and checked flat array, tuple, and
  record locals to experimental x86-64 ELF64 objects.
- Fold verified native constants while preserving checked failures and work
  accounting.
- Add bounded string concatenation, repetition, raw literals, and multiline
  literals.
- Retire unneeded interpreter frame values and move contract-checked results
  out of their frame slot.

## 0.0.7-alpha.2 — published GitHub prerelease, 2026-11-26

Published from [`638dc4c`](https://github.com/scarp-lang/scarp/commit/638dc4c227f34f1c660be66834bcc0cbdb22dec8).
[Release and downloads](https://github.com/scarp-lang/scarp/releases/tag/v0.0.7-alpha.2)

- Emit bounded, nonrecursive direct calls between pure integer functions in
  one x86-64 ELF64 object. Pass up to sixteen signed integer arguments across
  registers and stack slots. Verify the call depth and the stack footprint
  before emission, and preserve nested failures and their original byte spans.
- Reuse a caller-saved register for eligible scalar temporaries and retain
  verified stack slots for values that must survive a call.
- Add `scarp native-object` for checked functions, bounded functions and
  bounded call graphs. Publish objects atomically at new destinations and
  optionally report ABI, stack and failure-span metadata as JSON.
- Plan checked x86-64 layouts for tuples, records, fixed arrays, variants,
  Option and Result. The layout API provides storage descriptions for future
  aggregate lowering.
- Verify the extracted compiler's native object command on package hosts and
  boot a CLI-produced call chain in the existing QEMU smoke harness.

## 0.0.7-alpha.1 — published GitHub prerelease, 2026-11-25

Published from [`996bd5b`](https://github.com/scarp-lang/scarp/commit/996bd5b83c8cb26e98637563729f67bdacc61cd4).
[Release and downloads](https://github.com/scarp-lang/scarp/releases/tag/v0.0.7-alpha.1)

- Join fixed arrays with `array_concat(left, right)`. The resolver infers the
  combined length and checks the full element type. Empty arrays share storage.
- Accept 2–1024 integers in `integer_gcd`, its alias `integer_hcf`, and
  `integer_lcm`. A zero anywhere makes the LCM zero.
- Extend the experimental checked ELF64 function ABI to zero through sixteen
  integer parameters. The bounded API supports counted repeats, while loops
  and scalar conditional expressions. Checked layouts and temporary storage
  reuse preserve the per-call work allowance and error spans.
- Add stable vector `softplus` and live, FIFO token-cache quota changes.
- Reuse bounded interpreter frame buffers after ordinary calls and preserve
  the active evaluator buffers across terminal self-transfers.
- Check scalar IR expression and update-operator spans against the original
  statement ranges. Verify separate Cargo target builds and exact-source
  package inventories on the supported native hosts.

## 0.0.6-alpha.20 — published GitHub prerelease, 2026-11-24

Published from [`b1a9347`](https://github.com/scarp-lang/scarp/commit/b1a9347f93540fdf95b0ddecba4adc55a3b4bb98).
[Release and downloads](https://github.com/scarp-lang/scarp/releases/tag/v0.0.6-alpha.20)

- Emit compiler-owned x86-64 ELF64 objects for pure Scarp functions with up
  to six required integer parameters, integer and Boolean locals, checked
  arithmetic, and forward conditionals. The experimental Rust API returns
  object bytes and source spans for arithmetic failures.
- Include strings inside immutable aggregates in Rust host argument UTF-8
  accounting, while preserving top-level argument counts and atomic admission.
- Support exact base-ten `d` literals such as `0.1d` for the decimal type.
- Canonicalize new projects on lowercase `scarp.project`: `create` now reuses `init`'s v2 scaffolding, while older uppercase `Scarp.project` v1 projects remain readable. Not in published alpha.19 binaries.

- Implement `tuple[index] = value;` on mutable tuples as a type-checked,
  persistent snapshot replacement. Literal indices retain compile-time arity
  checks, aliases remain unchanged, and new tuple allocations consume existing
  work and tuple-byte quotas. Add module-source and cross-platform regressions.
  Not available in the published alpha.19 release.

- Implement lexicographic numeric tuple ordering with `<`, `<=`, `>` and `>=`
  for identical structural tuple types, including nested numeric tuples.
  Reject unordered elements and mismatched types statically; preserve `==`
  and `!=` behavior. Add compiler tests and a runnable exact-output example
  (PR #353, merge commit `fde0e374b445fb573dfd980bde36f83e9d76e729`).
  **Not included in the published v0.0.6-alpha.19 binaries.**

## 0.0.6-alpha.19 — published GitHub prerelease, 2026-11-23

Published from approved source commit
[`e534e73a81c9500e3aef9f3d51cafa0bc7bbcf75`](https://github.com/scarp-lang/scarp/commit/e534e73a81c9500e3aef9f3d51cafa0bc7bbcf75).
Post-publication tuple ordering and indexed tuple assignment are not included
in this immutable release.

- Introduce a checked callable-signature compatibility query with invariant parameter and result types and one-way pure-to-impure effect compatibility. This is Rust semantic groundwork, **not** source-level first-class functions.
- Add immutable, snapshot-bound Rust host callable handles, sparse named/default arguments and safe dynamic rebinding under explicit type/effect checks.
- Support module export-constrained host callable lookup with linked diagnostics and existing source-map boundaries.
- Record deterministic host execution resource usage, including failed invocations, and enforce cumulative interpreter, input and host-argument quotas across repeated embedded calls.
- Charge completed interpreter work even when a caller-owned output writer panics during replay; arbitrary input-reader panics during evaluation and process aborts remain outside this guarantee.
- Expand integration tests and host embedding examples. The release required exact-source CI and explicit publication approval.


## 0.0.6-alpha.18 — published GitHub prerelease, 2026-11-22

- Run an independent read-only candidate preflight after all three package
  targets pass, verifying the combined archives against the exact source commit
  before any publication authorization.

- Validate the exact release archive member inventory and reject path
  traversal, duplicates, symlinks, and unapproved bundled files.

- Canonicalize Windows ZIP and Unix tar.gz archives with stable file ordering,
  ownership, permissions, and timestamps. A clean second release build must
  reproduce the complete archive SHA-256 bytes on the same OS/toolchain runner.

- Make `scarp exec` resolve the current project's manifest entrypoint or a predictable
  `src/main.srp`/`main.srp` fallback, while still accepting explicit source paths.
  Enable bounded console and typed prompts by default for `exec` only; retain
  legacy `run` opt-in and the existing input quotas and failure diagnostics.
- Start **Scarp Lifetime** storage-retention hardening: empty finite/integer
  vector windows use shared empty allocations instead of retaining the original
  backing buffers. Nonempty windows remain zero-copy. Add weak-reference release
  tests, source-level quota regressions, and a repeatable microbenchmark.

- Add pure, quota-bounded persistent `array_set` for fixed-size arrays, with
  exact replacement types, index diagnostics, and source-level examples.
- Retry interrupted interactive prompt flushes while preserving execution
  budgets and requiring output flush before input consumption.
- Strengthen structured module-inspection JSON tests across records, variants,
  nested types, Unicode, and CRLF input.


## 0.0.6-alpha.17 — published GitHub prerelease, 2026-11-21

- Add `array_len` for counted traversal and `array_get_or` for typed fallback
  lookup on fixed arrays.
- Add `input_boolean(prompt)` with the existing interactive prompt behavior
  and E0084 for malformed Boolean input.
- Preserve the exact invalid length span and repair guidance for nested array
  type annotations.
- Point legacy tutorials to the official English and Hindi Scarp Docs site.

## 0.0.6-alpha.16 — published GitHub prerelease, 2026-11-20

- Add bounded `median` and type-7 `quantile` for finite numerical vectors.
  Existing Scarp functions with the same names retain priority.
- Add six bounded Unicode string utilities and eleven checked fixed-width
  integer utilities. Cover typed module composition and editor cache parity.
- Experimental Linux x86-64 ELF64 integer kernels probe each full stack page
  crossed by the frame prologue and preserve both existing return ABIs.
- Rust hosts can change `ConstantCache` retention quotas in place. Entries are
  evicted FIFO and previously compiled programs remain usable.
- Verify integer map snapshots and both captured streams at the work limit.

## 0.0.6-alpha.15 — published GitHub prerelease, 2026-11-19

- Apply the 1 MiB per-file source limit to token and AST inspection.
- Charge string equality and inequality by UTF-8 bytes when the operands have
  equal byte lengths. Constant evaluation and cache replay use the same charge.
- Reject caller-supplied semantic cache revisions that disagree with revisions
  returned alongside source text. Rust hosts should handle the new
  `CachedCompileError::RevisionMismatch` variant.
- Add checked left, arithmetic-right and logical-right shifts to the
  experimental x86-64 ELF64 checked-kernel API. Invalid counts return status 3
  and the operator's byte span.
- Enforce the immutable binding subset for both ELF64 kernel APIs. Apply the
  200,000-byte machine-code ceiling to the emitted bytes.

## 0.0.6-alpha.14 — published GitHub prerelease, 2026-11-18

- Report missing and duplicate separators, mismatched delimiters and end of
  input at the relevant nested destructuring group.
- Allow Rust hosts to adjust editor-cache retention quotas on a live cache.
  FIFO eviction preserves previously returned programs and editor snapshots.
- Verify cold and warm editor analysis across shared dependency edits and
  cover persistent map projection output and rollback.
- Add an experimental Linux x86-64 ELF64 kernel API for checked integer
  arithmetic, with a status and source span for the first failure.
- Reject native repeat operations that alias the count and index slots.

## 0.0.6-alpha.13 — published GitHub prerelease, 2026-11-17

- Forward Option and Result failures without a second payload allocation.
- Add caller-owned syntax and semantic cache quota updates and targeted semantic
  revision eviction. Preserve returned programs and FIFO order.
- Add an opt-in workspace generation check for static editor analysis results.
- Cover persistent map projections, aggregate quotas, and cache snapshot
  boundaries with exact-output examples and regression tests.

## 0.0.6-alpha.12 — published GitHub prerelease, 2026-11-16

- Add nominal records, tagged variants, nested destructuring and exhaustive scalar matching.
- Add concrete Option and Result types, typed propagation and cross-module type identity.
- Add bounded, immutable dynamic lists and deterministic maps with persistent updates and ordered projections.
- Add a multi-file task board and package-level checks for its output and tests.

## 0.0.6-alpha.11 — published GitHub prerelease, 2026-11-15

- Expose exact base-10 `decimal` in Scarp source, with checked arithmetic,
  comparisons, typed module interfaces and explicit conversion and rounding.
- Add `input_decimal(prompt)` for exact decimal console input and ship an
  interactive example with byte-exact UTF-8 fixtures.
- Charge the text quota before allocating strings produced by decimal formatting.

## 0.0.6-alpha.10 — published GitHub prerelease, 2026-11-14

- Add explicit UTF-8 input files, EOF-aware reads and opt-in terminal prompts.
  `input_integer`, `input_float` and `input_bigint` parse checked values.
  `try_read_line` keeps EOF distinct from empty lines and input errors.
- Add stdout and stderr flush calls, plus exact `.stdout` and `.stderr`
  comparison fixtures for project tests in both JSON report formats.
- Add immutable fixed-size arrays, including typed empty values, nested
  values, checked indexing, and an independent allocation limit.
  Array-bearing module inventories use JSON schema 3.
- Add advanced exact BigInt arithmetic, number theory, radix conversion,
  signed bitwise operations, and a single-pass quotient/remainder tuple.
- Add opt-in constant and revision-aware editor caches. Reduce allocations
  during scalar IR verification and retain cold-build diagnostic parity.
- Add an experimental exact Decimal foundation for Rust hosts.

## 0.0.6-alpha.9 — published GitHub prerelease, 2026-11-13

- Add opt-in `scarp test --json=v2` with compile, fixture, and runtime
  phases, diagnostic codes, and project-relative source locations.
- Add bounded exact `bigint` arithmetic, comparisons, explicit conversions,
  and support for BigInt values in typed functions, modules, and tuples.
- Keep project test schema 1 and the signed 64-bit `integer` type compatible.
  Rust hosts can use `..Default::default()` for the new execution limits and
  must extend exhaustive matches on the affected public enums.

## 0.0.6-alpha.8 — published GitHub prerelease, 2026-11-12

- Add bounded declarative `scarp.project` manifests, `scarp init`, project
  `run` and `verify`, and read-only `project info` with optional JSON.
- Run immediate project tests with `scarp test`, including read-only listing,
  JSON reports, strict empty-suite checks, filename filtering, and bounded
  per-test UTF-8 stdin fixtures.
- Add typed Boolean assertions with optional static messages and source-located
  `E0065` failures. `assert` becomes reserved. Rust hosts with exhaustive
  matches on `Statement` or `DiagnosticKind` must handle the new variants.
- Preserve legacy manifests, `create`, `launch`, and single-file execution.

## 0.0.6-alpha.7 — published GitHub prerelease, 2026-11-11

- Add typed conditional expressions with exact branch result types and lazy
  evaluation, including tuple values and function arguments and returns.
- Decode Unicode scalar escapes written as `\u{HEX}` with one to six ASCII hex
  digits. Malformed escapes and non-scalar values report `E0003`.

## 0.0.6-alpha.6 — published GitHub prerelease, 2026-11-10

- Add impure bounded `read_line()` with explicit Rust readers, redirected CLI
  stdin, UTF-8/CRLF/EOF diagnostics and independent line/byte/read limits.
- Add six pure scalar/text intrinsics: `integer_to_text`, `float_to_text`,
  `text_to_integer`, `text_to_float`, `text_trim` and `text_replace`.
- Add immutable heterogeneous tuples, singleton syntax, structural `tuple<...>`
  types, literal-only projection, exact equality and escaped nested display.
- Support tuples across typed/default/named function arguments, returns,
  contracts, recursion and imported interfaces with bounded work/payload quotas.
- Migrate source-facing Rust fields and `Value::kind()` to `SourceType`; add
  `ExecutionOptions.max_tuple_bytes`. Tuple-exposing module JSON uses schema 2;
  primitive IDs, primitive inventories and navigation/diagnostic schema remain.
- Add E0054-E0061 (E0051-E0053 remain reserved), compiler-owned basics fixtures,
  combined input/text/tuple regressions, Unicode/PTY checks and extracted-package
  stdin/example smoke tests. Native backends explicitly reject input/tuples.
- Interactive prompts, empty tuples, tuple constants, dynamic tuple indices,
  destructuring, mutable elements, BigInt and order statistics are excluded.

## 0.0.6-alpha.5 — published GitHub prerelease, 2026-11-09

- Add deterministic CPU-only nucleus selection `top_p(vector, mass) -> integers`
  and `top_p_values(vector, mass) -> vector`, with a normalized finite
  probability domain, original-index stable ties, compensated cumulative
  mass and bounded work/allocations (proposed development PR #161).
- Extend the experimental safe-Rust x86-64 ELF64 relocatable-object kernel
  from a standalone pure integer initializer to a verified bounded series
  of integer `bind` statements referencing earlier integer bindings
  (proposed development PR #162). Not a general compiler or public CLI.
- Add scientifically centered `pearson_correlation(left, right) -> float`
  with independent finite scaling, compensated moments, constant-input
  errors, argument spans and deterministic budgets (PR #163).
- Exercise the combined numerical/native/compiler changes together on
  Linux, Windows, macOS, Rust 1.74 and all three native package targets
  (integration draft #165).
- **Published:** alpha.5 was released on 9 November 2026 from immutable source
  `205e92f166f186c3905e5880d41c31717b697b8e`, with separate
  seven-job source/control CI matrices and seven verified native assets.
  See the [public release](https://github.com/scarp-lang/scarp/releases/tag/v0.0.6-alpha.5)
  and [postpublication record](docs/releases/v0.0.6-alpha.5-validation.md).
  The separately authorized one-time release request was retired.

## 0.0.6-alpha.4 — prerelease, 2026-11-08

- Merge pure sampled numerical kernels (`sample_area`, `sample_slopes`,
  `sample_cumulative_area`) with bounded trapezoidal integration (#153/#155).
- Add overflow-resistant `cosine_similarity` with independently scaled
  compensated reductions and explicit zero-vector domains (#157).
- Add first safe-Rust compiler-owned x86-64 ELF64 integer-kernel object
  emitter using verified scalar IR (#158). No native executable compiler,
  new CLI command or stable ABI.
- Exclude draft Top-P PR #149, GPU/tensors, and unimplemented keywords.
- **Published as a GitHub experimental prerelease** on 8 November 2026 from
  exact approved source `f04be0758cbce405ddcdb229699b0ff83c09d207`
  after both seven-job source/control CI gates and seven public asset checks.
  See the [public release](https://github.com/scarp-lang/scarp/releases/tag/v0.0.6-alpha.4)
  and [postpublication verification](docs/releases/v0.0.6-alpha.4-validation.md).
  The [candidate notes](docs/releases/v0.0.6-alpha.4.md) remain historical.

## 0.0.6-alpha.3 — prerelease, 2026-11-07

- Add deterministic, bounded Top-K model score selection:
  `topk(scores, count)` and `topk_values(scores, count)`, stable
  first-index tie resolution, finite value preservation and explicit
  work/vector-allocation budgets ([#146](https://github.com/scarp-lang/scarp/pull/146)).
- Add typed temperature-controlled probability and log-probability
  functions, with strictly positive finite temperatures, compensated
  normalization, large-magnitude handling and checked finite outputs
  ([#147](https://github.com/scarp-lang/scarp/pull/147)).
- Harden native scalar preflight, add bounded compiler-owned typed
  expression/statement IR, test-only differential execution, and
  path-safe slot-initialization verification
  ([#141](https://github.com/scarp-lang/scarp/pull/141)–[#145](https://github.com/scarp-lang/scarp/pull/145)).
- Add runnable inference examples and a strict alpha.3 archive
  verification contract. No general native code generator or new CLI
  commands are included.
- Published as an experimental GitHub prerelease on 7 November 2026 from
  exact source `ceff07d9014f466974e0c9e02fc5d23573f55777`, with
  seven verified native assets and two passing seven-job CI gates.
  See the [public release](https://github.com/scarp-lang/scarp/releases/tag/v0.0.6-alpha.3),
  [release notes](docs/releases/v0.0.6-alpha.3-publication.md) and
  [verification record](docs/releases/v0.0.6-alpha.3-validation.md).
  [Candidate notes](docs/releases/v0.0.6-alpha.3.md) remain historical.

## 0.0.6-alpha.2 — prerelease, 2026-11-06

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
- Connect bounded edit-list preflight and transitive importer impact to
  three-tier cache retention; invalid edit input causes no cache mutation,
  and unknown canonical identities trigger conservative full eviction.
- Add opt-in exact-loader-identity eviction for individual source versions
  across semantic, lexical and syntax cache tiers, with preserved FIFO
  survivors, quota accounting and retained-program safety regressions.
- Add approved-source public release-description notes that omit
  unpublished-candidate wording, require them in each native package and
  regression-test immutable source-note selection and tamper rejection.
- Allow a separately reviewed, structurally valid release-request control PR
  to satisfy the CI documentation checks; publication still requires exact
  maintainer authorization and both revisions' seven passing jobs.
- Refactor the experimental C11 scalar backend to reuse one typed
  compiler-owned acceptance and bounded workspace preflight, with parity
  regressions; no new CLI command, object writer, or stable native ABI.
- **Published GitHub prerelease (not stable)** from exact source
  `398b8ff705154a02423fe458ce3a427401e81e31` after both seven-job
  source/control checks. See the
  [publication verification record](docs/releases/v0.0.6-alpha.2-validation.md)
  and [GitHub release](https://github.com/scarp-lang/scarp/releases/tag/v0.0.6-alpha.2).
  No general native compiler, language server or package-registry release.

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
  [v0.0.6-alpha.1 publication record](docs/releases/v0.0.6-alpha.1-validation.md).

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
