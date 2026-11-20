use scarp::{
    diagnostics::{Diagnostic, DiagnosticKind},
    modules::{
        self,
        cache::{SyntaxCache, SyntaxCacheLimits},
        MemorySourceLoader,
    },
    types::{TypeKind, TypeLimits},
    ExecutionLimits, ExecutionOptions, RunError,
};

fn run(source: &str) -> Vec<u8> {
    let mut output = Vec::new();
    scarp::run(source, &mut output).unwrap();
    output
}

fn error(source: &str, kind: DiagnosticKind) -> Diagnostic {
    let error = scarp::compile(source).unwrap_err();
    assert_eq!(error.kind, kind, "{error:?}");
    error
}

#[test]
fn typed_submission_example_matches_golden() {
    assert_eq!(
        run(include_str!("../examples/variant_validation.srp")),
        include_bytes!("../examples/variant_validation.stdout"),
    );
}

#[test]
fn variant_has_nominal_registry_identity_and_a_typed_case_table() {
    let source = r#"
        record Item { state: variant State }
        variant State { Empty, Number(integer), Label(string) }
        bind x: record Item = new Item { state: new State::Label("ready") };
        print(x->state);
        print(new State::Empty() == new State::Empty());
        print(new State::Number(2) == new State::Number(3));
        print(new State::Label("ready") == new State::Label("ready"));
        print(new State::Empty() == new State::Label("ready"));
    "#;
    let compiled = scarp::compile(source).unwrap();
    let record = compiled.binding_types()[0].as_record().unwrap();
    let variant = record.fields()[0].1.as_variant().unwrap();
    assert_eq!(variant.name(), "State");
    assert_eq!(variant.cases().len(), 3);
    let Some(TypeKind::Variant { cases, .. }) = compiled.type_registry().get(variant.type_id())
    else {
        panic!("missing variant descriptor");
    };
    assert!(cases[0].is_none() && cases[1].is_some() && cases[2].is_some());
    assert_eq!(
        run(source),
        b"new State::Label(\"ready\")\ntrue\nfalse\ntrue\nfalse\n"
    );
}

#[test]
fn nested_values_work_in_tuples_fixed_arrays_defaults_and_contracts() {
    let source = r#"
        variant Payload { Empty, Ready(array<variant Status, 1>) }
        variant Status { Count(integer) }
        pure funcra wrap(n: integer = 8) -> tuple<variant Payload, array<variant Status, 0>> {
            return (new Payload::Ready([array: new Status::Count(n)]), [array: <variant Status>]);
        }
        pure funcra accepted(value: variant Status) -> boolean
            needs value == new Status::Count(8);
        {
            match on value {
                case Count(n) { return n == 8; }
            }
        }
        bind (p, items) = wrap();
        match on p {
            case Empty { print("missing"); }
            case Ready(group) {
                print(accepted(group[0]));
                match on group[0] { case Count(n) { print(n); } }
            }
        }
        print(items);
    "#;
    assert_eq!(run(source), b"true\n8\n[array: <variant Status>]\n");
}

#[test]
fn constructor_errors_use_case_and_payload_source_spans() {
    for (expression, expected, at) in [
        ("new State::Bad()", DiagnosticKind::VariantCase, "Bad"),
        ("new State::Ready()", DiagnosticKind::VariantCase, "Ready"),
        ("new State::Empty(2)", DiagnosticKind::VariantCase, "Empty"),
        (
            "new State::Ready(\"x\")",
            DiagnosticKind::TypeMismatch,
            "\"x\"",
        ),
    ] {
        let source =
            format!("// 🌍\nvariant State {{ Empty, Ready(integer) }} bind v = {expression};");
        let diagnostic = error(&source, expected);
        assert_eq!(&source[diagnostic.span.start..diagnostic.span.end], at);
    }
    error(
        "bind value = new Missing::None();",
        DiagnosticKind::VariantDefinition,
    );
    error(
        "record State {} bind value = new State::None();",
        DiagnosticKind::VariantDefinition,
    );
    error("variant Empty {}", DiagnosticKind::VariantDefinition);
    error("variant A { _ }", DiagnosticKind::VariantCase);
    error(
        "variant A { Ready, Ready(string) }",
        DiagnosticKind::VariantCase,
    );
}

