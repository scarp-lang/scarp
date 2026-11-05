# Proposal 0036: composite values and tensor boundaries

Status: **deferred design for review**, not implemented syntax or runtime support.
This proposal accompanies v0.0.6-alpha.1 preparation. The completed smaller
numerical slice is [RMS](../root-mean-square.md); this candidate adds no arrays,
tuples, generalized tensors, broadcasting, gradient tape or GPU backend.

## Why arrays are deferred

The current `types::TypeKind` registry can describe tuples, sequences and tensor
shapes for Rust tooling. Those descriptors are not source-language values.
`ast::TypeAnnotation.kind`, parameter kinds and function results still use
primitive `ValueType`; `Value` and the execution plan have no array value.
Adding only a literal parser or exposing descriptors as if they were runtime
types would leave functions, modules, contracts, inspection and retirement
inconsistent. A complete array slice requires all these layers together.

## Proposed first array slice

Before syntax is accepted, review an original Scarp type/literal grammar as a
separate language proposal. The first implementation should have a statically
known nonnegative length and one exact primitive element type, including length
zero. All elements must be explicitly initialized; no implicit fill, numeric
conversion, aliasing mutation, destructuring or nested arrays in the first slice.

Introduce bounded source type expressions whose resolved result is an interned
`TypeId`, with a compatibility strategy for current public AST constructors and
primitive host APIs. Function parameters, results, defaults, local annotations
and module signatures must all use that representation. Keep module constants
restricted until array constant evaluation has its own work/allocation policy.

Use immutable shared array storage. `bind` fixes a value; `var` may replace the
whole value with another of the same element type **and length**. Captured output,
returned values and arguments share immutable storage; replacing a binding
cannot change an earlier snapshot. Element mutation is a separate proposal.
Indexing requires an integer, checks negative and upper bounds before access,
and reports the original index span. Equality compares shape/type/contents and
prepays element work. No unchecked access or unsafe Rust is needed.

Set explicit limits for element count, type-expression depth, retained type
metadata, cumulative payload bytes and output snapshots. Check multiplication
overflow and reserve fallibly before allocation. Charge construction and
comparison work in advance; charge each newly owned payload once, and every
captured output according to its visible contents. Scope retirement must release
the binding while returned/shared values remain valid. Counters describe logical
budgets, not operating-system resident memory.

Completion requires parser/resolver/runtime integration, cross-file navigation
of value and type sites, exact type/length/index diagnostics, JSON schema review,
and explicit E0049 rejection by the C11 emitter. Verify zero/single/maximum
length, malformed types, wrong initialization counts/types, defaults/contracts,
function returns, public/private interfaces, recursion, shadowing, output
atomicity, exact budget boundaries and repeated compilation/execution. Add
runnable examples and a public AST/API migration note before merging syntax.

## Later N-dimensional numerical values

General tensors should build on a completed source type/value boundary rather
than reinterpreting today's two-axis `NumericTile`. A separate reviewed design
must decide static versus runtime dimensions, zero axes, checked shape-product
overflow, maximum rank/cell count, contiguous CPU ownership and view lifetimes.
Start with immutable contiguous finite binary64 storage and explicit CPU kernels;
introduce strides and broadcasting only with exact shape compatibility and
prepaid materialization/reduction costs. Views must retain their owners and all
kernels must operate on visible regions, following existing vector-window rules.

Serializable tensor values need a versioned format declaring element encoding,
endianness, dimensions, byte count and finiteness validation before allocation.
A gradient tape would require separate ownership, node/edge/work/byte budgets,
deterministic release and repeated-run semantics; explicit existing loss-gradient
kernels do not supply this. Device transfer and GPU execution require a later
capability/backend contract and CPU differential oracle. None is promised by
the current interpreter, tile API or experimental scalar C11 emitter.

Recommended sequence: source type expressions → complete immutable arrays →
shape/value policy → CPU tensor kernels → views/broadcasting → optional tape.
Each arrow requires its predecessor's tests and a separately reviewable slice.
