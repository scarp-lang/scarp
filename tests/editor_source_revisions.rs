use scarp::{
    diagnostics::DiagnosticKind as Kind,
    modules::{
        self,
        cache::{
            CacheStatus, EditorCacheLimits, EditorCompilationCache, EditorCompileError,
            EditorCompileLimits, SyntaxCache, SyntaxCacheLimits,
        },
        symbols::editor::EditorQueryError,
        LoadedSource, MemorySourceLoader, ModuleGraph, ModuleLimits, ModuleProgram,
        SourceLoadError, SourceLoadKind, SourceLoader,
    },
    ExecutionOptions,
};
use std::{cell::RefCell, sync::Arc};

fn load(loader: &dyn SourceLoader) -> ModuleGraph {
    modules::load("main.srp", loader, ModuleLimits::default()).unwrap()
}

fn execute(program: &ModuleProgram) -> Vec<u8> {
    let mut out = Vec::new();
    let mut err = Vec::new();
    program
        .execute(&mut out, &mut err, ExecutionOptions::default())
        .unwrap();
    assert!(err.is_empty());
    out
}

#[test]
fn queued_graph_keeps_the_revision_of_the_bytes_it_read() {
    let mut loader = MemorySourceLoader::new();
    loader
        .set_document(
            "main.srp",
            r#"import "lib.srp" as lib; print(lib.value);"#,
            3,
        )
        .unwrap();
    loader
        .set_document("lib.srp", "pub bind value = 7;", 8)
        .unwrap();
    let queued = load(&loader);
    let stale = load(&loader);
    assert_eq!(queued.source_revision(1), Some(8));
    loader
        .set_document("./lib.srp", "// नया 🌍\r\npub bind value = 9;", 9)
        .unwrap();
    assert_eq!(queued.sources().source(1), Some("pub bind value = 7;"));

    let limits = EditorCompileLimits::default();
    let mut cache = EditorCompilationCache::new(EditorCacheLimits::default());
    let current = cache
        .resolve_with_loaded_revisions(load(&loader), limits, None)
        .unwrap();
    let original = cache
        .resolve_with_loaded_revisions(queued, limits, None)
        .unwrap();
    assert_eq!(execute(&current.program), b"9\n");
    assert_eq!(execute(&original.program), b"7\n");
    assert!(current.snapshot.ensure_current(&[3, 9]).is_ok());
    assert_eq!(
        original.snapshot.ensure_current(&[3, 9]),
        Err(EditorQueryError::StaleVersion {
            source: 1,
            indexed: 8,
            current: 9
        })
    );

    let before = cache.stats();
    assert!(matches!(
        cache.resolve(stale, limits, |id| Some(if id == "lib.srp" { 9 } else { 3 }), None),
        Err(EditorCompileError::RevisionMismatch { source: 1, identity, loaded: 8, supplied: 9 })
            if identity == "lib.srp"
    ));
    assert_eq!(cache.stats(), before);
}

#[test]
fn cached_syntax_captures_current_revisions_in_discovery_order() {
    let mut loader = MemorySourceLoader::new();
    loader
        .set_document(
            "main.srp",
            r#"import "left.srp" as l; import "right.srp" as r; print(l.value,r.value);"#,
            1,
        )
        .unwrap();
    loader
        .set_document("left.srp", "pub bind value = 4;", 7)
        .unwrap();
    loader
        .set_document("right.srp", "pub bind value = 2;", 8)
        .unwrap();
    let mut syntax = SyntaxCache::new(SyntaxCacheLimits::default());
    let mut load = |loader: &MemorySourceLoader| {
        modules::load_with_syntax_cache("main.srp", loader, ModuleLimits::default(), &mut syntax)
            .unwrap()
    };
    let limits = EditorCompileLimits::default();
    let mut cache = EditorCompilationCache::new(EditorCacheLimits::default());
    let before = cache
        .resolve_with_loaded_revisions(load(&loader), limits, None)
        .unwrap();
    loader
        .set_document("left.srp", "pub bind value = 4;", 9)
        .unwrap();
    let refreshed = cache
        .resolve_with_loaded_revisions(load(&loader), limits, None)
        .unwrap();
    assert_eq!(refreshed.status, CacheStatus::Hit);
    assert!(Arc::ptr_eq(&before.program, &refreshed.program));
    assert!(before.snapshot.ensure_current(&[1, 7, 8]).is_ok());
    assert!(refreshed.snapshot.ensure_current(&[1, 9, 8]).is_ok());

    loader.set_document("main.srp", "// 🌍\r\nimport \"right.srp\" as r;\r\nimport \"left.srp\" as l;\r\nprint(l.value,r.value);", 2).unwrap();
    let reordered = load(&loader);
    assert_eq!(reordered.source_revision(1), Some(8));
    assert_eq!(reordered.source_revision(2), Some(9));
    let reordered = cache
        .resolve_with_loaded_revisions(reordered, limits, None)
        .unwrap();
    assert_eq!(reordered.status, CacheStatus::Stored);
    assert_eq!(reordered.program.modules()[1].identity, "right.srp");
    assert!(reordered.snapshot.ensure_current(&[2, 8, 9]).is_ok());
    assert_eq!(execute(&reordered.program), b"4 2\n");
    assert_eq!(syntax.stats().hits, 5);
}

