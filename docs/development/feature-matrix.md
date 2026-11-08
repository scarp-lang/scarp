# Feature completeness and development roadmap

Cargo declares `0.0.6-alpha.4` as the **published GitHub prerelease**.
The latest public prerelease is v0.0.6-alpha.4 (8 November 2026).
Earlier v0.0.6-alpha.3 introduced `topk`, `topk_values`, `softmax_temp`
and `logsoftmax_temp` and compiler-owned checked scalar IR.
Alpha.4 adds bounded sampled calculus, finite cosine similarity and
the first narrowly scoped compiler-owned x86-64 ELF64 integer-kernel
relocatable-object Rust API (one pure initializer). No general native
executable compiler or new CLI command is implied.
See the [alpha.4 public verification](../releases/v0.0.6-alpha.4-validation.md).
Top-P, Pearson correlation and multi-binding native dependencies are in
**post-alpha.4 draft PRs**, not in this published version.

Audit date: 2026-11-08. This inventory describes the published
**v0.0.6-alpha.4 prerelease** and development work remaining; update a row
when an implementation lands. No roadmap row implies a new CLI command.
The earlier 0.0.6-alpha.2 was published on 6 November 2026 from exact source
`398b8ff705154a02423fe458ce3a427401e81e31`;
see its [historical publication record](../releases/v0.0.6-alpha.2-validation.md).
The language remains experimental, without a stable public contract.
Future feature rows remain proposals.

Current audit baseline: `1aff6cabf93f2e0bce6953b29e1abe7d00ebd5fb`.
Historical initial main: `1d3b69a79bb0f656b4da3517560ce59e1e41f3bc`. Inspected development:
`05b4c40f63567267f5d9662ba8108d403d682f88` (148 main-only and 31 development-only
commits). That branch contains older native/project/CLI experiments and lacks
later numerical, module and safety work. It must not be merged wholesale.

