use scarp::{
    modules::{
        cache::{CacheLimits, CacheStatus, CompilationCache},
        ConstantLimits, MemorySourceLoader, ModuleLimits,
    },
    types::TypeLimits,
    ExecutionOptions,
};
use std::sync::Arc;

fn compile(
    cache: &mut CompilationCache,
    source: &str,
    revision: u64,
) -> scarp::modules::cache::CachedProgram {
    let mut loader = MemorySourceLoader::new();
    loader.insert("main.srp", source).unwrap();
    cache
        .compile_with_loader(
            "main.srp",
            &loader,
            ModuleLimits::default(),
            ConstantLimits::default(),
            TypeLimits::default(),
            &[revision],
        )
        .unwrap()
}

#[test]
fn half_open_range_evicts_only_matching_revisions() {
    let mut cache = CompilationCache::new(CacheLimits {
        max_entries: 8,
        ..CacheLimits::default()
    });
    let a = compile(&mut cache, "print(1);", 1);
    let b = compile(&mut cache, "print(2);", 2);
    let c = compile(&mut cache, "print(3);", 3);
    let d = compile(&mut cache, "print(4);", 4);
    assert_eq!(cache.evict_identity_revisions_in_range("main.srp", 2, 4), 2);
    assert_eq!(cache.stats().entries, 2);
    assert_eq!(cache.stats().evictions, 2);
    assert_eq!(compile(&mut cache, "print(1);", 1).status, CacheStatus::Hit);
    assert_eq!(compile(&mut cache, "print(4);", 4).status, CacheStatus::Hit);
    let replacement = compile(&mut cache, "print(2);", 2);
    assert_eq!(replacement.status, CacheStatus::Stored);
    assert!(!Arc::ptr_eq(&replacement.program, &b.program));
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    c.program
        .execute(&mut stdout, &mut stderr, ExecutionOptions::default())
        .unwrap();
    assert_eq!(stdout, b"3\\n");
    assert!(!Arc::ptr_eq(&a.program, &d.program));
}

#[test]
fn empty_unknown_and_extreme_intervals_are_no_ops() {
    let mut cache = CompilationCache::new(CacheLimits::default());
    compile(&mut cache, "print(8);", u64::MAX);
    let before = cache.stats();
    assert_eq!(cache.evict_identity_revisions_in_range("main.srp", 9, 9), 0);
    assert_eq!(cache.evict_identity_revisions_in_range("main.srp", 10, 1), 0);
    assert_eq!(cache.evict_identity_revisions_in_range("missing.srp", 0, u64::MAX), 0);
    assert_eq!(cache.evict_identity_revisions_in_range("main.srp", 0, u64::MAX), 0);
    assert_eq!(cache.stats(), before);
    assert_eq!(cache.evict_identity_revisions_at_or_after("main.srp", u64::MAX), 1);
    assert_eq!(cache.stats().retained_source_bytes, 0);
}

#[test]
fn range_fixture_recompiles_and_preserves_published_snapshot() {
    let source = include_str!("../examples/revision_range_snapshot.srp");
    let mut cache = CompilationCache::new(CacheLimits::default());
    let published = compile(&mut cache, source, 12);
    assert_eq!(cache.evict_identity_revisions_in_range("main.srp", 10, 13), 1);
    let next = compile(&mut cache, source, 12);
    assert_eq!(next.status, CacheStatus::Stored);
    assert!(!Arc::ptr_eq(&published.program, &next.program));
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    published
        .program
        .execute(&mut stdout, &mut stderr, ExecutionOptions::default())
        .unwrap();
    assert_eq!(
        stdout,
        include_bytes!("../examples/revision_range_snapshot.stdout")
    );
    assert_eq!(cache.stats().evictions, 1);
}
