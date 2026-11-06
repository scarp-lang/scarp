# Preflighted edit-driven cache eviction — opt-in Rust API

The source-scoped cache eviction tier can now be driven by the existing
`InvalidationSnapshot::plan_edits` dependency planner. This is an additional
`v0.0.6-alpha.2` source milestone for
[issue #117](https://github.com/scarp-lang/scarp/issues/117), not complete
incremental semantic compilation, and it changes no default CLI behavior.

## Public API

```rust
use scarp::modules::{
    cache::{
        purge_reported_edits, CacheLimits, CompilationCache,
        SyntaxCache, SyntaxCacheLimits, TokenCache, TokenCacheLimits,
    },
    invalidation::{InvalidationLimits, InvalidationSnapshot},
    MemorySourceLoader, ModuleLimits,
};

let mut loader = MemorySourceLoader::new();
loader.insert("main.srp", "import \"lib.srp\" as l; print(l.value);")?;
loader.insert("lib.srp", "pub bind value=1;")?;
let old = scarp::modules::compile_with_loader(
    "main.srp", &loader, ModuleLimits::default()
)?;
let old_graph = InvalidationSnapshot::new(
    &old, InvalidationLimits::default()
)?;

let mut semantics = CompilationCache::new(CacheLimits::default());
let mut tokens = TokenCache::new(TokenCacheLimits::default());
let mut syntax = SyntaxCache::new(SyntaxCacheLimits::default());

// After populating these caller-owned caches with normal compilation:
let result = purge_reported_edits(
    &old_graph, &["lib.srp"], &mut semantics, &mut tokens, &mut syntax
)?;
assert!(result.impact.must_rediscover);
assert!(!result.impact.unknown_identity);

// The caller MUST still recompile through the trusted source loader.
// This API deliberately never returns or reuses a compiled program.
# Ok::<(), Box<dyn std::error::Error>>(())
```

The return value `PlannedCachePurge` contains both the original
`EditImpact` and `SourceCacheEvictions`, which counts removed semantic,
token, and syntax entries separately. Source IDs in the impact refer to
the **old snapshot only**.

## Invariants and failure behavior

1. **Full preflight before mutation:** The existing snapshot planner
   validates the entire edit count and cumulative UTF-8 identity-byte
   quota before attempting any eviction. `EditLimit`, `IdentityLimit`
   and other planning errors leave all cache entries, retention counters,
   statistics and FIFO order unchanged.
2. **Known identities:** The semantic cache evicts whole-graph successes
   containing **any member of the old-graph transitive importer closure**.
   Only the explicitly changed identities lose their token and syntax
   entries. An unchanged importer may safely reuse its *original*
   source-relative lexer or AST data after fresh source loading; its
   previously linked whole-graph result is not treated as valid.
3. **Unknown identity:** The planner cannot prove how an unknown identity
   might modify imports or canonical loader mapping. All three caller-owned
   caches are therefore fully cleared. The result marks
   `unknown_identity=true` and `must_rediscover=true`.
   This is intentionally conservative and may drop unrelated cache entries.
4. **Empty edit list:** Returns an empty impact with no evictions.
   This reflects only a caller assertion, **not** a proof the filesystem,
   loader mappings or source bytes have remained unchanged.
5. **Duplicate edits:** The planner still counts every input for the quota
   but removes each source's token/syntax entries only once. Its importer
   closure is deduplicated, so semantic entry counts are not duplicated.
6. **Cold-build correctness:** Every later compile still reloads source,
   validates import identities and compiler budgets, relinks types/constants
   and rejects current diagnostics. A previous returned
   `Arc<ModuleProgram>` may stay live after eviction; this is neither
   secure erasure nor a resident-memory guarantee.

The API uses trusted, compiler-resolved loader identities, not display
labels, file chooser paths or import string literals. Avoid passing
untrusted path strings without the loader's identity resolution.
No filesystem access, path normalization, file watch, network or
background compiler work is performed here.

## Correctness verification

The `tests/module_edit_cache_purge.rs` suite checks:

- A diamond graph where editing a shared dependency discards old semantic
  results, preserves unrelated roots and unedited importer syntax/tokens,
  and recompiles to updated output while an old immutable program survives.
- Duplicate changed identities and empty inputs.
- Atomic failure on edit-count and cumulative identity-byte limits, even
  if the first identity is unknown.
- Unknown identities forcing full cache eviction, including unrelated
  entry roots.
- Cold-vs-warm semantic visibility error equivalence after Unicode-path
  source edits, without returning a stale successful program.

Source-scoped manual cleanup remains available through
[cache eviction](cache-eviction.md). Dependency-graph requirements and
remaining partial semantic reuse work are in
[invalidation](incremental-invalidation.md) and
[semantic caching](semantic-cache.md).
