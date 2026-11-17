//! Compare cold and cached static analysis across a diamond dependency graph.
//! Edits to one shared import must not leak stale types, symbol JSON or output.

use scarp::{
    modules::{
        self,
        cache::{
            CacheStatus, CachedEditorProgram, EditorCacheLimits, EditorCompilationCache,
            EditorCompileError, EditorCompileLimits, SyntaxCache, SyntaxCacheLimits,
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
    let graph = modules::load_with_syntax_cache(
        "main.srp",
        sources,
        ModuleLimits::default(),
        syntax,
    )
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
    cold.index()
        .write_symbols_json(&mut cold_symbols)
        .unwrap();
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

    sources
        .set_document("shared.srp", "// नया 🌍\r\npub bind other = 10;", 3)
        .unwrap();
    let failure = cached(&sources, &mut syntax, &mut editor).unwrap_err();
    let EditorCompileError::Module(failure) = failure else {
        panic!("expected a missing imported declaration")
    };
    let expected = cold(&sources).unwrap_err();
    assert_eq!(failure.diagnostic, expected.diagnostic);
    assert_eq!(failure.render(), expected.render());
    assert_eq!(output(&changed.program), b"11 12\n");

    // Returning to the same content can reuse an immutable compiled program,
    // but the editor snapshot must reflect the newly supplied document revision.
    sources
        .set_document("shared.srp", "// नया 🌍\r\npub bind value = 10;", 4)
        .unwrap();
    let restored = cached(&sources, &mut syntax, &mut editor).unwrap();
    assert_eq!(restored.status, CacheStatus::Hit);
    assert_equivalent(&restored, &cold(&sources).unwrap(), b"11 12\n");
    assert_eq!(restored.snapshot.ensure_current(&[1, 1, 4, 1]), Ok(()));
}
