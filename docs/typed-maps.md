# Source-level deterministic maps (unpublished compiler drafts)

> Design/verification notes for stacked draft PRs #268 and #269. These forms
> are **not available in the released Scarp v0.0.6-alpha.11 compiler**.

Scarp maps are immutable typed snapshots. A map has exact key and value
types, independent of the current entry count: `map<K, V>`.

The initial supported keys are `integer`, `boolean`, and `string`. Keys are
stored in canonical ascending order (numeric, false-before-true, or UTF-8
lexicographic, respectively). Floating-point, composite, and unit keys are
rejected. Values can be any supported bounded non-unit type, including
records, lists and nested maps.

```srp
bind empty: map<string, integer> = [map: <string, integer>];
bind snapshot = [map: "z": 9, "a": 1];
bind added = map_set(snapshot, "next", 4);
bind removed = map_remove(added, "z");
print(map_len(removed));
print(map_contains(removed, "a"));
print(map_get_or(removed, "missing", -1));
```

A nonempty map literal evaluates every key and value exactly once, in source
order, and then stores entries in canonical order. Distinct input order does
not change equality or display. Evaluated duplicate keys yield E0082 and
abort execution before the buffered output is published.

These **pure** call-position intrinsics are available unless shadowed by
a user-declared function:

| Name | Signature | Result |
| --- | --- | --- |
| `map_len(m)` | `map<K,V> -> integer` | Current size |
| `map_contains(m,k)` | `(map<K,V>, K) -> boolean` | Key existence |
| `map_get_or(m,k,fallback)` | `(map<K,V>,K,V) -> V` | Stored value or typed fallback |
| `map_set(m,k,v)` | `(map<K,V>,K,V) -> map<K,V>` | New snapshot with inserted/replaced value |
| `map_remove(m,k)` | `(map<K,V>,K) -> map<K,V>` | New snapshot without key, or shared unchanged snapshot if absent |

All arguments are positional and exactly typed. `map_get_or` evaluates
its fallback argument before performing the lookup, consistent with Scarp's
normal eager call semantics. If lazy evaluation is needed, put the fallback
computation in control flow rather than relying on this intrinsic.

Every map holds at most **1024 distinct entries** (E0081). Binary-search
comparisons, UTF-8 key scans, ordered insertions, and new immutable
snapshots consume deterministic work units. Source map allocations share
`ExecutionOptions.max_array_bytes` and `--max-array-bytes` with list and
fixed-array payloads (E0083). These account logical payload bytes, not
physical RSS. Snapshot aliases are never mutated.

This is not a general iterator API, persistent tree implementation,
first-class callback API, typed Option-returning `map_get`, GUI runtime or
stable host ABI. Native C11 emits a clear unsupported diagnostic until
aggregate lowering is explicitly implemented and verified.

Proof: `examples/typed_maps.srp`, `examples/persistent_maps.srp`, and
`tests/source_typed_maps.rs` / `tests/source_map_operations.rs`.
