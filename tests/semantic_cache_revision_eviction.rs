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

fn output(snapshot: &scarp::modules::cache::CachedProgram) -> Vec<u8> {
    let mut out = Vec::new();
    let mut err = Vec::new();
    snapshot
        .program
        .execute(&mut out, &mut err, ExecutionOptions::default())
        .unwrap();
    assert!(err.is_empty());
    out
}

#[test]
fn revision_eviction_is_selective_and_keeps_published_snapshots_live() {
    let mut cache = CompilationCache::new(CacheLimits::default());
    let first = compile(&mut cache, "print(1);", 10);
    let second = compile(&mut cache, "print(2);", 11);
    assert_eq!(cache.stats().entries, 2);
    let before = cache.stats();
    assert_eq!(cache.evict_identity_revision("main.srp", 10), 1);
    assert_eq!(cache.stats().entries, 1);
    assert_eq!(cache.stats().evictions, before.evictions + 1);
    let hit = compile(&mut cache, "print(2);", 11);
    assert_eq!(hit.status, CacheStatus::Hit);
    assert!(Arc::ptr_eq(&second.program, &hit.program));
    assert_eq!(output(&first), b"1\n");
    assert_eq!(output(&second), b"2\n");
    let recompiled = compile(&mut cache, "print(1);", 10);
    assert_eq!(recompiled.status, CacheStatus::Stored);
    assert!(!Arc::ptr_eq(&first.program, &recompiled.program));
}

#[test]
fn unknown_revision_and_identity_are_no_ops() {
    let mut cache = CompilationCache::new(CacheLimits::default());
    let first = compile(&mut cache, "print(9);", 7);
    let before = cache.stats();
    assert_eq!(cache.evict_identity_revision("missing.srp", 7), 0);
    assert_eq!(cache.evict_identity_revision("main.srp", 8), 0);
    assert_eq!(cache.stats(), before);
    let hit = compile(&mut cache, "print(9);", 7);
    assert_eq!(hit.status, CacheStatus::Hit);
    assert!(Arc::ptr_eq(&first.program, &hit.program));
}

#[test]
fn repeated_revision_eviction_is_idempotent() {
    let mut cache = CompilationCache::new(CacheLimits::default());
    let source = include_str!("../examples/revision_cache_snapshot.srp");
    let first = compile(&mut cache, source, 3);
    assert_eq!(
        output(&first),
        include_bytes!("../examples/revision_cache_snapshot.stdout")
    );
    assert_eq!(cache.evict_identity_revision("main.srp", 3), 1);
    assert_eq!(cache.evict_identity_revision("main.srp", 3), 0);
    assert_eq!(cache.stats().entries, 0);
    assert_eq!(cache.stats().retained_source_bytes, 0);
    assert_eq!(
        output(&first),
        include_bytes!("../examples/revision_cache_snapshot.stdout")
    );
}