#[test]
fn coverage_rejects_missing_duplicate_and_unreachable_cases() {
    let prefix =
        "variant Outcome { Empty, Count(integer), Error(string) } bind v = new Outcome::Empty();";
    for (arms, message) in [
        ("case Empty {}", "missing Count, Error"),
        ("case Empty {} case Empty {} case _ {}", "duplicate case"),
        ("case _ {} case Empty {}", "unreachable after the wildcard"),
        (
            "case Empty {} case Count(_) {} case Error(_) {} case _ {}",
            "wildcard is unreachable",
        ),
    ] {
        let source = format!("{prefix} match on v {{ {arms} }}");
        assert!(error(&source, DiagnosticKind::MatchCoverage)
            .message
            .contains(message));
    }
    for (arms, message) in [
        ("case Wrong {} case _ {}", "has no case"),
        ("case Count {} case _ {}", "use a payload binding"),
        ("case Empty(a) {} case _ {}", "has no payload"),
        ("case _(a) {}", "cannot bind a payload"),
    ] {
        let source = format!("{prefix} match on v {{ {arms} }}");
        assert!(error(&source, DiagnosticKind::VariantCase)
            .message
            .contains(message));
    }
    assert_eq!(
        run(&format!(
            "{prefix} match on v {{ case Count(_) {{ print(0); }} case _ {{ print(9); }} }}"
        )),
        b"9\n"
    );
}

#[test]
fn only_the_selected_arm_runs_and_the_subject_is_evaluated_once() {
    let source = r#"
        variant Field { Invalid, Text(string) }
        funcra read() -> variant Field {
            return new Field::Text(read_line());
        }
        match on read() {
            case Invalid { print(1 / 0); }
            case Text(value) { print(value); }
        }
        print(read_line());
    "#;
    let mut output = Vec::new();
    scarp::run_with_input(
        source,
        &mut &b"once\nremaining\n"[..],
        &mut output,
        &mut Vec::new(),
    )
    .unwrap();
    assert_eq!(output, b"once\nremaining\n");
}

#[test]
fn payload_bindings_shadow_in_arm_and_retire_across_loop_transfers() {
    let source = r#"
        variant Jump { Skip, Stop, Emit(integer) }
        bind value = 99;
        repeat 6 as i {
            bind action = if i == 1 { new Jump::Skip() } elif i == 4 { new Jump::Stop() } else { new Jump::Emit(i) };
            match on action {
                case Skip { continue; }
                case Stop { break; }
                case Emit(value) { print(value); }
            }
            print(value);
        }
    "#;
    assert_eq!(run(source), b"0\n99\n2\n99\n3\n99\n");
    let bad = r#"variant V { A(integer) } match on new V::A(1) { case A(value) { value = 2; } }"#;
    error(bad, DiagnosticKind::ImmutableAssignment);
    let leaked =
        r#"variant V { A(integer) } match on new V::A(1) { case A(value) {} } print(value);"#;
    assert!(scarp::compile(leaked)
        .unwrap_err()
        .message
        .contains("value"));
}

#[test]
fn exhaustive_matches_establish_return_and_keep_call_graph_edges() {
    let source = r#"
        variant V { One(integer), Zero }
        pure funcra read_value() -> variant V { return new V::One(7); }
        pure funcra value() -> integer {
            match on read_value() {
                case One(n) { return n; }
                case Zero { return 0; }
            }
        }
        print(value());
    "#;
    let compiled = scarp::compile(source).unwrap();
    assert_eq!(compiled.functions()[1].callees, ["read_value"]);
    assert_eq!(run(source), b"7\n");
    let missing = source.replace("case Zero { return 0; }", "case Zero {}");
    error(&missing, DiagnosticKind::MissingReturn);
    let impure = r#"variant V { A } funcra read() -> variant V { return new V::A(); }
        pure funcra blocked() -> integer { match on read() { case A { return 1; } } }"#;
    assert!(scarp::compile(impure).unwrap_err().message.contains("pure"));
}

