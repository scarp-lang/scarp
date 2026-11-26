# Feature completeness and development roadmap

Published `0.0.7-alpha.2` adds bounded native direct calls and an ELF64
object command. [Command and ABI metadata](../native-object-output.md) ·
[Publication verification](../releases/v0.0.7-alpha.2-validation.md)

Published `0.0.7-alpha.1` adds typed fixed-array concatenation, variadic
integer GCD, HCF and LCM, and bounded native loops in the Rust API.
[Publication verification](../releases/v0.0.7-alpha.1-validation.md)

Published `0.0.6-alpha.20` supports checked native integer functions with
one to six required parameters and forward scalar control flow.
[Target and ABI](../native-elf64.md) ·
[Publication verification](../releases/v0.0.6-alpha.20-validation.md)

Published `0.0.6-alpha.19` adds Rust-host callable lookup, checked rebinding,
and cumulative interpreter and host admission receipts.
[Public notes](../releases/v0.0.6-alpha.19-publication.md) ·
[Exact-source verification](../releases/v0.0.6-alpha.19-validation.md)

Published `0.0.6-alpha.18` adds unified zero-argument `scarp exec`,
automatic bounded typed input, fixed-array updates and early Lifetime
empty-view storage-retention hardening. Three native archives pass strict
clean-build reproducibility and exact-source release verification.
[Public notes](../releases/v0.0.6-alpha.18-publication.md) ·
[Exact-source verification](../releases/v0.0.6-alpha.18-validation.md)

Published `0.0.6-alpha.17` adds `array_len`, `array_get_or`, and `input_boolean`
to the interpreter and improves nested array type diagnostics.
[Public notes](../releases/v0.0.6-alpha.17-publication.md) ·
[Exact-source verification](../releases/v0.0.6-alpha.17-validation.md)

Published `0.0.6-alpha.16` adds bounded median and quantile, Unicode operations, and checked 64-bit
integer utilities.
[Public notes](../releases/v0.0.6-alpha.16-publication.md) ·
[Exact-source verification](../releases/v0.0.6-alpha.16-validation.md)

Published `0.0.6-alpha.15` adds checked shifts to integer ELF64 kernels,
validates loader revisions before semantic cache reuse, and meters
string comparisons.
[Public notes](../releases/v0.0.6-alpha.15-publication.md) ·
[Exact-source verification](../releases/v0.0.6-alpha.15-validation.md)

Published `0.0.6-alpha.14` adds detailed structured-pattern diagnostics, live
editor cache quotas for Rust hosts and checked integer operations in the
experimental Linux x86-64 ELF64 kernel API.
[Exact-source verification](../releases/v0.0.6-alpha.14-validation.md)

Published `0.0.6-alpha.13` adds cache retention and revision eviction APIs,
workspace-generation checks for editor results, and Option/Result payload reuse.
These are opt-in Rust host features; they do not constitute complete incremental
compilation or a native executable backend.
[Alpha.13 public notes](../releases/v0.0.6-alpha.13-publication.md)

Published `0.0.6-alpha.12` adds structured data and the multi-module task board.
[Alpha.12 verification](../releases/v0.0.6-alpha.12-validation.md)

Published alpha.11 adds exact Decimal values and typed decimal console input. [Alpha.11 verification](../releases/v0.0.6-alpha.11-validation.md)
The published alpha.10 adds typed console input,
EOF detection, stream flushing, exact output fixtures, fixed arrays and expanded
BigInt arithmetic. [Alpha.10 verification](../releases/v0.0.6-alpha.10-validation.md)

Alpha.9 added structured test reports and bounded BigInt arithmetic.
Alpha.8 added declarative projects, project tests, and assertions.
Alpha.7 introduced typed conditional expressions and Unicode scalar string escapes.
[Alpha.9 verification](../releases/v0.0.6-alpha.9-validation.md)

The published alpha.6 adds bounded program input, six scalar/text utilities and
immutable structural tuples. See
[publication verification](../releases/v0.0.6-alpha.6-validation.md) and
[Rust API migration](../tuples.md). Its [candidate notes](../releases/v0.0.6-alpha.6.md)
remain a historical preparation snapshot.

