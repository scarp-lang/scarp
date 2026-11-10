# Proposal 0040: immutable heterogeneous tuples

Status: **language contract for implementation review; not published support**.
Based on main `27a1e572f7fedb567319d3df83091fe4919089f2` and the
[preimplementation audit](../development/tuple-baseline-audit-2026-11-10.md).
The first implementation must cover execution and interfaces, not only parsing.

## Grammar and meaning

```ebnf
value_type = primitive_value_type | "tuple", "<", value_type, { ",", value_type }, [ "," ], ">" ;
tuple_literal = "(", expression, ",", [ expression, { ",", expression }, [ "," ] ], ")" ;
group = "(", expression, ")" ;
projection = postfix_expression, "[", nonnegative_integer_literal, "]" ;
```

`(10, "hello", true)` is ordered, immutable and heterogeneous. `(42,)` is a
singleton; `(42)` remains a group. `()` and `tuple<>` are rejected. Unit remains
an absent function result, not an empty tuple or a tuple member. Tuple is a
contextual type word; ordinary variables/functions named tuple remain legal.
Tuple literals share primary-expression precedence and existing postfix indexing.
Nested examples include `(1, ("x", false))[1][0]` and
`tuple<integer, tuple<string, boolean>>`. Type parsing must handle adjacent `>>`,
`>>>` and closing `>=` without changing numeric shift/comparison lexing.

Inference is complete. Explicit tuple annotations require exact ordered member
types and arity, recursively, without integer/float or numeric/text coercion.
Tuple indices are nonnegative integer literal nodes (grouping a literal is
allowed), checked during resolution. Negative, dynamic, computed and out-of-range
indices are rejected at the index source span. Numerical vector/integer indexing
retains its existing dynamic behavior. Chained projections and projections of
function results are supported. Element assignment and destructuring are absent.

## Types and interfaces

Use a portable immutable structural SourceType for source/runtime boundaries and
the existing bounded TypeRegistry for compilation-local interned TypeIds.
PrimitiveType identities remain universal and unchanged. Equal source tuple
shapes compare structurally across compilations; composite TypeIds belong to
one arena and are not cross-edit serialization identities.

Tuple values may initialize local/entry bindings, pass through typed parameters,
return values, defaults and pure contracts, and cross public/private function
module boundaries. A var may replace its whole value with the exact same type.
Copies share immutable storage. Bindings, function results and captured output
retain their owners after scopes retire. No GC, ownership syntax or manual memory
API is introduced. No ambient effects are added.

Module constants remain restricted to their published scalar subset in this
milestone. Tuple constant creation/projection is rejected clearly, including
exported entry constants. Public module functions supply tuple-valued interfaces.
A later constant proposal must define compilation allocation/materialization
budgets before removing that restriction.

The migration necessarily changes primitive-only public AST/type metadata fields
to structural source types. Rust hosts convert primitives explicitly with Into;
primitive Value constructors/accessors, IDs and numeric kernel signatures retain
their contracts. Document every changed field/method and exhaustive enum match.
Public APIs are experimental; source incompatibility must be explicit rather
than representing every tuple by one fake primitive identity.

## Equality, display and effects

`==` and `!=` require matching structural types, compare ordered element values,
and prepay bounded element/payload work, including nested text/numerical values.
Tuple ordering comparisons and arithmetic are rejected. Primitive equality
semantics, including finite float signed-zero equality, remain unchanged.

Display is `(10, "hello", true)` with comma-space separators. Singletons include
the trailing comma: `(42,)`. Strings inside tuples are quoted and escape quote,
backslash, LF, CR, tab, NUL and other control scalars (`\u{hex}`). Printable
Unicode is preserved. Nested tuples use the same format; existing finite-float
formatting retains float markers and signed zero. Top-level primitive strings
continue printing their raw text. Display is deterministic and unambiguous; it
is not a new general serialization/round-trip parser API.

All five output statements and stream-switch flushing retain their behavior.
Evaluation prepares output before replay; source, work, tuple allocation or
capture errors produce no program output. Host output failures can leave prior
writes visible, as before. Static verify/inspection do not execute tuple values.
Evaluating check retains runtime work/allocation/capture checks without replay.

## Limits and implementation safeguards

Initial tuple arity is 1..32; nested tuple type/value depth is at most 32 and
structural type nodes are limited to 4096. The existing expression ceiling
remains 64. The implementation selects the lower tuple ceiling to reserve
stack space for structural type/display/drop operations; its maximum-depth
program passed on a 256 KiB host stack. The source
parser rejects oversized shapes before materializing arbitrary member lists.
Public host constructors enforce the same shape bounds and reject unit members.

Tuple payload has an explicit cumulative quota (default 8 MiB), CLI
`--max-tuple-bytes`, shared across frames per execution. Account for fixed member
storage and structural metadata before fallible reservation. Shared clones do
not allocate/recharge payload; selected operations still consume work. Construction,
projection, equality and formatting have documented deterministic work costs.
Captured output conservatively charges nested tuple/payload/escaping size and
metadata so Arc sharing cannot bypass the existing capture cap. These are logical
payload/work caps, not exact process RSS or wall-clock guarantees.

Preserve existing execution/print/call/slot/text/vector/capture budgets. If a new
ExecutionOptions field is needed, document full struct-literal migration and use
Default in new examples. No unsafe code, unchecked indexing, dependency engine
or unbounded host-recursive interpreter path is permitted.

## Diagnostics, metadata and unsupported backends

Keep E0001-E0058 and BigInt draft codes distinct. Add focused tuple index/shape/
allocation diagnostics after inspecting the live registry; syntax errors retain
original token spans. Wrong annotations/argument/result/member types use the
existing type-mismatch family with structural type names and related sites.

Primitive-only inspection outputs stay byte-compatible. Metadata exposing tuple
signatures needs a documented versioned schema extension; source navigation
continues reporting correct original declaration/call/reference spans. Type words
are not fabricated value-reference sites. Both native scalar emitters reject any
unsupported tuple value/type in preflight with E0049, including unused tuple
functions, without creating an artifact or silently skipping it.

## Required proof and exclusions

Positive/negative/boundary fixtures must exercise literal/group/singleton forms,
nested/static indexing, structural annotations, mutation, calls/defaults/returns/
contracts/recursion, imports/visibility, output escaping, source spans, scope
retirement, concurrency, budgets and native rejection. Preserve old loops,
vectors, output, primitive host constructors and static/evaluating behavior.
Run full repository checks, debug/release, Rust 1.74 and exact-head Linux/Windows/
macOS/package CI. CI success on input/text branches is not tuple qualification.

Excluded: empty tuples, destructuring, dynamic tuple indices, tuple iteration,
mutable elements, arrays/lists/records/options/results, for/match syntax, generic
collections, native tuple ABI/code generation and publication. Public Docs,
By Example, VS Code and Playground changes are separately release-qualified.
Alpha.6 is only a proposed future milestone; no release control request is made.