#[test]
fn shape_limits_reject_mixed_cycles_and_program_limits_count_payloadless_cases() {
    let recursive = "record Node { next: variant Next } variant Next { End, More(record Node) }";
    error(recursive, DiagnosticKind::RecordDefinition);
    error(
        "variant Node { Link(array<variant Node, 0>) }",
        DiagnosticKind::VariantDefinition,
    );
    let limits = TypeLimits {
        max_components: 2,
        ..TypeLimits::default()
    };
    assert!(scarp::compile_with_type_limits("variant V { A, B, C }", limits).is_err());
    let cases: Vec<_> = (0..33).map(|number| format!("Case{number}")).collect();
    error(
        &format!("variant V {{ {} }}", cases.join(", ")),
        DiagnosticKind::VariantDefinition,
    );
}

#[test]
fn budgets_charge_payload_allocation_and_preserve_atomic_output() {
    let source = r#"variant V { Empty, Long(string) }
        print("buffered"); bind x = new V::Long("text");
        match on x { case Empty {} case Long(value) { print(value); } }"#;
    let mut output = Vec::new();
    let options = ExecutionOptions {
        max_tuple_bytes: 0,
        ..ExecutionOptions::default()
    };
    assert!(
        matches!(scarp::run_with_options(source, &mut output, options),
        Err(RunError::Diagnostic(diagnostic)) if diagnostic.kind == DiagnosticKind::VariantAllocation)
    );
    assert!(output.is_empty());
    scarp::run_with_options(
        "variant V { Empty } bind x = new V::Empty();",
        &mut output,
        options,
    )
    .unwrap();
    let limit = ExecutionOptions {
        limits: ExecutionLimits {
            max_steps: 3,
            ..ExecutionLimits::default()
        },
        ..ExecutionOptions::default()
    };
    assert!(scarp::run_with_options(source, &mut output, limit).is_err());
    assert!(output.is_empty());
}

#[test]
fn display_of_nested_payloads_draws_from_capture_budget() {
    let source = r#"variant Envelope { Data(string) }
        print(new Envelope::Data("line\nline\nline\nline"));"#;
    let options = ExecutionOptions {
        max_capture_bytes: 24,
        ..ExecutionOptions::default()
    };
    let mut output = Vec::new();
    assert!(scarp::run_with_options(source, &mut output, options).is_err());
    assert!(output.is_empty());
    assert_eq!(
        run(source),
        b"new Envelope::Data(\"line\\nline\\nline\\nline\")\n"
    );
}

const LIBRARY: &str = r#"
    // 🧭
    pub variant Status { Idle, Ready(integer) }
    variant Secret { Hidden }
    pub pure funcra ready(n: integer) -> variant Status { return new Status::Ready(n); }
"#;

fn loader(entry: &str, library: &str) -> MemorySourceLoader {
    let mut sources = MemorySourceLoader::new();
    sources.insert("main.srp", entry).unwrap();
    sources.insert("status.srp", library).unwrap();
    sources
}

