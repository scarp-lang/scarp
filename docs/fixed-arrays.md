# Immutable fixed-size arrays

Fixed arrays are available in the development interpreter. The source type
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
