use scarp::{
    diagnostics::DiagnosticKind as Kind,
    modules::{
        self,
        cache::{
            CacheLimits, CacheStatus, CachedCompileOptions, CompilationCache, SyntaxCache,
            SyntaxCacheLimits, TokenCache, TokenCacheLimits,
        },
        symbols::IndexLimits,
        MemorySourceLoader, ModuleLimits, SourceLoadError, SourceLoader,
    },
    ExecutionOptions,
};
use std::{cell::Cell, sync::Arc};

fn loader(files: &[(&str, &str)]) -> MemorySourceLoader {
    let mut loader = MemorySourceLoader::new();
    for &(name, source) in files {
        loader.insert(name, source).unwrap();
    }
    loader
}
fn warm(
    loader: &dyn SourceLoader,
    syntax: &mut SyntaxCache,
) -> Result<modules::ModuleProgram, modules::ModuleDiagnostic> {
    modules::resolve(modules::load_with_syntax_cache(
        "main.srp",
        loader,
        ModuleLimits::default(),
        syntax,
    )?)
}
fn run(program: &modules::ModuleProgram) -> Vec<u8> {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    program
        .execute(&mut out, &mut err, ExecutionOptions::default())
        .unwrap();
    assert!(err.is_empty());
    out
}
fn index_json(
    loader: &dyn SourceLoader,
    cache: Option<&mut SyntaxCache>,
) -> Vec<u8> {
    let indexed = match cache {
        Some(cache) => modules::resolve_indexed(
            modules::load_with_syntax_cache(
                "main.srp",
                loader,
                ModuleLimits::default(),
                cache,
            )
            .unwrap(),
            IndexLimits::default(),
        )
        .unwrap(),
        None => modules::compile_indexed_with_loader(
            "main.srp",
            loader,
            ModuleLimits::default(),
            IndexLimits::default(),
        )
        .unwrap(),
    };
    let mut data = Vec::new();
    indexed.index().write_references_json(&mut data).unwrap();
    data
}

#[test]
fn exact_warm_syntax_hits_still_load_sources_and_validate_semantics() {
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
        fn load(&self, identity: &str, limit: usize) -> Result<String, SourceLoadError> {
            self.loads.set(self.loads.get() + 1);
            self.inner.load(identity, limit)
        }
    }
    let source = loader(&[
        ("main.srp", "import \"lib.srp\" as lib; print(lib.f());"),
        ("lib.srp", "pub pure funcra f()->integer{return 42;}"),
    ]);
    let observed = Counting {
        inner: &source,
        loads: Cell::new(0),
        resolves: Cell::new(0),
    };
    let mut syntax = SyntaxCache::new(SyntaxCacheLimits::default());
    let first = warm(&observed, &mut syntax).unwrap();
    let second = warm(&observed, &mut syntax).unwrap();
    assert_eq!(run(&first), b"42\n");
    assert_eq!(run(&second), b"42\n");
    assert_eq!(observed.loads.get(), 4);
    assert_eq!(observed.resolves.get(), 4);
    let stats = syntax.stats();
    assert_eq!((stats.entries, stats.hits, stats.misses), (2, 2, 2));
    assert_eq!((stats.evictions, stats.bypasses), (0, 0));
    assert!(stats.retained_ast_nodes > 2);
}

#[test]
fn unicode_crlf_prefix_edits_rebase_all_nested_expression_and_navigation_spans() {
    let library = "pub pure funcra add(a: integer, b: integer=a)->integer needs a>=0; ensures result>=a; {return a+b;}";
    let first = loader(&[
        (
            "main.srp",
            "import \"lib.srp\" as lib; print(lib.add(2, b: 3));",
        ),
        ("lib.srp", library),
    ]);
    let second = loader(&[
        (
            "main.srp",
            "// 🌍 नमस्ते\r\n// second line\r\nimport \"lib.srp\" as lib; print(lib.add(2, b: 3));",
        ),
        ("lib.srp", library),
    ]);
    let mut syntax = SyntaxCache::new(SyntaxCacheLimits::default());
    let old = warm(&first, &mut syntax).unwrap();
    let new = warm(&second, &mut syntax).unwrap();
    let cold = modules::compile_with_loader("main.srp", &second, ModuleLimits::default()).unwrap();
    assert_eq!(syntax.stats().hits, 1);
    assert_eq!(syntax.stats().misses, 3);
    assert_ne!(old.sources().files()[1].range(), new.sources().files()[1].range());
    assert_eq!(new.sources().files()[1].range(), cold.sources().files()[1].range());
    assert_eq!(new.imports(), cold.imports());
    assert_eq!(new.constant_dependencies(), cold.constant_dependencies());
    assert_eq!(run(&new), run(&cold));
    assert_eq!(index_json(&second, Some(&mut syntax)), index_json(&second, None));
    assert_eq!(syntax.stats().hits, 3); // current entry and unchanged lib
}

