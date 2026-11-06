use scarp::{
    diagnostics::DiagnosticKind as Kind,
    modules::{
        self,
        cache::{
            purge_reported_edits, CacheLimits, CacheStatus, CachedCompileError,
            CachedCompileOptions, CompilationCache, PlannedCachePurge, SourceCacheEvictions,
            SyntaxCache, SyntaxCacheLimits, TokenCache, TokenCacheLimits,
        },
        invalidation::{EditImpact, InvalidationError, InvalidationLimits, InvalidationSnapshot},
        MemorySourceLoader, ModuleLimits,
    },
    ExecutionOptions,
};
use std::sync::Arc;

fn loader(sources: &[(&str, &str)]) -> MemorySourceLoader {
    let mut result = MemorySourceLoader::new();
    for &(name, content) in sources {
        result.insert(name, content).unwrap();
    }
    result
}

fn output(program: &scarp::modules::cache::CachedProgram) -> Vec<u8> {
    let mut out = Vec::new();
    let mut err = Vec::new();
    program
        .program
        .execute(&mut out, &mut err, ExecutionOptions::default())
        .unwrap();
    assert!(err.is_empty());
    out
}

fn caches() -> (CompilationCache, TokenCache, SyntaxCache) {
    (
        CompilationCache::new(CacheLimits::default()),
        TokenCache::new(TokenCacheLimits::default()),
        SyntaxCache::new(SyntaxCacheLimits::default()),
    )
}

#[test]
fn known_diamond_edit_drops_semantics_but_preserves_unedited_syntax_and_other_roots() {
    let original = loader(&[
        (
            "main.srp",
            "import \"a.srp\" as a; import \"b.srp\" as b; print(a.value,b.value);",
        ),
        (
            "a.srp",
            "import \"shared.srp\" as s; pub bind value=s.value;",
        ),
        (
            "b.srp",
            "import \"shared.srp\" as s; pub bind value=s.value;",
        ),
        ("shared.srp", "pub bind value=7;"),
    ]);
    let changed = loader(&[
        (
            "main.srp",
            "import \"a.srp\" as a; import \"b.srp\" as b; print(a.value,b.value);",
        ),
        (
            "a.srp",
            "import \"shared.srp\" as s; pub bind value=s.value;",
        ),
        (
            "b.srp",
            "import \"shared.srp\" as s; pub bind value=s.value;",
        ),
        ("shared.srp", "pub bind value=8;"),
    ]);
    let unrelated = loader(&[("tool.srp", "print(99);")]);
    let (mut semantic, mut tokens, mut syntax) = caches();
    let old = semantic
        .compile_with_frontend_caches(
            "main.srp",
            &original,
            CachedCompileOptions::new(&[1, 1, 1, 1]),
            &mut tokens,
            &mut syntax,
        )
        .unwrap();
    let other = semantic
        .compile_with_frontend_caches(
            "tool.srp",
            &unrelated,
            CachedCompileOptions::new(&[1]),
            &mut tokens,
            &mut syntax,
        )
        .unwrap();
    assert_eq!(old.status, CacheStatus::Stored);
    assert_eq!(other.status, CacheStatus::Stored);
    assert_eq!(output(&old), b"7 7\n");
    assert_eq!(tokens.stats().entries, 5);
    assert_eq!(syntax.stats().entries, 5);

    let snapshot = InvalidationSnapshot::new(&old.program, InvalidationLimits::default()).unwrap();
    let empty =
        purge_reported_edits(&snapshot, &[], &mut semantic, &mut tokens, &mut syntax).unwrap();
    assert_eq!(
        empty,
        PlannedCachePurge {
            impact: EditImpact {
                affected: vec![],
                unknown_identity: false,
                must_rediscover: false,
            },
            removed: SourceCacheEvictions::default(),
        }
    );
    let before = (semantic.stats(), tokens.stats(), syntax.stats());
    let edit = purge_reported_edits(
        &snapshot,
        &["shared.srp", "shared.srp"],
        &mut semantic,
        &mut tokens,
        &mut syntax,
    )
    .unwrap();
    assert_eq!(edit.impact.affected.len(), 4); // diamond/importers deduplicated
    assert!(edit.impact.must_rediscover && !edit.impact.unknown_identity);
    assert_eq!(
        edit.removed,
        SourceCacheEvictions {
            semantic_entries: 1,
            token_entries: 1,
            syntax_entries: 1,
        }
    );
    assert_eq!(semantic.stats().entries, 1);
    assert_eq!(tokens.stats().entries, 4);
    assert_eq!(syntax.stats().entries, 4);
    assert_eq!(semantic.stats().evictions, before.0.evictions + 1);
    assert_eq!(tokens.stats().evictions, before.1.evictions + 1);
    assert_eq!(syntax.stats().evictions, before.2.evictions + 1);
    assert_eq!(semantic.stats().hits, before.0.hits);
    assert_eq!(tokens.stats().misses, before.1.misses);
    assert_eq!(syntax.stats().bypasses, before.2.bypasses);

    let hit = semantic
        .compile_with_frontend_caches(
            "tool.srp",
            &unrelated,
            CachedCompileOptions::new(&[1]),
            &mut tokens,
            &mut syntax,
        )
        .unwrap();
    assert_eq!(hit.status, CacheStatus::Hit);
    assert!(Arc::ptr_eq(&hit.program, &other.program));

    let prior_syntax = syntax.stats();
    let fresh = semantic
        .compile_with_frontend_caches(
            "main.srp",
            &changed,
            CachedCompileOptions::new(&[1, 1, 2, 1]),
            &mut tokens,
            &mut syntax,
        )
        .unwrap();
    assert_eq!(fresh.status, CacheStatus::Stored);
    assert_eq!(output(&fresh), b"8 8\n");
    assert_eq!(output(&old), b"7 7\n"); // returned Arc remains immutable
    assert_eq!(syntax.stats().hits, prior_syntax.hits + 3);
    assert_eq!(syntax.stats().misses, prior_syntax.misses + 1);
    assert_eq!(
        purge_reported_edits(
            &snapshot,
            &["missing.srp"],
            &mut semantic,
            &mut tokens,
            &mut syntax,
        )
        .unwrap()
        .impact
        .unknown_identity,
        true
    );
}

