//! Compare cold and cached static analysis across a diamond dependency graph.
//! Edits to one shared import must not leak stale types, symbol JSON or output.

use scarp::{
    diagnostics::DiagnosticKind,
    modules::{
        self,
        cache::{
            CacheStatus, CachedEditorProgram, EditorAnalysisError, EditorCacheLimits,
            EditorCompilationCache, EditorCompileError, EditorCompileLimits, SyntaxCache,
            SyntaxCacheLimits,
        },
        symbols::{IndexLimits, IndexedProgram},
        MemorySourceLoader, ModuleLimits, ModuleProgram,
    },
    ExecutionOptions,
};

fn workspace() -> MemorySourceLoader {
    let mut files = MemorySourceLoader::new();
    for (path, source) in [
        (
            "main.srp",
            "import \"a.srp\" as a; import \"b.srp\" as b; print(a.value, b.value);",
        ),
        (
            "a.srp",
            "import \"shared.srp\" as s; pub bind value = s.value + 1;",
        ),
        (
            "b.srp",
            "import \"shared.srp\" as s; pub bind value = s.value + 2;",
        ),
        ("shared.srp", "// नमस्ते 🌍\r\npub bind value = 5;"),
    ] {
        files.set_document(path, source, 1).unwrap();
    }
    files
}

fn cached(
    sources: &MemorySourceLoader,
    syntax: &mut SyntaxCache,
    editor: &mut EditorCompilationCache,
) -> Result<CachedEditorProgram, EditorCompileError> {
    let graph =
        modules::load_with_syntax_cache("main.srp", sources, ModuleLimits::default(), syntax)
            .unwrap();
    editor.resolve_with_loaded_revisions(graph, EditorCompileLimits::default(), None)
}

fn cold(sources: &MemorySourceLoader) -> Result<IndexedProgram, modules::ModuleDiagnostic> {
    modules::resolve_indexed(
        modules::load("main.srp", sources, ModuleLimits::default()).unwrap(),
        IndexLimits::default(),
    )
}

fn output(program: &ModuleProgram) -> Vec<u8> {
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    program
        .execute(&mut stdout, &mut stderr, ExecutionOptions::default())
        .unwrap();
    assert!(stderr.is_empty());
    stdout
}

fn assert_equivalent(cached: &CachedEditorProgram, cold: &IndexedProgram, expected: &[u8]) {
    assert_eq!(output(&cached.program), expected);
    assert_eq!(output(cold.program()), expected);

    let mut warm_symbols = Vec::new();
    let mut cold_symbols = Vec::new();
    cached
        .snapshot
        .symbols()
        .write_symbols_json(&mut warm_symbols)
        .unwrap();
    cold.index().write_symbols_json(&mut cold_symbols).unwrap();
    assert_eq!(warm_symbols, cold_symbols);

    let mut warm_references = Vec::new();
    let mut cold_references = Vec::new();
    cached
        .snapshot
        .symbols()
        .write_references_json(&mut warm_references)
        .unwrap();
    cold.index()
        .write_references_json(&mut cold_references)
        .unwrap();
    assert_eq!(warm_references, cold_references);

    let mut warm_inventory = Vec::new();
    let mut cold_inventory = Vec::new();
    cached
        .program
        .write_inspection_json(&mut warm_inventory)
        .unwrap();
    cold.program()
        .write_inspection_json(&mut cold_inventory)
        .unwrap();
    assert_eq!(warm_inventory, cold_inventory);
}

fn assert_same_diagnostic(
    actual: &modules::ModuleDiagnostic,
    expected: &modules::ModuleDiagnostic,
) {
    assert_eq!(actual.diagnostic, expected.diagnostic);
    assert_eq!(actual.render(), expected.render());
    let mut warm_json = Vec::new();
    let mut cold_json = Vec::new();
    actual.write_json(&mut warm_json).unwrap();
    expected.write_json(&mut cold_json).unwrap();
    assert_eq!(warm_json, cold_json);
}

