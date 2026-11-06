# Source-scoped cache eviction — development API

This **opt-in Rust embedding API** complements the incremental compiler caches
in v0.0.6-alpha.2 source. It helps editor document closure, explicit retention
cleanup and discarding stale snapshots without flushing unrelated files.

It is **not** a file watcher, a guarantee of memory erasure, an incremental
semantic linker or a way to skip fresh compiler validation.

## Why eviction uses resolved identities

A source identity can appear in several compiled entry graphs, or in several
cached source versions. Display labels, original import spellings and unstable
SourceIds are not necessarily the same as the **trusted loader identity**.

Each cache now offers `evict_identity(&mut self, identity: &str) -> usize`,
using exact, case-sensitive matching of the resolved loader identity:

- `TokenCache` evicts **all** retained token entries for that identity,
  including different original source contents.
- `SyntaxCache` evicts **all** parsed AST entries for that identity.
- `CompilationCache` evicts **every** semantic whole-graph success containing
  that identity **anywhere in its dependency graph**. This includes any
  compiled importer that might depend on its exports and transitive imports.

Each method preserves the FIFO order of surviving entries, releases the
corresponding **logical retained** byte/token/node charges and returns the
number of removed entries. Existing cumulative `evictions` counters increase;
hits, misses and bypasses do not change. Absent identities are no-ops.

To evict from all three tiers with one call:

```rust
use scarp::modules::cache::{
    purge_source_identity, CacheLimits, CompilationCache,
    SyntaxCache, SyntaxCacheLimits, TokenCache, TokenCacheLimits,
};

let mut semantic = CompilationCache::new(CacheLimits::default());
let mut tokens = TokenCache::new(TokenCacheLimits::default());
let mut syntax = SyntaxCache::new(SyntaxCacheLimits::default());

// Populate caches with compile_with_frontend_caches before this call.
let removed = purge_source_identity(
    "lib.srp", &mut semantic, &mut tokens, &mut syntax
);
assert_eq!(removed.semantic_entries, 0); // empty cache in this example
assert_eq!(removed.token_entries, 0);
assert_eq!(removed.syntax_entries, 0);
```

`SourceCacheEvictions` holds separate semantic/token/syntax entry counts.
To discard everything, each cache still provides `clear()`.

## Ownership, correctness and safety boundaries

Eviction only changes **what these caches own**. A caller can retain a
previously returned `Arc<ModuleProgram>` after its cache entry is evicted or
cleared. This API does **not** revoke an issued program, wipe heap contents,
delete files, alter trusted loader behavior, or guarantee secure data erasure.
A host handling secrets must manage its own sensitive-data lifecycle.

The cache stats count **logical retained payload**, not exact heap size.
Allocator capacity, decoded token strings and separately retained compiler
results may remain live after cache eviction. Lower cache counters do not
prove resident-memory reclamation.

A source edit or import-alias retarget **does not require** eviction for
correctness: every compilation reloads the current graph, verifies imports,
source bytes, source labels, compiler limits and revision keys. A purge is an
optional retention-management choice, not proof of the source having changed.

Evicting a dependency from the front-end caches retains unrelated individual
source entries; its semantic eviction removes all graphs containing that
identity. The next compilation still runs the current loader and all ordinary
safety checks. Existing SourceIds and symbol IDs are never persistent keys.

## Verification and remaining work

`tests/module_cache_purge.rs` checks real compiler results across independent
entry roots, multiple versions of one imported source, surviving hits,
retained `Arc` programs, Unicode identity accounting, idempotence, per-tier
eviction, FIFO survivors and disabled caches.

The default compiler/CLI path is unchanged. Dependency-aware *semantic*
subgraph reuse, exact process-heap accounting and broader differential
diagnostic coverage are still unresolved in
[issue #117](https://github.com/scarp-lang/scarp/issues/117).