PR #70 (inspection) and #71 (constants) were independently checked against their
seven successful CI jobs, then integrated in that order. Retargeting #71 to main
preserved its verified source tree. Integration commits are `4ec11a3` and `93ae364`.
The guarded prerelease publisher requires an explicit reviewed request pinned
to a verified source commit. Alpha.5 was published from source
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
| Parser and AST | Flat expressions, bounded statements, typed declarations | Composite type grammar | Existing colon/type grammar | P0 | S | AST spans, malformed annotations, depth | Grammar/API migration |
| Immutable/mutable bindings | Implemented bind/var and optional annotations | Deferred initialization remains intentionally absent | Parser, typed resolver | P0 | S | Inferred/annotated parity, mismatch, shadowing | Variables/tutorial |
| Initialization analysis | Initializers mandatory; resolve before insertion | Deferred/conditional initialization not designed | CFG/data-flow first | P2 | L | Self-use, branch joins, loops | Initialization contract |
| Lexical shadowing | Implemented | Rich pattern bindings | Resolver scopes | P2 | M | Shadowing initializer, duplicates, captures | Scope rules |
| Assignments | =, +=, -=, *=, /=, %=, &=, |=, ^=, <<=, >>=, >>>= | General composite-value mutation requires design | Typed resolver and evaluator | P2 | M | Type preservation, overflow, shift bounds, atomic output | Operators |
| Integer and finite-float math | Checked i64/binary64 and exact explicit conversions | Additional scalar math after domain design | Built-in registry and evaluator | P0 | S | Bounds, signed zero, loss of precision | Numeric reference |
| Scalar constants | Implemented by #71; annotations checked before evaluation | Approved constant intrinsics only after separate design | Shared expression typing/evaluation | P0 | M | Order/cycles, import visibility, task/node bounds | Constant guide |
| Bitwise/radix/remainder | Implemented in main and published alpha.3; tests/bit_operations.rs covers full-width operations | No missing signed-i64 operator in this subset | Current lexer, parser, resolver and evaluator | P0 | M | All shift counts, overflow, precedence, module parity | Values/operators |
| Strings | UTF-8 literals/equality, shared storage, bounded queries and construction | Normalization and richer text policy | Typed pure built-ins and work charging | P0 | M | Combining marks, emoji, empty input, limits | Text indexing contract |
| Characters and bytes | Missing | Distinct types, decoding and conversion rules | Type representation and allocation limits | P2 | L | Encoding/invalid sequences, host parity | Type proposal |
| Tuples/fixed arrays | Deferred; [proposal 0036](../proposals/0036-composite-values-and-tensor-boundaries.md), no source/runtime implementation | Types, literals, destructuring, indexing | Composite type representation | P1 | L | Nested types, length/index/type errors | Grammar/types |
| Dynamic typed lists | Numeric vectors only | General element types and mutation policy | Composite types, allocation budgets | P2 | XL | Aliasing, shape, limits, retirement | Collection contract |
| Maps/sets | Missing | Key equality/hash policy, deterministic iteration | Composite types and allocation accounting | P2 | XL | Collisions/order/limits, Unicode keys | Collection contract |
| Optional/structured errors | Missing | Tagged values and propagation | Variants and match typing | P1 | L | Success/error paths, exhaustive handling | Error model |
| Records/structures | Missing | Declaration, fields, construction, access | Nominal/structural type decision | P1 | XL | Recursive/nested fields, visibility | Type-system proposal |
| Enums/tagged variants | Missing | Constructors, pattern typing | Composite types and exhaustiveness | P1 | L | Exhaustive/redundant patterns, recursion | Variant/match design |
| Generics/type aliases | Missing | Type parameters, constraints, monomorphization policy | Composite types and stable IR | P2 | XL | Substitution, recursion bounds, diagnostics | Type-system design |
| Arithmetic/comparison/logic | Implemented; strict primitive kinds | Conditional expression form | Typed expression nodes | P1 | M | Precedence, short circuit, dead-branch typing | Expressions |
| Indexing/slicing | Numeric vectors/windows and tile cells/lanes | General strings/collections and multidimensional syntax | Composite types; text scalar policy | P1 | L | Bounds, visible windows, backing lifetime | Collection guides |
| If/while/repeat | Implemented and bounded | General collection iteration/ranges | Iterator/range type design | P1 | L | Empty/range bounds, controls, budgets | Iteration grammar |
| Break/continue/early return | Implemented with retirement | Labeled control only if justified | CFG and scope cleanup | P2 | M | Nested exits, contracts, retirement | Control flow |
| Pattern matching | Missing | Patterns, exhaustiveness, binding analysis | Variants/composite types | P1 | L | Missing/redundant arms, scope joins | Match design |
| Typed functions | Implemented | First-class callables and coherent overload design | Callable types and captures | P1 | XL | Resolution, nested calls, captured lifetime | Functions/effects |
| Named/default arguments | Implemented | Variadics only after collection/type design | Call signature representation | P2 | L | Ordering, duplicates, defaults, source sites | Argument rules |
| Closures/higher-order functions | Missing | Capture policy and escape/lifetime semantics | Callable values, effect rules | P2 | XL | Recursive capture, frame cleanup, quotas | Closure design |
| Pure functions/contracts | Implemented | General effect system | Explicit external capabilities first | P1 | L | Transitive purity, defaults/postconditions | Effects/contracts |
| Recursion/tail handling | Bounded heap frames; terminal self transfer | Mutual-tail optimization and profiling | Call-graph/IR analysis | P2 | L | Depth/call parity, postconditions | Runtime trade-offs |
| Module loading/visibility | Implemented; trusted roots and bounds | Explicit re-exports and standard modules | Export/alias contract | P1 | M | Re-export collisions/private access, cycles | Modules |
| Import/dependency inspection | #70/#71 integrated; schema-v1 JSON and constant edges implemented; project-directory metadata inspection published in alpha.5 | Navigation builds on this inventory | Existing immutable metadata | P0 | M | Exact schema/order/escaping/I/O/CLI | Inspection schema |
| Source navigation | Published v0.0.6-alpha.1: symbol/reference snapshots and static JSON/text CLI; development adds strict UTF-16 mapping and bounded version-guarded editor snapshots (#116) | Type-symbol navigation, LSP, rename and incremental invalidation | Shared linker scopes and immutable source maps | P0 | L | Cross-file, shadowing, Unicode, exact JSON, limits, no execution | [Navigation contract](../symbol-index.md) |
| Incremental checking/cache | Development: bounded invalidation planning, exact-input semantic reuse, per-file source-relative lexer tokens and **parsed AST caching** (#117); no incremental linker | Dirty-subgraph semantic link/type reuse and resident memory accounting remain missing | Compiler dependency graph and [invalidation contract](incremental-invalidation.md) | P1 | L | Diamond changes, deleted/added imports, retargeting, full CI; future cold-vs-cached parity | [Syntax cache and remaining gates](syntax-cache.md) |
| Module initialization | Intentionally absent | Effectful initialization requires separate design | Capability/effect and cycle policy | P3 | XL | Failure/ordering/deduplication | Initialization design |
| Typed execution plan | Implemented lowered slot/operation plan | Explicit stable IR and CFG views | Resolver plan invariants | P1 | L | IR/source mapping and control edges | IR architecture |
| Optimizations | Constant modules, slot retirement, terminal self transfer | General folding, DCE and CFG passes | IR/effect/budget-equivalence contract | P1 | L | Differential output/errors/budgets | Optimization semantics |
| Native backend | Experimental scalar C11 library emitter published in alpha.5, plus shared internal preflight; no native CLI | Reviewed compiler-owned typed IR, target ABI and resource limits | Current resolver, scalar preflight and budgets | P1 | XL | Differential interpreter/C, rejection parity and IR verifier | [Native IR proposal](../proposals/0037-compiler-owned-native-ir.md) |
| Native object writer | Not implemented; first x86-64 ELF64 relocatable target proposed only | Safe-Rust sections, symbols, relocations, host contract and byte limits | Verified private scalar IR and approved target ABI | P1 | XL | Exact object fixtures, independent decoding and host differential checks | [Native IR proposal](../proposals/0037-compiler-owned-native-ir.md) |
| Native build/compile CLI | Historical PR #22 only; unavailable on main | Safe output publication and later opt-in object CLI after codegen validation | Reviewed object encoder, platform/ABI compatibility and artifact safety | P2 | L | Existing destination, symlink, race and unsupported-target checks | [Native IR proposal](../proposals/0037-compiler-owned-native-ir.md) |
| Math/linear algebra library | Bounded vector/tile kernels and exact scalar conversions; published v0.0.6-alpha.1 RMS with independent Decimal oracle | Broader scalar math catalogue after domain design | Pure intrinsic dispatch | P0 | M | Numeric domains, finiteness, exact budgets | Built-in reference |
| Sampled scientific calculus | Merged on `main` in PR #153: pure `sample_area` trapezoid and `sample_slopes` interval derivatives; **not** in published alpha.3. Also includes merged `sample_cumulative_area` prefix integration, unreleased | Adaptive integration, pointwise derivatives, higher-order schemes | Finite vector registry and execution budgets | P1 | M | Analytic linear oracle, irregular grid, overflow, subnormal, shape, work and allocation | [Sampled calculus contract](../sampled-calculus.md) |
| General text/algorithms | Unicode length/codepoint/search/prefix/suffix implemented | General collection sorting/search | Allocation/work policy, collection types | P0 | M | Unicode semantics, deterministic search, limits | Standard library |
| Formatting/JSON/serialization | Versioned module metadata JSON implemented | Language value JSON, general formatting and serialization | Error/composite values; bounded parsing | P1 | L | Escaping, schemas, invalid data, limits | Format/version policy |
| Filesystem/network/process I/O | Explicit source loader only | External capability model and structured errors | Effects, permissions, errors | P2 | XL | Denied access, roots, no ambient authority | Security model |
| Dates/time/random/environment | Missing | Explicit nondeterministic host capabilities | Effects/host interfaces | P3 | L | Seed/replay, denied host calls | Capability APIs |
| Numerical AI kernels | Vectors/tiles, stable softmax/loss/gradients; published RMS | Generalized tensors/broadcasting and shape types (proposal 0036) | Composite numeric type/shape design | P1 | XL | Reference models, shapes, budgets, precision | Tensor proposal |
| CPU training/inference | Small classification examples implemented | Parameter/layer APIs and general models | Tensor/shape and serialization design | P2 | XL | Numerical oracles, convergence, reproducibility | AI tutorials |
| Automatic differentiation | Loss gradient kernels only | Graph/tape and chain-rule architecture | Tensor representation and lifetime budgets | P2 | XL | Finite differences, graph reuse, memory | AD design |
| GPU/backend abstraction | Missing | Explicit devices, transfer and backend contracts | General tensors and stable IR | P3 | XL | CPU differential, deterministic error rules | No GPU-support claim |
| Scope/frame retirement | Implemented, shared immutable numerical storage | Profiling and measured live-memory reporting | Runtime instrumentation contract | P1 | M | Nested scopes, exits, recursion and aliases | Memory trade-offs |
| Allocation budgets | Work/output/calls/slots/vector payload bounded | Unified measured statistics and string construction budget | Distinguish cumulative/live/process memory | P1 | L | Exact boundary and no-partial-output tests | Budgets/profiling |
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

## This cycle's complete vertical slices

1. Integrate verified inspection/constants (#70 then #71).
2. Optional binding/constant type annotations with strict inference compatibility.
3. Exact numeric conversions and bounded Unicode text queries, without new text
   allocation or ambient external effects.
4. Versioned module JSON inventories and diagnostic explanations for developer tools.

Each implementation batch must update its status, tests, examples and docs, use a
focused PR and pass CI before integration. Remaining rows are explicit future work.
