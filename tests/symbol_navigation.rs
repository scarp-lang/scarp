use scarp::{
    diagnostics::DiagnosticKind as D,
    modules::{
        self,
        symbols::{IndexLimits, IndexedProgram, ReferenceKind as R, SymbolKind as K},
        MemorySourceLoader, ModuleLimits,
    },
};
fn compile(files: &[(&str, &str)]) -> IndexedProgram {
    let mut loader = MemorySourceLoader::new();
    for &(name, source) in files {
        loader.insert(name, source).unwrap();
    }
    modules::compile_indexed_with_loader(
        files[0].0,
        &loader,
        ModuleLimits::default(),
        IndexLimits::default(),
    )
    .unwrap()
}
fn text(p: &IndexedProgram, l: modules::SourceLocation) -> &str {
    &p.index().sources().source(l.source).unwrap()[l.span.start..l.span.end]
}
#[test]
fn locals_shadowing_initializers_and_assignments_follow_lexical_identity() {
    let source = "var x=1; {var x=x+1; x+=2; print(x);} x=3; print(x);";
    let p = compile(&[("main.srp", source)]);
    let index = p.index();
    let defs: Vec<_> = index
        .symbols()
        .iter()
        .filter(|d| d.name.as_ref() == "x")
        .collect();
    assert_eq!(defs.len(), 2);
    assert!(defs.iter().all(|d| d.kind == K::Binding && d.mutable));
    let outer: Vec<_> = index.find_references(defs[0].id).collect();
    let inner: Vec<_> = index.find_references(defs[1].id).collect();
    assert_eq!(
        outer.iter().map(|r| r.kind).collect::<Vec<_>>(),
        vec![R::Read, R::Write, R::Read]
    );
    assert_eq!(
        inner.iter().map(|r| r.kind).collect::<Vec<_>>(),
        vec![R::ReadWrite, R::Read]
    );
    for d in defs {
        let l = d.location.unwrap();
        assert_eq!(text(&p, l), "x");
        assert_eq!(
            index.goto_definition(l.source, l.span.start).unwrap().id,
            d.id
        );
    }
    for r in index.references() {
        assert_eq!(
            index
                .goto_definition(r.location.source, r.location.span.start)
                .unwrap()
                .id,
            r.target
        );
    }
    let mut out = Vec::new();
    p.program()
        .execute(&mut out, &mut Vec::new(), Default::default())
        .unwrap();
    assert_eq!(out, b"4\n3\n");
}
#[test]
fn imports_keep_alias_member_and_file_navigation_distinct_including_spaced_dots() {
    let p = compile(&[
        (
            "main.srp",
            "import \"lib.srp\" as lib; print(lib . f(n: lib . value));",
        ),
        (
            "lib.srp",
            "pub bind value=2; pub pure funcra f(n: integer)->integer{return n;}",
        ),
    ]);
    let index = p.index();
    for r in index.references() {
        let d = index.symbol(r.target).unwrap();
        match r.kind {
            R::Namespace => {
                assert_eq!(text(&p, r.location), "lib");
                assert_eq!(d.kind, K::ImportAlias);
                assert_eq!(d.target_module, Some(1));
            }
            R::Import => {
                assert_eq!(text(&p, r.location), "\"lib.srp\"");
                assert_eq!(d.kind, K::Module);
                assert_eq!(d.location.unwrap().source, 1);
            }
            R::Call => {
                assert_eq!(text(&p, r.location), "f");
                assert_eq!(d.location.unwrap().source, 1);
            }
            R::ArgumentLabel => {
                assert_eq!(text(&p, r.location), "n");
                assert_eq!(d.kind, K::Parameter);
                assert_eq!(d.location.unwrap().source, 1);
            }
            _ => {}
        }
        assert_eq!(
            index
                .goto_definition(r.location.source, r.location.span.start)
                .unwrap()
                .id,
            r.target
        );
    }
    assert!(index
        .references()
        .iter()
        .any(|r| r.kind == R::Read && text(&p, r.location) == "value"));
    let source = index.sources().source(0).unwrap();
    assert!(index
        .goto_definition(0, source.find(" . ").unwrap() + 1)
        .is_none());
}
#[test]
fn constant_initializers_and_folded_runtime_reads_retain_every_reference() {
    let p = compile(&[
        (
            "main.srp",
            "import \"lib.srp\" as l; pub bind a=l.n+1; pub bind b=a*2; print(b,l.n);",
        ),
        ("lib.srp", "bind private=2; pub bind n=private+1;"),
    ]);
    let index = p.index();
    for name in ["a", "b", "n", "private"] {
        let d = index
            .symbols()
            .iter()
            .find(|d| d.name.as_ref() == name)
            .unwrap();
        assert_eq!(d.kind, K::Constant);
        assert!(index.find_references(d.id).count() >= 1, "{name}");
    }
    let n = index
        .symbols()
        .iter()
        .find(|d| d.name.as_ref() == "n")
        .unwrap();
    assert_eq!(index.find_references(n.id).count(), 2);
    let mut out = Vec::new();
    p.program()
        .execute(&mut out, &mut Vec::new(), Default::default())
        .unwrap();
    assert_eq!(out, b"8 3\n");
}
#[test]
fn defaults_contracts_result_and_body_parameters_have_correct_scopes_without_execution() {
    let p=compile(&[("main.srp","pure funcra f(a: integer=1/0,b: integer=a)->integer needs b>0; ensures result==b; ensures result>=a; {bind a=b; return a;} print(f());")]);
    let i = p.index();
    let parameters: Vec<_> = i
        .symbols()
        .iter()
        .filter(|d| d.kind == K::Parameter)
        .collect();
    assert_eq!(parameters.len(), 2);
    let a = parameters[0];
    assert_eq!(i.find_references(a.id).count(), 2); // default b and postcondition
    let result = i
        .symbols()
        .iter()
        .find(|d| d.kind == K::ContractResult)
        .unwrap();
    assert!(result.location.is_none());
    assert!(result.owner.is_some());
    assert_eq!(i.find_references(result.id).count(), 2);
    let local = i
        .symbols()
        .iter()
        .find(|d| d.kind == K::Binding && d.name.as_ref() == "a")
        .unwrap();
    assert_eq!(i.find_references(local.id).count(), 1);
    assert!(p.program().check(Default::default()).is_err());
}
#[test]
fn loop_indices_do_not_capture_counts_and_each_nested_scope_remains_distinct() {
    let p = compile(&[(
        "main.srp",
        "bind n=2; repeat n as n {repeat n as n {print(n);} print(n);} print(n);",
    )]);
    let i = p.index();
    let defs: Vec<_> = i
        .symbols()
        .iter()
        .filter(|d| d.name.as_ref() == "n")
        .collect();
    assert_eq!(defs.len(), 3);
    assert_eq!(defs[0].kind, K::Binding);
    assert_eq!(defs[1].kind, K::RepeatIndex);
    assert_eq!(defs[2].kind, K::RepeatIndex);
    assert_eq!(i.find_references(defs[0].id).count(), 2);
    assert_eq!(i.find_references(defs[1].id).count(), 2);
    assert_eq!(i.find_references(defs[2].id).count(), 1);
}
#[test]
fn recursive_and_forward_calls_link_actual_definitions_not_signature_types() {
    let p=compile(&[("main.srp","pure funcra a(n: integer)->integer{if n==0{return 0;} return b(n-1);} pure funcra b(m: integer)->integer{if m==0{return 0;} return a(m-1);} print(a(n:3));")]);
    let i = p.index();
    let a = i
        .symbols()
        .iter()
        .find(|d| d.kind == K::Function && d.name.as_ref() == "a")
        .unwrap();
    let b = i
        .symbols()
        .iter()
        .find(|d| d.kind == K::Function && d.name.as_ref() == "b")
        .unwrap();
    assert_ne!(a.id, b.id);
    assert_eq!(i.find_references(a.id).count(), 2);
    assert_eq!(i.find_references(b.id).count(), 1);
}
#[test]
fn builtin_calls_have_no_invented_source_declaration_and_shadowing_is_module_local() {
    let p=compile(&[("main.srp","import \"lib.srp\" as l; pure funcra sum(n: integer)->integer{return n;} print(sum(1),l.f());"),("lib.srp","pub pure funcra f()->float{return sum([1.0]);}")]);
    let i = p.index();
    let sums: Vec<_> = i
        .symbols()
        .iter()
        .filter(|d| d.name.as_ref() == "sum")
        .collect();
    assert_eq!(sums.len(), 2);
    assert!(sums
        .iter()
        .any(|d| d.kind == K::Builtin && d.location.is_none()));
    assert!(sums
        .iter()
        .any(|d| d.kind == K::Function && d.location.is_some()));
    let p = compile(&[("main.srp", "pub bind sum=2; print(sum,sum([1.0]));")]);
    assert!(p
        .index()
        .symbols()
        .iter()
        .any(|d| d.kind == K::Builtin && d.name.as_ref() == "sum"));
}
#[test]
fn bom_crlf_unicode_positions_and_invalid_offsets_use_original_source_bytes() {
    let source = "\u{feff}// é🌍\r\nvar x=1; print(x);";
    let p = compile(&[("café.srp", source)]);
    let i = p.index();
    let r = i.references().iter().find(|r| r.kind == R::Read).unwrap();
    assert_eq!(r.location.span.start, source.rfind('x').unwrap());
    assert_eq!(r.location.start.line, 2);
    assert_eq!(r.location.start.column, 16);
    assert_eq!(text(&p, r.location), "x");
    for offset in [
        1,
        2,
        source.find('é').unwrap() + 1,
        source.len(),
        usize::MAX,
    ] {
        assert!(i.goto_definition(0, offset).is_none());
    }
    assert!(i.goto_definition(999, 0).is_none());
}
#[test]
fn empty_file_is_a_module_with_no_clickable_invented_identifier() {
    let p = compile(&[("main.srp", "")]);
    assert_eq!(p.index().symbols().len(), 1);
    assert!(p.index().references().is_empty());
    assert!(p.index().goto_definition(0, 0).is_none());
}
#[test]
fn index_limits_are_opt_in_and_compiler_failures_take_precedence() {
    let mut loader = MemorySourceLoader::new();
    loader.insert("main.srp", "bind x=1; print(x);").unwrap();
    modules::compile_with_loader("main.srp", &loader, ModuleLimits::default()).unwrap();
    for limits in [
        IndexLimits {
            max_symbols: 0,
            ..Default::default()
        },
        IndexLimits {
            max_references: 0,
            ..Default::default()
        },
        IndexLimits {
            max_name_bytes: 0,
            ..Default::default()
        },
        IndexLimits {
            max_work: 0,
            ..Default::default()
        },
    ] {
        let err = modules::compile_indexed_with_loader(
            "main.srp",
            &loader,
            ModuleLimits::default(),
            limits,
        )
        .unwrap_err();
        assert_eq!(err.diagnostic.kind, D::SymbolIndexLimit);
        assert!(err.sources.locate_span(err.diagnostic.span).is_some());
    }
    let mut loader = MemorySourceLoader::new();
    loader.insert("main.srp", "print(unknown);").unwrap();
    let err = modules::compile_indexed_with_loader(
        "main.srp",
        &loader,
        ModuleLimits::default(),
        IndexLimits {
            max_work: 0,
            ..Default::default()
        },
    )
    .unwrap_err();
    assert_eq!(err.diagnostic.kind, D::UnknownName);
}
#[test]
fn invalid_private_cyclic_and_incomplete_graphs_return_existing_errors() {
    for files in [
        vec![
            ("main.srp", "import \"lib.srp\" as l; print(l.secret);"),
            ("lib.srp", "bind secret=1;"),
        ],
        vec![("main.srp", "import \"missing.srp\" as l;")],
        vec![
            ("main.srp", "import \"lib.srp\" as l;"),
            ("lib.srp", "import \"main.srp\" as m;"),
        ],
        vec![("main.srp", "bind x=;")],
        vec![("main.srp", "bind x=1; bind x=2;")],
        vec![(
            "main.srp",
            "pure funcra f(n: integer)->integer{return false;}",
        )],
    ] {
        let mut loader = MemorySourceLoader::new();
        for (name, source) in files {
            loader.insert(name, source).unwrap();
        }
        let normal =
            modules::compile_with_loader("main.srp", &loader, ModuleLimits::default()).unwrap_err();
        let indexed = modules::compile_indexed_with_loader(
            "main.srp",
            &loader,
            ModuleLimits::default(),
            Default::default(),
        )
        .unwrap_err();
        assert_eq!(normal.diagnostic, indexed.diagnostic);
    }
}
#[test]
fn repeated_and_concurrent_indexes_are_immutable_and_loader_edits_rebuild() {
    let source = "bind x=1; print(x);";
    let p = compile(&[("main.srp", source)]);
    let index = p.index().clone();
    let threads: Vec<_> = (0..4)
        .map(|_| {
            let i = index.clone();
            std::thread::spawn(move || (i.symbols().to_vec(), i.references().to_vec()))
        })
        .collect();
    for t in threads {
        let (s, r) = t.join().unwrap();
        assert_eq!(s, index.symbols());
        assert_eq!(r, index.references());
    }
    let again = compile(&[("main.srp", source)]);
    assert_eq!(index.symbols(), again.index().symbols());
    assert_eq!(index.references(), again.index().references());
    let mut loader = MemorySourceLoader::new();
    loader.insert("main.srp", source).unwrap();
    let old = modules::compile_indexed_with_loader(
        "main.srp",
        &loader,
        Default::default(),
        Default::default(),
    )
    .unwrap();
    let mut loader = MemorySourceLoader::new();
    loader.insert("main.srp", "bind y=2; print(y);").unwrap();
    let new = modules::compile_indexed_with_loader(
        "main.srp",
        &loader,
        Default::default(),
        Default::default(),
    )
    .unwrap();
    assert!(new.index().symbols().iter().all(|d| d.name.as_ref() != "x"));
    assert!(old.index().symbols().iter().any(|d| d.name.as_ref() == "x"));
}

