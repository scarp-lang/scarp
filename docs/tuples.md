# Immutable tuple development contract

Status: **implemented in the combined alpha.6 candidate, under qualification**.
Published v0.0.6-alpha.5 does not accept this syntax. See
[proposal 0040](proposals/0040-immutable-tuples.md), the
[baseline/risk audit](development/tuple-baseline-audit-2026-11-10.md), the
[executable example](../examples/tuples.srp), its [exact output](../examples/tuples.stdout)
and [Rust host example](../examples/tuple_types.rs).

## Source values and types

```scarp
bind pair: tuple<integer, string> = (7, "नमस्ते");
print(pair, pair[0], pair[1]);
print((42,), (42));
```

Tuples are fixed-arity ordered heterogeneous values. `(42,)` is a singleton;
`(42)` remains grouping. Empty `()` / `tuple<>`, unit members, destructuring,
element mutation, dynamic/computed indices and tuple iteration are unsupported.
The contextual type word `tuple` remains a legal ordinary binding/function name.

Types are inferred or annotated structurally: same ordered member types and
arity, recursively. There are no implicit integer/float/text conversions.
Nested examples are `(1, ("x", true))` and
`tuple<integer, tuple<string, boolean>>`. Type closing angles work without
separating whitespace, including `tuple<tuple<integer>>=((1,),);`; numeric
shift/comparison/compound operators retain maximal-munch lexical behavior.

`value[0]` requires a nonnegative integer literal within arity, checked before
execution. Grouped literals and integer radix literals are accepted. A name,
computed expression, negative index or float is rejected with E0059 at the index
span, even on an unselected path. Original literal syntax is retained through
module constant folding, so `value[lib.zero]` does not become a literal index.
Numerical vector/integer indexing retains its existing dynamic rules. Projections
chain and can select tuples, primitives, numerical values or function results.

`bind` retains an immutable value. `var` may replace the whole tuple only with
the exact same structural type. Arguments, returns, defaults, recursion and
contracts use the same representation; pure functions keep all existing effect
restrictions. Public/private module function interfaces support tuples. Module
constant initializers deliberately keep the published scalar subset and reject
tuple creation/projection with E0037 until a compilation allocation policy is
reviewed. There is no source-level ownership, allocation or GC syntax.

## Display and equality

Tuple display is `(7, "नमस्ते")` with comma-space separators; a singleton ends
in `,)`. Nested strings are quoted and escape double quote, backslash, LF, CR,
tab, NUL and remaining control scalars (`\u{hex}`). Printable Unicode remains
UTF-8. Nested tuples use the same format. Existing finite-float formatting
retains float markers and signed zero. Top-level primitive strings retain raw
text rendering. This is deterministic value display, not a serialization API.

`==` / `!=` require equal structural types and compare all selected member values
using existing primitive/numerical equality, including signed-zero float equality.
Ordering and arithmetic on tuples are rejected. Equality prepays conservative
nested value work even if pointer sharing or an early mismatch would make the
host comparison faster. No implicit heterogeneous numeric equality is added.

All five output spellings retain newline/stream semantics. Source/runtime/budget
failure discards buffered program output; host output failures may leave earlier
writes visible, as before. Static verify and inspection do not evaluate tuple
payloads; legacy check evaluates selected work and all runtime quotas.

## Bounds and accounting

| Bound | Value / control |
| --- | --- |
| Arity | 1 through 32 |
| Tuple type/value depth | At most 32; existing expression ceiling remains 64 |
| Structural source-type nodes | At most 4096, counting repeated child shapes |
| Cumulative tuple payload | Default 8 MiB; `ExecutionOptions.max_tuple_bytes` / `--max-tuple-bytes` |
| Other runtime limits | Existing work/prints/calls/slots/text/vector/capture quotas still apply |

The lower tuple depth leaves room for bounded structural/display/drop recursion.
A maximum-depth annotated program is tested on a 256 KiB stack; interpreter
expression and call execution remain iterative. Shape constructors reject invalid
host values before collecting arbitrary-sized input metadata. Runtime member
buffers use fallible reservation after quota charging.

For each new tuple, the logical payload charge is `sizeof(TupleStorage)` plus
`arity * sizeof(Value)` plus `structural_nodes * sizeof(SourceType)`. Checked
arithmetic prevents overflow. This conservative charge includes metadata even
where immutable child metadata is shared. Retirement does not refund a cumulative
quota. Shared tuple clones/projections retain owners and do not recharge payload.
The new field is an explicit Rust struct-literal compatibility change.

