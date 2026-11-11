# Proposal 0041 — Immutable fixed-size arrays

Status **proposed for review**. Baseline `d2c475ecffd3d5f70e060b097b323cda31f72d80`
after the published alpha.6 tuple migration. This specifies the review gate in
[#118](https://github.com/scarp-lang/scarp/issues/118) and narrows the array
portion of [proposal 0036](0036-composite-values-and-tensor-boundaries.md).

## Source grammar

```ebnf
array_type    = "array", "<", value_type, ",", decimal_length, ">" ;
array_literal = "[", "array", ":", expression, { ",", expression }, [ "," ], "]"
              | "[", "array", ":", "<", value_type, ">", "]" ;
```

`array<integer, 2>` has exactly two integer elements. `[array: 10, 20]`
infers that type. The second literal form supplies the element type when there
are no values to infer it from. `[array: <integer>]` has type
`array<integer, 0>` in any expression position, including a default or
return. `[array:]` is rejected at the closing bracket with a hint to write a
typed empty literal. No contextual inference or filler value is needed.

This extends the existing tagged-bracket convention (`[integer: 1, 2]`).
Use the array marker only at the start of a bracket literal when the next
token after `array` is a colon. `[array]` still reads an ordinary binding.
`array[0]`, `array(0)` and `if array{}` keep their existing parses. Ordinary
brackets still form a numerical vector. In type position the parser should
reuse the tuple type-closing logic for `>>`, `>>>` and adjacent `=`.
The length is a nonnegative base-10 integer literal with the usual decimal
digit separators. Reject signs, radices, computed lengths and out-of-range
lengths at the original length span before constructing a type descriptor.

```text
bind labels: array<string, 2> = [array: "नमस्ते", "Scarp"];
bind grid = [array: [array: 1, 2], [array: 3, 4]];
bind empty: array<array<integer, 2>, 0> = [array: <array<integer, 2>>];
var index = 1;
print(labels[index], grid[index][0], empty);
```

Nonempty literals evaluate once per element in source order. Every element
has the same exact structural type. Report a mismatching element at its own
span and relate it to the first element or explicit declaration. Reject a
length mismatch at the initializer with the annotation as the related site.
Array types may contain any existing non-unit source type, including arrays
or tuples. Tuple members may contain arrays. An array of vectors owns shared
vector values and has no implicit conversion to or from a numerical vector,
integer collection or tile. An empty array retains its complete element type.

## Values and operations

- Use one immutable backing slice shared by clones, function frames and output
  snapshots. A `var` can replace the whole value with exactly the same
  structural type. An indexed expression cannot be an assignment target.
  Scope retirement drops the slot's reference. Returned, projected and
  captured values retain their own references.
- Reuse postfix `value[index]`. The index is a checked `integer` expression
  evaluated once after the receiver. Reject negative or out-of-range indices
  at the index expression's byte span before access. Use a checked integer
  conversion and checked access on every target. Tuple projection still
  requires a literal index. No implicit indexing coercion or array slicing.
- `==` and `!=` require exact structural type on both operands. Precharge
  conservative traversal work before comparing the elements in order.
  Primitive comparisons keep existing behavior, including signed-zero
  equality. Reject ordering, arithmetic and element mutation.
- Display `[array: 1, 2]` and `[array: <integer>]` with deterministic spacing.
  Nested strings use the same quoting/escaping policy as tuple members.
  Existing standalone strings retain raw display. Both streams use existing
  buffered execution and replay. A pre-replay diagnostic leaves both streams
  empty. Host writer errors retain the current replay contract.

## Structural types and limits

Extend `SourceType` with an immutable checked array descriptor carrying its
element type and length. Intern it using `TypeKind::Sequence { element,
length: Some(n) }` in the same bounded registry used for tuples. Portable
structural equality includes both fields. Primitive type IDs and numerical
collection kinds retain their meanings. A composite `TypeId` remains local to
one registry. Descriptors with `length: None` remain tooling descriptors.

The limits below are proposed initial ceilings. Each new constructor and the
resolver must enforce them before allocating backing storage.

| Limit | Proposed contract |
| --- | --- |
| Elements in one array | 0 through 4,096, checked before parsing/materializing excess elements |
| Structural depth | 32 composite levels across both arrays and tuples, including typed empty arrays |
| Structural metadata | 4,096 type nodes. An array counts its element type once. A tuple counts each member type |
| Expanded value shape | 65,536 nodes. Count a primitive value as one, an array as `1 + length × child_nodes`, and a tuple as `1 + sum(member_nodes)` |
| Cumulative array payload | 8 MiB initially, per execution across all frames. Proposed `ExecutionOptions.max_array_bytes` and `--max-array-bytes` |

Use checked addition and multiplication. An empty array still contributes
element-type depth and metadata even though it has no element values. The
expanded-shape cap prevents a small type and shared payload from implying
unbounded comparison or formatting work. Numeric/text payloads still use
their existing limits. Keep tuple-only shape limits and the behavior of
`tuple_depth()` for existing values. Both the parser and host constructors
must reject over-depth mixed types without relying on the registry's larger
independent ceiling. Validate the combined type budget during parsing, before
accumulating child descriptors beyond it.

Charge construction work for each member and its bounded metadata processing.
Charge new backing storage, fixed array metadata and bounded retained type
metadata against the array quota before reservation. Use fallible reservation
for variable-size storage. Move elements from the evaluator stack only after
preflighting the charge. Clones and projections share storage and pay their
normal operation work without recharging owned backing. Shared descendants
still contribute their visible bytes and work at capture, equality and
formatting time. Use checked accounting or a deterministic over-budget
sentinel. A typed empty's formatted type also contributes work and capture
bytes. These are logical quotas. They do not measure allocator overhead or
process RSS. Match the tuple implementation's small fixed shared-header
allocation constraints on Rust 1.74.

## Functions, modules and tools

- Type-check annotated/inferred bindings, positional and named arguments,
  defaults, return paths, purity and pre/postconditions through the existing
  structural resolver. The same type must pass through public/private
  module interfaces. Function evaluation still uses bounded frames.
- Keep compile-time module constants scalar. Report E0037 when construction
  or projection would require composite constant evaluation. Function-based
  exports cover the initial cross-module use cases.
- Document new `SourceType`, `Value` and public AST match cases and any full
  `ExecutionOptions` literal migration. Host construction validates arity,
  homogeneity, type shape and zero length. Host accessors return borrowed
  slices/types and preserve immutable sharing.
- When an array occurs anywhere in an exposed signature, propose module
  inspection schema 3 with canonical `array<T, N>` type strings. Keep
  primitive-only and tuple-only inventories byte-identical (schema 1 and 2).
  An array used only in a body does not alter inventory version. Test schema
  negotiation with downstream consumers before publication. Symbol/reference
  and diagnostic JSON retain their current schemas and UTF-8 byte spans.
- Use current syntax/type/runtime diagnostic families for common failures.
  Allocate distinct array shape, index and allocation codes after checking the
  live registry and the BigInt draft. Preserve source spans in UTF-8 and CRLF
  files. Static verify never evaluates an index or runs a function.
- Reject arrays in shared native preflight with E0049 before C11 or ELF64
  emission, including an intermediate projected to a scalar and an unused
  array-valued function. Preserve scalar artifact/output behavior.

## Implementation and acceptance

Keep grammar, type and value implementation in one coherent, independently
reviewable branch after this contract is accepted. The parser, AST span
rebasing, interned type resolution, evaluator, retirement, module inspection
and native rejection must agree before source syntax is advertised.

| Proof | Required cases |
| --- | --- |
| Grammar and types | Infer scalars, nested arrays and mixed arrays/tuples. Test typed empty, malformed lengths/markers, adjacent type closers, vectors and existing uses of the name `array` |
| Access and mutation | Evaluate indices once and in order, check zero/max/negative/i64 endpoints, chain nested projections, reject assignment to an element and prove snapshot isolation |
| Limits | Zero/one/exact/over for length, mixed depth, structural nodes, expanded nodes, backing bytes, work and capture. Verify repeated construction versus shared aliases and small host stacks |
| Interfaces | Rust host constructors, defaults, named arguments, contracts, returns, recursion, imports/visibility, scalar-only constants and exact schema 1/2/3 fixtures |
| Output and native | Exact Unicode stdout/stderr, atomic failure after queued output, byte-accurate cross-file diagnostics and C11/ELF64 rejection of all array paths |
| Release | Runnable examples and exact stdout, Rust 1.74 plus stable Linux/macOS/Windows CI, all three package jobs and downstream schema review at the final source SHA |

Iteration over arrays, mutation of elements, implicit conversion, dynamic
lists, tensor operations and a native array ABI are separate proposals.
