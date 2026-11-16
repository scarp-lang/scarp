use scarp::modules::{
    self,
    cache::{SyntaxCache, SyntaxCacheLimits},
    MemorySourceLoader, ModuleLimits,
};

fn loader(main: &str, library: &str) -> MemorySourceLoader {
    let mut source = MemorySourceLoader::new();
    source.insert("main.srp", main).unwrap();
    source.insert("library.srp", library).unwrap();
    source
}

fn compile(source: &MemorySourceLoader, cache: &mut SyntaxCache) -> Vec<u8> {
    let graph =
        modules::load_with_syntax_cache("main.srp", source, ModuleLimits::default(), cache)
            .unwrap();
    let program = modules::resolve(graph).unwrap();
    let mut stdout = Vec::new();
    program
        .execute(&mut stdout, &mut Vec::new(), Default::default())
        .unwrap();
    stdout
}

const MAIN: &str = "import \"library.srp\" as lib; print(lib.answer());";
const LIB: &str = "pub pure funcra answer() -> integer { return 42; }";

#[test]
fn shrinking_entry_quota_evicts_oldest_and_preserves_semantics() {
    let source = loader(MAIN, LIB);
    let mut cache = SyntaxCache::new(SyntaxCacheLimits::default());
    assert_eq!(compile(&source, &mut cache), b"42\n");
    assert_eq!(cache.stats().entries, 2);
    let old = cache.stats();
    let limits = SyntaxCacheLimits {
        max_entries: 1,
        ..SyntaxCacheLimits::default()
    };
    cache.set_limits(limits);
    assert_eq!(cache.limits(), limits);
    let after = cache.stats();
    assert_eq!(after.entries, 1);
    assert_eq!(after.evictions, old.evictions + 1);
    assert!(after.retained_source_bytes <= limits.max_source_bytes);
    assert!(after.retained_identity_bytes <= limits.max_identity_bytes);
    assert!(after.retained_ast_nodes <= limits.max_ast_nodes);
    assert_eq!(compile(&source, &mut cache), b"42\n");
    assert_eq!(cache.stats().entries, 1);
}

#[test]
fn every_zero_quota_disables_retention_and_reexpansion_restores_hits() {
    for disabled in 0..4 {
        let source = loader(MAIN, LIB);
        let mut cache = SyntaxCache::new(SyntaxCacheLimits::default());
        assert_eq!(compile(&source, &mut cache), b"42\n");
        let mut limits = SyntaxCacheLimits::default();
        match disabled {
            0 => limits.max_entries = 0,
            1 => limits.max_source_bytes = 0,
            2 => limits.max_identity_bytes = 0,
            _ => limits.max_ast_nodes = 0,
        }
        cache.set_limits(limits);
        let after = cache.stats();
        assert_eq!(after.entries, 0);
        assert_eq!(after.retained_source_bytes, 0);
        assert_eq!(after.retained_identity_bytes, 0);
        assert_eq!(after.retained_ast_nodes, 0);
        assert_eq!(after.evictions, 2);
        assert_eq!(compile(&source, &mut cache), b"42\n");
        assert_eq!(cache.stats().entries, 0);
        assert!(cache.stats().bypasses >= 2);
        cache.set_limits(SyntaxCacheLimits::default());
        assert_eq!(compile(&source, &mut cache), b"42\n");
        assert_eq!(cache.stats().entries, 2);
        let hits = cache.stats().hits;
        assert_eq!(compile(&source, &mut cache), b"42\n");
        assert_eq!(cache.stats().hits, hits + 2);
    }
}

#[test]
fn changing_byte_and_node_quotas_evicts_until_all_limits_hold() {
    let source = loader(MAIN, LIB);
    let mut cache = SyntaxCache::new(SyntaxCacheLimits::default());
    assert_eq!(compile(&source, &mut cache), b"42\n");
    let old = cache.stats();
    let limits = SyntaxCacheLimits {
        max_entries: 100,
        max_source_bytes: old.retained_source_bytes - 1,
        max_identity_bytes: old.retained_identity_bytes - 1,
        max_ast_nodes: old.retained_ast_nodes - 1,
    };
    cache.set_limits(limits);
    let stats = cache.stats();
    assert!(stats.entries < old.entries);
    assert!(stats.retained_source_bytes <= limits.max_source_bytes);
    assert!(stats.retained_identity_bytes <= limits.max_identity_bytes);
    assert!(stats.retained_ast_nodes <= limits.max_ast_nodes);
    assert_eq!(compile(&source, &mut cache), b"42\n");
}

#[test]
fn syntax_reconfiguration_does_not_cache_stale_semantics() {
    let mut cache = SyntaxCache::new(SyntaxCacheLimits::default());
    let before = loader(MAIN, LIB);
    assert_eq!(compile(&before, &mut cache), b"42\n");
    cache.set_limits(SyntaxCacheLimits {
        max_entries: 1,
        ..SyntaxCacheLimits::default()
    });
    cache.set_limits(SyntaxCacheLimits::default());
    let changed = loader(
        MAIN,
        "pub pure funcra answer() -> integer { return 99; }",
    );
    assert_eq!(compile(&changed, &mut cache), b"99\n");
    assert_eq!(compile(&before, &mut cache), b"42\n");
}
