use scarp::{
    diagnostics::{Diagnostic, DiagnosticKind},
    modules::{
        self,
        cache::{SyntaxCache, SyntaxCacheLimits},
        MemorySourceLoader, ModuleProgram,
    },
    types::{SourceType, TypeKind, TypeLimits},
    value::ArrayValue,
    ExecutionLimits, ExecutionOptions, RunError,
};

fn run(source: &str) -> Vec<u8> {
    let mut output = Vec::new();
    scarp::run(source, &mut output).unwrap();
    output
}

fn error(source: &str, kind: DiagnosticKind) -> Diagnostic {
    let err = scarp::compile(source).unwrap_err();
    assert_eq!(err.kind, kind, "{err:?}");
    err
}

#[test]
fn task_panel_example_executes_and_matches_golden() {
    assert_eq!(
        run(include_str!("../examples/app_state.srp")),
        include_bytes!("../examples/app_state.stdout"),
    );
}

#[test]
fn nominal_nested_records_share_the_checked_type_registry() {
    let source = r#"
        record State { pair: tuple<record Item, boolean>, empty: array<record Item, 0> }
        record Item { value: decimal }
        pure funcra make(item: record Item = new Item { value: decimal("1.25") }) -> record State {
            return new State { empty: [array: <record Item>], pair: (item, true) };
        }
        bind state: record State = make();
        print(state->pair[0]->value);
        print(state);
    "#;
    let compiled = scarp::compile(source).unwrap();
    let id = compiled.binding_type_ids()[0];
    let layout = compiled.binding_types()[0].as_record().unwrap();
    assert_eq!(layout.type_id(), id);
    assert_eq!(layout.fields()[0].0, "pair");
    assert!(
        matches!(compiled.type_registry().get(id), Some(TypeKind::Record { fields, .. }) if fields.len() == 2)
    );
    assert_eq!(
        run(source),
        b"1.25\nnew State {pair: (new Item {value: 1.25}, true), empty: [array: <record Item>]}\n",
    );
}

#[test]
fn construction_uses_written_field_evaluation_order_and_declared_storage_order() {
    let source = r#"
        record Pair { first: string, second: string }
        bind pair = new Pair { second: read_line(), first: read_line() };
        print(pair);
    "#;
    let mut output = Vec::new();
    scarp::run_with_input(
        source,
        &mut &b"second\nfirst\n"[..],
        &mut output,
        &mut Vec::new(),
    )
    .unwrap();
    assert_eq!(output, b"new Pair {first: \"first\", second: \"second\"}\n");
}

#[test]
fn lookup_and_construction_diagnostics_point_at_the_offending_bytes() {
    let source = "// 🌍\nrecord A { x: integer } bind obj = new A { x: 1 }; print(obj->oops);";
    let err = error(source, DiagnosticKind::RecordField);
    assert_eq!(&source[err.span.start..err.span.end], "oops");
    let source = "record A { x: integer } bind obj = new A { x: 1, x: 2 };";
    let err = error(source, DiagnosticKind::RecordField);
    assert_eq!(&source[err.span.start..err.span.end], "x");
    assert!(err.related[0].span.start < err.span.start);
    let source = "record A { x: integer } bind obj = new A { x: \"wrong\" };";
    let err = error(source, DiagnosticKind::TypeMismatch);
    assert_eq!(&source[err.span.start..err.span.end], "\"wrong\"");
    let err = error(
        "record A { x: integer } bind obj = new A {};",
        DiagnosticKind::RecordField,
    );
    assert!(err.message.contains("missing field"));
    error(
        "record A { x: integer, x: integer }",
        DiagnosticKind::RecordField,
    );
    error(
        "bind thing = new Missing {};",
        DiagnosticKind::RecordDefinition,
    );
    error("print(1->x);", DiagnosticKind::RecordField);
    error("{ record Inside {} }", DiagnosticKind::RecordDefinition);
    error(
        "record A {} var x = new A {}; x->value = 1;",
        DiagnosticKind::ImmutableAssignment,
    );
}

#[test]
fn nominal_mismatch_cycles_and_expanded_arrays_are_rejected() {
    error(
        "record A { x: integer } record B { x: integer } bind x: record A = new B { x: 1 };",
        DiagnosticKind::TypeMismatch,
    );
    error(
        "record A { b: record B } record B { a: array<record A, 0> }",
        DiagnosticKind::RecordDefinition,
    );
    error(
        "record Leaf { nums: array<integer, 4096> } record Big { leaves: array<record Leaf, 16> }",
        DiagnosticKind::RecordDefinition,
    );
    let mut long = String::new();
    for n in 0..40 {
        long.push_str(&format!("record R{n} {{ value: record R{} }}\n", n + 1));
    }
    long.push_str("record R40 {}");
    error(&long, DiagnosticKind::RecordDefinition);
    assert!(scarp::compile_with_type_limits(
        "record Leaf {}",
        TypeLimits {
            max_types: 10,
            ..TypeLimits::default()
        },
    )
    .is_err());
    let unresolved = SourceType::RecordRef("Unknown".into());
    assert!(ArrayValue::new(unresolved, Vec::new()).is_none());
}

