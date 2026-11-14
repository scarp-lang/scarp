//! Correctness-checked frontend/metadata workloads; timings are not runtime speedups.
use scarp::{
    modules::{
        self,
        cache::{
            CacheLimits, CacheStatus, CachedCompileOptions, CompilationCache, ConstantCache,
            ConstantCacheLimits, EditorCacheLimits, EditorCompilationCache, EditorCompileLimits,
            SyntaxCache, SyntaxCacheLimits, TokenCache, TokenCacheLimits,
        },
        symbols::editor::EditorSnapshot,
        ConstantLimits, MemorySourceLoader, ModuleLimits,
    },
    types::{FunctionEffect, PrimitiveType, TypeKind, TypeLimits, TypeRegistryBuilder},
    ExecutionOptions,
};
use std::{hint::black_box, sync::Arc, time::Instant};

fn measure(name: &str, bytes: usize, iterations: u32, mut work: impl FnMut()) {
    for sample in 1..=3 {
        let start = Instant::now();
        for _ in 0..iterations {
            work();
        }
        let mean = start.elapsed().as_secs_f64() * 1_000_000.0 / f64::from(iterations);
        println!("{name},{bytes},{iterations},{sample},{mean:.3}");
    }
}

fn measure_constant_edits(original: &MemorySourceLoader, files: &[(&str, &str)], iterations: u32) {
    let mut edited = MemorySourceLoader::new();
    for &(name, source) in files {
        let replacement = if name == "right.srp" {
            r#"import "base.srp" as base; pub bind value: integer=base.value*3;"#
        } else {
            source
        };
        assert_eq!(source.len(), replacement.len());
        edited.insert(name, replacement).unwrap();
    }
    let load = |source: &MemorySourceLoader, syntax: &mut SyntaxCache| {
        modules::load_with_syntax_cache("main.srp", source, ModuleLimits::default(), syntax)
            .unwrap()
    };
    let resolve_cached = |graph, constants: &mut ConstantCache| {
        modules::resolve_with_constant_cache(
            graph,
            ConstantLimits::default(),
            TypeLimits::default(),
            constants,
        )
        .unwrap()
    };
    let mut cold_syntax = SyntaxCache::new(SyntaxCacheLimits::default());
    let mut warm_syntax = SyntaxCache::new(SyntaxCacheLimits::default());
    let mut constants = ConstantCache::new(ConstantCacheLimits::default());
    for (source, expected) in [(original, b"1537\n"), (&edited, b"2049\n")] {
        let cold = modules::resolve(load(source, &mut cold_syntax)).unwrap();
        let warm = resolve_cached(load(source, &mut warm_syntax), &mut constants);
        assert_eq!(cold.constant_dependencies(), warm.constant_dependencies());
        for program in [cold, warm] {
            let (mut out, mut err) = (Vec::new(), Vec::new());
            program
                .execute(&mut out, &mut err, ExecutionOptions::default())
                .unwrap();
            assert_eq!(&*out, expected);
            assert!(err.is_empty());
        }
    }
    assert_eq!(constants.stats().hits, 515);
    assert_eq!(constants.stats().misses, 517);

    // Both paths alternate the same two source versions with warm AST caches.
    let bytes = files.iter().map(|(_, text)| text.len()).sum();
    let mut cold_edits = [original, &edited].into_iter().cycle();
    measure(
        "module_constant_edits_cold_4files",
        bytes,
        iterations,
        || {
            let graph = load(cold_edits.next().unwrap(), &mut cold_syntax);
            black_box(modules::resolve(graph).unwrap());
        },
    );
    let mut warm_edits = [original, &edited].into_iter().cycle();
    measure(
        "module_constant_edits_warm_4files",
        bytes,
        iterations,
        || {
            let graph = load(warm_edits.next().unwrap(), &mut warm_syntax);
            black_box(resolve_cached(graph, &mut constants));
        },
    );
}

