# Source-relative parsed AST reuse — development API

This is the fourth incremental-compiler milestone for [issue #117](https://github.com/scarp-lang/scarp/issues/117). It builds on [invalidation planning](incremental-invalidation.md), [source-relative lexical token reuse](token-cache.md), and [exact-input semantic compilation reuse](semantic-cache.md).

## What is cached?

The new caller-owned `modules::cache::SyntaxCache` stores successfully parsed Scarp `Program` trees keyed by **trusted loader identity + exact original UTF-8 source text**. It can skip both lexing and parsing of an unchanged individual file. Cached syntax spans are **source-relative**, including nested statements, import paths/aliases, parameter defaults, function contracts, type annotations, labeled call arguments, operator sites, and flat expression nodes.

Every request still **reads each source file** through its trusted loader and performs current import resolution, file/import/depth/size checks, module graph cycle checking, import visibility, and semantic linking/type/constant checks. A syntax hit is not proof that the current module graph or program is valid. Error diagnostics and linked source IDs are derived from current compilation, not cached failures.

Source files can move in graph discovery order, be preceded by changed-length Unicode/CRLF files, or have their import target identity changed without retaining stale absolute offsets. Each hit clones the immutable source-relative syntax and exhaustively rebases spans to the newly assigned `SourceMap` base. The compiler's normal parser creates the AST on a miss; **no second parser or validator** is implemented.

The original cold `modules::load` and `modules::compile_with_loader` functions remain unchanged.

## Public Rust entry points

- `modules::load_with_syntax_cache(entry, loader, ModuleLimits::default(), &mut syntax)`
- `modules::load_with_frontend_caches(entry, loader, ModuleLimits::default(), &mut tokens, &mut syntax)` — on an AST miss, the independently owned token cache may avoid lexing.
- `CompilationCache::compile_with_syntax_cache(entry, loader, CachedCompileOptions::new(&revisions), &mut syntax)`
- `CompilationCache::compile_with_frontend_caches(entry, loader, CachedCompileOptions::new(&revisions), &mut tokens, &mut syntax)` — combines all three independent, opt-in caches.

Example:

```rust
use scarp::modules::{
    self, MemorySourceLoader, ModuleLimits,
    cache::{SyntaxCache, SyntaxCacheLimits},
};
let mut files = MemorySourceLoader::new();
files.insert("main.srp", "import \"lib.srp\" as lib; print(lib.value);")?;
files.insert("lib.srp", "pub bind value=42;")?;

let mut syntax = SyntaxCache::new(SyntaxCacheLimits::default());
let first = modules::resolve(modules::load_with_syntax_cache(
    "main.srp", &files, ModuleLimits::default(), &mut syntax,
)?)?;
let again = modules::resolve(modules::load_with_syntax_cache(
    "main.srp", &files, ModuleLimits::default(), &mut syntax,
)?)?;
assert_eq!(first.sources().files().len(), again.sources().files().len());
assert_eq!(syntax.stats().hits, 2);
# Ok::<(), Box<dyn std::error::Error>>(())
```

## Cache limits and ownership

`SyntaxCacheLimits` defaults to:

| Resource | Logical cap |
| --- | ---: |
| Entries | 128 |
| Original UTF-8 source bytes | 8 MiB |
| Loader identity bytes | 1 MiB |
| Retained AST nodes (statements + expression nodes) | 1,000,000 |

An oversized entry, zero-capacity cache, or failed cache reservation still allows ordinary compilation; it is simply not retained. Only successful **parses** are stored: lexer or parser failures never produce cache entries. A successfully parsed module that later fails import validation or semantic checking may have its AST retained, but every subsequent request reruns those checks and must still fail when appropriate.

`SyntaxCacheStats` contains retained entries, payload counters, hits, misses, bypasses, and FIFO evictions. Hits never refresh eviction order. `clear()` explicitly frees cache-owned entries while preserving lifetime counters. Cache memory may also remain live outside the cache when held by a separate compiler result.

`SyntaxCache::evict_identity(identity)` drops all cached parsed versions for
one exact resolved loader identity, leaving unrelated syntax entries intact
and preserving FIFO order. The [three-tier source purge](cache-eviction.md)
also evicts matching token entries and whole-graph semantic entries that
depend on this source. Eviction never revokes already returned programs.

**These are logical quotas, not a heap-memory bound.** AST allocations, decoded strings, container overhead, temporary clones, and other compiler structures are not precisely charged. Benchmarking and actual memory accounting remain separate work.

The cache is process-local, opt-in and nonpersistent. The source loader remains trusted by the embedder; file-system trust and path resolution are not outsourced to the cache.

## Correctness coverage

`tests/module_syntax_cache.rs` checks exact-hit loader activity, utf-8/CRLF span movement, cold-vs-warm symbol reference JSON, changed source discovery order, parser and lexical errors, missing modules, visibility/import budgets, alias retargeting with unchanged syntax, FIFO and quota bypass, and combination with prior token and semantic caches.

The internal AST rebasing visitor uses exhaustive matches on all current statement/expression variants, so new AST variants that carry spans require explicit compiler changes rather than silently skipping remapping.

**Still unresolved in issue #117:** fine-grained dirty-subgraph semantic linking/type/constant reuse, memory-size accounting beyond logical payload counts, differential diagnostics across a broader malformed-source corpus and deterministic performance benchmarks. Do not describe this milestone as a complete incremental compiler or a prerelease; it is merged source development only after green CI.
