# Experimental numerical tensor storage

This change adds a Rust-host storage foundation for future source-level tensors.
It does not add a Scarp grammar keyword, a new runtime Value variant, native
code generation, broadcasting, GPU support, or automatic differentiation.

## Invariants

- Finite binary64 cells in immutable row-major storage shared with NumericVector.
- Rank 0 through 8, at most 65,536 cells, and at most 65,536 on any axis.
- Rank zero requires exactly one cell. Any zero axis yields zero cells, while
  preserving the full shape. All axis bounds are validated even for empty shapes.
- Reshape preserves element count and shares storage. Leading-axis slices share
  immutable nonempty windows; empty windows detach from the original allocation.
- Axis permutations require a bijection and allocate a bounded buffer only
  when needed. They are host operations, not metered interpreter built-ins.
- The rank-two bridge uses the existing NumericTile representation and checked
  MatMul kernel rather than introducing a second matrix multiplication engine.

## Existing source-level matrix calculation

The following uses existing Scarp tile syntax, not the new host API:

```scarp
bind a = tile([1.0, 2.0, 3.0, 4.0], 2, 2);
bind b = tile([5.0, 6.0, 7.0, 8.0], 2, 2);
bind result = matmul(a, b);
print(result);
```

Expected: `tile(2x2)[[19.0, 22.0], [43.0, 50.0]]`.

## Source-language follow-up

The proposed syntax `tensor<float, 2, 2>` needs a separately reviewed parser
and semantic type design. Static shape checking, nested literal construction,
runtime allocation/work accounting, serialization and formatting must be
integrated before it can be exposed to programs. Native lowering should reject
unsupported tensor operations explicitly until implemented. Batched matrix
multiplication, broadcasting, and differentiation need independent specifications
and tests. No release version or publication is implied by this work.
