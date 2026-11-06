use scarp::{
    modules::{
        cache::{
            purge_source_identity, CacheLimits, CacheStatus, CachedCompileOptions,
            CompilationCache, SourceCacheEvictions, SyntaxCache, SyntaxCacheLimits, TokenCache,
            TokenCacheLimits,
        },
        MemorySourceLoader,
    },
    ExecutionOptions,
};
use std::sync::Arc;

fn loader(files: &[(&str, &str)]) -> MemorySourceLoader {
    let mut loader = MemorySourceLoader::new();
    for &(name, source) in files {
        loader.insert(name, source).unwrap();
    }
    loader
}

fn output(program: &scarp::modules::cache::CachedProgram) -> Vec<u8> {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    program
        .program
        .execute(&mut out, &mut err, ExecutionOptions::default())
        .unwrap();
    assert!(err.is_empty());
    out
}

#[test]
fn purge_evicts_dependency_across_all_three_tiers_but_keeps_unrelated_graphs() {
    let main_source = "import \"lib.srp\" as lib; print(lib.value);";
    let first = loader(&[("main.srp", main_source), ("lib.srp", "pub bind value=7;")]);
    let edited = loader(&[("main.srp", main_source), ("lib.srp", "pub bind value=8;")]);
    let other = loader(&[("tool.srp", "print(99);")]);
    let mut semantic = CompilationCache::new(CacheLimits {
        max_entries: 5,
        max_retained_source_bytes: 8192,
    });
    let mut tokens = TokenCache::new(TokenCacheLimits::default());
    let mut syntax = SyntaxCache::new(SyntaxCacheLimits::default());

    let old = semantic
        .compile_with_frontend_caches(
            "main.srp",
            &first,
            CachedCompileOptions::new(&[1, 1]),
            &mut tokens,
            &mut syntax,
        )
        .unwrap();
    let independent = semantic
        .compile_with_frontend_caches(
            "tool.srp",
            &other,
            CachedCompileOptions::new(&[1]),
            &mut tokens,
            &mut syntax,
        )
        .unwrap();
    let newest = semantic
        .compile_with_frontend_caches(
            "main.srp",
            &edited,
            CachedCompileOptions::new(&[1, 2]),
            &mut tokens,
            &mut syntax,
        )
        .unwrap();
    assert_eq!(output(&old), b"7\n");
    assert_eq!(output(&newest), b"8\n");
    assert_eq!(output(&independent), b"99\n");
    assert_eq!(semantic.stats().entries, 3);
    assert_eq!(tokens.stats().entries, 4); // main + both lib versions + tool
    assert_eq!(syntax.stats().entries, 4);

    let before_semantic = semantic.stats();
    let before_tokens = tokens.stats();
    let before_syntax = syntax.stats();
    let first_purge = purge_source_identity("LIB.srp", &mut semantic, &mut tokens, &mut syntax);
    assert_eq!(first_purge, SourceCacheEvictions::default());
    assert_eq!(semantic.stats(), before_semantic);
    assert_eq!(tokens.stats(), before_tokens);
    assert_eq!(syntax.stats(), before_syntax);

    let removed = purge_source_identity("lib.srp", &mut semantic, &mut tokens, &mut syntax);
    assert_eq!(
        removed,
        SourceCacheEvictions {
            semantic_entries: 2,
            token_entries: 2,
            syntax_entries: 2,
        }
    );
    assert_eq!(semantic.stats().entries, 1);
    assert_eq!(tokens.stats().entries, 2);
    assert_eq!(syntax.stats().entries, 2);
    assert_eq!(semantic.stats().evictions, before_semantic.evictions + 2);
    assert_eq!(tokens.stats().evictions, before_tokens.evictions + 2);
    assert_eq!(syntax.stats().evictions, before_syntax.evictions + 2);
    assert!(semantic.stats().retained_source_bytes < before_semantic.retained_source_bytes);
    assert_eq!(
        tokens.stats().retained_source_bytes,
        main_source.len() + "print(99);".len()
    );
    assert_eq!(
        syntax.stats().retained_identity_bytes,
        "main.srp".len() + "tool.srp".len()
    );
    assert_eq!(
        tokens.stats().retained_identity_bytes,
        "main.srp".len() + "tool.srp".len()
    );
    assert!(syntax.stats().retained_ast_nodes > 0);
    assert!(tokens.stats().retained_tokens > 0);
    assert_eq!(
        purge_source_identity("lib.srp", &mut semantic, &mut tokens, &mut syntax),
        SourceCacheEvictions::default()
    );

    // The unaffected graph stays a semantic cache hit with the same Arc.
    let retained = semantic
        .compile_with_frontend_caches(
            "tool.srp",
            &other,
            CachedCompileOptions::new(&[1]),
            &mut tokens,
            &mut syntax,
        )
        .unwrap();
    assert_eq!(retained.status, CacheStatus::Hit);
    assert!(Arc::ptr_eq(&retained.program, &independent.program));
    assert_eq!(output(&retained), b"99\n");

    // Old returned Arc snapshots continue working after removal; changed lib
    // is compiled again while the unchanged importer still hits syntax cache.
    assert_eq!(output(&old), b"7\n");
    let sem_before_reload = semantic.stats();
    let syntax_before_reload = syntax.stats();
    let recompiled = semantic
        .compile_with_frontend_caches(
            "main.srp",
            &edited,
            CachedCompileOptions::new(&[1, 2]),
            &mut tokens,
            &mut syntax,
        )
        .unwrap();
    assert_eq!(recompiled.status, CacheStatus::Stored);
    assert!(!Arc::ptr_eq(&newest.program, &recompiled.program));
    assert_eq!(output(&recompiled), b"8\n");
    assert_eq!(semantic.stats().misses, sem_before_reload.misses + 1);
    assert_eq!(syntax.stats().hits, syntax_before_reload.hits + 1);
    assert_eq!(syntax.stats().misses, syntax_before_reload.misses + 1);

    // Eviction and clear remove only cache-owned entries, maintaining logical
    // payload counters and cumulative eviction accounting.
    assert_eq!(semantic.evict_identity("main.srp"), 1);
    assert_eq!(tokens.evict_identity("main.srp"), 1);
    assert_eq!(syntax.evict_identity("main.srp"), 1);
    semantic.clear();
    tokens.clear();
    syntax.clear();
    assert_eq!(semantic.stats().entries, 0);
    assert_eq!(semantic.stats().retained_source_bytes, 0);
    assert_eq!(tokens.stats().entries, 0);
    assert_eq!(tokens.stats().retained_source_bytes, 0);
    assert_eq!(tokens.stats().retained_identity_bytes, 0);
    assert_eq!(tokens.stats().retained_tokens, 0);
    assert_eq!(syntax.stats().entries, 0);
    assert_eq!(syntax.stats().retained_source_bytes, 0);
    assert_eq!(syntax.stats().retained_identity_bytes, 0);
    assert_eq!(syntax.stats().retained_ast_nodes, 0);
    assert_eq!(output(&newest), b"8\n");
}

