use scarp::{
    modules::{
        cache::{CacheLimits, CacheStatus, CompilationCache},
        ConstantLimits, MemorySourceLoader, ModuleLimits,
    },
    types::TypeLimits,
    ExecutionOptions,
};
use std::sync::Arc;

fn compile(cache: &mut CompilationCache, source: &str, revision: u64) -> scarp::modules::cache::CachedProgram {
    let mut loader = MemorySourceLoader::new();
    loader.insert("main.srp", source).unwrap();
    cache.compile_with_loader(
        "main.srp", &loader, ModuleLimits::default(), ConstantLimits::default(),
        TypeLimits::default(), &[revision],
    ).unwrap()
}

#[test]
fn ceiling_evicts_inclusive_range_but_preserves_older_snapshot() {
    let mut cache = CompilationCache::new(CacheLimits {
        max_entries: 8,
        ..CacheLimits::default()
    });
    let old = compile(&mut cache, "print(1);", 1);
    let middle = compile(&mut cache, "print(2);", 2);
    let newest = compile(&mut cache, "print(3);", 3);
    assert_eq!(cache.evict_identity_revisions_at_or_after("main.srp", 2), 2);
    assert_eq!(cache.stats().entries, 1);
    assert_eq!(cache.stats().evictions, 2);
    let hit = compile(&mut cache, "print(1);", 1);
    assert_eq!(hit.status, CacheStatus::Hit);
    assert!(Arc::ptr_eq(&old.program, &hit.program));
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    newest.program.execute(&mut stdout, &mut stderr, ExecutionOptions::default()).unwrap();
    assert_eq!(stdout, b"3\n");
    stdout.clear();
    middle.program.execute(&mut stdout, &mut stderr, ExecutionOptions::default()).unwrap();
    assert_eq!(stdout, b"2\n");
}

#[test]
fn unknown_identity_and_max_ceiling_preserve_counters() {
    let mut cache = CompilationCache::new(CacheLimits::default());
    compile(&mut cache, "print(7);", 0);
    let before = cache.stats();
    assert_eq!(cache.evict_identity_revisions_at_or_after("unknown.srp", 0), 0);
    assert_eq!(cache.evict_identity_revisions_at_or_after("main.srp", u64::MAX), 0);
    assert_eq!(cache.stats(), before);
    assert_eq!(cache.evict_identity_revisions_at_or_after("main.srp", 0), 1);
    assert_eq!(cache.stats().retained_source_bytes, 0);
    assert_eq!(cache.evict_identity_revisions_at_or_after("main.srp", 0), 0);
}

#[test]
fn ceiling_fixture_recompiles_after_speculative_revision_discard() {
    let source = include_str!("../examples/revision_ceiling_snapshot.srp");
    let mut cache = CompilationCache::new(CacheLimits::default());
    let original = compile(&mut cache, source, 5);
    assert_eq!(cache.evict_identity_revisions_at_or_after("main.srp", 5), 1);
    let restored = compile(&mut cache, source, 4);
    assert_eq!(restored.status, CacheStatus::Stored);
    assert!(!Arc::ptr_eq(&original.program, &restored.program));
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    restored.program.execute(&mut stdout, &mut stderr, ExecutionOptions::default()).unwrap();
    assert_eq!(stdout, include_bytes!("../examples/revision_ceiling_snapshot.stdout"));
}