#[test]
fn shared_module_edits_have_cold_warm_symbol_and_runtime_parity() {
    let mut sources = workspace();
    let mut syntax = SyntaxCache::new(SyntaxCacheLimits::default());
    let mut editor = EditorCompilationCache::new(EditorCacheLimits::default());

    let original = cached(&sources, &mut syntax, &mut editor).unwrap();
    assert_eq!(original.status, CacheStatus::Stored);
    assert_equivalent(&original, &cold(&sources).unwrap(), b"6 7\n");

    sources
        .set_document("shared.srp", "// नया 🌍\r\npub bind value = 10;", 2)
        .unwrap();
    let changed = cached(&sources, &mut syntax, &mut editor).unwrap();
    assert_eq!(changed.status, CacheStatus::Stored);
    assert_equivalent(&changed, &cold(&sources).unwrap(), b"11 12\n");

    // The old returned program is immutable even though the cached workspace
    // has since changed and module dependents were recompiled.
    assert_eq!(output(&original.program), b"6 7\n");
    assert_eq!(changed.program.modules().len(), 4);

    for (revision, source) in [
        (3, "// नया 🌍\r\npub bind other = 10;"),
        (4, "// नया 🌍\r\npub bind value = \"ten\";"),
    ] {
        sources
            .set_document("shared.srp", source, revision)
            .unwrap();
        let failure = cached(&sources, &mut syntax, &mut editor).unwrap_err();
        let EditorCompileError::Module(failure) = failure else {
            panic!("expected a compiler diagnostic after changing the shared export")
        };
        assert_same_diagnostic(&failure, &cold(&sources).unwrap_err());
    }
    assert_eq!(output(&changed.program), b"11 12\n");

    // Returning to the same content can reuse an immutable compiled program,
    // but the editor snapshot must reflect the newly supplied document revision.
    sources
        .set_document("shared.srp", "// नया 🌍\r\npub bind value = 10;", 5)
        .unwrap();
    let restored = cached(&sources, &mut syntax, &mut editor).unwrap();
    assert_eq!(restored.status, CacheStatus::Hit);
    assert_equivalent(&restored, &cold(&sources).unwrap(), b"11 12\n");
    assert_eq!(restored.snapshot.ensure_current(&[1, 1, 5, 1]), Ok(()));
}

#[test]
fn diamond_load_failures_and_reopening_preserve_cold_warm_parity() {
    let mut sources = workspace();
    let mut syntax = SyntaxCache::new(SyntaxCacheLimits::default());
    let mut editor = EditorCompilationCache::new(EditorCacheLimits::default());
    let original = cached(&sources, &mut syntax, &mut editor).unwrap();

    let limits = ModuleLimits::default();
    let compile_limits = EditorCompileLimits::default();
    let mut stale_analyses = Vec::new();
    for (generation, source, expected_kind) in [
        (2, None, DiagnosticKind::ModuleLoad),
        (
            3,
            Some("// बदला 🌍\r\npub bind value = @;"),
            DiagnosticKind::InvalidCharacter,
        ),
    ] {
        match source {
            Some(source) => sources
                .set_document("shared.srp", source, generation)
                .unwrap(),
            None => assert!(sources.remove_document("shared.srp").unwrap()),
        }
        let expected = modules::load("main.srp", &sources, limits).unwrap_err();
        let warm = modules::load_with_syntax_cache("main.srp", &sources, limits, &mut syntax);
        let analysis = editor.analyze_loaded(generation, warm, compile_limits, None);
        let EditorAnalysisError::Compile(EditorCompileError::Module(actual)) =
            analysis.into_current(generation).unwrap_err()
        else {
            panic!("expected a current load diagnostic");
        };
        assert_eq!(actual.diagnostic.kind, expected_kind);
        assert_same_diagnostic(&actual, &expected);

        let warm = modules::load_with_syntax_cache("main.srp", &sources, limits, &mut syntax);
        stale_analyses.push((
            generation,
            editor.analyze_loaded(generation, warm, compile_limits, None),
        ));
    }

    sources
        .set_document("shared.srp", "// नमस्ते 🌍\r\npub bind value = 5;", 4)
        .unwrap();
    for (generation, analysis) in stale_analyses {
        assert!(matches!(
            analysis.into_current(4),
            Err(EditorAnalysisError::Stale { analyzed, current: 4 }) if analyzed == generation
        ));
    }

    let too_few_files = ModuleLimits {
        max_files: 3,
        ..limits
    };
    let cold_error = modules::load("main.srp", &sources, too_few_files).unwrap_err();
    let warm_error =
        modules::load_with_syntax_cache("main.srp", &sources, too_few_files, &mut syntax)
            .unwrap_err();
    assert_eq!(warm_error.diagnostic.kind, DiagnosticKind::ModuleLimit);
    assert_same_diagnostic(&warm_error, &cold_error);

    let restored = cached(&sources, &mut syntax, &mut editor).unwrap();
    assert_eq!(restored.status, CacheStatus::Hit);
    assert_equivalent(&restored, &cold(&sources).unwrap(), b"6 7\n");
    assert_eq!(restored.snapshot.ensure_current(&[1, 1, 4, 1]), Ok(()));
    assert_eq!(original.snapshot.ensure_current(&[1, 1, 1, 1]), Ok(()));
}
