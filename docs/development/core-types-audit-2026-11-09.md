# Core data types and open PR audit — 9 November 2026

## Checkpoint and delivered work

The audited main is [`27a1e572f7fedb567319d3df83091fe4919089f2`](https://github.com/scarp-lang/scarp/commit/27a1e572f7fedb567319d3df83091fe4919089f2).
The latest published prerelease is
[v0.0.6-alpha.5](https://github.com/scarp-lang/scarp/releases/tag/v0.0.6-alpha.5),
published 2026-11-09T11:39:41Z from source
`205e92f166f186c3905e5880d41c31717b697b8e`. Later main changes record and retire
that publication request; they do not make unmerged draft features available.

This branch implements six bounded, pure scalar/text operations:
`integer_to_text`, `float_to_text`, `text_to_integer`, `text_to_float`,
`text_trim` and `text_replace`. The [exact contract](../text-foundations.md)
defines decimal grammar, binary64 rounding, Unicode, error spans and resource
charges. Nineteen new tests cover runtime, type checking, functions, contracts,
source modules, CLI stages, diagnostics and backend rejection. The executable
[example](../../examples/text_foundations.srp) and its stdout fixture are compiler
regressions. These additions are absent from published alpha.5.

The existing BigInt implementation was reused through
[PR #172](https://github.com/scarp-lang/scarp/pull/172), rather than duplicated.
Its history-preserving main reconciliation is
[`b0134e2acc7e795c95ee27a34175e80bcf80e85f`](https://github.com/scarp-lang/scarp/commit/b0134e2acc7e795c95ee27a34175e80bcf80e85f).
That independent draft retains published-release provenance, current native
regressions and all BigInt safeguards. It is not merged or released.

No PR was merged/closed, no issue/PR discussion comment was posted, and no
release/tag was created by this work. Updating #172's description records the
new tested source; it does not submit a review or imply approval.

## Audit method and limits

The [machine-readable checkpoint](core-types-pr-audit-2026-11-09.json) records all
15 open PRs, exact head/base SHAs, changed-file counts, review/thread/comment
counts, exact-head workflow/job results, main divergence, simulated merge
conflicts, and SHA-256 hashes of the reviewed patches. PR descriptions, source
diffs, tests and review/discussion state were inspected. Diffs were compared
with current main where overlap affects the recommendation. Historic branches
were not all executed locally; their CI results are attributed to their recorded
heads and declared bases. This is a foundation/integration audit, not a complete
security or performance certification of every historical branch.

All 15 had zero submitted reviews and zero inline review threads at the
checkpoint. Only #22 and #24 had discussion comments (one existing drift warning
each); the evidence includes their URLs. There is consequently no unresolved
inline review conversation to dismiss. A green historical stack is not proof
that its patch safely merges into today's main.

## Data-type completeness matrix

Status refers to audited main unless a draft PR is named. P0 means immediate
foundation, P1 next enabling work, P2 dependent work, P3 later architecture.
S/M/L/XL denote local, multi-layer, substantial and multi-phase changes.

| Type or foundation | Implemented now | Gap / decision | Dependency | Priority / size | Verification needed |
| --- | --- | --- | --- | --- | --- |
| `integer` | Checked signed i64; full-width arithmetic, remainder, radix/bitwise/shifts; explicit conversions | Wider widths need an actual storage/FFI use case; no implicit widening | Existing scalar rules | P0 / S | Extremes, divide/shift edges, annotations/constants/modules; existing suites |
| `float` | Finite binary64, scientific literals, signed zero/subnormals, domain/overflow checks | No exact decimal or f32 type | Explicit conversion and finiteness invariant | P0 / S | Boundary rounding, signed zero, underflow/overflow, mixed-type rejection |
| `boolean` | Strict logical type, equality, short circuit and conditions | No implicit truthiness intended | Existing resolver/operators | P0 / S | Unselected paths still type checked; skipped runtime faults |
| `string` | Immutable UTF-8; literals/escapes/equality, scalar queries, search, concat/slice/repeat, bounded storage/output | Ordering, split/join, broader escapes/formatting/normalization not supplied | Explicit Unicode and collection policy | P0 / M | Scalar/byte/grapheme distinction, Unicode/NUL, snapshots and quotas |
| Scalar/text boundary | Missing on audited main; six intrinsics implemented in this draft | Integrate after review; constants/native remain explicit rejection | Typed built-in registry, work and text budgets | P0 / M | 19 new tests and byte-exact example; full stable suite |
| BigInt | Draft #172: exact signed arbitrary precision within configured bounds | Integrate existing work; no bitwise/float/constant/native support | Exact type/storage/CLI/budget contract | P0 / L | 897 tests/profile, Python oracle, minimum Rust, packages |
| `character` | Not implemented; code-point queries currently return integer | Scalar invariant, literal/escape grammar, explicit checked conversions | Reviewed primitive design and all type/value layers | P1 / M | Surrogates/range, multi-scalar strings, spans, modules, rejection paths |
| `byte` | Not implemented | 0..255 invariant, arithmetic versus no arithmetic, text encoding policy | Reviewed primitive/binary-data boundary | P1 / M | Checked conversions, limits, no truncation or implicit UTF-8 claim |
| `unit` | Internal marker and function result kind | Not ordinary absence/failure/null; no optional semantics | Existing call/result rules | P0 / S | Return checking and non-value restrictions |
| `vector`, `integers`, `tile` | Specialized immutable finite-float vectors, i64 vectors and 2-axis float tiles; bounded kernels/views | Not general arrays/lists/tensors | Existing specialized types and payload budgets | P0 / M | Shape, visible windows, finiteness, aliasing/output and allocation |
| General fixed arrays | Deferred proposal 0036, metadata descriptor only | Source grammar, type expressions, runtime storage/indexing/equality | Uniform resolved TypeId through calls/modules | P1 / XL | Zero/max length, mixed types/counts, functions/modules, quotas |
| Dynamic lists | Not implemented | Length-independent type, immutable growth and accounting | Completed source collection/type boundary | P2 / L | Append/slice/index/snapshot and resource behavior |
| Heterogeneous tuples | Host descriptors only | Source values, projection, member types; no parser-only shortcut | Uniform composite TypeId and Value storage | P2 / L | Member types, functions/modules/metadata, empty tuple/unit policy |
| Maps and sets | Not implemented | Key/hash rules, deterministic order, collision/work policy | Collections plus stable key equality policy | P2 / XL | Deterministic output/iteration, malformed keys, bounded construction |
| Records | Not implemented | Nominal identity, fields/visibility/initialization | Source type declarations and module identity | P2 / XL | Missing/duplicate/wrong fields, exports and immutable replacement |
| Enums/tagged variants | Not implemented | Tags, payload types and construction | Nominal type declarations | P2 / XL | Exhaustive identity/payload/visibility checks |
| Optional / structured result | Not implemented | Explicit absent/present and success/error alternatives | Tagged variants, then control-flow design | P2 / XL | Never conflate unit/false/zero/empty with absence or errors |
| Pattern matching | Not implemented | Exhaustiveness, binding scope and evaluation order | Completed variants and control-flow analysis | P2 / L | Unreachable/missing arms, returns, nested patterns and budgets |
| Generics / general tensors | Host type descriptors do not implement either | Constraints/instantiation and rank/shape policy respectively | Uniform types, collections, nominal declarations | P3 / XL | Bounded metadata/instantiations, shape products and differential kernels |
| Exact decimal | Not implemented; historical #17 is binary64 | Coefficient/scale, precision, rounding/division and resource design | Reviewed numeric design; possible BigInt coefficient reuse | P2 / XL | Independent decimal oracle and rounding/scale extremes |

No new reproducible scalar correctness defect was established on current main
in this audit. Missing features above are recorded as missing, not relabeled
as bugs. #149's old Top-P branch has weaker probability-domain validation than
the later integrated implementation; reintroducing it would regress behavior.
String construction quotas already exist on main; their absence in older
roadmap wording was documentation drift, corrected in the matrix.

The host type registry can intern structural metadata without executing it.
`TypeAnnotation.kind`, function parameter/result kinds and runtime `Value` are
still primitive boundaries. [The staging proposal](../proposals/core-types-staging.md)
specifies a complete transition before general arrays, rather than exposing
metadata as if it were a usable source language type.

## All open compiler PRs

CI below is successful jobs / recorded jobs at the exact checkpoint head.
“Conflicts” means simulated integration with main `27a1e57`, not necessarily
the PR's historical declared base. Full hashes, workflow links and individual
job identities are in the JSON evidence. Counts before #172's follow-up are
preserved rather than silently rewriting that checkpoint.

| PR | Reviewed head | Declared base | CI | Current-main result and disposition |
| --- | --- | --- | --- | --- |
| [#173 tutorial navigation](https://github.com/scarp-lang/scarp/pull/173) | `8d57149affab` | main | 7/7 | README/docs-index conflicts; blocked on external destination PRs/deployment and alpha.5 wording. Preserve compiler fixtures. |
| [#172 bounded BigInt](https://github.com/scarp-lang/scarp/pull/172) | `8e2728e2a064` | main | 7/7 | High-priority reuse; initial README/matrix conflicts reconciled by `b0134e2acc7e`. Fresh 7/7 now green; needs review, not a duplicate engine. |
| [#171 order statistics](https://github.com/scarp-lang/scarp/pull/171) | `ff876ae973c6` | main | 7/7 | README/docs-index conflicts; useful median/type-7 quantile slice, deferred behind foundations and revalidation on current main. |
| [#149 Top-P](https://github.com/scarp-lang/scarp/pull/149) | `2567b9a60436` | main | 7/7 | Seven content conflicts; superseded by later integrated #161/#167. Current main validates each probability in [0,1] before accumulation; preserve that correction. |
| [#27 typed functions](https://github.com/scarp-lang/scarp/pull/27) | `6a0d1e7b6fe5` | scarp/output-statements | 8/8 | Old stack conflicts; functions/contracts/defaults/named args/heap frames already supplied by main. Superseded; first-class callables remain separate. |
| [#26 output streams](https://github.com/scarp-lang/scarp/pull/26) | `4b1b3e2532c7` | scarp/bind-var-syntax | 8/8 | Interpreter streams/arguments exist. Historical C emitter stream ABI is not supplied by current native IR; partially superseded, any unique native work needs a fresh port. |
| [#25 bind/var](https://github.com/scarp-lang/scarp/pull/25) | `3449af94910a` | scarp/canonical-cli | 8/8 | Syntax already implemented with later safeguards/docs; superseded, do not merge old stack. |
| [#24 canonical CLI](https://github.com/scarp-lang/scarp/pull/24) | `984c59e1cc1c` | scarp/cli-foundation | 8/8 | Breaking no-alias policy conflicts with current compatibility aliases; deferred design, not a safe foundation fix. |
| [#23 CLI foundation](https://github.com/scarp-lang/scarp/pull/23) | `2553e02ddaef` | scarp/native-build-cli | 8/8 | Static verify/inspection/guidance mostly independently integrated. Review only unique remaining ideas; do not revive obsolete backend assumptions. |
| [#22 native CLI](https://github.com/scarp-lang/scarp/pull/22) | `49549f8cdb16` | scarp/freestanding-c-backend | 8/8 | Native build/compile CLI remains unavailable on main. Deferred backend-driver work with extensive conflicts; not fully superseded, not merge-ready. |
| [#20 freestanding C](https://github.com/scarp-lang/scarp/pull/20) | `b47d69dc320d` | scarp/integer-bit-operations | 8/8 | Partial successor in current C11/Rust-host verified scalar IR; old emit-c CLI/host ABI differ. Port unique pieces only after current backend design review. |
| [#19 bit operations](https://github.com/scarp-lang/scarp/pull/19) | `fdd9a8effdef` | scarp/numeric-vectors | 7/7 | i64 radix/masks/shifts/remainder now covered by main and current tests; superseded. |
| [#18 numeric vectors](https://github.com/scarp-lang/scarp/pull/18) | `90be3968799b` | scarp/finite-decimals | 7/7 | Specialized numeric values/kernels independently implemented and extended; superseded, not a general-list implementation. |
| [#17 finite decimals](https://github.com/scarp-lang/scarp/pull/17) | `9a14eb18ea21` | scarp/bounded-loops | 7/7 | This patch uses finite binary64 despite its title. Superseded scalar-float subset; cannot claim exact Decimal support. |
| [#16 bounded loops](https://github.com/scarp-lang/scarp/pull/16) | `6bc405b72191` | scarp/execution-workspace | 7/7 | Head is already an ancestor of main (0 head-only commits); integrated feature, owner may reconcile stale PR metadata separately. |

Each old-stack PR #16–#27 lacks 537 commits on current main. Their declared-base
mergeability and old CI cannot authorize a wholesale merge. #16's own head is
already contained; later stack heads introduce 4–48 historical commits and
conflict with today's runtime, CLI, docs and tests. The four modern PRs initially
reported conflicts to main. #172 alone was changed here; parallel work on
#171/#173 was left intact for its owner.

## Follow-up BigInt evidence

On reconciled head `b0134e2acc7e795c95ee27a34175e80bcf80e85f`:

- All **897 tests passed in debug and release** with stable Rust locally.
- [Rust checks](https://github.com/scarp-lang/scarp/actions/runs/42637685941)
  passed all four jobs: stable Linux/Windows/macOS and minimum Rust 1.74.0.
- [Package CI](https://github.com/scarp-lang/scarp/actions/runs/42637685893)
  passed all three jobs: Linux x64, Windows x64 and macOS Apple Silicon.
- The independent Python oracle passed **304 pairs / 3,344 arithmetic and
  comparison results**, exec/check parity and static verify. Opt-in controlled
  linear-operation cases passed at 19, 100, 1,000, 10,000, 100,000 and 1,000,000
  decimal digits. This is not a claim of practical million-digit quadratic
  multiplication/division; the general-operation benchmark stops at 10,000.
- Published alpha.5 files and source remain unchanged. No new public download
  includes BigInt; CI archives remain development artifacts.

This text branch and #172 are independent main-based drafts. Both extend the
built-in registry, so their eventual integration requires resolving that shared
file, then testing the combined tree. Separate green results do not establish
combined-source compatibility, nor authorize publishing either branch.

## Text-branch validation and boundaries

The main baseline has 872 tests/profile. This branch has **891 tests/profile**
with zero failures, including 19 new tests and a deterministic 10,000-value
scalar round-trip corpus. `SCARP_REQUIRE_C=1 sh scripts/check.sh --offline`
completed with Rust 1.99.1 on Linux: fmt, warnings-as-errors Clippy, debug/release,
Rust documentation, host examples, language examples and static/evaluating
checks all passed. The stdout fixture is tested byte for byte.

Rust 1.74.0 independently passed all 891 tests in both debug and release.
Runtime, numerics and compiler benchmark harnesses completed their `--quick`
correctness smoke runs; timings from this shared environment are not a performance
improvement claim. The independent standard-library Python oracle passed
**1,010 results** covering seeded i64 values, finite binary64 bit patterns,
Unicode trim and literal replacement, plus static verify/evaluating check parity:

```sh
python3 scripts/test_text_foundations.py --scarp target/debug/scarp
```

No primitive identity, diagnostic number, runtime option field or Cargo dependency
was added. Existing `ExecutionLimits`/`ExecutionOptions` shapes remain intact.
Host exhaustive `Builtin` matches need the usual additions. New calls are
accepted by static verify but malformed decimal text fails only when evaluated;
module constants and experimental native C11/ELF reject them explicitly.

Character/byte values, general arrays/lists/tuples, records, variants,
Option/Result, stdin, full generics, exact decimal and native executable tooling
are not implemented by this PR. Their dependencies and test gates are in the
staging proposal. No line-count or “all basic types complete” claim is made.

## Migration, manual steps and next delivery

The navigation draft #173 depends on
[Docs #91](https://github.com/scarp-lang/scarp-docs/pull/91), head
`52a35dad192bf91c138d812e5849dddbee7bab75`, and
[By Example #57](https://github.com/scarp-lang/scarp-by-example/pull/57), head
`860eb60750bdede49bc3b1bc5cdc0a38c68d7b02`. Both were open/unmerged at this
checkpoint. Independent public-browser checks returned 404 for all three
referenced guide URLs:

- https://scarp-lang.github.io/scarp-docs/guide/first-program-alpha4
- https://scarp-lang.github.io/scarp-docs/hi/guide/first-program-alpha4
- https://scarp-lang.github.io/scarp-docs/language/scientific-kernels-alpha4

Those results establish that these destinations were unavailable at the check,
not that an unpublished preview build is broken. An owner continuing migration
should review the By Example and Docs changes, resolve current compiler-doc
conflicts, deploy the approved destination changes, then verify the live English
and Hindi URLs before replacing compiler guide links. Technical contracts,
regression fixtures and fixed historical release documents stay in this repo.
No deployment or merge was performed here.

No issue comment or issue creation is required to use this delivery. If the owner
wants to communicate the migration blocker manually, this ready-to-paste note
belongs on [PR #173](https://github.com/scarp-lang/scarp/pull/173):

> Review checkpoint 2026-11-09: please reconcile README/docs-index with published
> alpha.5 main and keep compiler regression fixtures. Docs #91 and By Example
> #57 are still open; the three linked public guide routes currently return
> 404. Please deploy and verify those destinations, including Hindi, before
> merging the compiler redirects. Current green CI does not verify live routes.

Next, review these two bounded foundation drafts, reconcile their shared
built-in registry if both are accepted, and run combined validation. Then choose
the reviewed character/byte design or uniform source-type migration as a new
complete slice. A prerelease needs an approved exact final source and version,
fresh CI, verified native archives and aligned versioned learner examples; this
audit does not bypass those gates or request approval for an unfinished release.