Published alpha.5 introduced deterministic `top_p`, `top_p_values`, centered
`pearson_correlation` and dependent integer bindings in the narrow ELF64 object
API; see its [verification](../releases/v0.0.6-alpha.5-validation.md).

Published alpha.4 introduced sampled numerical calculus, cosine similarity and
the first narrow ELF64 object writer; alpha.3 introduced `topk`,
`topk_values`, `softmax_temp` and `logsoftmax_temp`. No published
release provides a general native executable compiler, stochastic sampling,
GPU/tensor runtime or native `scarp compile` CLI command.
Future feature rows remain proposals; historical notes are preserved.

The earlier 0.0.6-alpha.2 was published on 6 November 2026 from exact source
`398b8ff705154a02423fe458ce3a427401e81e31`;
see its [historical publication record](../releases/v0.0.6-alpha.2-validation.md).
The language remains experimental, without a stable public contract.
Future feature rows remain proposals.

Historical audit baseline: `1aff6cabf93f2e0bce6953b29e1abe7d00ebd5fb`.
Historical initial main: `1d3b69a79bb0f656b4da3517560ce59e1e41f3bc`. Inspected development:
`05b4c40f63567267f5d9662ba8108d403d682f88` (148 main-only and 31 development-only
commits). That branch contains older native/project/CLI experiments and lacks
later numerical, module and safety work. It must not be merged wholesale.

PR #70 (inspection) and #71 (constants) were independently checked against their
seven successful CI jobs, then integrated in that order. Retargeting #71 to main
preserved its verified source tree. Integration commits are `4ec11a3` and `93ae364`.
The guarded prerelease publisher requires an explicit reviewed request pinned
to a verified source commit. The earlier v0.0.5-alpha.5 was published from source
`2eeed0aa52ccb59523307841ad8ffbd83f8911a8` with seven verified assets;
see the [publication record](../releases/v0.0.5-alpha.5-validation.md).
The completed request is retired in a post-publication PR; no package registry
publication occurred. The [alpha.4 record](../releases/v0.0.5-alpha.4-validation.md)
remains a historical snapshot.

Priorities: P0 current foundation; P1 next enabling work; P2 later; P3 long term.
Complexity: S local; M several compiler layers; L substantial design/runtime; XL
requires separate architecture and multiple implementation phases.

