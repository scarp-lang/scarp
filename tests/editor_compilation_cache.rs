use scarp::{
    diagnostics::DiagnosticKind as Kind,
    modules::{
        self,
        cache::{
            CacheLimits, CacheStatus, CachedEditorProgram, ConstantCache, ConstantCacheLimits,
            EditorCacheLimits, EditorCompilationCache, EditorCompileError, EditorCompileLimits,
            SyntaxCache, SyntaxCacheLimits,
        },
        symbols::{
            editor::{EditorBuildError, EditorLimits, EditorQueryError, Utf16Position},
            IndexLimits,
        },
        ConstantLimits, MemorySourceLoader, ModuleGraph, ModuleLimits, SourceLoadError,
        SourceLoader,
    },
    types::TypeLimits,
    ExecutionOptions,
};
use std::{ptr, sync::Arc};

fn loader(files: &[(&str, &str)]) -> MemorySourceLoader {
    let mut loader = MemorySourceLoader::new();
    for &(name, source) in files {
        loader.insert(name, source).unwrap();
    }
    loader
}

fn graph(loader: &dyn SourceLoader) -> ModuleGraph {
    modules::load("main.srp", loader, ModuleLimits::default()).unwrap()
}

fn compile(
    cache: &mut EditorCompilationCache,
    loader: &dyn SourceLoader,
    revisions: &[(&str, u64)],
) -> Result<CachedEditorProgram, EditorCompileError> {
    cache.resolve(
        graph(loader),
        EditorCompileLimits::default(),
        |identity| {
            revisions
                .iter()
                .find(|&&(id, _)| id == identity)
                .map(|&(_, v)| v)
        },
        None,
    )
}

fn execute(result: &CachedEditorProgram) -> Vec<u8> {
    let mut out = Vec::new();
    let mut err = Vec::new();
    result
        .program
        .execute(&mut out, &mut err, ExecutionOptions::default())
        .unwrap();
    assert!(err.is_empty());
    out
}

#[test]
fn revision_only_refresh_shares_navigation_and_keeps_old_snapshot_immutable() {
    let entry = "import \"lib/café.srp\" as lib;\r\nprint(\"🌍\", lib.value);";
    let sources = loader(&[
        ("main.srp", entry),
        ("lib/café.srp", "// नमस्ते\r\npub bind value = 7;"),
    ]);
    let mut cache = EditorCompilationCache::new(EditorCacheLimits::default());
    let old = compile(
        &mut cache,
        &sources,
        &[("lib/café.srp", 8), ("main.srp", 3)],
    )
    .unwrap();
    let same = compile(
        &mut cache,
        &sources,
        &[("main.srp", 3), ("lib/café.srp", 8)],
    )
    .unwrap();
    let refreshed = compile(
        &mut cache,
        &sources,
        &[("lib/café.srp", 9), ("main.srp", 3)],
    )
    .unwrap();
    assert_eq!(old.status, CacheStatus::Stored);
    assert_eq!(same.status, CacheStatus::Hit);
    assert_eq!(refreshed.status, CacheStatus::Hit);
    assert!(Arc::ptr_eq(&old.program, &same.program));
    assert!(Arc::ptr_eq(&old.program, &refreshed.program));
    assert!(ptr::eq(
        old.snapshot.symbols().symbols(),
        refreshed.snapshot.symbols().symbols()
    ));
    assert_eq!(cache.stats().entries, 1);
    assert_eq!(cache.stats().hits, 2);

    let offset = entry.find("value").unwrap();
    let position = refreshed
        .snapshot
        .byte_to_utf16(&[3, 9], 0, offset)
        .unwrap();
    assert_eq!(position, Utf16Position::new(1, 16));
    let definition = refreshed
        .snapshot
        .goto_definition(&[3, 9], 0, position)
        .unwrap()
        .unwrap();
    assert_eq!(definition.location.unwrap().source, 1);
    assert_eq!(
        old.snapshot.goto_definition(&[3, 9], 0, position),
        Err(EditorQueryError::StaleVersion {
            source: 1,
            indexed: 8,
            current: 9,
        })
    );
    assert!(old.snapshot.ensure_current(&[3, 8]).is_ok());
    assert!(refreshed.snapshot.ensure_current(&[3, 8]).is_err());
}

