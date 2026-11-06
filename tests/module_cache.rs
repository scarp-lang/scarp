use scarp::{
    diagnostics::DiagnosticKind as Kind,
    modules::{
        cache::{CacheLimits, CacheStatus, CachedCompileError, CompilationCache},
        ConstantLimits, MemorySourceLoader, ModuleLimits, SourceLoadError, SourceLoader,
    },
    types::TypeLimits,
    ExecutionOptions,
};
use std::{cell::Cell, sync::Arc};

fn loader(files: &[(&str, &str)]) -> MemorySourceLoader {
    let mut loader = MemorySourceLoader::new();
    for &(name, text) in files {
        loader.insert(name, text).unwrap();
    }
    loader
}

fn compile(
    cache: &mut CompilationCache,
    loader: &dyn SourceLoader,
    revisions: &[u64],
) -> Result<scarp::modules::cache::CachedProgram, CachedCompileError> {
    cache.compile_with_loader(
        "main.srp",
        loader,
        ModuleLimits::default(),
        ConstantLimits::default(),
        TypeLimits::default(),
        revisions,
    )
}
fn execute(result: &scarp::modules::cache::CachedProgram) -> Vec<u8> {
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    result
        .program
        .execute(&mut stdout, &mut stderr, ExecutionOptions::default())
        .unwrap();
    assert!(stderr.is_empty());
    stdout
}

#[test]
fn unchanged_program_reuses_only_resolved_semantics_and_still_reloads_sources() {
    struct Counting<'a> {
        inner: &'a MemorySourceLoader,
        loads: Cell<u32>,
        resolves: Cell<u32>,
    }
    impl SourceLoader for Counting<'_> {
        fn resolve(
            &self,
            importer: Option<&str>,
            specifier: &str,
        ) -> Result<String, SourceLoadError> {
            self.resolves.set(self.resolves.get() + 1);
            self.inner.resolve(importer, specifier)
        }
        fn load(&self, identity: &str, max_bytes: usize) -> Result<String, SourceLoadError> {
            self.loads.set(self.loads.get() + 1);
            self.inner.load(identity, max_bytes)
        }
    }
    let memory = loader(&[
        ("main.srp", "import \"lib.srp\" as lib; print(lib.value);"),
        ("lib.srp", "pub bind value=17;"),
    ]);
    let counting = Counting {
        inner: &memory,
        loads: Cell::new(0),
        resolves: Cell::new(0),
    };
    let mut cache = CompilationCache::new(CacheLimits::default());
    let first = compile(&mut cache, &counting, &[4, 8]).unwrap();
    assert_eq!(first.status, CacheStatus::Stored);
    let second = compile(&mut cache, &counting, &[4, 8]).unwrap();
    assert_eq!(second.status, CacheStatus::Hit);
    assert!(Arc::ptr_eq(&first.program, &second.program));
    assert_eq!(counting.loads.get(), 4); // both source files loaded twice
    assert_eq!(counting.resolves.get(), 4); // entry and import resolved twice
    assert_eq!(execute(&second), b"17\n");
    let stats = cache.stats();
    assert_eq!((stats.entries, stats.hits, stats.misses), (1, 1, 1));
    assert_eq!((stats.evictions, stats.bypasses), (0, 0));
}

