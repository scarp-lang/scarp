use scarp::{
    diagnostics::DiagnosticKind as Kind,
    modules::{
        self,
        cache::{CacheLimits, CacheStatus, CompilationCache, TokenCache, TokenCacheLimits},
        symbols::IndexLimits,
        ConstantLimits, MemorySourceLoader, ModuleLimits, SourceLoadError, SourceLoader,
    },
    types::TypeLimits,
    ExecutionOptions,
};

fn loader(files: &[(&str, &str)]) -> MemorySourceLoader {
    let mut loader = MemorySourceLoader::new();
    for &(name, source) in files {
        loader.insert(name, source).unwrap();
    }
    loader
}

fn with_token_cache(
    loader: &dyn SourceLoader,
    tokens: &mut TokenCache,
) -> Result<modules::ModuleProgram, modules::ModuleDiagnostic> {
    modules::resolve(modules::load_with_token_cache(
        "main.srp",
        loader,
        ModuleLimits::default(),
        tokens,
    )?)
}

fn run(program: &modules::ModuleProgram) -> Vec<u8> {
    let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
    program
        .execute(&mut stdout, &mut stderr, ExecutionOptions::default())
        .unwrap();
    assert!(stderr.is_empty());
    stdout
}

#[test]
fn repeated_compilation_reuses_module_tokens_not_loader_or_parser_validation() {
    use std::cell::Cell;

    struct Counting<'a> {
        source: &'a MemorySourceLoader,
        loads: Cell<usize>,
        resolves: Cell<usize>,
    }
    impl SourceLoader for Counting<'_> {
        fn resolve(
            &self,
            importer: Option<&str>,
            specifier: &str,
        ) -> Result<String, SourceLoadError> {
            self.resolves.set(self.resolves.get() + 1);
            self.source.resolve(importer, specifier)
        }
        fn load(&self, identity: &str, bytes: usize) -> Result<String, SourceLoadError> {
            self.loads.set(self.loads.get() + 1);
            self.source.load(identity, bytes)
        }
    }

    let source = loader(&[
        ("main.srp", "import \"lib.srp\" as lib; print(lib.f());"),
        ("lib.srp", "pub pure funcra f()->integer { return 42; }"),
    ]);
    let observed = Counting {
        source: &source,
        loads: Cell::new(0),
        resolves: Cell::new(0),
    };
    let mut tokens = TokenCache::new(TokenCacheLimits::default());
    let first = with_token_cache(&observed, &mut tokens).unwrap();
    let next = with_token_cache(&observed, &mut tokens).unwrap();
    assert_eq!(run(&first), b"42\n");
    assert_eq!(run(&next), b"42\n");
    assert_eq!(observed.loads.get(), 4);
    assert_eq!(observed.resolves.get(), 4);
    let stat = tokens.stats();
    assert_eq!((stat.entries, stat.misses, stat.hits), (2, 2, 2));
    assert_eq!(stat.bypasses, 0);
    assert!(stat.retained_tokens > 2);
}