#[test]
fn current_alias_targets_and_display_names_are_part_of_the_cache_key() {
    struct Retarget<'a> {
        files: &'a MemorySourceLoader,
        target: &'a str,
        second_label: &'a str,
    }
    impl SourceLoader for Retarget<'_> {
        fn resolve(
            &self,
            importer: Option<&str>,
            specifier: &str,
        ) -> Result<String, SourceLoadError> {
            if importer.is_some() && specifier == "target.srp" {
                return Ok(self.target.to_owned());
            }
            self.files.resolve(importer, specifier)
        }
        fn load(&self, identity: &str, max_bytes: usize) -> Result<String, SourceLoadError> {
            self.files.load(identity, max_bytes)
        }
        fn display_name(&self, identity: &str) -> String {
            if identity == "two.srp" {
                self.second_label.to_owned()
            } else {
                identity.to_owned()
            }
        }
    }

    let entry = r#"import "one.srp" as a; import "target.srp" as b; import "two.srp" as c; print(b.value);"#;
    let files = loader(&[
        ("main.srp", entry),
        ("one.srp", "pub bind value = 7;"),
        ("two.srp", "pub bind value = 7;"),
    ]);
    let mut cache = EditorCompilationCache::new(EditorCacheLimits::default());
    let revisions = [("main.srp", 1), ("one.srp", 5), ("two.srp", 6)];
    let resolve = |cache: &mut EditorCompilationCache, target, second_label| {
        compile(
            cache,
            &Retarget {
                files: &files,
                target,
                second_label,
            },
            &revisions,
        )
        .unwrap()
    };
    let first = resolve(&mut cache, "one.srp", "two.srp");
    let second = resolve(&mut cache, "two.srp", "two.srp");
    let relabeled = resolve(&mut cache, "two.srp", "editor/café.srp");
    assert_eq!(second.status, CacheStatus::Stored);
    assert_eq!(relabeled.status, CacheStatus::Stored);
    assert_eq!(
        first.program.dependencies(0),
        second.program.dependencies(0)
    );
    let target = |result: &CachedEditorProgram| {
        result
            .snapshot
            .symbols()
            .goto_definition(0, entry.rfind("value").unwrap())
            .unwrap()
            .location
            .unwrap()
            .source
    };
    assert_eq!(target(&first), 1);
    assert_eq!(target(&second), 2);
    assert_eq!(target(&relabeled), 2);
    assert_eq!(
        relabeled.program.sources().files()[2].name,
        "editor/café.srp"
    );
}

#[test]
fn source_edits_reuse_constant_folds_and_rebuild_navigation_in_current_source_order() {
    let before = loader(&[
        (
            "main.srp",
            r#"import "left.srp" as l; import "right.srp" as r; print(l.value, r.value);"#,
        ),
        ("left.srp", "pub bind value = 2;"),
        ("right.srp", "pub bind value = 3;"),
    ]);
    let after = loader(&[
        ("main.srp", "// 🌍\r\nimport \"right.srp\" as r;\r\nimport \"left.srp\" as l;\r\nprint(l.value, r.value);"),
        ("left.srp", "// बदला\r\npub bind value = 4;"),
        ("right.srp", "pub bind value = 3;"),
    ]);
    let mut editor = EditorCompilationCache::new(EditorCacheLimits::default());
    let mut constants = ConstantCache::new(ConstantCacheLimits::default());
    let mut syntax = SyntaxCache::new(SyntaxCacheLimits::default());
    let mut compile_with_caches = |sources: &dyn SourceLoader| {
        let graph = modules::load_with_syntax_cache(
            "main.srp",
            sources,
            ModuleLimits::default(),
            &mut syntax,
        )
        .unwrap();
        editor
            .resolve(
                graph,
                EditorCompileLimits::default(),
                |id| match id {
                    "main.srp" => Some(4),
                    "left.srp" => Some(6),
                    "right.srp" => Some(9),
                    _ => None,
                },
                Some(&mut constants),
            )
            .unwrap()
    };
    let old = compile_with_caches(&before);
    let warm = compile_with_caches(&after);
    let returned = compile_with_caches(&before);
    assert_eq!(warm.status, CacheStatus::Stored);
    assert_eq!(returned.status, CacheStatus::Hit);
    assert!(Arc::ptr_eq(&old.program, &returned.program));
    assert_eq!(constants.stats().hits, 1);
    assert_eq!(execute(&old), b"2 3\n");
    assert_eq!(execute(&warm), b"4 3\n");
    assert!(warm.snapshot.ensure_current(&[4, 9, 6]).is_ok());
    assert!(warm.snapshot.ensure_current(&[4, 6, 9]).is_err());
    assert_eq!(warm.program.modules()[1].identity, "right.srp");

    let cold = modules::resolve_indexed(graph(&after), IndexLimits::default()).unwrap();
    let mut actual = Vec::new();
    let mut expected = Vec::new();
    warm.snapshot
        .symbols()
        .write_references_json(&mut actual)
        .unwrap();
    cold.index().write_references_json(&mut expected).unwrap();
    assert_eq!(actual, expected);
}