#[test]
fn source_edits_and_changed_revisions_never_reuse_a_previous_output() {
    let mut cache = CompilationCache::new(CacheLimits::default());
    let initial = loader(&[
        ("main.srp", "import \"lib.srp\" as lib; print(lib.value);"),
        ("lib.srp", "pub bind value=1;"),
    ]);
    let changed = loader(&[
        ("main.srp", "import \"lib.srp\" as lib; print(lib.value);"),
        ("lib.srp", "pub bind value=2;"),
    ]);
    let first = compile(&mut cache, &initial, &[3, 6]).unwrap();
    let next = compile(&mut cache, &changed, &[3, 7]).unwrap();
    assert_eq!(next.status, CacheStatus::Stored);
    assert!(!Arc::ptr_eq(&first.program, &next.program));
    assert_eq!(execute(&first), b"1\n");
    assert_eq!(execute(&next), b"2\n");

    // Exact source bytes still must match even if a host accidentally reuses
    // document version numbers. Revisions are an additional, not sole, gate.
    let wrong_revision = compile(&mut cache, &changed, &[3, 6]).unwrap();
    assert_eq!(wrong_revision.status, CacheStatus::Stored);
    assert!(!Arc::ptr_eq(&first.program, &wrong_revision.program));
    assert_eq!(execute(&wrong_revision), b"2\n");

    // Revision changes alone force a recompile; a caller cannot accidentally
    // leak a same-content old document snapshot via this cache.
    let same_source_new_revision = compile(&mut cache, &changed, &[3, 8]).unwrap();
    assert_eq!(same_source_new_revision.status, CacheStatus::Stored);
    assert!(!Arc::ptr_eq(
        &wrong_revision.program,
        &same_source_new_revision.program
    ));
}

#[test]
fn loader_failures_and_semantic_errors_never_return_stale_cached_success() {
    let mut cache = CompilationCache::new(CacheLimits::default());
    let good = loader(&[
        ("main.srp", "import \"lib.srp\" as lib; print(lib.value);"),
        ("lib.srp", "pub bind value=7;"),
    ]);
    let good_result = compile(&mut cache, &good, &[1, 1]).unwrap();
    assert_eq!(good_result.status, CacheStatus::Stored);

    let missing = loader(&[("main.srp", "import \"lib.srp\" as lib; print(lib.value);")]);
    assert!(matches!(
        compile(&mut cache, &missing, &[1, 2]),
        Err(CachedCompileError::Module(_))
    ));
    let private = loader(&[
        ("main.srp", "import \"lib.srp\" as lib; print(lib.value);"),
        ("lib.srp", "bind value=7;"),
    ]);
    let err = compile(&mut cache, &private, &[1, 2]).unwrap_err();
    match err {
        CachedCompileError::Module(e) => assert_eq!(e.diagnostic.kind, Kind::ModuleVisibility),
        other => panic!("unexpected error: {other}"),
    }
    let syntax = loader(&[
        ("main.srp", "import \"lib.srp\" as lib; print(lib.value);"),
        ("lib.srp", "pub bind value=;"),
    ]);
    assert!(matches!(
        compile(&mut cache, &syntax, &[1, 2]),
        Err(CachedCompileError::Module(_))
    ));
    assert_eq!(cache.stats().entries, 1);
    assert_eq!(cache.stats().hits, 0);
    // A later exact request is still permitted to reuse the earlier success,
    // but the failed requests themselves never return it.
    let again = compile(&mut cache, &good, &[1, 1]).unwrap();
    assert_eq!(again.status, CacheStatus::Hit);
    assert!(Arc::ptr_eq(&good_result.program, &again.program));
}

#[test]
fn compiler_limit_changes_force_semantic_resolution() {
    let source = loader(&[
        ("main.srp", "import \"lib.srp\" as lib; print(lib.f());"),
        ("lib.srp", "pub pure funcra f()->integer {return 8;}"),
    ]);
    let mut cache = CompilationCache::new(CacheLimits::default());
    let versions = [1, 2];
    let first = compile(&mut cache, &source, &versions).unwrap();
    assert_eq!(first.status, CacheStatus::Stored);

    let altered_constants = cache
        .compile_with_loader(
            "main.srp",
            &source,
            ModuleLimits::default(),
            ConstantLimits {
                max_steps: 1,
                ..ConstantLimits::default()
            },
            TypeLimits::default(),
            &versions,
        )
        .unwrap();
    assert_eq!(altered_constants.status, CacheStatus::Stored);
    assert!(!Arc::ptr_eq(&first.program, &altered_constants.program));

    let altered_types = cache
        .compile_with_loader(
            "main.srp",
            &source,
            ModuleLimits::default(),
            ConstantLimits::default(),
            TypeLimits {
                max_arity: 8,
                ..TypeLimits::default()
            },
            &versions,
        )
        .unwrap();
    assert_eq!(altered_types.status, CacheStatus::Stored);

    let altered_modules = cache
        .compile_with_loader(
            "main.srp",
            &source,
            ModuleLimits {
                max_imports: 16,
                ..ModuleLimits::default()
            },
            ConstantLimits::default(),
            TypeLimits::default(),
            &versions,
        )
        .unwrap();
    assert_eq!(altered_modules.status, CacheStatus::Stored);

    let return_to_original = compile(&mut cache, &source, &versions).unwrap();
    assert_eq!(return_to_original.status, CacheStatus::Hit);
    assert!(Arc::ptr_eq(&first.program, &return_to_original.program));
}

