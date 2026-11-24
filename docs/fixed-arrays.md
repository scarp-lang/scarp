# Immutable fixed-size arrays

Fixed arrays have been available in the interpreter since alpha.10. The source type
`array<T, N>` has exactly `N` values of the same structural type `T`.
Element types include primitives other than `unit`, tuples, and other arrays.

```scarp
bind labels: array<string, 2> = [array: "नमस्ते", "Scarp"];
bind empty: array<integer, 0> = [array: <integer>];
var index = 1;
print(labels[index], empty);
```

The interpreter prints `Scarp [array: <integer>]` followed by LF. The nonempty
literal infers its element type from the first expression. Each remaining
element must have that exact type. A trailing comma is allowed on a nonempty
literal. Use `[array: <T>]` for zero elements. Length annotations accept
decimal digits with underscores between digit groups, such as `4_096`.

The contextual `array:` marker works inside a bracket literal. Ordinary
brackets still make numerical vectors, and `[integer: 1, 2]` still makes
an integer collection. You can use `array` as an ordinary binding or function
name.

## Access and equality

- `values[index]` evaluates the receiver and then the integer index once.
  Literal and computed indices use the same runtime bounds check. The
  diagnostic points to the index expression. `scarp verify` checks types
  without evaluating bounds.
- A `var` can replace its whole array with a value of the same type.
  Captured output, aliases, and values passed to functions share immutable
  backing storage. An indexed expression cannot be assigned to.
- `==` and `!=` require matching length and element type. Comparison uses
  ordinary element equality, including equality of positive and negative
  floating-point zero. Ordering and arithmetic require numeric scalars.
- Display uses `[array: 1, 2]` or `[array: <integer>]`. Nested strings are
  quoted and escaped. A runtime diagnostic before output replay leaves both
  output streams empty.

## Traversal and fallback lookup in alpha.17 source

- `array_len(values)` returns the number of elements as an `integer`. Use it
  as the count in `repeat array_len(values) as index` to visit elements in
  source order. An empty array runs zero iterations.
- `array_get_or(values, index, fallback)` returns the element at the integer
  index or the fallback for a negative or out-of-bounds index. The fallback
  must have the array's exact element type. All three arguments evaluate once
  in source order, including the fallback for an in-bounds lookup.

Both operations are pure. They use constant-time length and index access
and share immutable values without copying array backing storage. See the
[runnable traversal example](../examples/array_traversal.srp).

## Persistent element updates (alpha.18 candidate)

`array_set(values, index, value)` is a pure, persistent fixed-array operation.
It returns a new array with exactly one element replaced; the old array and
all existing aliases remain unchanged. The replacement must have the exact
array element type, and the result keeps the original `array<T, N>` type.
Arguments are evaluated exactly once in source order.

```scarp
bind original = [array: 4, 8, 12];
bind changed = array_set(original, 1, 99);
print(original[1], changed[1]); // 8 99
```

Negative or out-of-bounds indices raise E0063 at the index expression. Every
successful update allocates a fresh fixed-array backing and charges the
cumulative array allocation budget (E0064), including for nested arrays.
This is not an in-place mutation; the existing indexed assignment restriction
remains unchanged. See the [runnable update example](../examples/array_updates.srp).

## Pure array reordering (development source)

- `array_reverse(values)` returns a new fixed array with elements in reverse
  order. Empty and single-element arrays are supported.
- `array_swap(values, first, second)` returns a new fixed array with the
  two indexed elements exchanged. Equal indices are valid; both indices must
  be integers within the array bounds.

Both functions preserve the exact `array<T, N>` type and leave the input
and its aliases unchanged. Arguments evaluate once, from left to right.
Negative or out-of-bounds swap indices report E0063 at the first invalid
index expression. Each successful operation reserves and charges a new
immutable backing under the shared collection quota (E0064), even for empty
arrays. Reordering also charges execution work proportional to the array
length. Neither operation changes the existing 4,096-element maximum.

See the [runnable reordering example](../examples/array_reordering.srp).
## Bounds and accounting

| Measure | Ceiling |
| --- | --- |
| One array length | 4,096 elements |
| Array and tuple nesting combined | 32 levels |
| Structural type nodes | 4,096 |
| Expanded value shape | 65,536 nodes |
| Cumulative array backing and type payload | 8 MiB by default |

Structural metadata counts the element descriptor once per array. Expanded
shape counts a primitive as one node, an array as one plus its length times
the element's expanded shape, and a tuple as one plus its members' shapes.
A typed empty array retains the full type of its element, which must
independently meet these limits.

For each new array, the interpreter charges a fixed storage record, one
`Value` slot per element, and one `SourceType` slot per type node. Set
`--max-array-bytes` or `ExecutionOptions.max_array_bytes` to adjust the
cumulative logical quota across calls. Cloning and projection share storage.
Comparing and printing charge work for visible values, including repeated
references to shared descendants. Output snapshots also charge for visible
payload and the type text of typed empty arrays.

E0062 covers array shape or length bounds, E0063 covers index bounds, and
E0064 covers cumulative allocation or reservation failure. See
`scarp explain E0062`, `scarp explain E0063`, and `scarp explain E0064`.

## Modules and Rust embedding

Function parameters, defaults, results, contracts, and imports accept exact
array types. Module constants remain scalar. An inventory uses schema 3
when it serializes an array type, including a nested array in a private
signature. Symbol and reference inventories and diagnostic JSON retain
their current versions. See [module JSON](module-json.md).

| Rust API | Embedding change |
| --- | --- |
| `SourceType::Array` | Construct with `SourceType::array(element, length)` and borrow the descriptor with `as_array()`. |
| `TypeKind::Sequence` | An interned fixed array has `length: Some(n)`. Composite type IDs belong to their registry. |
| `Value::Array` | Construct checked storage with `ArrayValue::new(element, values)`. `elements()` and `element_type()` borrow from that storage. |
| `ExpressionKind::Array` | The public AST exposes child indices and the element type for a typed empty literal. |
| `DiagnosticKind` | Handle `ArrayLimit`, `ArrayIndex`, and `ArrayAllocation` in exhaustive matches. |
| `ExecutionOptions` | Include `max_array_bytes` in complete literals, or use `..ExecutionOptions::default()`. |

`SourceType::structural_depth()` counts mixed array/tuple nesting.
`tuple_depth()` continues to count tuple-only nesting.
The experimental C11 and ELF64 backends reject programs using arrays with
E0049. Run [the example](../examples/fixed_arrays.srp) with
`cargo run -- exec examples/fixed_arrays.srp`.

### Concatenation on the development branch

`array_concat(left, right)` joins two fixed arrays with identical element
types in source order. For lengths M and N, the resolver infers an array of
length M + N. It checks both the 4096-element limit and the complete nested
shape before execution. Each argument is evaluated once, left to right.
An empty operand allows the result to share the other immutable snapshot.
Two nonempty operands require one new backing buffer and work proportional to
M + N. The runtime charges the array quota before reserving that buffer.
The [runnable example](../examples/array_concat.srp) joins measurements while
preserving the original two-element array.