#[test]
fn changing_earlier_utf8_source_length_rebases_unchanged_imported_tokens_exactly() {
    let a = loader(&[
        (
            "main.srp",
            "import \"lib/café.srp\" as lib; print(lib.twice(2));",
        ),
        (
            "lib/café.srp",
            "pub pure funcra twice(n: integer)->integer {return n*2;}",
        ),
    ]);
    let b = loader(&[
        (
            "main.srp",
            "// नमस्ते 🌍 with longer CRLF\r\nimport \"lib/café.srp\" as lib; print(lib.twice(2));",
        ),
        (
            "lib/café.srp",
            "pub pure funcra twice(n: integer)->integer {return n*2;}",
        ),
    ]);

    let mut tokens = TokenCache::new(TokenCacheLimits::default());
    let original = with_token_cache(&a, &mut tokens).unwrap();
    let warm = with_token_cache(&b, &mut tokens).unwrap();
    let cold = modules::compile_with_loader("main.srp", &b, ModuleLimits::default()).unwrap();
    let stat = tokens.stats();
    assert_eq!(stat.hits, 1); // the unchanged imported source
    assert_eq!(stat.misses, 3); // first two files, then new entry content

    assert_eq!(
        warm.sources().files()[1].range(),
        cold.sources().files()[1].range()
    );
    assert_ne!(
        original.sources().files()[1].range(),
        cold.sources().files()[1].range()
    );
    assert_eq!(warm.imports(), cold.imports());
    assert_eq!(warm.constant_dependencies(), cold.constant_dependencies());
    assert_eq!(run(&warm), run(&cold));

    // Navigation uses absolute graph spans built from the newest source bases.
    let cached_index = modules::resolve_indexed(
        modules::load_with_token_cache("main.srp", &b, ModuleLimits::default(), &mut tokens)
            .unwrap(),
        IndexLimits::default(),
    )
    .unwrap();
    let cold_index = modules::compile_indexed_with_loader(
        "main.srp",
        &b,
        ModuleLimits::default(),
        IndexLimits::default(),
    )
    .unwrap();
    let (mut warm_json, mut cold_json) = (Vec::new(), Vec::new());
    cached_index
        .index()
        .write_references_json(&mut warm_json)
        .unwrap();
    cold_index
        .index()
        .write_references_json(&mut cold_json)
        .unwrap();
    assert_eq!(warm_json, cold_json);
}

#[test]
fn source_reorder_changes_discovery_ids_but_not_token_offsets() {
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
    let mut tokens = TokenCache::new(TokenCacheLimits::default());
    let _ = with_token_cache(&before, &mut tokens).unwrap();
    let warm = with_token_cache(&after, &mut tokens).unwrap();
    let cold = modules::compile_with_loader("main.srp", &after, ModuleLimits::default()).unwrap();
    assert_eq!(tokens.stats().hits, 2);
    assert_eq!(tokens.stats().misses, 4);
    assert_eq!(
        warm.sources()
            .files()
            .iter()
            .map(|file| file.name.as_str())
            .collect::<Vec<_>>(),
        ["main.srp", "b.srp", "a.srp"]
    );
    assert_eq!(warm.imports(), cold.imports());
    assert_eq!(run(&warm), b"1 2\n");
}

#[test]
fn cached_tokens_never_hide_parser_errors_or_lex_failures() {
    let invalid_parser = loader(&[
        ("main.srp", "import \"lib.srp\" as x; print(x.value);"),
        ("lib.srp", "pub bind value = ;"),
    ]);
    let mut tokens = TokenCache::new(TokenCacheLimits::default());
    let cold = modules::compile_with_loader("main.srp", &invalid_parser, ModuleLimits::default())
        .unwrap_err();
    for _ in 0..2 {
        let err = with_token_cache(&invalid_parser, &mut tokens).unwrap_err();
        assert_eq!(err.diagnostic, cold.diagnostic);
        assert_eq!(err.render(), cold.render());
    }
    assert_eq!(tokens.stats().hits, 2); // both files are re-parsed from cached tokens
    assert_eq!(tokens.stats().entries, 2); // parse failures can cache valid lexing

    let invalid_lexer = loader(&[
        ("main.srp", "import \"lib.srp\" as x; print(x.value);"),
        ("lib.srp", "pub bind value = \"unfinished"),
    ]);
    for _ in 0..2 {
        let err = with_token_cache(&invalid_lexer, &mut tokens).unwrap_err();
        assert_eq!(err.diagnostic.kind, Kind::UnterminatedString);
    }
    // Invalid lexical results are never stored or reused.
    assert_eq!(tokens.stats().entries, 2);
}

