# Tuple foundation audit — 10 November 2026

Audit baseline: [scarp-lang/scarp](https://github.com/scarp-lang/scarp), main
`27a1e572f7fedb567319d3df83091fe4919089f2`, tree
`6f522ea57b8624981b7bb959f21a19193c78802f`. Latest published compiler is
[v0.0.6-alpha.5](https://github.com/scarp-lang/scarp/releases/tag/v0.0.6-alpha.5),
9 November, approved source `205e92f166f186c3905e5880d41c31717b697b8e`.
No alpha.6 release exists at audit time. This record precedes tuple coding.

## Implemented / proposed / deferred

| Capability | Live source evidence | State |
| --- | --- | --- |
| bind/var, scalar values, vectors/integer collections/tiles | AST, parser, value, resolver, existing executable corpus | Published |
| while/repeat, break/continue, typed funcra and contracts | Parser/resolver/functions, iterative machine, control-flow tests | Published |
| UTF-8 text queries/construction, checked numeric conversions | Pure numeric registry and text kernels | Published |
| Structural tuple descriptors | `types::TypeKind::Tuple`, bounded arena registry | Host tooling only; no source/runtime tuple |
| Tuple literals/types/projections/value storage | No AST/parser/value implementation on audited main | Missing; target of this work |
| read_line input | [#181](https://github.com/scarp-lang/scarp/pull/181), head `4950cc34c767cd45197b11d9a8e4475a476edeb2` | Working unmerged implementation, not in published alpha.5 |
| Basic conformance corpus | [#182](https://github.com/scarp-lang/scarp/pull/182), head `4c26b07a778e34e92771f2f7e2b5c4893c41ce0a` | Unmerged; stacked on #181 |
| Input design | [#180](https://github.com/scarp-lang/scarp/pull/180), head `d99091b613a83263932821d8f4600e617415eeb1` | Proposal only, separate implementation #181 |
| Scalar text formatting/parsing/trim/replacement | [#179](https://github.com/scarp-lang/scarp/pull/179), head `c9c5b7773adb90cdd434c8b997aaf9602f1bae56` | Working unmerged implementation |
| Release reference repair | [#183](https://github.com/scarp-lang/scarp/pull/183), head `1942f98e1902b26933f61113d6c42ce7230a82ca` | Unmerged docs only |
| BigInt | [#172](https://github.com/scarp-lang/scarp/pull/172), head `b0134e2acc7e795c95ee27a34175e80bcf80e85f` | Separate draft; no tuple dependency |
| General arrays, records, optional/results, for/match, mutable elements | Proposals / feature matrix | Deferred; excluded from tuple slice |

All 20 open PRs were enumerated. Relevant #179/#180/#181/#182/#183/#172
metadata and inline review-thread lists were refreshed; those lists were empty.
Their previously passing CI must not be presented as tuple CI. Historical
#16–#27 are old stacks with features independently implemented on main and are
not appropriate tuple dependencies. #171 is an unrelated order-statistics draft;
#149 is an older Top-P branch superseded by the published implementation.

## Dependency map

Tuple contract → structural source-type boundary → AST/parser/resolver/runtime
integration → functions/modules/tooling/quotas → executable conformance → exact
platform/package qualification. Input and text are independent features; no old
PR is merged to acquire them. The reviewed type-staging design in #179 and
[proposal 0036](../proposals/0036-composite-values-and-tensor-boundaries.md)
identify the same primitive-only boundary and require a complete value slice.

## Compatibility risk matrix

| Boundary | Current assumption / risk | Required mitigation |
| --- | --- | --- |
| AST annotations and parameters/results | Public primitive `ValueType`, Copy annotations | Explicit structural SourceType migration; retain PrimitiveType IDs, document Rust field conversions and loss of Copy |
| Resolved slots/expressions/functions | Primitive slots; node IDs inferred with primitive-only assumptions | One bounded type registry per linked compilation; structural tuple identity, shared IDs inside a compilation |
| Runtime Value | Exhaustive primitive variants; host kind inspection | Add immutable shared tuple value, exact structural type metadata and bounded host construction; document exhaustive-match changes |
| Parser tokens | `>>`, `>>>`, `>=` are maximal-munch operators | Split closing angle tokens only inside type parsing; retain expression operators and original spans |
| Grouping / commas | Parentheses currently group one expression | Comma selects tuple; `(x,)` singleton, `(x)` grouping, `()` rejected |
| Modules/constants | Scalar constant folding and primitive signature inventory | Support tuple function interfaces; keep tuple module constants explicitly rejected until independent constant allocation policy |
| Navigation / JSON | Current metadata schemas assume primitive type strings | Preserve primitive output; version tuple-exposing metadata and retain file-local UTF-8 spans |
| Output / equality | Scalar display, shared payload snapshots, prepaid numerical work | Escaped nested strings, singleton delimiter, bounded recursive value work and capture accounting; unchanged top-level primitives |
| Resource limits | ExecutionOptions/limits host literals | Add documented tuple payload quota; no unbounded allocation or host recursion |
| Native backends | Tiny scalar-only IR and preflight | Explicit E0049 tuple rejection; keep scalar differential tests |
| Unmerged #181/#172/#179 | Overlap in resolver/value/diagnostics/limits/scripts | Do not duplicate; reserve tuple codes after live E0058; require combined-tree qualification before integration |

This audit authorizes no release, tag, version bump, publication request, issue
comment or PR comment. Public teaching/editor/browser adoption follows reviewed
compiler behavior and version qualification.