#[test]
fn public_variants_survive_module_linking_editor_index_and_shifted_caches() {
    let mut cache = SyntaxCache::new(SyntaxCacheLimits::default());
    let body = r#"import "status.srp" as types;
        bind original: variant types.Status = new types.Status::Idle();
        match on types.ready(8) {
            case Idle { print(original); }
            case Ready(value) { print(value); }
        }"#;
    for prefix in ["", "// shift 🧪\n"] {
        let entry = format!("{prefix}{body}");
        let loader = loader(&entry, LIBRARY);
        let graph =
            modules::load_with_syntax_cache("main.srp", &loader, Default::default(), &mut cache)
                .unwrap();
        let indexed = modules::resolve_indexed(graph, Default::default()).unwrap();
        let ctor = entry.find("types.Status::Idle").unwrap() + "types.".len();
        assert_eq!(
            indexed
                .index()
                .goto_definition(0, ctor)
                .unwrap()
                .name
                .as_ref(),
            "Status"
        );
        let payload = entry.rfind("value").unwrap();
        assert_eq!(
            indexed
                .index()
                .goto_definition(0, payload)
                .unwrap()
                .name
                .as_ref(),
            "value"
        );
        let mut out = Vec::new();
        indexed
            .program()
            .execute(&mut out, &mut Vec::new(), Default::default())
            .unwrap();
        assert_eq!(out, b"8\n");
        let mut json = Vec::new();
        indexed.program().write_inspection_json(&mut json).unwrap();
        let json = String::from_utf8(json).unwrap();
        assert_eq!(indexed.program().inspection_schema_version(), 5);
        assert!(json.contains(r#""kind":"variant","cases":[{"name":"Idle","payload":null},{"name":"Ready","payload":"integer"}]"#));
    }
    assert!(cache.stats().hits > 0);
}

#[test]
fn private_variant_names_and_payload_types_cannot_escape_module_exports() {
    for (entry, module) in [
        (
            r#"import "status.srp" as lib; bind data = new lib.Secret::Hidden();"#,
            LIBRARY,
        ),
        (
            r#"import "status.srp" as lib;"#,
            "record Secret {} pub variant Leaked { Some(record Secret) }",
        ),
        (
            r#"import "status.srp" as lib;"#,
            "variant Secret { A } pub record Leaked { value: variant Secret }",
        ),
    ] {
        let err =
            modules::compile_with_loader("main.srp", &loader(entry, module), Default::default())
                .unwrap_err();
        assert_eq!(err.diagnostic.kind, DiagnosticKind::ModuleVisibility);
    }
}

#[test]
fn compiler_checks_unreachable_arms_and_legacy_contextual_names_still_work() {
    let source = "variant X { A, B } match on new X::A() { case A {} case B { print(unknown); } }";
    assert!(scarp::compile(source)
        .unwrap_err()
        .message
        .contains("unknown"));
    let legacy = "pure funcra match(n: integer) -> integer { return n; } bind variant = true; print(match(3), variant);";
    assert_eq!(run(legacy), b"3 true\n");
}

#[test]
fn native_backends_diagnose_variant_construction_and_matching() {
    for source in [
        "variant V { A } bind x = new V::A();",
        "variant V { A } match on new V::A() { case A {} }",
    ] {
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
    }
}

#[test]
fn record_and_variant_declarations_can_follow_ordinary_statements() {
    let source = r#"
        bind n = 3;
        record Item { count: integer }
        bind item = new Item { count: n };
        variant Event { Accepted(record Item) }
        match on new Event::Accepted(item) { case Accepted(value) { print(value->count); } }
    "#;
    assert_eq!(run(source), b"3\n");
}

#[test]
fn maximum_nested_matches_respect_the_block_limit() {
    fn nested(depth: usize) -> String {
        format!(
            "variant V {{ A }} bind selected = new V::A(); {} print(1); {}",
            "match on selected { case A {".repeat(depth),
            "}}".repeat(depth)
        )
    }
    std::thread::Builder::new()
        .stack_size(512 * 1024)
        .spawn(|| {
            let source = nested(scarp::parser::MAX_BLOCK_DEPTH);
            let syntax = scarp::parse(&source).unwrap();
            let program = scarp::resolver::resolve(syntax).unwrap();
            let mut out = Vec::new();
            scarp::interpreter::execute(&program, &mut out).unwrap();
            assert_eq!(out, b"1\n");
            let sources = loader(&source, "");
            let graph = modules::load("main.srp", &sources, Default::default()).unwrap();
            let indexed = modules::resolve_indexed(graph, Default::default()).unwrap();
            out.clear();
            indexed
                .program()
                .execute(&mut out, &mut Vec::new(), Default::default())
                .unwrap();
            assert_eq!(out, b"1\n");
            error(
                &nested(scarp::parser::MAX_BLOCK_DEPTH + 1),
                DiagnosticKind::BlockNesting,
            );
        })
        .unwrap()
        .join()
        .unwrap();
}
