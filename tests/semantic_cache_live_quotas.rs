use scarp::{
    modules::{
        cache::{CacheLimits, CacheStatus, CompilationCache},
        ConstantLimits, MemorySourceLoader, ModuleLimits,
    },
    types::TypeLimits,
    ExecutionOptions,
};
use std::sync::Arc;

fn source(text: &str) -> MemorySourceLoader {
    let mut loader = MemorySourceLoader::new();
    loader.insert("main.srp", text).unwrap();
    loader
}

fn compile(
    cache: &mut CompilationCache,
    loader: &MemorySourceLoader,
    revision: u64,
) -> scarp::modules::cache::CachedProgram {
    cache
        .compile_with_loader(
            "main.srp",
            loader,
            ModuleLimits::default(),
            ConstantLimits::default(),
            TypeLimits::default(),
            &[revision],
        )
        .unwrap()
}

fn output(program: &scarp::modules::cache::CachedProgram) -> Vec<u8> {
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    program
        .program
        .execute(&mut stdout, &mut stderr, ExecutionOptions::default())
        .unwrap();
    assert!(stderr.is_empty());
    stdout
}

#[test]
fn shrinking_entry_quota_evicts_fifo_but_preserves_published_snapshots() {
    let mut cache = CompilationCache::new(CacheLimits {
        max_entries: 3,
        max_retained_source_bytes: 4096,
    });
    let first = compile(&mut cache, &source("print(1);"), 1);
    let second = compile(&mut cache, &source("print(2);"), 2);
    let third = compile(&mut cache, &source("print(3);"), 3);
    assert_eq!(cache.stats().entries, 3);
    let before = cache.stats();
    let limits = CacheLimits {
        max_entries: 2,
        max_retained_source_bytes: 4096,
    };
    cache.set_limits(limits);
    assert_eq!(cache.limits(), limits);
    assert_eq!(cache.stats().entries, 2);
    assert_eq!(cache.stats().evictions, before.evictions + 1);
    assert_eq!(compile(&mut cache, &source("print(3);"), 3).status, CacheStatus::Hit);
    assert_eq!(compile(&mut cache, &source("print(2);"), 2).status, CacheStatus::Hit);
    assert_eq!(output(&first), b"1\n");
    assert_eq!(output(&second), b"2\n");
    assert_eq!(output(&third), b"3\n");
}

#[test]
fn shrinking_source_bytes_evicts_oldest_until_all_quotas_hold() {
    let mut cache = CompilationCache::new(CacheLimits {
        max_entries: 4,
        max_retained_source_bytes: 4096,
    });
    let a = source("print(11);");
    let b = source("print(22);");
    let old = compile(&mut cache, &a, 1);
    let newer = compile(&mut cache, &b, 2);
    let before = cache.stats();
    assert!(before.retained_source_bytes > 0);
    cache.set_limits(CacheLimits {
        max_entries: 4,
        max_retained_source_bytes: 0,
    });
    assert_eq!(cache.stats().entries, 0);
    assert_eq!(cache.stats().retained_source_bytes, 0);
    assert_eq!(cache.stats().evictions, before.evictions + 2);
    assert_eq!(output(&old), b"11\n");
    assert_eq!(output(&newer), b"22\n");
    assert_eq!(compile(&mut cache, &a, 1).status, CacheStatus::Uncached);
}

#[test]
fn zero_entry_limit_and_restoration_do_not_reset_counters() {
    let mut cache = CompilationCache::new(CacheLimits::default());
    let input = source("print(42);");
    assert_eq!(compile(&mut cache, &input, 1).status, CacheStatus::Stored);
    assert_eq!(compile(&mut cache, &input, 1).status, CacheStatus::Hit);
    let before = cache.stats();
    cache.set_limits(CacheLimits {
        max_entries: 0,
        max_retained_source_bytes: 4096,
    });
    assert_eq!(cache.stats().entries, 0);
    assert_eq!(cache.stats().evictions, before.evictions + 1);
    assert_eq!(compile(&mut cache, &input, 1).status, CacheStatus::Uncached);
    cache.set_limits(CacheLimits::default());
    let restored = compile(&mut cache, &input, 1);
    assert_eq!(restored.status, CacheStatus::Stored);
    let reused = compile(&mut cache, &input, 1);
    assert_eq!(reused.status, CacheStatus::Hit);
    assert!(Arc::ptr_eq(&restored.program, &reused.program));
    assert_eq!(output(&reused), b"42\n");
    assert_eq!(cache.stats().hits, before.hits + 1);
}

#[test]
fn quota_reconfiguration_never_reuses_a_changed_source() {
    let mut cache = CompilationCache::new(CacheLimits::default());
    let first = compile(&mut cache, &source("print(7);"), 1);
    cache.set_limits(CacheLimits {
        max_entries: 1,
        max_retained_source_bytes: 4096,
    });
    let edited = compile(&mut cache, &source("print(8);"), 1);
    assert_eq!(edited.status, CacheStatus::Stored);
    assert!(!Arc::ptr_eq(&first.program, &edited.program));
    assert_eq!(output(&first), b"7\n");
    assert_eq!(output(&edited), b"8\n");
    assert_eq!(cache.stats().entries, 1);
}