| Feature | Current status | Missing subfeatures | Dependencies | Priority | Complexity | Required tests | Documentation |
| --- | --- | --- | --- | --- | --- | --- | --- |
| UTF-8 source and lexical rules | Implemented; ASCII identifiers | Unicode identifier policy, additional escapes | Lexer/source spans | P2 | M | BOM, invalid UTF-8, boundaries, truncation | Lexical grammar |
| Parser and AST | Flat expressions, bounded statements, typed declarations | Further general composite types | Structural tuple and fixed-array grammar | P0 | S | AST spans, malformed annotations, depth | Grammar/API migration |
| Immutable/mutable bindings | Implemented bind/var and optional annotations | Deferred initialization remains intentionally absent | Parser, typed resolver | P0 | S | Inferred/annotated parity, mismatch, shadowing | Variables/tutorial |
| Initialization analysis | Initializers mandatory; resolve before insertion | Deferred/conditional initialization not designed | CFG/data-flow first | P2 | L | Self-use, branch joins, loops | Initialization contract |
| Lexical shadowing | Implemented | Rich pattern bindings | Resolver scopes | P2 | M | Shadowing initializer, duplicates, captures | Scope rules |
| Assignments | =, +=, -=, *=, /=, %=, &=, |=, ^=, <<=, >>=, >>>= | General composite-value mutation requires design | Typed resolver and evaluator | P2 | M | Type preservation, overflow, shift bounds, atomic output | Operators |
| Integer and finite-float math | Checked i64/binary64 and exact explicit conversions | Additional scalar math after domain design | Built-in registry and evaluator | P0 | S | Bounds, signed zero, loss of precision | Numeric reference |
| Scalar constants | Implemented by #71; annotations checked before evaluation | Approved constant intrinsics only after separate design | Shared expression typing/evaluation | P0 | M | Order/cycles, import visibility, task/node bounds | Constant guide |
| Bitwise/radix/remainder | Implemented in main and published alpha.3; tests/bit_operations.rs covers full-width operations | No missing signed-i64 operator in this subset | Current lexer, parser, resolver and evaluator | P0 | M | All shift counts, overflow, precedence, module parity | Values/operators |
| Strings | UTF-8 literals/equality, shared storage, bounded queries and construction | Normalization and richer text policy | Typed pure built-ins and work charging | P0 | M | Combining marks, emoji, empty input, limits | Text indexing contract |
| Scalar/text foundations (published alpha.6) | Six pure intrinsics for explicit decimal parsing/formatting, Unicode trim and literal replacement; absent from published alpha.5 | Char/byte types, split/join and general collections remain separate | Existing typed built-in registry and text/work budgets | P0 | M | Exact scalar round trips, invalid input, resource bounds, module/CLI parity | [Contract](../text-foundations.md) |
| Characters and bytes | Missing | Distinct types, decoding and conversion rules | Type representation and allocation limits | P2 | L | Encoding/invalid sequences, host parity | Type proposal |
| Immutable heterogeneous tuples | Published alpha.6; absent from alpha.5 | Empty tuples, dynamic indices and tuple constants deferred. Alpha.12 supports destructuring. | Structural SourceType, tuple values, registry and bounded interpreter integration | P0 | L | Creation, projection, calls/modules, equality/display, quotas, spans and host migration | [Tuple contract](../tuples.md) |
| Fixed typed arrays | Published alpha.10 supports homogeneous literals, typed empty arrays, checked indexing and whole-value replacement. Development adds length queries for counted traversal and typed fallback lookup. | Slicing, general iteration and native array ABI deferred | Structural source types, immutable payload and schema 3 | P1 | L | Length/index/type errors, quotas, output, module and host parity | [Fixed arrays](../fixed-arrays.md) |
| Dynamic typed lists | Published alpha.12 supports immutable typed lists and persistent updates | General collection iteration and mutable lists | Composite types, allocation budgets | P2 | XL | Aliasing, shape, limits, retirement | [Lists](../typed-dynamic-lists.md) |
| Maps/sets | Published alpha.12 supports immutable typed maps with canonically ordered projections | Sets and general map iteration | Composite types and allocation accounting | P2 | XL | Key order, exact quotas, Unicode keys | [Maps](../typed-maps.md) |
| Optional/structured errors | Published alpha.12 supports concrete Option and Result and `?` in immutable function bindings | Generalized error model | Variants and match typing | P1 | L | Success/error paths, exhaustiveness | [Option and Result](../option-result.md) |
| Records/structures | Published alpha.12 supports nominal records, field access, and nested binding patterns | Recursive layouts and field mutation | Nominal types and bounded layouts | P1 | XL | Nested fields, allocation, visibility | [Records](source-records.md) |
| Enums/tagged variants | Published alpha.12 supports nominal variants and checked payload matching | Nested tag tests and recursive layouts | Composite types and exhaustiveness | P1 | L | Exhaustive/redundant patterns, nominal identity | [Variants](source-variants.md) |
| Generics/type aliases | Missing | Type parameters, constraints, monomorphization policy | Composite types and stable IR | P2 | XL | Substitution, recursion bounds, diagnostics | Type-system design |
| Arithmetic/comparison/logic | Strict primitive kinds and typed conditional expressions | Conditional expressions in constants or native emission | Typed expression nodes | P1 | M | Precedence, short circuit, dead-branch typing | Expressions |
| BigInt | Published alpha.9 adds exact signed runtime arithmetic. Alpha.10 adds power, number theory, radix conversion, signed bitwise operations and divmod | Constant folding, large multiplication/division optimization and native lowering | Independent safe-Rust magnitude engine and shared Budget | P1 | L | Seeded Rust/Python oracles, budgets, CLI/module parity, opt-in large fixtures | [BigInt](../bigint.md) |
| Indexing/slicing | Numeric vectors/windows, tile cells/lanes, tuple projection and fixed-array indexing | General strings/collections and multidimensional syntax | Composite types and text scalar policy | P1 | L | Bounds, visible windows, backing lifetime | Collection guides |
| If/while/repeat | Implemented and bounded | General collection iteration/ranges | Iterator/range type design | P1 | L | Empty/range bounds, controls, budgets | Iteration grammar |
| Break/continue/early return | Implemented with retirement | Labeled control only if justified | CFG and scope cleanup | P2 | M | Nested exits, contracts, retirement | Control flow |
| Pattern matching | Published alpha.12 supports exhaustive tagged and scalar matching, and nested payload bindings | Guards and refutable nested patterns | Variants/composite types | P1 | L | Missing/redundant arms, scope joins | [Structured patterns](source-patterns.md) |
| Typed functions | Implemented | First-class callables and coherent overload design | Callable types and captures | P1 | XL | Resolution, nested calls, captured lifetime | Functions/effects |
| Named/default arguments | Implemented | Variadics only after collection/type design | Call signature representation | P2 | L | Ordering, duplicates, defaults, source sites | Argument rules |
| Closures/higher-order functions | Missing | Capture policy and escape/lifetime semantics | Callable values, effect rules | P2 | XL | Recursive capture, frame cleanup, quotas | Closure design |
| Pure functions/contracts | Implemented | General effect system | Explicit external capabilities first | P1 | L | Transitive purity, defaults/postconditions | Effects/contracts |
| Recursion/tail handling | Bounded heap frames; terminal self transfer | Mutual-tail optimization and profiling | Call-graph/IR analysis | P2 | L | Depth/call parity, postconditions | Runtime trade-offs |
| Module loading/visibility | Implemented; trusted roots and bounds | Explicit re-exports and standard modules | Export/alias contract | P1 | M | Re-export collisions/private access, cycles | Modules |
| Import/dependency inspection | #70/#71 integrated; schema-v1 JSON and constant edges implemented; project-directory metadata inspection published in alpha.5 | Navigation builds on this inventory | Existing immutable metadata | P0 | M | Exact schema/order/escaping/I/O/CLI | Inspection schema |
| Source navigation | Published v0.0.6-alpha.1: symbol/reference snapshots and static JSON/text CLI; development adds strict UTF-16 mapping and bounded version-guarded editor snapshots (#116) | Type-symbol navigation, LSP, rename and incremental invalidation | Shared linker scopes and immutable source maps | P0 | L | Cross-file, shadowing, Unicode, exact JSON, limits, no execution | [Navigation contract](../symbol-index.md) |
| Incremental checking/cache | Published in v0.0.6-alpha.2: bounded invalidation planning, exact-input semantic reuse, source-relative lexer token and parsed AST caching; no incremental linker | Dirty-subgraph semantic link/type reuse and resident memory accounting remain missing | Compiler dependency graph and [invalidation contract](incremental-invalidation.md) | P1 | L | Diamond changes, deleted/added imports, retargeting, full CI; future cold-vs-cached parity | [Syntax cache and remaining gates](syntax-cache.md) |
| Module initialization | Intentionally absent | Effectful initialization requires separate design | Capability/effect and cycle policy | P3 | XL | Failure/ordering/deduplication | Initialization design |
| Typed execution plan | Implemented lowered slot/operation plan | Explicit stable IR and CFG views | Resolver plan invariants | P1 | L | IR/source mapping and control edges | IR architecture |
| Optimizations | Constant modules, slot retirement, terminal self transfer | General folding, DCE and CFG passes | IR/effect/budget-equivalence contract | P1 | L | Differential output/errors/budgets | Optimization semantics |
| Native backend | Experimental scalar C11 library emitter in v0.0.5-alpha.5, compiler-owned verified scalar IR in v0.0.6-alpha.3, and narrow ELF64 object writer in v0.0.6-alpha.4; no native CLI | Reviewed compiler-owned typed IR, target ABI and resource limits | Current resolver, scalar preflight and budgets | P1 | XL | Differential interpreter/C, rejection parity and IR verifier | [Native IR proposal](../proposals/0037-compiler-owned-native-ir.md) |
| Native object writer | Experimental safe-Rust x86-64 ELF64 relocatable integer-kernel API published for one initializer in v0.0.6-alpha.4; multiple dependent integer `bind` initializers published in v0.0.6-alpha.5 | General-purpose lowering, checked runtime arithmetic, cross-platform object formats, executable linking and stable ABI remain missing | Verified private scalar IR and approved target ABI | P1 | XL | Exact object fixtures, independent decoding and host differential checks | [Native IR proposal](../proposals/0037-compiler-owned-native-ir.md) |
| Native build/compile CLI | Historical PR #22 only; unavailable on main | Safe output publication and later opt-in object CLI after codegen validation | Reviewed object encoder, platform/ABI compatibility and artifact safety | P2 | L | Existing destination, symlink, race and unsupported-target checks | [Native IR proposal](../proposals/0037-compiler-owned-native-ir.md) |
| Math/linear algebra library | Bounded vector/tile kernels and exact scalar conversions; published v0.0.6-alpha.1 RMS with independent Decimal oracle | Broader scalar math catalogue after domain design | Pure intrinsic dispatch | P0 | M | Numeric domains, finiteness, exact budgets | Built-in reference |
| Sampled scientific calculus | Published v0.0.6-alpha.4: bounded `sample_area`, `sample_slopes` and `sample_cumulative_area` for finite samples | Adaptive integration, pointwise derivatives, higher-order schemes | Finite vector registry and execution budgets | P1 | M | Analytic linear oracle, irregular grid, overflow, subnormal, shape, work and allocation | [Sampled calculus contract](../sampled-calculus.md) |
| General text/algorithms | Unicode length/codepoint/search/prefix/suffix implemented | General collection sorting/search | Allocation/work policy, collection types | P0 | M | Unicode semantics, deterministic search, limits | Standard library |
| Formatting/JSON/serialization | Versioned module metadata JSON implemented | Language value JSON, general formatting and serialization | Error/composite values; bounded parsing | P1 | L | Escaping, schemas, invalid data, limits | Format/version policy |
| Program line input | Published alpha.6 adds bounded line input. Alpha.10 adds explicit files, EOF fallback and opt-in flushed prompts | Broader I/O capability design | Explicit BufRead capability, independent input quotas and existing work/text/output budgets | P0 | M | UTF-8/CRLF/EOF, purity, atomic replay, static nonexecution, injected readers, CLI and exact limits | [Compiler input contract](../program-input.md) |
| Filesystem/network/process I/O | Explicit source loader only | External capability model and structured errors | Effects, permissions, errors | P2 | XL | Denied access, roots, no ambient authority | Security model |
| Dates/time/random/environment | Missing | Explicit nondeterministic host capabilities | Effects/host interfaces | P3 | L | Seed/replay, denied host calls | Capability APIs |
| Numerical AI kernels | Published: vectors/tiles, stable softmax/loss/gradients, RMS, alpha.3 Top-K and temperature-controlled softmax, alpha.4 cosine similarity; published alpha.5 adds deterministic Top-P and Pearson correlation | Generalized tensors/broadcasting and shape types (proposal 0036) | Composite numeric type/shape design | P1 | XL | Reference models, shapes, budgets, precision | Tensor proposal |
| CPU training/inference | Small classification examples implemented | Parameter/layer APIs and general models | Tensor/shape and serialization design | P2 | XL | Numerical oracles, convergence, reproducibility | AI tutorials |
| Automatic differentiation | Loss gradient kernels only | Graph/tape and chain-rule architecture | Tensor representation and lifetime budgets | P2 | XL | Finite differences, graph reuse, memory | AD design |
| GPU/backend abstraction | Missing | Explicit devices, transfer and backend contracts | General tensors and stable IR | P3 | XL | CPU differential, deterministic error rules | No GPU-support claim |
| Scope/frame retirement (Scarp Lifetime) | Implemented compiler-planned cleanup for lexical scopes and returns/loop transfers; immutable Arc-backed strings, vectors and composites. Draft PR #327 detaches empty numeric/integer windows from retained backing buffers; **not yet released**. | Measured peak live memory and RSS, closure/cycle/FFI/concurrency rules, native and tensor retention policies | Runtime diagnostics and escape-model design | P0 | XL | Nested exits, call frames, composite aliases, weak-reference release, budget and memory-retention regressions | [Lifetime](../memory-lifetimes.md) |
| Allocation budgets | Work/output/calls/slots/vector and string payload bounded; cumulative text construction limits already on main | Unified measured statistics and future general-composite budgets | Distinguish cumulative/live/process memory | P1 | L | Exact boundary and no-partial-output tests | Budgets/profiling |
| CLI exec/verify/inspect/create/launch | Implemented and published in alpha.4, including static Scarp.project verification | Additional commands require complete contracts | Source graph, manifest and diagnostics | P0 | M | Help/usage/native paths/status/output failures | CLI reference |
| fmt/test/bench/doc/doctor | Not CLI commands | Complete contracts/tool discovery before advertising | Parser/trivia or runner/host capabilities | P2 | L | Idempotence/discovery/status/side effects | CLI proposals |
| LSP/IDE/debugger | Development adds revision-guarded editor snapshots (#116); no LSP server or external deployment | Protocol, cancellation, document change transport and stepping | Stable diagnostics/source/reference metadata | P1 | XL | Protocol, edits/cancellation, UTF-16 mapping | IDE design |
| Package/build tooling | Cargo builds Scarp; published alpha.4 has bounded `Scarp.project` and interpreter-backed `create`/`launch` but no native build or registry | Dependency trust and lockfiles | Module/package identity design | P2 | XL | Reproducibility, traversal, offline cache | Package design |
| Interoperability/FFI | Rust host APIs only | Safe value/ABI and ownership boundaries | Stable IR/types/capabilities | P3 | XL | Lifetime/ABI/error/host permission tests | FFI design |
| Concurrency/async | Programs reusable across host threads | Language scheduling, synchronization, cancellation | Effects, values and runtime model | P3 | XL | Races/replay/deadlocks and quotas | Concurrency design |
| Performance/benchmarks | Runtime/numerics/compiler/module/JSON harnesses | Measured memory statistics and controlled optimization studies | Stable workload/reference recording | P1 | M | Repeated measurements, correctness oracles | Benchmark methodology |
| Security | Unsafe forbidden; bounded input/runtime; trusted source roots | External capabilities, hostile concurrent FS policy | Effects and host design | P1 | L | Escape/symlink/race/threat-specific tests | Threat model |
| Tests/CI/distribution | Stable/minimum debug/release; three native platforms | Broader feature corpus as rows land | Reproducible source/artifact evidence | P0 | M | Fmt/Clippy/docs/exact fixtures/native verification | Contribution/testing |
| Documentation/compatibility | In-repo guides and fixed release records | Synchronize separate website after scope/access established | Implemented contracts only | P0 | S | Relative links, runnable examples, snippets | Feature inventory stays current |

## Historical branch findings

Open PRs #16–#18 contain old loops/floats/vectors now independently implemented on
main. #19 contains older radix/bit/remainder code now independently implemented
and tested on current main (and shipped in alpha.3). #20 adds a freestanding
C11 scalar backend; #22 wraps it in build/compile commands. They use
old AST/type/operator/diagnostic conventions, reject newer types/functions/modules,
and need a deliberate port plus differential tests, not a wholesale merge. #23–#27
contain historical CLI/bind/output/function stacks already superseded on main.
Current main has a tested interpreter-backed project workflow and static
project-directory verification, not a native compiler, registry, general
tensor runtime or GPU backend. Historical branches were not wholesale merged.

## Historical completed development slices

1. Integrate verified inspection/constants (#70 then #71).
2. Optional binding/constant type annotations with strict inference compatibility.
3. Exact numeric conversions and bounded Unicode text queries, without new text
   allocation or ambient external effects.
4. Versioned module JSON inventories and diagnostic explanations for developer tools.

Each implementation batch must update its status, tests, examples and docs, use a
focused PR and pass CI before integration. Remaining rows are explicit future work.
