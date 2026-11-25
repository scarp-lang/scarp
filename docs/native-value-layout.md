# Experimental Linux x86-64 value layout

`scarp::native_object::plan_x86_64_value_layout` accepts a canonical semantic
type ID and its registry. It computes target sizes, alignments, field offsets,
fixed-array strides and tagged payload positions without allocating or
serializing a target value.

| Value | Size and alignment |
| --- | --- |
| `integer` | 8 bytes, alignment 8 |
| `float` | 8 bytes, alignment 8 |
| `boolean` | 1 byte, alignment 1 |
| Tuples and records | Lay out fields in declaration order. Align each field and round the total size to the maximum field alignment. |
| Fixed arrays | Use the padded size of the element as the stride. A zero-length array keeps the element alignment. |
| Variants, `option<T>`, `result<T, E>` | A four-byte, little-endian tag at byte zero and a shared, naturally aligned payload area when any case has a payload. |

Records follow declaration order even when a constructor supplies fields in
a different order. An empty aggregate has size zero and alignment one.
Layouts use checked 64-bit arithmetic. The query limits traversal to 64
levels and 65,536 component visits. It rejects foreign type IDs, empty
variant descriptors, callables, and dynamic or heap-backed types. The same
aggregate type can be resolved and queried on other host
platforms, since the target shapes are independent of the Rust host layout.

## Tagged layout

A tag is the zero-based case index in declaration order. For `option<T>` the
order is `None`, `Some`. For `result<T, E>` it is `Ok`, `Err`. Every case with
a payload uses the same `payload_offset`. The area can hold the largest case
payload, and the full value is padded to the maximum of four and the maximum
payload alignment. A tag-only variant has size and alignment four, with no
payload offset. A zero-sized typed payload still carries its alignment and
has an offset. Every case counts toward layout work, including payloadless
cases. An unsupported payload rejects the whole type even if that case is
inactive in a particular value.

```rust
use scarp::native_object::{plan_x86_64_value_layout, NativeLayoutShape};

let program = scarp::compile("bind pair = (true, 42);")?;
let id = program.binding_type_ids()[0];
let layout = plan_x86_64_value_layout(program.type_registry(), id)?;
assert_eq!((layout.size, layout.alignment), (16, 8));
assert_eq!(layout.shape, NativeLayoutShape::Fields { offsets: vec![0, 8] });
# Ok::<(), Box<dyn std::error::Error>>(())
```

The value planner describes target storage for future aggregate code
generation. The ELF64 emitter accepts scalar values through its existing
calling convention. Interpreter values use their existing representations.