#[test]
fn complete_input_preflight_is_transactional_even_for_unknown_paths() {
    let original = loader(&[
        ("main.srp", "import \"lib.srp\" as lib; print(lib.value);"),
        ("lib.srp", "pub bind value=42;"),
    ]);
    let (mut semantic, mut tokens, mut syntax) = caches();
    let old = semantic
        .compile_with_frontend_caches(
            "main.srp",
            &original,
            CachedCompileOptions::new(&[1, 1]),
            &mut tokens,
            &mut syntax,
        )
        .unwrap();
    let snapshot = InvalidationSnapshot::new(
        &old.program,
        InvalidationLimits {
            max_changed_files: 2,
            ..InvalidationLimits::default()
        },
    )
    .unwrap();
    let expected = (semantic.stats(), tokens.stats(), syntax.stats());
    let too_many = purge_reported_edits(
        &snapshot,
        &["unknown.srp", "lib.srp", "lib.srp"],
        &mut semantic,
        &mut tokens,
        &mut syntax,
    );
    assert!(matches!(too_many, Err(InvalidationError::EditLimit)));
    assert_eq!((semantic.stats(), tokens.stats(), syntax.stats()), expected);

    let limited = InvalidationSnapshot::new(
        &old.program,
        InvalidationLimits {
            max_identity_bytes: 19,
            ..InvalidationLimits::default()
        },
    )
    .unwrap();
    assert!(matches!(
        purge_reported_edits(
            &limited,
            &["unknown.srp", "main.srp", "lib.srp"],
            &mut semantic,
            &mut tokens,
            &mut syntax,
        ),
        Err(InvalidationError::IdentityLimit)
    ));
    assert_eq!((semantic.stats(), tokens.stats(), syntax.stats()), expected);

    // Once the entire input is valid, an unknown loader identity invalidates
    // all caches, even unrelated roots outside the old snapshot.
    let other_loader = loader(&[("tool.srp", "print(99);")]);
    let _other = semantic
        .compile_with_frontend_caches(
            "tool.srp",
            &other_loader,
            CachedCompileOptions::new(&[1]),
            &mut tokens,
            &mut syntax,
        )
        .unwrap();
    let old_counts = (semantic.stats(), tokens.stats(), syntax.stats());
    let purged = purge_reported_edits(
        &snapshot,
        &["unknown.srp"],
        &mut semantic,
        &mut tokens,
        &mut syntax,
    )
    .unwrap();
    assert!(purged.impact.unknown_identity && purged.impact.must_rediscover);
    assert_eq!(purged.impact.affected.len(), 2);
    assert_eq!(
        purged.removed,
        SourceCacheEvictions {
            semantic_entries: old_counts.0.entries,
            token_entries: old_counts.1.entries,
            syntax_entries: old_counts.2.entries,
        }
    );
    assert_eq!(semantic.stats().entries, 0);
    assert_eq!(tokens.stats().entries, 0);
    assert_eq!(syntax.stats().entries, 0);
    assert_eq!(semantic.stats().retained_source_bytes, 0);
    assert_eq!(tokens.stats().retained_source_bytes, 0);
    assert_eq!(syntax.stats().retained_ast_nodes, 0);
    assert_eq!(
        semantic.stats().evictions,
        old_counts.0.evictions + old_counts.0.entries as u64
    );
    assert_eq!(
        tokens.stats().evictions,
        old_counts.1.evictions + old_counts.1.entries as u64
    );
    assert_eq!(
        syntax.stats().evictions,
        old_counts.2.evictions + old_counts.2.entries as u64
    );
    assert_eq!(output(&old), b"42\n");
}