#[test]
fn changed_import_graph_and_module_limits_still_run_all_checks() {
    let good = loader(&[
        ("main.srp", "import \"lib.srp\" as l; print(l.value);"),
        ("lib.srp", "pub bind value=7;"),
    ]);
    let mut tokens = TokenCache::new(TokenCacheLimits::default());
    let graph =
        modules::load_with_token_cache("main.srp", &good, ModuleLimits::default(), &mut tokens)
            .unwrap();
    assert_eq!(graph.file_count(), 2);
    let result = modules::resolve(graph).unwrap();
    assert_eq!(run(&result), b"7\n");

    let forbidden = loader(&[
        ("main.srp", "import \"lib.srp\" as l; print(l.value);"),
        ("lib.srp", "bind value=7;"),
    ]);
    let error = with_token_cache(&forbidden, &mut tokens).unwrap_err();
    assert_eq!(error.diagnostic.kind, Kind::ModuleVisibility);

    let tight = ModuleLimits {
        max_imports: 0,
        ..ModuleLimits::default()
    };
    let err = modules::load_with_token_cache("main.srp", &good, tight, &mut tokens).unwrap_err();
    assert_eq!(err.diagnostic.kind, Kind::ModuleLimit);
    assert!(tokens.stats().hits >= 1);
}

#[test]
fn token_cache_budgets_fifo_and_clear_are_independent_of_semantic_cache() {
    let first = loader(&[("main.srp", "print(1);")]);
    let second = loader(&[("main.srp", "print(2);")]);
    let mut tokens = TokenCache::new(TokenCacheLimits {
        max_entries: 1,
        max_source_bytes: 1024,
        max_identity_bytes: 1024,
        max_tokens: 256,
    });
    with_token_cache(&first, &mut tokens).unwrap();
    with_token_cache(&first, &mut tokens).unwrap();
    assert_eq!(tokens.stats().hits, 1);
    with_token_cache(&second, &mut tokens).unwrap();
    assert_eq!((tokens.stats().entries, tokens.stats().evictions), (1, 1));
    with_token_cache(&first, &mut tokens).unwrap();
    assert_eq!((tokens.stats().misses, tokens.stats().evictions), (3, 2));
    tokens.clear();
    assert_eq!(tokens.stats().entries, 0);
    assert_eq!(tokens.stats().retained_source_bytes, 0);
    assert_eq!(tokens.stats().retained_identity_bytes, 0);
    assert_eq!(tokens.stats().retained_tokens, 0);
    assert_eq!(tokens.stats().evictions, 3);

    for limits in [
        TokenCacheLimits {
            max_entries: 0,
            ..TokenCacheLimits::default()
        },
        TokenCacheLimits {
            max_source_bytes: 1,
            ..TokenCacheLimits::default()
        },
        TokenCacheLimits {
            max_tokens: 1,
            ..TokenCacheLimits::default()
        },
        TokenCacheLimits {
            max_identity_bytes: 1,
            ..TokenCacheLimits::default()
        },
    ] {
        let mut bounded = TokenCache::new(limits);
        with_token_cache(&first, &mut bounded).unwrap();
        with_token_cache(&first, &mut bounded).unwrap();
        assert_eq!(bounded.stats().entries, 0);
        assert_eq!(bounded.stats().hits, 0);
        assert_eq!(bounded.stats().bypasses, 2);
    }
}

#[test]
fn token_cache_works_with_opt_in_whole_graph_semantic_reuse() {
    let source = loader(&[
        ("main.srp", "import \"lib.srp\" as lib; print(lib.f());"),
        ("lib.srp", "pub pure funcra f()->integer {return 9;}"),
    ]);
    let mut tokens = TokenCache::new(TokenCacheLimits::default());
    let mut semantics = CompilationCache::new(CacheLimits::default());
    let first = semantics
        .compile_with_token_cache(
            "main.srp",
            &source,
            ModuleLimits::default(),
            ConstantLimits::default(),
            TypeLimits::default(),
            &[1, 1],
            &mut tokens,
        )
        .unwrap();
    let next = semantics
        .compile_with_token_cache(
            "main.srp",
            &source,
            ModuleLimits::default(),
            ConstantLimits::default(),
            TypeLimits::default(),
            &[1, 1],
            &mut tokens,
        )
        .unwrap();
    assert_eq!(first.status, CacheStatus::Stored);
    assert_eq!(next.status, CacheStatus::Hit);
    assert_eq!(tokens.stats().hits, 2);
    assert_eq!(semantics.stats().hits, 1);
    assert_eq!(run(&next.program), b"9\n");
}
