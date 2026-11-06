# Bounded source-relative token cache — development API

This is an incremental front-end milestone for [issue #117](https://github.com/scarp-lang/scarp/issues/117), built on the merged whole-graph [semantic cache](semantic-cache.md) and [invalidation planning](incremental-invalidation.md). It is **not** an incremental parser, module linker, or a complete compiler cache.

## What it does

The compiler's regular `modules::load()` behavior is unchanged. Embedders may opt into `modules::load_with_token_cache(entry, loader, module_limits, &mut token_cache)` or combine the lexical cache with the existing semantic cache by calling `CompilationCache::compile_with_token_cache(entry, loader, CachedCompileOptions::new(&revisions), &mut token_cache)`. The options value groups the regular module/constant/type budgets and explicit per-source revisions.

Every request still performs normal source loading and re-resolves **all** imports using the trusted `SourceLoader`. It checks file/import/depth/source-byte budgets, runs the **full parser** and validates import declarations, path identities, cycle rules, linker visibility, constants and types as before. **The only skipped work on a token hit is lexing that unchanged individual source.**

The `TokenCache` records the original loader identity, exact UTF-8 source bytes, and compiler-produced **source-relative** lexer tokens. It never caches a token with the absolute `SourceMap` base. When graph traversal assigns new offsets, cached tokens are cloned and their byte spans shifted to the **new** base before parsing, preserving original source coordinates and diagnostics.

This matters when the entry file grows, new imports appear, discovery order changes, or one imported module is reached through several aliases. Symbol/reference IDs are still fresh for each resolved graph. A different source text at the same identity always triggers lexing again.

Lexically invalid source is **not** cached. Lexically valid source with parser errors may retain tokens, but syntax checking and errors still occur on every compile. The same applies to invalid imports, private exports, cycles and resource limits. Errors are **never** converted into cached successful results.

## Caller-owned limits

`TokenCacheLimits::default()` has separate logical caps:

| Resource | Default |
| --- | ---: |
| Cached file entries | 128 |
| Cumulative original UTF-8 source bytes | 8 MiB |
| Cumulative UTF-8 loader identity bytes | 1 MiB |
| Cumulative lexer token count, including EOF tokens | 1,000,000 |

These are **logical payload bounds**, not a verified heap-memory cap. Decoded token strings, vector capacities, allocator metadata and program memory may exceed the quotas. Do not claim bounded resident memory on this basis.

The loader-identity budget limits retained path/key text independently of
source content, avoiding unexpectedly large metadata allocations for
custom loaders. An over-limit identity still compiles normally without
being stored.

Cache replacement is deterministic FIFO (hits do not refresh insertion order). Disabled or oversized caches return fresh lexer results normally; `TokenCacheStats` reports hits, misses, bypasses, evictions, stored entries, source bytes and tokens. `clear()` explicitly drops all retained entries. TokenCache and CompilationCache are independent owners with independent quotas, statistics and eviction.

## Example

```rust
use scarp::{
    modules::{
        self,
        cache::{TokenCache, TokenCacheLimits},
        MemorySourceLoader, ModuleLimits,
    },
};
let mut loader = MemorySourceLoader::new();
loader.insert("main.srp", "import \"lib.srp\" as lib; print(lib.value);")?;
loader.insert("lib.srp", "pub bind value=42;")?;

let mut tokens = TokenCache::new(TokenCacheLimits::default());
let first = modules::resolve(modules::load_with_token_cache(
    "main.srp", &loader, ModuleLimits::default(), &mut tokens
)?)?;
let second = modules::resolve(modules::load_with_token_cache(
    "main.srp", &loader, ModuleLimits::default(), &mut tokens
)?)?;
assert_eq!(first.sources().files().len(), second.sources().files().len());
assert_eq!(tokens.stats().hits, 2);
# Ok::<(), Box<dyn std::error::Error>>(())
```

## Verification and remaining milestones

`tests/module_token_cache.rs` covers repeated cold/warm loads, unchanged imported sources after changed-length UTF-8/CRLF entry files, source reorder and SourceId renumbering, exact symbol-index JSON and import location equivalence, parser/lexer failures, visibility and limit errors, deterministic resource quotas, and combination with opt-in semantic caching.

The follow-on [source-relative syntax cache](syntax-cache.md) can now reuse successfully parsed file trees and rebase all nested syntax spans when source offsets change. It may optionally combine with this token cache, which helps on syntax misses. Future work remains dependency-aware incremental semantic linking and constant checking, audited resident memory, and cold-vs-incremental diagnostic benchmarks. Keep issue #117 open.

This is a development-only opt-in API after v0.0.6-alpha.1. No release or CLI behavior is changed by this milestone.
