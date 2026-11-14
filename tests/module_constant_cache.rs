use scarp::{
    diagnostics::DiagnosticKind as Kind,
    modules::{
        self,
        cache::{ConstantCache, ConstantCacheLimits, SyntaxCache, SyntaxCacheLimits},
        symbols::{IndexLimits, IndexedProgram},
        ConstantLimits, MemorySourceLoader, ModuleDiagnostic, ModuleLimits, ModuleProgram,
        SourceLoader,
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

fn compile(
    loader: &dyn SourceLoader,
    modules: ModuleLimits,
    constants: ConstantLimits,
    cache: Option<&mut ConstantCache>,
) -> Result<ModuleProgram, ModuleDiagnostic> {
    let graph = modules::load("main.srp", loader, modules)?;
    match cache {
        Some(cache) => {
            modules::resolve_with_constant_cache(graph, constants, TypeLimits::default(), cache)
        }
        None => modules::resolve_with_constant_limits(graph, constants),
    }
}

fn cached(loader: &dyn SourceLoader, cache: &mut ConstantCache) -> ModuleProgram {
    compile(
        loader,
        ModuleLimits::default(),
        ConstantLimits::default(),
        Some(cache),
    )
    .unwrap()
}

fn output(program: &ModuleProgram) -> String {
    let mut out = Vec::new();
    let mut err = Vec::new();
    program
        .execute(&mut out, &mut err, ExecutionOptions::default())
        .unwrap();
    assert!(err.is_empty());
    String::from_utf8(out).unwrap()
}

fn assert_same_index(warm: &IndexedProgram, cold: &IndexedProgram) {
    assert_eq!(
        warm.program().constant_dependencies(),
        cold.program().constant_dependencies()
    );
    assert_eq!(warm.index().usage(), cold.index().usage());
    let mut warm_json = Vec::new();
    let mut cold_json = Vec::new();
    warm.index().write_references_json(&mut warm_json).unwrap();
    cold.index().write_references_json(&mut cold_json).unwrap();
    assert_eq!(warm_json, cold_json);
    warm_json.clear();
    cold_json.clear();
    warm.program()
        .write_inspection_json(&mut warm_json)
        .unwrap();
    cold.program()
        .write_inspection_json(&mut cold_json)
        .unwrap();
    assert_eq!(warm_json, cold_json);
}

fn assert_same_error(warm: &ModuleDiagnostic, cold: &ModuleDiagnostic, expected: Kind) {
    assert_eq!(cold.diagnostic.kind, expected);
    assert_eq!(warm.diagnostic, cold.diagnostic);
    assert_eq!(warm.render(), cold.render());
}

#[test]
fn diamond_edits_reuse_unaffected_values_and_rebuild_current_navigation() {
    let before = r#"
import "a.srp" as a;
import "b.srp" as b;
import "extra.srp" as extra;
pub bind total: integer = a.value + b.value;
print(total, extra.message);
"#;
    let after = concat!(
        "// नया 🌍\r\n",
        "import \"extra.srp\" as extra;\r\n",
        "import \"b.srp\" as b;\r\n",
        "import \"a.srp\" as a;\r\n",
        "pub bind total: integer = a.value + b.value;\r\n",
        "print(total, extra.message);",
    );
    let graph_sources = |main, shared| {
        loader(&[
            ("main.srp", main),
            (
                "a.srp",
                r#"import "café.srp" as c; pub bind value = c.base * 3;"#,
            ),
            (
                "b.srp",
                r#"import "café.srp" as c; pub bind value = c.base + 1;"#,
            ),
            ("café.srp", shared),
            ("extra.srp", "pub bind message = \"नमस्ते 🌍\";"),
        ])
    };
    let old = graph_sources(before, "pub bind base = 7; pub bind fixed = 4 + 8;");
    let new = graph_sources(
        after,
        "// बदला 🌍\r\npub bind fixed = 4 + 8;\r\npub bind base = 8;",
    );
    let mut constants = ConstantCache::new(ConstantCacheLimits::default());
    let mut syntax = SyntaxCache::new(SyntaxCacheLimits::default());
    let mut indexed = |sources: &dyn SourceLoader| {
        let graph = modules::load_with_syntax_cache(
            "main.srp",
            sources,
            ModuleLimits::default(),
            &mut syntax,
        )
        .unwrap();
        modules::resolve_indexed_with_constant_cache(
            graph,
            ConstantLimits::default(),
            TypeLimits::default(),
            IndexLimits::default(),
            &mut constants,
        )
        .unwrap()
    };
    let previous = indexed(&old);
    let warm = indexed(&new);
    let cold = modules::compile_indexed_with_loader(
        "main.srp",
        &new,
        ModuleLimits::default(),
        IndexLimits::default(),
    )
    .unwrap();
    assert_eq!(constants.stats().hits, 2); // fixed and message
    assert_eq!(constants.stats().misses, 10);
    assert_eq!(syntax.stats().hits, 3); // a, b, extra
    assert_eq!(output(previous.program()), "29 नमस्ते 🌍\n");
    assert_eq!(output(warm.program()), "33 नमस्ते 🌍\n");
    assert_eq!(output(warm.program()), output(cold.program()));
    assert_same_index(&warm, &cold);
}

#[test]
fn equal_retargeted_values_can_be_reused_but_visibility_is_checked_again() {
    let old = loader(&[
        (
            "main.srp",
            r#"import "old.srp" as dep; pub bind value = true or dep.flag; print(value);"#,
        ),
        ("old.srp", "pub bind flag = false;"),
    ]);
    let new_main = r#"import "new.srp" as dep; pub bind value = true or dep.flag; print(value);"#;
    let new = loader(&[
        ("main.srp", new_main),
        ("new.srp", "// नाम\r\n\r\npub bind flag = false;"),
    ]);
    let mut cache = ConstantCache::new(ConstantCacheLimits::default());
    assert_eq!(output(&cached(&old, &mut cache)), "true\n");
    let warm = modules::resolve_indexed_with_constant_cache(
        modules::load("main.srp", &new, ModuleLimits::default()).unwrap(),
        ConstantLimits::default(),
        TypeLimits::default(),
        IndexLimits::default(),
        &mut cache,
    )
    .unwrap();
    let cold = modules::compile_indexed_with_loader(
        "main.srp",
        &new,
        ModuleLimits::default(),
        IndexLimits::default(),
    )
    .unwrap();
    assert_eq!(cache.stats().hits, 1); // value
    assert_same_index(&warm, &cold);

    for (files, kind) in [
        (vec![("main.srp", new_main)], Kind::ModuleLoad),
        (
            vec![
                ("main.srp", new_main),
                ("new.srp", "// नाम\r\nbind flag = false;"),
            ],
            Kind::ModuleVisibility,
        ),
        (
            vec![
                ("main.srp", new_main),
                (
                    "new.srp",
                    r#"import "main.srp" as root; pub bind flag = false;"#,
                ),
            ],
            Kind::ModuleCycle,
        ),
    ] {
        let changed = loader(&files);
        let modules = ModuleLimits::default();
        let constants = ConstantLimits::default();
        let warm = compile(&changed, modules, constants, Some(&mut cache)).unwrap_err();
        let cold = compile(&changed, modules, constants, None).unwrap_err();
        assert_same_error(&warm, &cold, kind);
    }
}

#[test]
fn referenced_floats_distinguish_signed_zero_and_preserve_reference_order() {
    let make_sources = |left, right| {
        loader(&[
            (
                "main.srp",
                r#"import "left.srp" as left; import "right.srp" as right;
pub bind copy = left.zero;
pub bind difference: integer = left.number - right.number;
print(copy, difference);"#,
            ),
            ("left.srp", left),
            ("right.srp", right),
        ])
    };
    let before = make_sources(
        "pub bind zero = 0.0; pub bind number = 10;",
        "pub bind number = 3;",
    );
    let after = make_sources(
        "pub bind zero = -0.0; pub bind number = 3;",
        "pub bind number = 10;",
    );
    let mut cache = ConstantCache::new(ConstantCacheLimits::default());
    assert_eq!(output(&cached(&before, &mut cache)), "0.0 7\n");
    assert_eq!(output(&cached(&after, &mut cache)), "-0.0 -7\n");
    assert_eq!(cache.stats().hits, 0);
    assert_eq!(output(&cached(&after, &mut cache)), "-0.0 -7\n");
    assert_eq!(cache.stats().hits, 5);
}

#[test]
fn changed_reference_can_select_a_previously_skipped_failure() {
    let entry = r#"import "lib.srp" as lib; pub bind value = lib.skip or (1 / 0 > 0);"#;
    let before = loader(&[("main.srp", entry), ("lib.srp", "pub bind skip = true;")]);
    let after = loader(&[("main.srp", entry), ("lib.srp", "pub bind skip = false;")]);
    let mut cache = ConstantCache::new(ConstantCacheLimits::default());
    cached(&before, &mut cache);
    let modules = ModuleLimits::default();
    let constants = ConstantLimits::default();
    let warm = compile(&after, modules, constants, Some(&mut cache)).unwrap_err();
    let cold = compile(&after, modules, constants, None).unwrap_err();
    assert_same_error(&warm, &cold, Kind::DivisionByZero);
    assert_eq!(cache.stats().hits, 0);

    // A failure can retain the earlier successful fold of skip, but the
    // failed division will still be evaluated on a repeated request.
    let retry = compile(&after, modules, constants, Some(&mut cache)).unwrap_err();
    assert_same_error(&retry, &cold, Kind::DivisionByZero);
    assert_eq!(cache.stats().hits, 1);
    assert_eq!(cache.stats().entries, 3);
}

#[test]
fn cumulative_limits_report_the_same_first_failing_span_on_warm_builds() {
    let sources = loader(&[
        (
            "main.srp",
            r#"import "lib.srp" as lib; pub bind value = lib.base + 2; print(value);"#,
        ),
        ("lib.srp", "pub bind base = 1 + 2;"),
    ]);
    let mut cache = ConstantCache::new(ConstantCacheLimits::default());
    cached(&sources, &mut cache);
    let modules = ModuleLimits::default();
    let exact = ConstantLimits {
        max_nodes: 6,
        max_steps: 8,
    };
    let warm = compile(&sources, modules, exact, Some(&mut cache)).unwrap();
    assert_eq!(output(&warm), "5\n");
    assert_eq!(cache.stats().hits, 2);

    for constants in [
        ConstantLimits {
            max_steps: 7,
            ..exact
        },
        ConstantLimits {
            max_nodes: 5,
            ..exact
        },
    ] {
        let before = cache.stats();
        let warm = compile(&sources, modules, constants, Some(&mut cache)).unwrap_err();
        let cold = compile(&sources, modules, constants, None).unwrap_err();
        assert_same_error(&warm, &cold, Kind::ModuleLimit);
        // The first fold consumes budget on both paths. The failing second
        // fold cannot report a hit or create a new entry.
        assert_eq!(cache.stats().hits, before.hits + 1);
        assert_eq!(cache.stats().entries, before.entries);
    }
}

#[test]
fn warm_string_folds_preserve_expansion_and_index_quotas() {
    let text = "नमस्ते 🌍";
    let library = format!("pub bind original = \"{text}\";");
    let sources = loader(&[
        (
            "main.srp",
            r#"import "lib.srp" as lib; pub bind copy = lib.original; print(copy);"#,
        ),
        ("lib.srp", &library),
    ]);
    let mut cache = ConstantCache::new(ConstantCacheLimits::default());
    assert_eq!(output(&cached(&sources, &mut cache)), format!("{text}\n"));
    let constants = ConstantLimits::default();
    for bytes in [text.len() - 1, text.len() * 2 - 1, text.len() * 3 - 1] {
        let modules = ModuleLimits {
            max_expansion_bytes: bytes,
            ..ModuleLimits::default()
        };
        let warm = compile(&sources, modules, constants, Some(&mut cache)).unwrap_err();
        let cold = compile(&sources, modules, constants, None).unwrap_err();
        assert_same_error(&warm, &cold, Kind::ModuleLimit);
    }

    let index = IndexLimits {
        max_references: 0,
        ..IndexLimits::default()
    };
    let load = || modules::load("main.srp", &sources, ModuleLimits::default()).unwrap();
    let warm = modules::resolve_indexed_with_constant_cache(
        load(),
        constants,
        TypeLimits::default(),
        index,
        &mut cache,
    )
    .unwrap_err();
    let cold = modules::resolve_indexed(load(), index).unwrap_err();
    assert_same_error(&warm, &cold, Kind::SymbolIndexLimit);
}

#[test]
fn fifo_retention_and_identity_eviction_leave_old_results_usable() {
    let make_sources = |identity, declaration| {
        let entry = format!(r#"import "{identity}" as lib; print(lib.value);"#);
        loader(&[("main.srp", &entry), (identity, declaration)])
    };
    let first = make_sources("a.srp", "pub bind value = 3;");
    let second = make_sources("b.srp", "pub bind value = 4;");
    let third = make_sources("c.srp", "pub bind value = 5;");
    let mut cache = ConstantCache::new(ConstantCacheLimits {
        max_entries: 2,
        ..ConstantCacheLimits::default()
    });
    let old = cached(&first, &mut cache);
    cached(&second, &mut cache);
    cached(&first, &mut cache);
    assert_eq!(cache.stats().hits, 1);
    cached(&third, &mut cache);
    assert_eq!(cache.stats().evictions, 1);
    assert_eq!(cache.evict_identity("a.srp"), 0); // the hit did not promote it
    assert_eq!(cache.evict_identity("b.srp"), 1);
    assert_eq!(cache.stats().entries, 1);
    assert_eq!(output(&old), "3\n");
    cache.clear();
    assert_eq!(cache.stats().entries, 0);
    assert_eq!(cache.stats().retained_bytes, 0);
    assert_eq!(cache.stats().evictions, 3);
}

#[test]
fn byte_quota_counts_source_and_result_utf8_and_can_bypass_retention() {
    let source = "pub bind value = \"é🌍\"; print(value);";
    let bytes = "main.srp".len() + r#"bind value = "é🌍";"#.len() + "é🌍".len();
    let first = loader(&[("main.srp", source)]);
    for allowance in [0, bytes - 1, bytes] {
        let mut cache = ConstantCache::new(ConstantCacheLimits {
            max_entries: 2,
            max_retained_bytes: allowance,
        });
        for _ in 0..2 {
            assert_eq!(output(&cached(&first, &mut cache)), "é🌍\n");
        }
        let stats = cache.stats();
        if allowance == bytes {
            assert_eq!(
                (stats.entries, stats.retained_bytes, stats.hits),
                (1, bytes, 1)
            );
        } else {
            assert_eq!(
                (stats.entries, stats.retained_bytes, stats.bypasses),
                (0, 0, 2)
            );
        }
    }

    let second = loader(&[("main.srp", "pub bind value = \"è🌎\"; print(value);")]);
    let mut cache = ConstantCache::new(ConstantCacheLimits {
        max_entries: 2,
        max_retained_bytes: bytes,
    });
    cached(&first, &mut cache);
    assert_eq!(output(&cached(&second, &mut cache)), "è🌎\n");
    let stats = cache.stats();
    assert_eq!(
        (stats.entries, stats.retained_bytes, stats.evictions),
        (1, bytes, 1)
    );
}