#[test]
fn bounded_construction_and_capture_are_atomic() {
    let source = "record A { x: integer } print(1); bind a = new A { x: 2 }; print(a);";
    let mut output = Vec::new();
    let options = ExecutionOptions {
        max_tuple_bytes: 0,
        ..ExecutionOptions::default()
    };
    assert!(matches!(
        scarp::run_with_options(source, &mut output, options),
        Err(RunError::Diagnostic(err)) if err.kind == DiagnosticKind::RecordAllocation
    ));
    assert!(output.is_empty());
    let options = ExecutionOptions {
        limits: ExecutionLimits {
            max_steps: 2,
            ..ExecutionLimits::default()
        },
        ..ExecutionOptions::default()
    };
    assert!(scarp::run_with_options(source, &mut output, options).is_err());
    assert!(output.is_empty());
    let options = ExecutionOptions {
        max_capture_bytes: 1,
        ..ExecutionOptions::default()
    };
    assert!(scarp::run_with_options(source, &mut output, options).is_err());
    assert!(output.is_empty());
    assert_eq!(run(source), b"1\nnew A {x: 2}\n");
}

const LIBRARY: &str = r#"
    // 🧭
    record Private { n: integer }
    pub record Panel { title: string }
    pub pure funcra update(panel: record Panel) -> record Panel {
        bind private = new Private { n: 1 };
        if private->n > 0 { return new Panel { title: "new" }; }
        return panel;
    }
"#;

fn with_library(entry: &str, library: &str) -> MemorySourceLoader {
    let mut loader = MemorySourceLoader::new();
    loader.insert("main.srp", entry).unwrap();
    loader.insert("panel.srp", library).unwrap();
    loader
}

fn module_output(program: &ModuleProgram) -> Vec<u8> {
    let mut out = Vec::new();
    program
        .execute(&mut out, &mut Vec::new(), Default::default())
        .unwrap();
    out
}

#[test]
fn public_module_records_rebase_and_link_under_shifted_syntax_cache_hits() {
    let mut cache = SyntaxCache::new(SyntaxCacheLimits::default());
    let program = r#"import "panel.srp" as ui;
        bind old: record ui.Panel = new ui.Panel { title: "old" };
        print(ui.update(old)->title);"#;
    for prefix in ["", "// shifted 🌍\n"] {
        let source = format!("{prefix}{program}");
        let loader = with_library(&source, LIBRARY);
        let graph =
            modules::load_with_syntax_cache("main.srp", &loader, Default::default(), &mut cache)
                .unwrap();
        let indexed = modules::resolve_indexed(graph, Default::default()).unwrap();
        assert_eq!(module_output(indexed.program()), b"new\n");
        assert_eq!(indexed.program().inspection_schema_version(), 4);
        let mut json = Vec::new();
        indexed.program().write_inspection_json(&mut json).unwrap();
        let json = String::from_utf8(json).unwrap();
        assert!(json.contains(r#""kind":"record","fields":[{"name":"title","type":"string"}]"#));
        let offset = source.find("new ui.Panel").unwrap() + "new ui.".len();
        let definition = indexed.index().goto_definition(0, offset).unwrap();
        assert_eq!(definition.name.as_ref(), "Panel");
    }
    assert!(cache.stats().hits >= 1);
}

#[test]
fn private_records_cannot_leak_through_construction_or_exported_signatures() {
    for (entry, library) in [
        (
            r#"import "panel.srp" as ui; bind x = new ui.Private { n: 1 };"#,
            LIBRARY,
        ),
        (
            r#"import "panel.srp" as ui;"#,
            "record Secret {} pub record Public { x: record Secret }",
        ),
        (
            r#"import "panel.srp" as ui;"#,
            "record Secret {} pub funcra reveal() -> record Secret { return new Secret {}; }",
        ),
    ] {
        let loader = with_library(entry, library);
        let err =
            modules::compile_with_loader("main.srp", &loader, Default::default()).unwrap_err();
        assert_eq!(err.diagnostic.kind, DiagnosticKind::ModuleVisibility);
    }
}

#[test]
fn native_backends_reject_record_operations_and_keywords_remain_contextual() {
    let source = "record Empty {} bind value = new Empty {};";
    assert_eq!(
        scarp::c_backend::emit_c(source).unwrap_err().kind.code(),
        "E0049"
    );
    assert_eq!(
        scarp::native_object::emit_elf64_integer_kernel(source)
            .unwrap_err()
            .kind
            .code(),
        "E0049"
    );
    assert_eq!(
        run("bind new = true; var record = false; record = new and true; print(record);"),
        b"true\n"
    );
}

#[test]
fn identically_named_records_in_two_modules_are_distinct() {
    let source = r#"import "a.srp" as a; import "b.srp" as b;
        bind item: record a.Panel = new b.Panel { count: 1 };"#;
    let mut loader = MemorySourceLoader::new();
    loader.insert("main.srp", source).unwrap();
    for path in ["a.srp", "b.srp"] {
        loader
            .insert(path, "pub record Panel { count: integer }")
            .unwrap();
    }
    let err = modules::compile_with_loader("main.srp", &loader, Default::default()).unwrap_err();
    assert_eq!(err.diagnostic.kind, DiagnosticKind::TypeMismatch);
}

#[test]
fn record_resolution_rejects_mixed_deep_types_on_small_stack() {
    let mut source = String::new();
    for i in 0..32 {
        source.push_str(&format!(
            "record R{i} {{ field: {}record R{}{} }}\n",
            "tuple<".repeat(14),
            i + 1,
            ">".repeat(14)
        ));
    }
    source.push_str("record R32 {}");
    let program = scarp::parse(&source).unwrap();
    std::thread::Builder::new()
        .stack_size(512 * 1024)
        .spawn(move || {
            let err = scarp::resolver::resolve(program).unwrap_err();
            assert_eq!(err.kind, DiagnosticKind::RecordDefinition);
        })
        .unwrap()
        .join()
        .unwrap();
}