#[test]
fn exact_resolved_alias_retargeting_is_not_a_cache_hit() {
    struct Redirect<'a> {
        source: &'a MemorySourceLoader,
        second: &'a str,
    }
    impl SourceLoader for Redirect<'_> {
        fn resolve(
            &self,
            importer: Option<&str>,
            specifier: &str,
        ) -> Result<String, SourceLoadError> {
            if importer.is_some() {
                let target = match specifier {
                    "a.srp" => Some("one.srp"),
                    "b.srp" => Some(self.second),
                    "c.srp" => Some("two.srp"),
                    _ => None,
                };
                if let Some(target) = target {
                    return Ok(target.to_owned());
                }
            }
            self.source.resolve(importer, specifier)
        }
        fn load(&self, identity: &str, max_bytes: usize) -> Result<String, SourceLoadError> {
            self.source.load(identity, max_bytes)
        }
    }
    let memory = loader(&[
        (
            "main.srp",
            "import \"a.srp\" as a; import \"b.srp\" as b; import \"c.srp\" as c; print(a.value,b.value,c.value);",
        ),
        ("one.srp", "pub bind value=1;"),
        ("two.srp", "pub bind value=2;"),
    ]);
    let mut cache = CompilationCache::new(CacheLimits::default());
    let first = compile(
        &mut cache,
        &Redirect {
            source: &memory,
            second: "one.srp",
        },
        &[1, 2, 3],
    )
    .unwrap();
    let next = compile(
        &mut cache,
        &Redirect {
            source: &memory,
            second: "two.srp",
        },
        &[1, 2, 3],
    )
    .unwrap();

    // Same source bytes, same graph identities and same deduplicated import
    // target set, but the b alias is now bound to another declaration.
    assert_eq!(
        first.program.sources().source(0),
        next.program.sources().source(0)
    );
    assert_eq!(first.program.dependencies(0), next.program.dependencies(0));
    assert_eq!(first.status, CacheStatus::Stored);
    assert_eq!(next.status, CacheStatus::Stored);
    assert!(!Arc::ptr_eq(&first.program, &next.program));
    assert_eq!(execute(&first), b"1 1 2\n");
    assert_eq!(execute(&next), b"1 2 2\n");
}

#[test]
fn diagnostic_source_labels_are_part_of_the_cache_key() {
    struct Labels<'a> {
        source: &'a MemorySourceLoader,
        prefix: &'a str,
    }
    impl SourceLoader for Labels<'_> {
        fn resolve(
            &self,
            importer: Option<&str>,
            specifier: &str,
        ) -> Result<String, SourceLoadError> {
            self.source.resolve(importer, specifier)
        }
        fn load(&self, identity: &str, max_bytes: usize) -> Result<String, SourceLoadError> {
            self.source.load(identity, max_bytes)
        }
        fn display_name(&self, identity: &str) -> String {
            format!("{}/{identity}", self.prefix)
        }
    }
    let memory = loader(&[
        (
            "main.srp",
            "import \"lib/café.srp\" as lib; print(lib.value);",
        ),
        ("lib/café.srp", "pub bind value=3;"),
    ]);
    let mut cache = CompilationCache::new(CacheLimits::default());
    let first = compile(
        &mut cache,
        &Labels {
            source: &memory,
            prefix: "old",
        },
        &[1, 2],
    )
    .unwrap();
    let next = compile(
        &mut cache,
        &Labels {
            source: &memory,
            prefix: "new",
        },
        &[1, 2],
    )
    .unwrap();
    assert_eq!(first.status, CacheStatus::Stored);
    assert_eq!(next.status, CacheStatus::Stored);
    assert_eq!(first.program.sources().files()[1].name, "old/lib/café.srp");
    assert_eq!(next.program.sources().files()[1].name, "new/lib/café.srp");
}