#[test]
fn unicode_identity_purge_accounts_for_exact_utf8_bytes_and_is_idempotent() {
    let identity = "café.srp";
    let text = "print(\"雪\");";
    let files = loader(&[(identity, text)]);
    let mut semantic = CompilationCache::new(CacheLimits::default());
    let mut tokens = TokenCache::new(TokenCacheLimits::default());
    let mut syntax = SyntaxCache::new(SyntaxCacheLimits::default());

    let first = semantic
        .compile_with_frontend_caches(
            identity,
            &files,
            CachedCompileOptions::new(&[10]),
            &mut tokens,
            &mut syntax,
        )
        .unwrap();
    assert_eq!(first.status, CacheStatus::Stored);
    assert_eq!(tokens.stats().retained_identity_bytes, identity.len());
    assert_eq!(syntax.stats().retained_identity_bytes, identity.len());
    assert_eq!(tokens.stats().retained_source_bytes, text.len());
    assert_eq!(syntax.stats().retained_source_bytes, text.len());

    assert_eq!(
        purge_source_identity("cafe.srp", &mut semantic, &mut tokens, &mut syntax),
        SourceCacheEvictions::default()
    );
    assert_eq!(
        purge_source_identity(identity, &mut semantic, &mut tokens, &mut syntax),
        SourceCacheEvictions {
            semantic_entries: 1,
            token_entries: 1,
            syntax_entries: 1,
        }
    );
    assert_eq!(
        purge_source_identity(identity, &mut semantic, &mut tokens, &mut syntax),
        SourceCacheEvictions::default()
    );
    assert_eq!(semantic.stats().retained_source_bytes, 0);
    assert_eq!(tokens.stats().retained_identity_bytes, 0);
    assert_eq!(syntax.stats().retained_identity_bytes, 0);
    assert_eq!(syntax.stats().retained_ast_nodes, 0);
    assert_eq!(output(&first), "雪\n".as_bytes());
}

#[test]
fn disabled_caches_have_no_source_entries_to_evict() {
    let source = loader(&[("main.srp", "print(1);")]);
    let mut semantic = CompilationCache::new(CacheLimits {
        max_entries: 0,
        max_retained_source_bytes: 0,
    });
    let mut tokens = TokenCache::new(TokenCacheLimits {
        max_entries: 0,
        ..TokenCacheLimits::default()
    });
    let mut syntax = SyntaxCache::new(SyntaxCacheLimits {
        max_entries: 0,
        ..SyntaxCacheLimits::default()
    });
    let compiled = semantic
        .compile_with_frontend_caches(
            "main.srp",
            &source,
            CachedCompileOptions::new(&[1]),
            &mut tokens,
            &mut syntax,
        )
        .unwrap();
    assert_eq!(compiled.status, CacheStatus::Uncached);
    assert_eq!(output(&compiled), b"1\n");
    assert_eq!(
        purge_source_identity("main.srp", &mut semantic, &mut tokens, &mut syntax),
        SourceCacheEvictions::default()
    );
    assert_eq!(semantic.stats().evictions, 0);
    assert_eq!(tokens.stats().evictions, 0);
    assert_eq!(syntax.stats().evictions, 0);
}
