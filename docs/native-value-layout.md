# Experimental Linux x86-64 value layout

`scarp::native_object::plan_x86_64_value_layout` accepts a canonical semantic
type ID and its registry. It computes target sizes, alignments, field offsets
and fixed-array strides without allocating or serializing a target value.

| Value | Size and alignment |
| --- | --- |
| `integer` | 8 bytes, alignment 8 |
| `float` | 8 bytes, alignment 8 |
| `boolean` | 1 byte, alignment 1 |
| Tuples and records | Lay out fields in declaration order. Align each field and round the total size to the maximum field alignment. |
| Fixed arrays | Use the padded size of the element as the stride. A zero-length array keeps the element alignment. |

Records follow declaration order even when a constructor supplies fields in
a different order. An empty aggregate has size zero and alignment one.
Layouts use checked 64-bit arithmetic. The query limits traversal to 64
levels and 65,536 component visits. It rejects foreign type IDs, unresolved
representations such as variants and callables, and dynamic or heap-backed
types. The same aggregate type can be resolved and queried on other host
platforms, since the target shapes are independent of the Rust host layout.

```rust
use scarp::native_object::{plan_x86_64_value_layout, NativeLayoutShape};

let program = scarp::compile("bind pair = (true, 42);")?;
let id = program.binding_type_ids()[0];
let layout = plan_x86_64_value_layout(program.type_registry(), id)?;
assert_eq!((layout.size, layout.alignment), (16, 8));
assert_eq!(layout.shape, NativeLayoutShape::Fields { offsets: vec![0, 8] });
# Ok::<(), Box<dyn std::error::Error>>(())
```

The value planner provides a representation for future aggregate code
generation. Interpreter storage, compiler scalar stack slots and the
experimental ELF64 calling convention continue to have their own rules.