fn measure_editor_revision_refresh(loader: &MemorySourceLoader, bytes: usize, iterations: u32) {
    let limits = EditorCompileLimits::default();
    let mut syntax = SyntaxCache::new(SyntaxCacheLimits::default());
    let mut load = || {
        modules::load_with_syntax_cache("main.srp", loader, ModuleLimits::default(), &mut syntax)
            .unwrap()
    };
    let mut editor = EditorCompilationCache::new(EditorCacheLimits::default());
    let first = editor.resolve(load(), limits, |_| Some(0), None).unwrap();
    let next = editor.resolve(load(), limits, |_| Some(1), None).unwrap();
    assert_eq!(next.status, CacheStatus::Hit);
    assert!(Arc::ptr_eq(&first.program, &next.program));
    assert!(first.snapshot.ensure_current(&[1; 4]).is_err());
    assert!(next.snapshot.ensure_current(&[1; 4]).is_ok());

    // Both measurements start with the same warm AST cache.
    let mut revision = 1;
    measure("editor_revision_rebuild_4files", bytes, iterations, || {
        revision += 1;
        let indexed = modules::resolve_indexed(load(), limits.index_limits).unwrap();
        let snapshot =
            EditorSnapshot::new(indexed.index(), &[revision; 4], limits.editor_limits).unwrap();
        black_box((indexed, snapshot));
    });
    measure("editor_revision_cache_4files", bytes, iterations, || {
        revision += 1;
        let result = editor
            .resolve(load(), limits, |_| Some(revision), None)
            .unwrap();
        assert_eq!(result.status, CacheStatus::Hit);
        black_box(result);
    });
}