#[test]
fn missing_revisions_and_tighter_limits_do_not_reuse_a_success() {
    let sources = loader(&[
        (
            "main.srp",
            r#"import "lib.srp" as lib; print(lib.value, lib.label);"#,
        ),
        ("lib.srp", "pub bind value = 3; pub bind label = \"hello\";"),
    ]);
    let mut cache = EditorCompilationCache::new(EditorCacheLimits::default());
    let first = compile(&mut cache, &sources, &[("main.srp", 1), ("lib.srp", 2)]).unwrap();
    let before = cache.stats();
    assert!(matches!(
        compile(&mut cache, &sources, &[("main.srp", 1), ("lib", 2)]),
        Err(EditorCompileError::MissingRevision { source: 1, identity }) if identity == "lib.srp"
    ));

    let tight = EditorCompileLimits::default();
    for (limits, expected) in [
        (
            EditorCompileLimits {
                constant_limits: ConstantLimits {
                    max_steps: 0,
                    ..tight.constant_limits
                },
                ..tight
            },
            Kind::ModuleLimit,
        ),
        (
            EditorCompileLimits {
                type_limits: TypeLimits {
                    max_types: 1,
                    ..tight.type_limits
                },
                ..tight
            },
            Kind::TypeComplexity,
        ),
        (
            EditorCompileLimits {
                index_limits: IndexLimits {
                    max_references: 0,
                    ..tight.index_limits
                },
                ..tight
            },
            Kind::SymbolIndexLimit,
        ),
    ] {
        let actual = cache
            .resolve(graph(&sources), limits, |_| Some(2), None)
            .unwrap_err();
        let cold = modules::resolve_indexed_with_compiler_limits(
            graph(&sources),
            limits.constant_limits,
            limits.type_limits,
            limits.index_limits,
        )
        .unwrap_err();
        let EditorCompileError::Module(actual) = actual else {
            panic!("expected compiler diagnostic")
        };
        assert_eq!(actual.diagnostic.kind, expected);
        assert_eq!(actual.diagnostic, cold.diagnostic);
        assert_eq!(actual.render(), cold.render());
    }
    assert!(matches!(
        cache.resolve(
            graph(&sources),
            EditorCompileLimits {
                editor_limits: EditorLimits { max_lines: 1 },
                ..tight
            },
            |_| Some(2),
            None
        ),
        Err(EditorCompileError::Editor(EditorBuildError::LineLimit))
    ));
    let limited_graph = modules::load(
        "main.srp",
        &sources,
        ModuleLimits {
            max_expansion_bytes: 0,
            ..ModuleLimits::default()
        },
    )
    .unwrap();
    assert!(matches!(
        cache.resolve(limited_graph, tight, |_| Some(2), None),
        Err(EditorCompileError::Module(e)) if e.diagnostic.kind == Kind::ModuleLimit
    ));
    assert_eq!(cache.stats(), before);
    assert_eq!(execute(&first), b"3 hello\n");
}

#[test]
fn failed_visibility_edit_leaves_the_previous_snapshot_at_its_original_revision() {
    let entry = r#"import "lib.srp" as lib; print(lib.value);"#;
    let valid = loader(&[("main.srp", entry), ("lib.srp", "pub bind value = 3;")]);
    let edited = loader(&[
        ("main.srp", entry),
        ("lib.srp", "// निजी 🌍\r\nbind value = 3;"),
    ]);
    let original_revisions = [("main.srp", 1), ("lib.srp", 2)];
    let mut cache = EditorCompilationCache::new(EditorCacheLimits::default());
    let original = compile(&mut cache, &valid, &original_revisions).unwrap();
    let before = cache.stats();
    let error = compile(&mut cache, &edited, &[("main.srp", 1), ("lib.srp", 3)]).unwrap_err();
    let EditorCompileError::Module(error) = error else {
        panic!("expected compiler diagnostic")
    };
    let cold = modules::resolve_indexed(graph(&edited), IndexLimits::default()).unwrap_err();
    assert_eq!(error.diagnostic.kind, Kind::ModuleVisibility);
    assert_eq!(error.diagnostic, cold.diagnostic);
    assert_eq!(error.render(), cold.render());
    assert_eq!(cache.stats(), before);
    assert!(original.snapshot.ensure_current(&[1, 3]).is_err());
    let recovered = compile(&mut cache, &valid, &original_revisions).unwrap();
    assert_eq!(recovered.status, CacheStatus::Hit);
    assert!(Arc::ptr_eq(&original.program, &recovered.program));
    assert!(recovered.snapshot.ensure_current(&[1, 2]).is_ok());
}