#[test]
fn exact_index_caps_report_measured_logical_counts_and_keep_execution_separate() {
    let p = compile(&[("main.srp", "bind a=1; print(a);")]);
    let used = p.index().usage();
    assert_eq!(used.symbols, 2);
    assert_eq!(used.references, 1);
    assert_eq!(used.name_bytes, "main.srp".len() + 1);
    let mut loader = MemorySourceLoader::new();
    loader.insert("main.srp", "bind a=1; print(a);").unwrap();
    let exact = IndexLimits {
        max_symbols: used.symbols,
        max_references: used.references,
        max_name_bytes: used.name_bytes,
        max_work: used.work,
    };
    modules::compile_indexed_with_loader("main.srp", &loader, Default::default(), exact).unwrap();
    for limits in [
        IndexLimits {
            max_symbols: used.symbols - 1,
            ..exact
        },
        IndexLimits {
            max_references: used.references - 1,
            ..exact
        },
        IndexLimits {
            max_name_bytes: used.name_bytes - 1,
            ..exact
        },
        IndexLimits {
            max_work: used.work - 1,
            ..exact
        },
    ] {
        assert_eq!(
            modules::compile_indexed_with_loader("main.srp", &loader, Default::default(), limits)
                .unwrap_err()
                .diagnostic
                .kind,
            D::SymbolIndexLimit
        );
    }
    p.program()
        .check(scarp::ExecutionOptions {
            limits: scarp::ExecutionLimits {
                max_steps: 0,
                ..Default::default()
            },
            ..Default::default()
        })
        .unwrap_err();
}
#[test]
fn default_parameter_self_name_uses_outer_constant_and_result_roles_do_not_alias() {
    let p=compile(&[("main.srp","pub bind n=2; pub bind result=1; pure funcra f(n: integer=n)->integer needs result==1; ensures result==n; {bind result=n; return result;} print(f());")]);
    let i = p.index();
    let module_n = i
        .symbols()
        .iter()
        .find(|d| d.name.as_ref() == "n" && d.kind == K::Constant)
        .unwrap();
    assert_eq!(i.find_references(module_n.id).count(), 1);
    let results: Vec<_> = i
        .symbols()
        .iter()
        .filter(|d| d.name.as_ref() == "result")
        .collect();
    assert_eq!(results.len(), 3);
    for d in results {
        assert_eq!(i.find_references(d.id).count(), 1);
    }
}
#[test]
fn large_flat_expressions_are_indexed_without_recursive_expression_visitors() {
    let mut source = String::from("bind x=1; print(");
    for n in 0..10_000 {
        if n != 0 {
            source.push('+');
        }
        source.push('x');
    }
    source.push_str(");");
    let p = compile(&[("main.srp", &source)]);
    assert_eq!(p.index().references().len(), 10_000);
    let id = p.index().references()[0].target;
    assert_eq!(p.index().find_references(id).count(), 10_000);
}
#[test]
fn loader_callbacks_are_not_repeated_and_removed_imports_do_not_return_cached_indexes() {
    use modules::{SourceLoadError, SourceLoader};
    use std::cell::{Cell, RefCell};
    struct Live {
        loader: RefCell<MemorySourceLoader>,
        reads: Cell<usize>,
    }
    impl SourceLoader for Live {
        fn resolve(
            &self,
            importer: Option<&str>,
            specifier: &str,
        ) -> Result<String, SourceLoadError> {
            self.loader.borrow().resolve(importer, specifier)
        }
        fn load(&self, identity: &str, max: usize) -> Result<String, SourceLoadError> {
            self.reads.set(self.reads.get() + 1);
            self.loader.borrow().load(identity, max)
        }
    }
    let mut first = MemorySourceLoader::new();
    first
        .insert("main.srp", "import \"lib.srp\" as l; print(l.x);")
        .unwrap();
    first.insert("lib.srp", "pub bind x=1;").unwrap();
    let live = Live {
        loader: RefCell::new(first),
        reads: Cell::new(0),
    };
    let old = modules::compile_indexed_with_loader(
        "main.srp",
        &live,
        Default::default(),
        Default::default(),
    )
    .unwrap();
    assert_eq!(live.reads.get(), 2);
    let mut updated = MemorySourceLoader::new();
    updated
        .insert("main.srp", "import \"lib.srp\" as l; print(l.y);")
        .unwrap();
    updated.insert("lib.srp", "pub bind y=2;").unwrap();
    live.loader.replace(updated);
    let new = modules::compile_indexed_with_loader(
        "main.srp",
        &live,
        Default::default(),
        Default::default(),
    )
    .unwrap();
    assert_eq!(live.reads.get(), 4);
    assert!(new.index().symbols().iter().all(|d| d.name.as_ref() != "x"));
    assert!(old.index().symbols().iter().any(|d| d.name.as_ref() == "x"));
    let mut removed = MemorySourceLoader::new();
    removed
        .insert("main.srp", "import \"lib.srp\" as l;")
        .unwrap();
    live.loader.replace(removed);
    assert!(modules::compile_indexed_with_loader(
        "main.srp",
        &live,
        Default::default(),
        Default::default()
    )
    .is_err());
}
#[test]
fn diamond_imports_deduplicate_definitions_and_keep_alias_sites_source_owned() {
    let p = compile(&[
        (
            "main.srp",
            "import \"a.srp\" as a; import \"b.srp\" as b; print(a.x,b.x);",
        ),
        ("a.srp", "import \"base.srp\" as base; pub bind x=base.x;"),
        ("b.srp", "import \"base.srp\" as base; pub bind x=base.x;"),
        ("base.srp", "pub bind x=1;"),
    ]);
    assert_eq!(p.index().sources().files().len(), 4);
    assert_eq!(
        p.index()
            .symbols()
            .iter()
            .filter(|d| d.kind == K::Module)
            .count(),
        4
    );
    let base = p
        .index()
        .symbols()
        .iter()
        .find(|d| d.kind == K::Constant && d.location.unwrap().source == 2)
        .unwrap();
    assert_eq!(p.index().find_references(base.id).count(), 2);
}
#[test]
fn json_streams_are_deterministic_and_writer_errors_preserve_io_kind() {
    let p = compile(&[("main.srp", "bind x=1; print(x);")]);
    let mut first = Vec::new();
    p.index().write_references_json(&mut first).unwrap();
    let mut again = Vec::new();
    p.index().write_references_json(&mut again).unwrap();
    assert_eq!(first, again);
    struct Limited(usize);
    impl std::io::Write for Limited {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            if self.0 == 0 {
                return Err(std::io::ErrorKind::PermissionDenied.into());
            }
            let n = self.0.min(bytes.len());
            self.0 -= n;
            Ok(n)
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    assert_eq!(
        p.index()
            .write_references_json(&mut Limited(7))
            .unwrap_err()
            .kind(),
        std::io::ErrorKind::PermissionDenied
    );
    assert_eq!(
        p.index()
            .write_symbols_json(&mut Limited(0))
            .unwrap_err()
            .kind(),
        std::io::ErrorKind::PermissionDenied
    );
}

#[test]
fn schema_snapshots_and_legacy_module_metadata_are_exact() {
    let source = include_str!("../examples/symbol_navigation.srp");
    let library = include_str!("../examples/modules/navigation_values.srp");
    let p = compile(&[
        ("symbol_navigation.srp", source),
        ("modules/navigation_values.srp", library),
    ]);
    let mut out = Vec::new();
    p.index().write_symbols_json(&mut out).unwrap();
    assert_eq!(
        out,
        include_bytes!("../examples/symbol_navigation.symbols.json")
    );
    out.clear();
    p.index().write_references_json(&mut out).unwrap();
    assert_eq!(
        out,
        include_bytes!("../examples/symbol_navigation.references.json")
    );
    let mut loader = MemorySourceLoader::new();
    loader.insert("symbol_navigation.srp", source).unwrap();
    loader
        .insert("modules/navigation_values.srp", library)
        .unwrap();
    let normal =
        modules::compile_with_loader("symbol_navigation.srp", &loader, Default::default()).unwrap();
    let mut expected = Vec::new();
    normal.write_inspection_json(&mut expected).unwrap();
    out.clear();
    p.program().write_inspection_json(&mut out).unwrap();
    assert_eq!(out, expected);
    assert_eq!(
        format!("{:?}", normal.program()),
        format!("{:?}", p.program().program())
    );
    out.clear();
    p.program()
        .execute(&mut out, &mut Vec::new(), Default::default())
        .unwrap();
    assert_eq!(out, include_bytes!("../examples/symbol_navigation.stdout"));
}
#[test]
fn indexed_embedding_retains_custom_constant_and_type_limits() {
    let mut loader = MemorySourceLoader::new();
    loader
        .insert(
            "main.srp",
            "pub bind x=1+2; pure funcra f()->integer{return x;}",
        )
        .unwrap();
    let error = modules::resolve_indexed_with_compiler_limits(
        modules::load("main.srp", &loader, Default::default()).unwrap(),
        modules::ConstantLimits {
            max_nodes: 0,
            ..Default::default()
        },
        Default::default(),
        Default::default(),
    )
    .unwrap_err();
    let normal = modules::resolve_with_constant_limits(
        modules::load("main.srp", &loader, Default::default()).unwrap(),
        modules::ConstantLimits {
            max_nodes: 0,
            ..Default::default()
        },
    )
    .unwrap_err();
    assert_eq!(error.diagnostic, normal.diagnostic);
    let error = modules::resolve_indexed_with_compiler_limits(
        modules::load("main.srp", &loader, Default::default()).unwrap(),
        Default::default(),
        scarp::types::TypeLimits {
            max_types: 8,
            ..Default::default()
        },
        Default::default(),
    )
    .unwrap_err();
    assert_eq!(error.diagnostic.kind, D::TypeComplexity);
}