#[test]
fn source_discovery_reordering_rebases_symbols_by_new_source_id() {
    let before = loader(&[
        (
            "main.srp",
            "import \"a.srp\" as a; import \"b.srp\" as b; print(a.value,b.value);",
        ),
        ("a.srp", "pub bind value=1;"),
        ("b.srp", "pub bind value=2;"),
    ]);
    let after = loader(&[
        (
            "main.srp",
            "import \"b.srp\" as b; import \"a.srp\" as a; print(a.value,b.value);",
        ),
        ("a.srp", "pub bind value=1;"),
        ("b.srp", "pub bind value=2;"),
    ]);
    let mut syntax = SyntaxCache::new(SyntaxCacheLimits::default());
    let _ = warm(&before, &mut syntax).unwrap();
    let new = warm(&after, &mut syntax).unwrap();
    assert_eq!(run(&new), b"1 2\n");
    assert_eq!(syntax.stats().hits, 2);
    assert_eq!(syntax.stats().misses, 4);
    assert_eq!(index_json(&after, Some(&mut syntax)), index_json(&after, None));
}

#[test]
fn old_success_cannot_hide_syntax_loader_visibility_or_import_budget_failures() {
    let valid = loader(&[
        ("main.srp", "import \"lib.srp\" as lib; print(lib.value);"),
        ("lib.srp", "pub bind value=7;"),
    ]);
    let private = loader(&[
        ("main.srp", "import \"lib.srp\" as lib; print(lib.value);"),
        ("lib.srp", "bind value=7;"),
    ]);
    let invalid = loader(&[
        ("main.srp", "import \"lib.srp\" as lib; print(lib.value);"),
        ("lib.srp", "pub bind value = ;"),
    ]);
    let lex_invalid = loader(&[
        ("main.srp", "import \"lib.srp\" as lib; print(lib.value);"),
        ("lib.srp", "pub bind value = \"unterminated"),
    ]);
    let mut syntax = SyntaxCache::new(SyntaxCacheLimits::default());
    assert_eq!(run(&warm(&valid, &mut syntax).unwrap()), b"7\n");
    assert_eq!(warm(&private, &mut syntax).unwrap_err().diagnostic.kind, Kind::ModuleVisibility);
    let cold_error = modules::compile_with_loader(
        "main.srp",
        &invalid,
        ModuleLimits::default(),
    )
    .unwrap_err();
    for _ in 0..2 {
        let err = warm(&invalid, &mut syntax).unwrap_err();
        assert_eq!(err.diagnostic, cold_error.diagnostic);
        assert_eq!(err.render(), cold_error.render());
    }
    for _ in 0..2 {
        assert_eq!(
            warm(&lex_invalid, &mut syntax).unwrap_err().diagnostic.kind,
            Kind::UnterminatedString
        );
    }
    // No failed syntax or lexical results enter the cache.
    assert_eq!(syntax.stats().entries, 3); // success main, success lib, private lib
    let no_imports = ModuleLimits {
        max_imports: 0,
        ..ModuleLimits::default()
    };
    let err = modules::load_with_syntax_cache(
        "main.srp",
        &valid,
        no_imports,
        &mut syntax,
    )
    .unwrap_err();
    assert_eq!(err.diagnostic.kind, Kind::ModuleLimit);
    let missing = loader(&[("main.srp", "import \"lib.srp\" as lib; print(lib.value);")]);
    assert_eq!(
        warm(&missing, &mut syntax).unwrap_err().diagnostic.kind,
        Kind::ModuleLoad
    );
}

#[test]
fn loader_alias_retargeting_still_relinks_on_the_same_cached_syntax() {
    struct Redirect<'a> {
        memory: &'a MemorySourceLoader,
        b: &'a str,
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
                    "b.srp" => Some(self.b),
                    "c.srp" => Some("two.srp"),
                    _ => None,
                };
                if let Some(target) = target {
                    return Ok(target.to_owned());
                }
            }
            self.memory.resolve(importer, specifier)
        }
        fn load(&self, identity: &str, max_bytes: usize) -> Result<String, SourceLoadError> {
            self.memory.load(identity, max_bytes)
        }
    }
    let files = loader(&[
        (
            "main.srp",
            "import \"a.srp\" as a; import \"b.srp\" as b; import \"c.srp\" as c; print(a.value,b.value,c.value);",
        ),
        ("one.srp", "pub bind value=1;"),
        ("two.srp", "pub bind value=2;"),
    ]);
    let mut syntax = SyntaxCache::new(SyntaxCacheLimits::default());
    let before = warm(&Redirect { memory: &files, b: "one.srp" }, &mut syntax).unwrap();
    let after = warm(&Redirect { memory: &files, b: "two.srp" }, &mut syntax).unwrap();
    assert_eq!(run(&before), b"1 1 2\n");
    assert_eq!(run(&after), b"1 2 2\n");
    assert_eq!(syntax.stats().hits, 3);
}