Construction charges the existing visit/task work and one unit per member.
Projection charges existing expression/index tasks and performs a bounded lookup
plus shared clone. Clone work is constant for immutable backing storage. Each
tuple caches conservative display/equality work: one unit for its tuple node,
32 per primitive scalar, `6 * bytes + 2` per string, `32 * cells + 2` per vector,
`24 * cells + 12` per integer collection, and `32 * cells + rows + 16` per tile;
nested tuple cost is reused. Equality prepays the larger operand cost; output
prepays its tuple cost, including in check mode. Vector/text input creation
retains its original separate quotas.

Output snapshot capture charges a tuple header, member Value storage and
conservative nested payload, including escaped string size and visible numerical
payload. Sharing cannot bypass repeated snapshot charges. These are logical
payload/work caps; standard Arc/metadata allocation follows the existing safe
Rust runtime and does not promise recoverable process-wide OOM or an RSS bound.

E0060 reports tuple shape bounds; E0061 reports tuple quota/reservation failure.
Existing type mismatch, purity, work and capture diagnostics retain their codes.
E0051-E0058 are deliberately left for existing independent BigInt/input work.
Both native scalar backends reject tuples with E0049, including tuple-valued
intermediates projected to scalars and unused tuple function signatures.

## Rust API and metadata migration

PrimitiveType and `value::ValueType` remain the existing primitive enum with
unchanged universal IDs and numeric Builtin signatures. Structural fields now
use `types::SourceType`; use `primitive.into()` or `SourceType::from(primitive)`.
`SourceType::tuple` and `TupleValue::new` validate bounded shapes. Runtime tuples
own shared immutable storage. Structural shapes compare across compilations;
composite TypeIds stay local to one registry and must not be persisted/mixed.

| Public boundary | Migration |
| --- | --- |
| AST TypeAnnotation.kind, Parameter.kind, FunctionDeclaration.result | Use SourceType; annotations are Clone rather than Copy |
| FunctionInfo.result, ParameterInfo.kind, ResolvedProgram.binding_types | Structural SourceType metadata |
| modules::SymbolKind parameters/result/constants | Structural SourceType metadata |
| Value::kind() | Returns SourceType; `kind().as_primitive()` adapts primitive consumers |
| Value enum / ExpressionKind enum | Add Tuple arms in exhaustive matches |
| ExpressionKind::Index | Retain `source_literal` provenance when constructing/rebasing ASTs |
| ExecutionOptions full literals | Add max_tuple_bytes or use `..ExecutionOptions::default()` |
| DiagnosticKind exhaustive matches | Add TupleIndex, TupleLimit and TupleAllocation |

Primitive value constructors/accessors, kernel argument/result primitives,
function calls and existing writer execution API signatures remain available.
The structural field/return changes are intentional experimental host source
incompatibilities, not a stable ABI claim. `tuple_types.rs` demonstrates migration.

Module JSON uses schema 2 only when a function/constant type exposed in that
inventory is structural; type strings use canonical `tuple<...>` spelling.
Primitive-only inventories remain byte-compatible schema 1. A local tuple whose
type is not part of module symbol metadata does not change its schema version.
Consumers must branch on schema_version and reject unsupported versions.
Navigation symbol/reference JSON has no type fields and retains schema 1 and its
UTF-8 span contract. Diagnostic JSON also retains its existing schema. Human AST
inspection remains an unstable debug representation.

## Qualification and ecosystem

Run `cargo test --locked --offline --test tuples --test tuples_modules_cli`, the
full check scripts, debug/release and Rust 1.74 checks. Coverage includes seeded
properties, exact fixtures, Unicode/source spans, cached AST rebasing, imports,
visibility, all output streams, broken pipes, ownership retirement, concurrency,
zero/exact quotas, small stacks and native rejection. Native packages execute
the example and require its schema-2 inventory; all older examples still require
schema 1. Exact-head platform CI is required before integration.

This branch does not merge input #181, text #179, BigInt #172 or old stacks.
Combined-tree compatibility is a separate integration gate. Public reference
belongs in Scarp Docs and teaching lessons in Scarp by Example after compiler
review/version qualification. VS Code grammar and Playground adoption remain
separate and release-gated. No prerelease, version bump or release request is
created by tuple development.