fn main() {
    let iterations = if std::env::args().any(|arg| arg == "--quick") {
        3
    } else {
        25
    };
    let mut source = String::from("var total: integer=0;\n");
    for index in 0..512 {
        source.push_str(&format!(
            "bind v{index}: integer={index}; total+=v{index};\n"
        ));
    }
    source.push_str("print(total);");
    let mut expected = Vec::new();
    scarp::run(&source, &mut expected).unwrap();
    assert_eq!(expected, b"130816\n");
    assert_eq!(scarp::compile(&source).unwrap().binding_count(), 513);

    let mut base = String::from("bind v0: integer=0;\n");
    for index in 1..=512 {
        base.push_str(&format!("bind v{index}: integer=v{}+1;\n", index - 1));
    }
    base.push_str("pub bind value: integer=v512;");
    let files=[("main.srp","import \"left.srp\" as left; import \"right.srp\" as right; print(left.value+right.value);"),
        ("left.srp","import \"base.srp\" as base; pub bind value: integer=base.value+1;"),
        ("right.srp","import \"base.srp\" as base; pub bind value: integer=base.value*2;"),("base.srp",base.as_str())];
    let mut loader = MemorySourceLoader::new();
    for (name, text) in files {
        loader.insert(name, text).unwrap();
    }
    let compile =
        || modules::compile_with_loader("main.srp", &loader, ModuleLimits::default()).unwrap();
    let program = compile();
    assert_eq!(program.modules().len(), 4);
    assert_eq!(program.constant_dependencies().len(), 515);
    let (mut out, mut err) = (Vec::new(), Vec::new());
    program
        .execute(&mut out, &mut err, ExecutionOptions::default())
        .unwrap();
    assert_eq!(out, b"1537\n");
    assert!(err.is_empty());
    let mut json = Vec::new();
    program.write_inspection_json(&mut json).unwrap();
    let size = json.len();
    assert!(json.starts_with(b"{\"schema_version\":1,"));
    let mut again = Vec::new();
    program.write_inspection_json(&mut again).unwrap();
    assert_eq!(json, again);

    println!("workload,input_bytes,iterations,sample,mean_us");
    measure("annotated_bindings_512", source.len(), iterations, || {
        black_box(scarp::compile(black_box(&source)).unwrap());
    });
    measure(
        "module_constants_4files",
        files.iter().map(|(_, text)| text.len()).sum(),
        iterations,
        || {
            black_box(compile());
        },
    );
    // Warm front-end parses and whole-graph semantic cache checks use the
    // same exact source graph and expected output as the cold benchmark.
    // Report timings without assuming a universal speedup.
    let input_bytes: usize = files.iter().map(|(_, text)| text.len()).sum();
    let mut syntax_cache = SyntaxCache::new(SyntaxCacheLimits::default());
    let warmed = modules::load_with_syntax_cache(
        "main.srp",
        &loader,
        ModuleLimits::default(),
        &mut syntax_cache,
    )
    .unwrap();
    assert_eq!(
        modules::resolve(warmed)
            .unwrap()
            .constant_dependencies()
            .len(),
        515
    );
    measure("module_syntax_warm_4files", input_bytes, iterations, || {
        let graph = modules::load_with_syntax_cache(
            "main.srp",
            &loader,
            ModuleLimits::default(),
            &mut syntax_cache,
        )
        .unwrap();
        let resolved = modules::resolve(graph).unwrap();
        assert_eq!(resolved.constant_dependencies().len(), 515);
        black_box(resolved);
    });
    assert!(syntax_cache.stats().hits >= 4);
    measure_constant_edits(&loader, &files, iterations);

    let mut token_cache = TokenCache::new(TokenCacheLimits::default());
    modules::load_with_token_cache(
        "main.srp",
        &loader,
        ModuleLimits::default(),
        &mut token_cache,
    )
    .unwrap();
    measure("module_tokens_warm_4files", input_bytes, iterations, || {
        let graph = modules::load_with_token_cache(
            "main.srp",
            &loader,
            ModuleLimits::default(),
            &mut token_cache,
        )
        .unwrap();
        let resolved = modules::resolve(graph).unwrap();
        assert_eq!(resolved.constant_dependencies().len(), 515);
        black_box(resolved);
    });
    assert!(token_cache.stats().hits >= 4);

    let mut semantic_cache = CompilationCache::new(CacheLimits::default());
    let mut combined_tokens = TokenCache::new(TokenCacheLimits::default());
    let mut combined_syntax = SyntaxCache::new(SyntaxCacheLimits::default());
    let initial = semantic_cache
        .compile_with_frontend_caches(
            "main.srp",
            &loader,
            CachedCompileOptions::new(&[1, 1, 1, 1]),
            &mut combined_tokens,
            &mut combined_syntax,
        )
        .unwrap();
    assert_eq!(initial.status, CacheStatus::Stored);
    assert_eq!(initial.program.constant_dependencies().len(), 515);
    measure(
        "module_all_caches_warm_4files",
        input_bytes,
        iterations,
        || {
            let compiled = semantic_cache
                .compile_with_frontend_caches(
                    "main.srp",
                    &loader,
                    CachedCompileOptions::new(&[1, 1, 1, 1]),
                    &mut combined_tokens,
                    &mut combined_syntax,
                )
                .unwrap();
            assert_eq!(compiled.status, CacheStatus::Hit);
            black_box(compiled);
        },
    );
    assert!(semantic_cache.stats().hits > 0);
    assert!(combined_syntax.stats().hits >= 4);

    let compile_indexed = || {
        modules::compile_indexed_with_loader(
            "main.srp",
            &loader,
            ModuleLimits::default(),
            Default::default(),
        )
        .unwrap()
    };
    let indexed = compile_indexed();
    assert_eq!(indexed.program().modules().len(), 4);
    for reference in indexed.index().references() {
        assert_eq!(
            indexed
                .index()
                .goto_definition(reference.location.source, reference.location.span.start)
                .unwrap()
                .id,
            reference.target
        );
    }
    measure(
        "indexed_module_constants_4files",
        files.iter().map(|(_, text)| text.len()).sum(),
        iterations,
        || {
            black_box(compile_indexed());
        },
    );
    measure_editor_revision_refresh(&loader, input_bytes, iterations);
    measure("inspection_json_4files", size, iterations, || {
        json.clear();
        program.write_inspection_json(black_box(&mut json)).unwrap();
        black_box(json.len());
    });
    let typed = scarp::compile(&source).unwrap();
    let node_count: usize = typed
        .typed_expressions()
        .map(|expression| expression.nodes().count())
        .sum();
    assert!(node_count > typed.binding_count());
    measure("typed_plan_nodes_512", source.len(), iterations, || {
        let count: usize = typed
            .typed_expressions()
            .map(|expression| {
                expression
                    .nodes()
                    .map(|node| {
                        assert!(typed.type_registry().get(node.kind).is_some());
                        1usize
                    })
                    .sum::<usize>()
            })
            .sum();
        assert_eq!(black_box(count), node_count);
    });
    measure("canonical_signatures_4096", 4096, iterations, || {
        let mut registry = TypeRegistryBuilder::new(TypeLimits::default()).unwrap();
        let signature = || TypeKind::Function {
            parameters: vec![PrimitiveType::Integer.id(), PrimitiveType::Float.id()].into(),
            result: PrimitiveType::Float.id(),
            effect: FunctionEffect::Pure,
        };
        let first = registry.intern(signature()).unwrap();
        for _ in 1..4096 {
            assert_eq!(registry.intern(signature()).unwrap(), first);
        }
        assert_eq!(registry.len(), PrimitiveType::ALL.len() + 1);
        black_box(registry.finish());
    });
}