#[test]
fn planned_purge_does_not_rescue_semantically_invalid_edits() {
    let original = loader(&[
        ("main.srp", "import \"café.srp\" as lib; print(lib.value);"),
        ("café.srp", "pub bind value=7;"),
    ]);
    let private = loader(&[
        ("main.srp", "import \"café.srp\" as lib; print(lib.value);"),
        ("café.srp", "bind value=7;"),
    ]);
    let (mut semantic, mut tokens, mut syntax) = caches();
    let saved = semantic
        .compile_with_frontend_caches(
            "main.srp",
            &original,
            CachedCompileOptions::new(&[1, 1]),
            &mut tokens,
            &mut syntax,
        )
        .unwrap();
    let snapshot =
        InvalidationSnapshot::new(&saved.program, InvalidationLimits::default()).unwrap();
    let impact = purge_reported_edits(
        &snapshot,
        &["café.srp"],
        &mut semantic,
        &mut tokens,
        &mut syntax,
    )
    .unwrap();
    assert_eq!(impact.removed.semantic_entries, 1);
    assert_eq!(impact.removed.token_entries, 1);
    assert_eq!(impact.removed.syntax_entries, 1);
    assert_eq!(semantic.stats().entries, 0);

    let actual = semantic
        .compile_with_frontend_caches(
            "main.srp",
            &private,
            CachedCompileOptions::new(&[1, 2]),
            &mut tokens,
            &mut syntax,
        )
        .unwrap_err();
    let cold =
        modules::compile_with_loader("main.srp", &private, ModuleLimits::default()).unwrap_err();
    match actual {
        CachedCompileError::Module(diagnostic) => {
            assert_eq!(diagnostic.diagnostic.kind, Kind::ModuleVisibility);
            assert_eq!(diagnostic.diagnostic.kind, cold.diagnostic.kind);
            assert_eq!(diagnostic.render(), cold.render());
        }
        error => panic!("expected visibility diagnostic, got {error}"),
    }
    assert_eq!(semantic.stats().entries, 0);
    assert_eq!(output(&saved), b"7\n");
}
