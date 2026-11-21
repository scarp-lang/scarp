# Typed dynamic lists

This feature is available in alpha.12 interpreter builds.

Dynamic lists are **typed, immutable snapshots**, not mutable heap containers.
Unlike fixed `array<T, N>`, a `list<T>` has no length in its type identity.

```srp
bind empty: list<integer> = [list: <integer>];
bind items = [list: 10, 20, 30];
bind longer = list_push(items, 40);
bind changed = list_set(longer, 0, 11);
bind shorter = list_remove(changed, 2);
print(list_len(shorter), shorter[0]);
```

The literal `[list: first, ...]` infers one exact homogeneous element type.
An empty list must specify `[list: <T>]`. A list can contain records, tuples,
arrays, variants and nested lists wherever the existing shape and memory
limits permit; `unit` is never a valid element type.

The following pure operations are compiler-recognized in call position unless
shadowed by a user-declared function of the same name:

| Operation | Type | Behavior |
| --- | --- | --- |
| `list_len(xs)` | `list<T> -> integer` | Returns current length |
| `list_push(xs, value)` | `(list<T>, T) -> list<T>` | Appends, returns new snapshot |
| `list_set(xs, index, value)` | `(list<T>, integer, T) -> list<T>` | Replaces, returns new snapshot |
| `list_remove(xs, index)` | `(list<T>, integer) -> list<T>` | Removes, returns new snapshot |
| `list_reverse(xs)` | `list<T> -> list<T>` | Reverses elements in a new snapshot |
| `list_swap(xs, first, second)` | `(list<T>, integer, integer) -> list<T>` | Swaps two indexed elements in a new snapshot |

Arguments are positional and type-checked exactly. Runtime indexing uses a
zero-based `integer` and E0079 on negative/out-of-range indices. Push rejects
length > 4096 (E0078). Update operations charge proportional work and full
fresh snapshot allocation **before** publishing a new value. Lists, maps and
fixed arrays share the configured `ExecutionOptions.max_array_bytes`
allocation budget; immutable aliases retain their original value. Full
execution output is committed only on success. The byte allowance is logical
payload accounting and not a promise about process RSS.

`list_reverse` accepts an empty list. `list_swap` evaluates the receiver and both
indices exactly once, in source order; equal indices are valid. A negative or
out-of-bounds index produces E0079 at the first invalid index argument.
Both operations allocate and charge a complete new immutable list snapshot,
including for empty reversed lists. See
[the runnable reordering example](../examples/list_reordering.srp).

The implementation does **not** define an `in-place append` operation,
persistent tree optimization, map keys, iteration syntax, slicing, native
aggregate code generation, an ABI, or a GUI toolkit. These need separate
review, benchmarks and compatibility tests.

Executable references: `examples/typed_dynamic_lists.srp`,
`examples/persistent_list_updates.srp`,
`tests/source_dynamic_lists.rs`, and `tests/source_list_operations.rs`.
