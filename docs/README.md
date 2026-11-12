# Scarp documentation

**0.0.6-alpha.7 prerelease** was published on **11 November 2026**, adding
typed conditional expressions and Unicode scalar string escapes.
[Public notes](releases/v0.0.6-alpha.7-publication.md) ·
[Exact-source verification](releases/v0.0.6-alpha.7-validation.md)

**0.0.6-alpha.6 prerelease** was published on **10 November 2026**, adding
bounded input, scalar/text utilities and immutable tuples. See [public notes](releases/v0.0.6-alpha.6-publication.md)
and [exact-source verification](releases/v0.0.6-alpha.6-validation.md).

**0.0.6-alpha.5 prerelease** is **published on 9 November 2026**,
with pure [Top-P selection](nucleus-selection.md), [Pearson correlation](pearson-correlation.md)
and [limited ELF64 native binding dependencies](native-elf64.md).
The [public GitHub release](https://github.com/scarp-lang/scarp/releases/tag/v0.0.6-alpha.5),
[exact-source verification](releases/v0.0.6-alpha.5-validation.md),
and [public release notes](releases/v0.0.6-alpha.5-publication.md)
identify all seven audited assets. The [candidate notes](releases/v0.0.6-alpha.5.md)
remain an immutable historical snapshot.

**0.0.6-alpha.4 prerelease** was published on 8 November 2026.
Its [release](https://github.com/scarp-lang/scarp/releases/tag/v0.0.6-alpha.4)
and [verification record](releases/v0.0.6-alpha.4-validation.md)
preserve the first ELF64 object-writer and sampled-calculus milestone.
The [alpha.4 candidate notes](releases/v0.0.6-alpha.4.md) also remain historical.

**Published experimental native feature:** [the first ELF64 integer-kernel
object writer](native-elf64.md) uses the verified private scalar IR.
It does not implement a general native compile command, executable linker
or stable ABI.

**0.0.6-alpha.3 prerelease** was **published on 7 November 2026**.
It includes [Top-K ranking](topk-selection.md),
[temperature softmax](temperature-softmax.md), and verified private
scalar IR. The [public release](https://github.com/scarp-lang/scarp/releases/tag/v0.0.6-alpha.3)
and [exact-source verification record](releases/v0.0.6-alpha.3-validation.md)
identify the annotated tag and seven verified assets. The
[candidate notes](releases/v0.0.6-alpha.3.md) remain a historical snapshot.

**0.0.6-alpha.2 prerelease** is **published** (6 November 2026).
It includes optional [invalidation planning](development/incremental-invalidation.md),
[source-relative token caching](development/token-cache.md),
[parsed AST caching](development/syntax-cache.md),
[whole-graph semantic caching](development/semantic-cache.md), and
[edit-driven cache eviction](development/cache-edit-planning.md).
See the [public alpha.2 release](https://github.com/scarp-lang/scarp/releases/tag/v0.0.6-alpha.2)
and [publication verification](releases/v0.0.6-alpha.2-validation.md).
These are opt-in Rust APIs, not new CLI commands.

**0.0.6-alpha.1 prerelease** is published (5 November 2026), adding
[bounded symbol navigation](symbol-index.md) and [RMS](root-mean-square.md);
[release notes](releases/v0.0.6-alpha.1.md) describe the implementation and
deferred features. See the
[publication verification record](releases/v0.0.6-alpha.1-validation.md)
for exact source, CI and public archive digests.

**0.0.5-alpha.5 prerelease** is published with project-directory module
inspection and an opt-in experimental C11 scalar backend for Rust hosts;
see [alpha.5 release notes](releases/v0.0.5-alpha.5.md) and the
[publication record](releases/v0.0.5-alpha.5-validation.md) for limits and verification.

Development declarations use `bind` and `var`; see the [migration guide](syntax-migration.md). Published tags retain their original syntax.

Choose a guide by the task you want to complete. Current tutorials are
updated for the **0.0.6-alpha.6 published prerelease**, retaining
[source modules](modules.md), numerical vectors/tiles, typed functions,
contracts, loops and the Scarp-first CLI.
The latest published prerelease is
[0.0.6-alpha.7](https://github.com/scarp-lang/scarp/releases/tag/v0.0.6-alpha.7),
with native archives and checksums for Windows x64, Linux x64 and macOS Apple
Silicon. [Alpha.1 preparation notes](releases/v0.0.5-alpha.1.md) preserve that
earlier candidate contract. [Repository migration](migrations/scarp-lang-Scarp.md)
records the verified maintainer transfer to scarp-lang/scarp. Historical release
notes describe fixed snapshots. Tensors, general native executable compilation, a package manager and
website deployment remain separate.

**Published tuple support:** [immutable heterogeneous tuples](tuples.md) are implemented
in published alpha.6 and absent from alpha.5. Review the structural
Rust API/schema migration before adopting that source.

## Unreleased development documentation

The following guides describe **draft, unmerged compiler work** and are
**not features of the published v0.0.6-alpha.7 release**:

- [Declarative project manifest](project-v2.md) — draft PR #217.
- [Project test runner and JSON reporting](project-testing.md) — drafts
  #218 and #220.
- [Read-only project metadata](project-info.md) — draft PR #219.
- [Native source-language assertions](assertions.md) — draft PR #221:
  `assert(boolean_condition);` or an optional bounded literal message.

These guides specify development branch behavior only. Feature availability
must be verified against the exact compiler commit; no merging, tagging,
or publication is implied by their presence here.

## Guides and what each adds

The [feature-completeness matrix](development/feature-matrix.md) distinguishes
implemented features, historical experiments and future dependencies. Current
declarations accept explicit primitive annotations; see [variables](variables.md).

For foundation design history, the [9 November core-type and PR audit](development/core-types-audit-2026-11-09.md)
records the exact main/release checkpoint and every open compiler PR. The
[scalar/text contract](text-foundations.md) describes six intrinsics on
published alpha.6, absent from alpha.5. The
[type staging proposal](proposals/core-types-staging.md) keeps future character,
byte and general collection support distinct from implemented source types.

The published alpha.3 compiler introduced
[module inspection](module-inspection.md) and [constant expressions](constant-expressions.md).
The older alpha.1 snapshot lacks those capabilities. Published alpha.4 adds
project creation, launching and static directory verification; see the
[project guide](project-workflow.md), [release notes](releases/v0.0.5-alpha.4.md),
[verification record](releases/v0.0.5-alpha.4-validation.md) and
[CLI reference](reference/cli.md) for version-specific availability. Published
alpha.5 additionally includes project-aware module inspection and the
experimental Rust-hosted C11 scalar emitter; see
[alpha.5 notes](releases/v0.0.5-alpha.5.md).

| Document | Use it to |
| --- | --- |
| [JSON inspection and explain](module-json.md) | Consume versioned module metadata and repair diagnostic codes without source I/O |
| [Scalar conversions and text](scalar-text.md) | Convert exactly and query immutable Unicode strings with bounded work |
| [Getting started](getting-started.md) | Select a version, build the intended checkout, and verify your executable |
| [Your first program](tutorials/first-program.md) | Learn the file-to-output workflow, comments, escapes, and a first syntax repair |
| [Source modules](modules.md) | Organize files, choose public/private declarations and understand loading limits |
| [Module inspection](module-inspection.md) | Inspect files, import edges, dependencies, signatures and source locations without execution |
| [Constant expressions](constant-expressions.md) | Compile bounded scalar expressions, use earlier/imported constants and inspect reference metadata |
| [Import tutorial](tutorials/modules.md) | Run a three-file numerical program and repair private-member/cycle errors |
| [Finite floats](floats.md) | Use the alpha's binary64 scalars, numeric diagnostics, and function integration |
| [Numerical vectors](vectors.md) | Use immutable numerical data, pure kernels, typed vector calls and bounded allocation |
| [Sampled calculus (published)](sampled-calculus.md) | Integrate measurements, accumulate prefix areas and calculate bounded interval derivatives with checked coordinates |
| [Cosine similarity (published)](cosine-similarity.md) | Compare orientation of two finite vectors using bounded, independently scaled numerical reductions |
| [Top-K inference ranking](topk-selection.md) | Select deterministic ranked model candidates and original scores using two published typed built-ins |
| [Top-P nucleus ranking](nucleus-selection.md) | Rank a deterministic normalized probability prefix with stable tie ordering; published alpha.5, not random sampling |
| [Pearson correlation](pearson-correlation.md) | Compute centered finite vector correlation with independent scaling and strict input limits; published alpha.5 |
| [Temperature softmax](temperature-softmax.md) | Adjust finite logit probabilities and log probabilities without randomness or nonfinite values |
| [Root mean square](root-mean-square.md) | Use the published v0.0.6-alpha.1 finite RMS reduction, visible windows and exact work/allocation rules |
| [Your first numerical program](tutorials/numerical-program.md) | Build a small typed numerical calculation and compare probabilities safely |
| [Numerical errors](reference/numerical-errors.md) | Repair type, shape, index, domain, overflow and quota failures |
| [Functions](functions.md) | Learn typed calls, defaults, contracts, and bounded invocation resources |
| [Program input (published alpha.6)](program-input.md) | Review explicit readers, UTF-8 lines, quotas and terminal restrictions; absent from published alpha.5 |
| [Output](output.md) | Choose print/write, control newlines, and route explicit stderr |
| [Variables and mutation](tutorials/variables.md) | Introduce names, copy values, choose mutability, and avoid forward references |
| [Scopes and shadowing](tutorials/scopes.md) | Predict which declaration a name uses and what survives a block |
| [Decisions](tutorials/decisions.md) | Select an if/elif/else body and distinguish static checks from executed work |
| [Loops and budgets](proposals/0006-bounded-loops.md) | Repeat computations with scoped state, loop control, and per-call bounds |
| [Iteration](iteration.md) | Choose while/repeat, understand count snapshots, immutable indices, and nested control |
| [Execution budgets](budgets.md) | Set work/print limits, interpret failures, and embed bounded execution |
| [Command-line reference](reference/cli.md) | Look up commands, version availability, paths, streams, and exit codes |
| [Scarp projects](project-workflow.md) | Use `create`/`launch`/`verify DIRECTORY` from published alpha.4 onward |
| [Diagnostic reference](reference/diagnostics.md) | Interpret all error codes and repair common failures |
| [Values and operators](reference/values.md) | Check precedence, numeric boundaries, assignment rules, and string limits |
| [Rust embedding](embedding.md) | Select a library stage, capture output, reuse compiled programs, and render errors |
| [Testing changes](testing.md) | Choose relevant tests, add exact-output examples, and validate documentation |
| [Troubleshooting](troubleshooting.md) | Separate installation, path, encoding, version, language, and package failures |
| [Compatibility](compatibility.md) | Compare released/development behavior and migrate an embedding host |
| [Performance](performance.md) | Reproduce stage benchmarks and assess measured runtime improvements |

## Alpha.4 numerical and runtime prerelease

Read [tiles and linear algebra](tiles.md) for immutable two-axis values,
matrix products, arithmetic, shape rules, costs and seven executable examples.
[Automatic value retirement](memory-lifetimes.md) covers blocks, loops,
function completion and shared immutable snapshots.

The release also integrates windows/stitching, log-space kernels, selection,
and categorical loss/gradient helpers developed since alpha.3. The
[publication record](releases/v0.0.4-alpha.4-validation.md) identifies the approved
source, passing CI and verified public packages. Published alpha.3 assets remain unchanged.

## Reading paths

For a first program, follow getting started, the first-program tutorial, then
variables and scopes on the development branch. Keep the CLI reference open
when switching between installed binaries and Cargo commands.

For a failure, identify the executable and exit code in troubleshooting, then
look up the diagnostic. Read both primary and related source locations before
changing a declaration.

For a Rust host, begin with compatibility and embedding. Parsing, compilation,
validation, and execution intentionally have different results and effects.

For a contribution, read testing and the root
[contribution guide](../CONTRIBUTING.md). The variable milestone is integrated
into main; further work uses separate branches and PRs.

## Exact rules and project records

The tutorials teach complete tasks. The existing [language reference](language.md)
and [variable reference](variables.md) spell out the implemented semantics.
[Architecture notes](architecture.md) explain the module and execution boundaries.

Design records cover [integer expressions](proposals/0001-integer-expressions.md),
[variables](proposals/0002-variables.md), and
[string values](proposals/0003-string-bindings.md). Read each record's status;
a proposed capability is not implemented merely because it appears in a roadmap.
The [boolean design](proposals/0004-boolean-expressions.md) records implemented
comparison, precedence, and short-circuit rules.
The [conditional design](proposals/0005-conditional-statements.md) explains scoped
bodies, flat branch execution, and checks in unselected paths.

Published behavior is recorded in the [v0.0.1 notes](releases/v0.0.1.md).
The [v0.0.2 notes](releases/v0.0.2.md) describe the variable prerelease.
The [v0.0.3-beta.1 notes](releases/v0.0.3-beta.1.md) describe the iteration release;
the [v0.0.3-beta.2 notes](releases/v0.0.3-beta.2.md) cover declaration migration,
and output streams. The [v0.0.3-beta.3 notes](releases/v0.0.3-beta.3.md) cover
functions and CLI resource controls. Current downloads are recorded in the
[alpha.5 publication record](releases/v0.0.5-alpha.5-validation.md). Use [compatibility](compatibility.md)
before running a current tutorial with an older installed executable.
[Release preparation](releasing.md) describes the separate packaging and
publication process. The root [changelog](../CHANGELOG.md) distinguishes released
work from unmerged development changes.

## Example conventions

`scarp` code fences contain complete runnable programs for the stated version,
or explicitly named files in a documented multi-file example.
An accompanying output block shows exact text, with a final line break after
each print. Fences marked `scarp-invalid` intentionally fail with the discussed
diagnostic; they are examples of repairs, not runnable success cases.

Shell commands assume the described checkout and working directory. Complete
Rust programs in the embedding guide target the development API. Source examples
under `examples/` have `.stdout` companions and are checked by the test suite.

If a guide disagrees with an executable, first confirm the version and branch.
For a documentation bug, include the page, command, source, actual result, and
version so the mismatch can be reproduced.

## Published v0.0.6-alpha.1 compiler navigation

The [symbol/reference index](symbol-index.md) documents source navigation APIs,
static inventories and versioned JSON contracts published in
v0.0.6-alpha.1. Older alpha.5 binaries do not support these commands.