#[test]
fn deterministic_fifo_and_all_resource_quota_bypasses() {
    let first = loader(&[("main.srp", "print(1);")]);
    let second = loader(&[("main.srp", "print(2);")]);
    let mut syntax = SyntaxCache::new(SyntaxCacheLimits {
        max_entries: 1,
        max_source_bytes: 1024,
        max_identity_bytes: 1024,
        max_ast_nodes: 100,
    });
    warm(&first, &mut syntax).unwrap();
    warm(&first, &mut syntax).unwrap();
    assert_eq!(syntax.stats().hits, 1);
    warm(&second, &mut syntax).unwrap();
    assert_eq!((syntax.stats().entries, syntax.stats().evictions), (1, 1));
    warm(&first, &mut syntax).unwrap();
    assert_eq!((syntax.stats().misses, syntax.stats().evictions), (3, 2));
    syntax.clear();
    assert_eq!(syntax.stats().entries, 0);
    assert_eq!(syntax.stats().retained_ast_nodes, 0);
    assert_eq!(syntax.stats().retained_identity_bytes, 0);
    assert_eq!(syntax.stats().retained_source_bytes, 0);
    assert_eq!(syntax.stats().evictions, 3);

    for limits in [
        SyntaxCacheLimits {
            max_entries: 0,
            ..SyntaxCacheLimits::default()
        },
        SyntaxCacheLimits {
            max_source_bytes: 1,
            ..SyntaxCacheLimits::default()
        },
        SyntaxCacheLimits {
            max_identity_bytes: 1,
            ..SyntaxCacheLimits::default()
        },
        SyntaxCacheLimits {
            max_ast_nodes: 0,
            ..SyntaxCacheLimits::default()
        },
    ] {
        let mut disabled = SyntaxCache::new(limits);
        assert_eq!(run(&warm(&first, &mut disabled).unwrap()), b"1\n");
        assert_eq!(run(&warm(&first, &mut disabled).unwrap()), b"1\n");
        assert_eq!(disabled.stats().entries, 0);
        assert_eq!(disabled.stats().bypasses, 2);
        assert_eq!(disabled.stats().hits, 0);
    }
}

#[test]
fn token_syntax_and_semantics_caches_can_be_combined_without_stale_results() {
    let first = loader(&[
        ("main.srp", "import \"lib.srp\" as lib; print(lib.value);"),
        ("lib.srp", "pub bind value=7;"),
    ]);
    let edited = loader(&[
        ("main.srp", "// 👋\r\nimport \"lib.srp\" as lib; print(lib.value);"),
        ("lib.srp", "pub bind value=7;"),
    ]);
    let mut tokens = TokenCache::new(TokenCacheLimits::default());
    let mut syntax = SyntaxCache::new(SyntaxCacheLimits::default());
    let mut semantics = CompilationCache::new(CacheLimits::default());
    let one = semantics
        .compile_with_frontend_caches(
            "main.srp",
            &first,
            CachedCompileOptions::new(&[1, 1]),
            &mut tokens,
            &mut syntax,
        )
        .unwrap();
    let again = semantics
        .compile_with_frontend_caches(
            "main.srp",
            &first,
            CachedCompileOptions::new(&[1, 1]),
            &mut tokens,
            &mut syntax,
        )
        .unwrap();
    let changed = semantics
        .compile_with_frontend_caches(
            "main.srp",
            &edited,
            CachedCompileOptions::new(&[2, 1]),
            &mut tokens,
            &mut syntax,
        )
        .unwrap();
    assert_eq!(one.status, CacheStatus::Stored);
    assert_eq!(again.status, CacheStatus::Hit);
    assert_eq!(changed.status, CacheStatus::Stored);
    assert!(Arc::ptr_eq(&one.program, &again.program));
    assert!(!Arc::ptr_eq(&one.program, &changed.program));
    assert_eq!(run(&changed.program), b"7\n");
    assert_eq!(syntax.stats().hits, 3);
    assert_eq!(tokens.stats().hits, 0); // AST hits bypass token lookup entirely
    assert_eq!(tokens.stats().misses, 3); // first two modules + changed entry
    assert_eq!(semantics.stats().hits, 1);
}