#[test]
fn fifo_eviction_and_explicit_clear_have_deterministic_stats() {
    let one = loader(&[("main.srp", "print(1);")]);
    let two = loader(&[("main.srp", "print(2);")]);
    let mut cache = CompilationCache::new(CacheLimits {
        max_entries: 1,
        max_retained_source_bytes: 4096,
    });
    let first = compile(&mut cache, &one, &[1]).unwrap();
    assert_eq!(first.status, CacheStatus::Stored);
    assert_eq!(
        compile(&mut cache, &one, &[1]).unwrap().status,
        CacheStatus::Hit
    );
    assert_eq!(
        compile(&mut cache, &two, &[2]).unwrap().status,
        CacheStatus::Stored
    );
    assert_eq!(cache.stats().entries, 1);
    assert_eq!(cache.stats().evictions, 1);
    assert_eq!(
        compile(&mut cache, &one, &[1]).unwrap().status,
        CacheStatus::Stored
    );
    assert_eq!(cache.stats().evictions, 2);
    cache.clear();
    assert_eq!(cache.stats().entries, 0);
    assert_eq!(cache.stats().retained_source_bytes, 0);
    assert_eq!(cache.stats().evictions, 3);
    assert_eq!(
        compile(&mut cache, &one, &[1]).unwrap().status,
        CacheStatus::Stored
    );
    assert_eq!(cache.stats().entries, 1);
}

#[test]
fn quotas_and_revision_count_fail_closed_without_silencing_the_compiler() {
    let source = loader(&[("main.srp", "print(42);")]);
    for limits in [
        CacheLimits {
            max_entries: 0,
            max_retained_source_bytes: 4096,
        },
        CacheLimits {
            max_entries: 1,
            max_retained_source_bytes: 1,
        },
    ] {
        let mut cache = CompilationCache::new(limits);
        let first = compile(&mut cache, &source, &[1]).unwrap();
        let second = compile(&mut cache, &source, &[1]).unwrap();
        assert_eq!(first.status, CacheStatus::Uncached);
        assert_eq!(second.status, CacheStatus::Uncached);
        assert_eq!(execute(&first), b"42\n");
        assert_eq!(cache.stats().entries, 0);
        assert_eq!(cache.stats().bypasses, 2);
        assert_eq!(cache.stats().misses, 2);
        assert_eq!(cache.stats().retained_source_bytes, 0);
    }
    let mut cache = CompilationCache::new(CacheLimits::default());
    assert!(matches!(
        compile(&mut cache, &source, &[]),
        Err(CachedCompileError::RevisionCount {
            expected: 1,
            actual: 0,
        })
    ));
    assert_eq!(cache.stats().entries, 0);
    assert_eq!(cache.stats().misses, 0);
}

#[test]
fn cache_returns_shared_immutable_programs_across_reader_threads() {
    let memory = loader(&[("main.srp", "bind answer=42; print(answer);")]);
    let mut cache = CompilationCache::new(CacheLimits::default());
    let cached = compile(&mut cache, &memory, &[1]).unwrap();
    let repeated = compile(&mut cache, &memory, &[1]).unwrap();
    assert_eq!(repeated.status, CacheStatus::Hit);
    assert!(Arc::ptr_eq(&cached.program, &repeated.program));
    let handles: Vec<_> = (0..3)
        .map(|_| {
            let item = repeated.clone();
            std::thread::spawn(move || {
                assert_eq!(execute(&item), b"42\n");
                assert_eq!(item.program.sources().files().len(), 1);
            })
        })
        .collect();
    for handle in handles {
        handle.join().unwrap();
    }
}
