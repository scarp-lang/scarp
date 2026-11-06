# Exact-input semantic compilation cache (development API)

The post-v0.0.6-alpha.1 `modules::cache::CompilationCache` is the second,
deliberately limited step toward [issue #117](https://github.com/scarp-lang/scarp/issues/117).
It reuses successful **whole-program semantic resolution** on **exactly
identical inputs**. It is not per-module incremental compilation, a file
watcher, an LSP server or a persistent build cache.

## What is—and is not—reused

Every cache request calls the compiler's existing trusted `load` pipeline:
entry resolution, all source loads, lexical analysis, parsing, import
validation, canonical resolved import graph construction and resource checks.
An incomplete program, changed path, missing module, malformed import or
syntax error is rejected before considering a cached semantic result.

An exact hit skips `resolve_with_compiler_limits` (linking, constant folding
and type checking) and returns an `Arc<ModuleProgram>` shared with the
previous successful compilation. No statement is executed as part of
compilation. Execution uses existing interpreter options and checks.

No shortcut is taken if the graph cannot be loaded. Failed semantic
compilations are **never** stored and never return stale successes. Existing
successful entries are retained after a failure so an exact future request
may still hit. Hosts may explicitly `clear()` the cache.

## Exact validation key

To reuse a cached result the fresh, fully loaded graph must match all these
fields:

1. All `ModuleLimits`, `ConstantLimits` and `TypeLimits`, including
   compile-time limits which might cause new diagnostics.
2. Exact original entry text, **current** source discovery order, number
   of files, and canonical identity of each source from the trusted loader.
3. All original UTF-8 source bytes and loader-provided display labels; labels
   affect diagnostic and editor locations.
4. Every **resolved import declaration's target SourceId in order**, not
   only the deduplicated target set. This rejects equal-text alias changes
   such as `[A, A, B]` becoming `[A, B, B]`.
5. One explicit, caller-owned `u64` source revision per current SourceId.
   Revision count must equal the current graph's source count. Revisions must
   advance on every document edit. **Full byte comparison still takes place
   even if revisions are accidentally reused**.

No hash-only identity, mtime, weak validation of import aliases, process-global
state, network or runtime dependency is used. Since the loaded graph and
compiler code both live in the same process, compilation state is scoped to
this binary version; caches are not serialized or reused across upgrades.
The editor remains responsible for rejecting stale navigation requests
via `EditorSnapshot::ensure_current`. Cached source IDs and symbol IDs are
never persistent identities.

## Example

```rust
use scarp::{
    modules::{
        cache::{CacheLimits, CacheStatus, CompilationCache},
        ConstantLimits, MemorySourceLoader, ModuleLimits,
    },
    types::TypeLimits,
};
use std::sync::Arc;

let mut loader = MemorySourceLoader::new();
loader.insert("main.srp", "print(42);")?;
let mut cache = CompilationCache::new(CacheLimits::default());

let mut compile = || cache.compile_with_loader(
    "main.srp",
    &loader,
    ModuleLimits::default(),
    ConstantLimits::default(),
    TypeLimits::default(),
    &[7], // in the current graph's SourceId discovery order
);
let first = compile()?;
assert_eq!(first.status, CacheStatus::Stored);
let second = compile()?;
assert_eq!(second.status, CacheStatus::Hit);
assert!(Arc::ptr_eq(&first.program, &second.program));
# Ok::<(), Box<dyn std::error::Error>>(())
```

## Capacity, eviction, and stats

`CacheLimits` defaults to **four entries** and **16 MiB cumulative retained
source bytes**. An entry whose source text exceeds the byte allowance, or
a zero-capacity cache, compiles normally but is returned with
`CacheStatus::Uncached`. A valid successful result remains available to the
caller even when retaining the key fails to allocate.

Eviction is **FIFO** in successful insertion order, not an LRU policy.
The oldest cached success is removed until a new item fits the entry and
logical source-byte quotas. Hits do not change eviction order.
`CacheStats` reports entry count, logical source payload, hits, misses,
bypasses and evictions. Calling `clear()` drops retained entries and
increments cumulative evictions. The API does not store failed builds.

**The quota is not a hard process-memory cap.** Cache entries also retain
resolved functions, source metadata, imported symbol tables, and an
`Arc<ModuleProgram>` which callers may keep alive after eviction.
The compiler's ordinary source/type/constant limits still apply; a precise
heap-size allocator budget, full AST accounting and general bounded resident
memory are not implemented. Do not advertise this as a capped-heap cache.

## Correctness gates

The `tests/module_cache.rs` suite checks:

- Identical graph requests share the same `Arc<ModuleProgram>` without
  skipping source I/O, lexing, parsing, or graph validation.
- Changes to source bytes, source revisions, importer targets (including
  equal-set alias retargets) or Unicode display labels all force a miss.
- Compiler/module/type/constant budget changes force semantic revalidation.
- Missing sources, invalid syntax, private exports and malformed revision
  arrays never return earlier cached successes.
- FIFO eviction, explicit clear, small quotas, disabled caching and immutable
  shared program reads across threads.

The earlier [invalidation-planning API](incremental-invalidation.md) remains
useful for discovering affected importers and comparing successful graphs.
This cache is a separate opt-in interface; it does **not** yet reuse
unaffected module ASTs or resolve only dirty subgraphs.

## Future work, still open in #117

- Build a verified per-module parse/link cache without duplicating parser or
  resolver rules and with failure-safe transitive invalidation.
- Prove cold/incremental equivalence for diagnostics, navigation metadata,
  exports/constants and runtime outcomes across edits, cycles, removals,
  symlink/loader changes and budget boundaries.
- Enforce an independently measured memory budget for cached compiler-owned
  data and deterministic eviction for finer-grained entries.
- Establish benchmark baselines before claiming any speedup.

This code is merged development work only after checks; it does **not**
change previously published v0.0.6-alpha.1 binaries or create a new release.