#[test]
fn every_retention_budget_bypasses_storage_without_changing_the_result() {
    let sources = loader(&[
        ("main.srp", r#"import "lib.srp" as lib; print(lib.value);"#),
        ("lib.srp", "pub bind value = 3;"),
    ]);
    let revisions = [("main.srp", 1), ("lib.srp", 2)];
    let mut measuring = EditorCompilationCache::new(EditorCacheLimits::default());
    compile(&mut measuring, &sources, &revisions).unwrap();
    let use_ = measuring.stats();
    let exact = EditorCacheLimits {
        graphs: CacheLimits {
            max_entries: 1,
            max_retained_source_bytes: use_.retained_source_bytes,
        },
        max_retained_index_items: use_.retained_index_items,
        max_retained_name_bytes: use_.retained_name_bytes,
        max_retained_lines: use_.retained_lines,
    };
    let mut cache = EditorCompilationCache::new(exact);
    assert_eq!(
        compile(&mut cache, &sources, &revisions).unwrap().status,
        CacheStatus::Stored
    );
    for limits in [
        EditorCacheLimits {
            graphs: CacheLimits {
                max_entries: 0,
                ..exact.graphs
            },
            ..exact
        },
        EditorCacheLimits {
            graphs: CacheLimits {
                max_retained_source_bytes: exact.graphs.max_retained_source_bytes - 1,
                ..exact.graphs
            },
            ..exact
        },
        EditorCacheLimits {
            max_retained_index_items: exact.max_retained_index_items - 1,
            ..exact
        },
        EditorCacheLimits {
            max_retained_name_bytes: exact.max_retained_name_bytes - 1,
            ..exact
        },
        EditorCacheLimits {
            max_retained_lines: exact.max_retained_lines - 1,
            ..exact
        },
    ] {
        let mut cache = EditorCompilationCache::new(limits);
        let result = compile(&mut cache, &sources, &revisions).unwrap();
        assert_eq!(result.status, CacheStatus::Uncached);
        assert_eq!(execute(&result), b"3\n");
        assert_eq!(cache.stats().entries, 0);
        assert_eq!(cache.stats().bypasses, 1);
    }
}

#[test]
fn fifo_and_identity_eviction_account_for_all_retained_records() {
    let compile = |cache: &mut EditorCompilationCache, path: &str| {
        let entry = format!(r#"import "{path}" as lib; print(lib.value);"#);
        let files = loader(&[("main.srp", &entry), (path, "pub bind value = 3;")]);
        cache
            .resolve(
                graph(&files),
                EditorCompileLimits::default(),
                |_| Some(1),
                None,
            )
            .unwrap()
    };
    let mut cache = EditorCompilationCache::new(EditorCacheLimits {
        // Each graph has two lines. This limit forces eviction below the entry cap.
        max_retained_lines: 4,
        ..EditorCacheLimits::default()
    });
    let first = compile(&mut cache, "a.srp");
    let first_usage = cache.stats();
    compile(&mut cache, "b.srp");
    assert_eq!(compile(&mut cache, "a.srp").status, CacheStatus::Hit);
    compile(&mut cache, "c.srp");
    assert_eq!(cache.stats().evictions, 1);
    assert_eq!(cache.evict_identity("a.srp"), 0);
    assert_eq!(cache.evict_identity("b.srp"), 1);
    let usage = cache.stats();
    assert_eq!(usage.entries, 1);
    assert_eq!(
        usage.retained_source_bytes,
        first_usage.retained_source_bytes
    );
    assert_eq!(usage.retained_index_items, first_usage.retained_index_items);
    assert_eq!(usage.retained_name_bytes, first_usage.retained_name_bytes);
    assert_eq!(usage.retained_lines, first_usage.retained_lines);
    cache.clear();
    let cleared = cache.stats();
    assert_eq!((cleared.entries, cleared.evictions), (0, 3));
    assert_eq!(
        (
            cleared.retained_source_bytes,
            cleared.retained_index_items,
            cleared.retained_name_bytes,
            cleared.retained_lines
        ),
        (0, 0, 0, 0)
    );
    assert_eq!(execute(&first), b"3\n");
    assert!(first.snapshot.ensure_current(&[1, 1]).is_ok());
}