#[test]
fn legacy_sources_can_supply_revisions_through_the_existing_callback() {
    let mut loader = MemorySourceLoader::new();
    loader
        .set_document(
            "main.srp",
            r#"import "lib.srp" as lib; print(lib.value);"#,
            0,
        )
        .unwrap();
    loader.insert("lib.srp", "pub bind value = 7;").unwrap();
    assert!(loader.insert("./lib.srp", "pub bind value = 3;").is_err());
    let graph = load(&loader);
    assert_eq!(graph.source_revision(0), Some(0));
    assert_eq!(graph.source_revision(1), None);
    assert_eq!(graph.source_revision(2), None);
    let mut cache = EditorCompilationCache::new(EditorCacheLimits::default());
    let limits = EditorCompileLimits::default();
    assert!(matches!(
        cache.resolve_with_loaded_revisions(graph, limits, None),
        Err(EditorCompileError::MissingRevision { source: 1, identity }) if identity == "lib.srp"
    ));
    let compiled = cache
        .resolve(
            load(&loader),
            limits,
            |id| Some(if id == "lib.srp" { 2 } else { 0 }),
            None,
        )
        .unwrap();
    assert_eq!(execute(&compiled.program), b"7\n");
    assert!(compiled.snapshot.ensure_current(&[0, 2]).is_ok());
}

#[test]
fn deleted_and_invalid_edits_return_fresh_diagnostics() {
    let mut loader = MemorySourceLoader::new();
    loader
        .set_document(
            "main.srp",
            r#"import "lib.srp" as lib; print(lib.value);"#,
            1,
        )
        .unwrap();
    let valid = "pub bind value = 7;";
    loader.set_document("lib.srp", valid, 2).unwrap();
    let limits = EditorCompileLimits::default();
    let mut cache = EditorCompilationCache::new(EditorCacheLimits::default());
    let before = cache
        .resolve_with_loaded_revisions(load(&loader), limits, None)
        .unwrap();
    let mut syntax = SyntaxCache::new(SyntaxCacheLimits::default());
    let mut load = |loader: &MemorySourceLoader| {
        modules::load_with_syntax_cache("main.srp", loader, ModuleLimits::default(), &mut syntax)
    };
    load(&loader).unwrap();
    loader
        .set_document("lib.srp", "// नाम\r\npub bind value = ;", 3)
        .unwrap();
    let warm = load(&loader).unwrap_err();
    let cold = modules::load("main.srp", &loader, ModuleLimits::default()).unwrap_err();
    assert_eq!(warm.diagnostic, cold.diagnostic);
    assert_eq!(warm.render(), cold.render());

    assert!(loader.remove_document("./lib.srp").unwrap());
    assert!(!loader.remove_document("lib.srp").unwrap());
    assert_eq!(load(&loader).unwrap_err().diagnostic.kind, Kind::ModuleLoad);
    assert!(before.snapshot.ensure_current(&[1, 2]).is_ok());
    loader.set_document("lib.srp", valid, 4).unwrap();
    let recovered = cache
        .resolve_with_loaded_revisions(load(&loader).unwrap(), limits, None)
        .unwrap();
    assert_eq!(recovered.status, CacheStatus::Hit);
    assert!(recovered.snapshot.ensure_current(&[1, 4]).is_ok());
    assert!(before.snapshot.ensure_current(&[1, 4]).is_err());
}

#[test]
fn revisioned_loading_reads_each_resolved_source_once_and_enforces_byte_limits() {
    struct CountingLoader {
        sources: MemorySourceLoader,
        reads: RefCell<Vec<String>>,
    }
    impl SourceLoader for CountingLoader {
        fn resolve(
            &self,
            importer: Option<&str>,
            specifier: &str,
        ) -> Result<String, SourceLoadError> {
            self.sources.resolve(importer, specifier)
        }
        fn load(&self, identity: &str, max_bytes: usize) -> Result<String, SourceLoadError> {
            self.sources.load(identity, max_bytes)
        }
        fn load_with_revision(
            &self,
            identity: &str,
            max_bytes: usize,
        ) -> Result<LoadedSource, SourceLoadError> {
            self.reads.borrow_mut().push(identity.to_owned());
            self.sources.load_with_revision(identity, max_bytes)
        }
    }
    let source = "import \"lib.srp\" as a; import \"./lib.srp\" as b;";
    let library = "// नमस्ते 🌍";
    let mut loader = CountingLoader {
        sources: MemorySourceLoader::new(),
        reads: RefCell::new(Vec::new()),
    };
    loader.sources.set_document("main.srp", source, 1).unwrap();
    loader.sources.set_document("lib.srp", library, 7).unwrap();
    let limits = ModuleLimits {
        max_total_bytes: source.len() + library.len(),
        ..ModuleLimits::default()
    };
    let loaded = modules::load("main.srp", &loader, limits).unwrap();
    assert_eq!(loaded.file_count(), 2);
    assert_eq!(loaded.source_revision(1), Some(7));
    assert_eq!(loader.reads.borrow().as_slice(), ["main.srp", "lib.srp"]);
    assert_eq!(
        loader
            .load_with_revision("lib.srp", library.len() - 1)
            .unwrap_err()
            .kind,
        SourceLoadKind::Limit
    );
    assert_eq!(
        modules::load(
            "main.srp",
            &loader,
            ModuleLimits {
                max_total_bytes: limits.max_total_bytes - 1,
                ..limits
            }
        )
        .unwrap_err()
        .diagnostic
        .kind,
        Kind::ModuleLimit
    );
}
