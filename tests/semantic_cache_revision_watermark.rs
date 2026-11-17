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
fn watermark_removes_only_older_revisions_and_preserves_snapshots() {
    let mut cache = CompilationCache::new(CacheLimits { max_entries: 8, ..CacheLimits::default() });
    let old = compile(&mut cache, "print(1);", 1);
    let middle = compile(&mut cache, "print(2);", 2);
    let current = compile(&mut cache, "print(3);", 3);
    assert_eq!(cache.stats().entries, 3);
    assert_eq!(cache.evict_identity_revisions_before("main.srp", 3), 2);
    assert_eq!(cache.stats().entries, 1);
    assert_eq!(cache.stats().evictions, 2);
    let hit = compile(&mut cache, "print(3);", 3);
    assert_eq!(hit.status, CacheStatus::Hit);
    assert!(Arc::ptr_eq(&current.program, &hit.program));
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    old.program.execute(&mut stdout, &mut stderr, ExecutionOptions::default()).unwrap();
    assert_eq!(stdout, b"1\n");
    stdout.clear();
    middle.program.execute(&mut stdout, &mut stderr, ExecutionOptions::default()).unwrap();
    assert_eq!(stdout, b"2\n");
}

#[test]
fn zero_and_unknown_watermarks_do_not_change_accounting() {
    let mut cache = CompilationCache::new(CacheLimits::default());
    compile(&mut cache, "print(7);", 0);
    let before = cache.stats();
    assert_eq!(cache.evict_identity_revisions_before("main.srp", 0), 0);
    assert_eq!(cache.evict_identity_revisions_before("unknown.srp", u64::MAX), 0);
    assert_eq!(cache.stats(), before);
    assert_eq!(cache.evict_identity_revisions_before("main.srp", 1), 1);
    assert_eq!(cache.stats().retained_source_bytes, 0);
    assert_eq!(cache.evict_identity_revisions_before("main.srp", u64::MAX), 0);
}

#[test]
fn watermark_fixture_recompiles_without_changing_source_semantics() {
    let source = include_str!("../examples/revision_watermark_snapshot.srp");
    let mut cache = CompilationCache::new(CacheLimits::default());
    let first = compile(&mut cache, source, 10);
    assert_eq!(cache.evict_identity_revisions_before("main.srp", 11), 1);
    let next = compile(&mut cache, source, 11);
    assert_eq!(next.status, CacheStatus::Stored);
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    next.program.execute(&mut stdout, &mut stderr, ExecutionOptions::default()).unwrap();
    assert_eq!(stdout, include_bytes!("../examples/revision_watermark_snapshot.stdout"));
    assert!(!Arc::ptr_eq(&first.program, &next.program));
}
