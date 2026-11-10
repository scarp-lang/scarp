# Core type staging after published alpha.5

Status: **design for review**. None of the character, byte, general collection,
record, variant or pattern syntax described here is implemented by this change.
The completed small implementation is [scalar/text foundations](../text-foundations.md).
The [dated audit](../development/core-types-audit-2026-11-09.md) records exact
source and PR evidence. This supplements, rather than replaces,
[proposal 0036](0036-composite-values-and-tensor-boundaries.md).

## Finish explicit scalar boundaries first

Keep `integer` as checked signed i64, `float` as finite binary64 and `boolean`
strictly boolean. Preserve exact `to_float`/`to_integer` rules. Text conversion
is an explicit operation, with its own grammar and error rules. Do not silently
widen a literal or overflowing integer to BigInt, float or decimal. Review and
integrate the existing bounded BigInt PR #172 before creating another engine;
its work, digit and payload limits are part of its language contract.

There is no demonstrated foundation requirement for an entire family of
integer/float widths yet. Introduce each additional width only with a concrete
storage, FFI or arithmetic use case, explicit overflow/conversion policy and
full compiler support. In particular, `f32`, `u64`, and exact `decimal` are not
aliases for current binary64, i64 and BigInt respectively.

## Character and byte decision records

Propose one `character` value representing a Unicode scalar, excluding surrogate
code points. It is neither a UTF-8 byte nor a grapheme cluster. Its representation
can be Rust `char`; ordinary source spans remain UTF-8 byte offsets. Review
literal/escape spelling before reserving a new token: do not introduce an
unreviewed single-quote grammar in a lexer-only PR. Decide display versus escaped
debug representation, equality/order by scalar value, and explicit checked
integer/code-point and one-scalar-string conversions. Empty/multiple-scalar
strings and surrogate/out-of-range integers fail at the argument span.

Propose one `byte` value in 0..255 for future binary I/O. Conversion from i64 is
checked, without truncation or implicit numeric mixing. Before implementation,
decide whether byte arithmetic is checked in that range or intentionally absent;
do not inherit wrapping behavior accidentally from a Rust `u8`. Keep byte-to-text
encoding separate from character/code-point conversion. A sequence of bytes need
not be valid UTF-8 and cannot reuse the `string` invariant.

For either primitive, append its stable primitive identity and update all
exhaustive AST/type/value/operator/built-in/module/inspection/native paths in
one reviewable slice. Include annotation/inference parity, function parameters
and results, defaults, contracts, constants (supported subset or explicit
rejection), module export metadata, Unicode spans, immutable snapshots and
diagnostic rendering. Test 0, maximum, negative/overflow, malformed escapes,
surrogates, combining marks, four-byte scalars, wrong-type operations, budgets
and both backend rejections. A design record alone does not add a source type.

## One semantic type boundary before source collections

Current `TypeKind::{Tuple,Sequence,Applied,Parameter,Function,Tensor}` descriptors
serve Rust tooling. They are not source collections or first-class values.
Expression metadata contains interned identities, but `TypeAnnotation.kind`,
parameter/result declarations, built-in signatures and runtime `Value` still
use primitive kinds. Extending the parser alone leaves calls and modules unsound.

First migrate those boundaries to resolved interned types, while retaining a
documented compatibility adapter for primitive Rust-host APIs. Specify ownership
of the shared type arena across a linked module graph. Canonicalize equivalent
types inside that graph, reject foreign-arena handles, and keep universal primitive
IDs unchanged. Bound type depth, node count, child count and imported metadata.
Avoid serializing process-local TypeId integers as stable cross-process IDs;
version JSON/export schemas when structural source types are exposed.

## First collection: fully initialized fixed homogeneous arrays

Review Scarp's source type, literal and indexing grammar before coding it. The
first semantic array has `(element TypeId, nonnegative fixed length)`. Require
explicit initialization of every element; start with primitive elements and no
nested arrays. Empty arrays need an annotation or another unambiguous source
of element type. Reject mixed element types without implicit promotion.

Use immutable shared storage. Whole-value `var` replacement must preserve both
element type and length; `bind`, arguments, returned values and output snapshots
retain immutable values. No element mutation or hidden aliasing is implied.
An integer index must be nonnegative and less than the fixed length, with an
error at the index expression. Equality compares type, shape and contents after
prepaying element work; ordered array comparison is a separate decision.

Specify hard element and metadata limits, cumulative payload and captured-output
charges before allocation. Check length-times-element-size overflow and reserve
fallibly; prepay construction, equality, formatting and indexing traversal where
required. Account for a new payload once and sharing separately. Logical quotas
do not claim to cap allocator overhead or process RSS. Scope/frame retirement
releases bindings without invalidating live returned/captured values.

Completion means parsing, resolution, execution plans, functions, defaults,
contracts, modules, inspection, source navigation, errors and native rejection
all agree. Test zero/single/maximum length, count/type mismatch, malformed types,
out-of-range indices, imported signatures, recursion/shadowing, byte-exact output,
atomic errors and exact quota boundaries. Constant arrays remain rejected until
their evaluation/allocation contract is approved. No native fallback is allowed.

## Remaining types in dependency order

| Milestone | Decision before implementation | Completion boundary |
| --- | --- | --- |
| Dynamic lists | Length-independent type identity, immutable growth and amortization/budget policy | Shared type/runtime boundary, append/slice/index/equality, bounds and payload tests |
| Heterogeneous tuples | Fixed positional member types, projection syntax and empty tuple versus unit | Calls/modules/metadata support all member types; no generic `Value` escape hatch |
| Records | Nominal identity across modules, field visibility/order and initialization | All fields initialized; exact field/type errors; whole-value immutable semantics |
| Tagged variants/enums | Tag/payload identities and visibility | Construction and checked discrimination; no invalid payload state |
| Optional values | Explicit absent/present variants over a resolved member type | Absence remains distinct from unit, false, zero, empty string and failure |
| Structured results/errors | Success/error member types, propagation and diagnostic boundary | No conversion of interpreter faults to ordinary values without a reviewed policy |
| Pattern matching | Exhaustiveness, binding scopes, evaluation order and unreachable arms | Integrates tagged variants and control-flow return analysis |
| Maps/sets | Key equality/hash policy, deterministic iteration and collision/work limits | Stable behavior for imports/output; byte and element quotas; no float hash shortcut |
| Generics | Declaration identity, substitutions/constraints and monomorphization policy | Type depth/instantiation budgets and cross-module diagnostics |
| Exact decimal | Coefficient/scale, precision, rounding modes, division and formatting | Independent oracle plus magnitude/work limits; not finite binary64 under a new name |

Unit is an internal no-result marker and function result kind today. It is not
an optional value, error, null or an empty collection. Preserve this distinction
through every milestone. Immutable storage does not expose ownership/lifetime
syntax or manual allocation to Scarp users; the Rust implementation's sharing
details remain host/runtime concerns.

## Release and ecosystem gates

Keep each coherent implementation on a small independently tested branch.
Integrate only after review and compatibility checks. Public Docs, Hindi pages
and By Example should target that integrated revision and execute the examples
against the matching compiler. Keep technical language contracts, compiler
regression fixtures and fixed historical release records in this repository.
Record the exact final source, complete CI and verified packages before requesting
a prerelease version. This proposal neither authorizes publication nor changes
the published alpha.5 feature set.
