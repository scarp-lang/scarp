use scarp::{
    ast::{PatternKind, Statement},
    diagnostics::{Diagnostic, DiagnosticKind as Kind},
    modules::{
        self,
        cache::{SyntaxCache, SyntaxCacheLimits},
        MemorySourceLoader,
    },
    ExecutionLimits, ExecutionOptions, RunError,
};

fn run(source: &str) -> Vec<u8> {
    let mut stdout = Vec::new();
    scarp::run(source, &mut stdout).unwrap();
    stdout
}

fn error(source: &str, kind: Kind) -> Diagnostic {
    let problem = scarp::compile(source).unwrap_err();
    assert_eq!(problem.kind, kind, "{problem:?}");
    problem
}

#[test]
fn panel_event_example_matches_golden() {
    assert_eq!(
        run(include_str!("../examples/ui_event_patterns.srp")),
        include_bytes!("../examples/ui_event_patterns.stdout"),
    );
}

#[test]
fn record_selection_uses_written_field_order_and_validates_nested_shapes() {
    let source = r#"
        record Item { title: string, area: tuple<integer, integer> }
        record Panel { settings: record Item, marks: array<integer, 2> }
        bind p = new Panel {
            marks: [array: 17, 18],
            settings: new Item { area: (20, 30), title: "Settings" },
        };
        bind {marks: [first, _], settings: {area: (width, height), title: label}} = p;
        print(label, first, width, height);
        bind {settings: {title: title,},} = p;
        print(title);
    "#;
    assert_eq!(run(source), b"Settings 17 20 30\nSettings\n");
}

#[test]
fn record_bindings_preserve_initializer_order_and_replacement_semantics() {
    let source = r#"
        record R { label: string, flags: tuple<boolean, boolean> }
        bind title = "Before";
        {
            var {label: title, flags: (on, _)} = new R { label: title, flags: (true, false) };
            print(title, on);
            title = "Local";
            on = false;
            print(title, on);
        }
        print(title);
    "#;
    assert_eq!(run(source), b"Before true\nLocal false\nBefore\n");
}

#[test]
fn nested_patterns_work_on_selected_match_arm_with_a_pure_return() {
    let source = r#"
        record Limits { bounds: tuple<integer, integer> }
        variant Request { Update(tuple<record Limits, array<integer, 2>>), Ignore }
        pure funcra handle(value: variant Request) -> integer {
            match on value {
                case Update(({bounds: (left, right)}, [first, _])) { return left + right + first; }
                case Ignore { return 0; }
            }
        }
        print(handle(new Request::Update((new Limits { bounds: (4, 7) }, [array: 10, 20]))));
        print(handle(new Request::Ignore()));
    "#;
    assert_eq!(run(source), b"21\n0\n");
}

#[test]
fn a_selected_payload_is_evaluated_once_and_other_arms_do_not_run() {
    let source = r#"
        record Pair { data: tuple<string, string> }
        variant Message { Skip, Content(record Pair) }
        funcra read_message() -> variant Message {
            return new Message::Content(new Pair { data: (read_line(), read_line()) });
        }
        match on read_message() {
            case Skip { print(1 / 0); }
            case Content({data: (headline, body)}) { print(headline, body); }
        }
        print(read_line());
    "#;
    let mut stdout = Vec::new();
    scarp::run_with_input(
        source,
        &mut &b"First\nSecond\nThird\n"[..],
        &mut stdout,
        &mut Vec::new(),
    )
    .unwrap();
    assert_eq!(stdout, b"First Second\nThird\n");
}

#[test]
fn nested_payload_bindings_are_immutable_and_do_not_escape_the_arm() {
    error(
        r#"variant V { Pair(tuple<integer, integer>) }
        match on new V::Pair((1, 2)) { case Pair((key, _)) { key = 2; } }"#,
        Kind::ImmutableAssignment,
    );
    let unsafe_scope = r#"variant V { Pair(tuple<integer, integer>), Empty }
        match on new V::Pair((1, 2)) { case Pair((key, _)) {} case Empty {} }
        print(key);"#;
    assert!(scarp::compile(unsafe_scope)
        .unwrap_err()
        .message
        .contains("key"));
    let source = r#"record D { x: integer }
        bind n = 100;
        variant V { Stop, Skip(record D), Use(record D) }
        repeat 6 as i {
            bind event = if i == 4 { new V::Stop() } elif i == 2 { new V::Skip(new D { x: i }) }
                else { new V::Use(new D { x: i }) };
            match on event {
                case Stop { break; }
                case Skip({x: n}) { assert(n == 2); continue; }
                case Use({x: n}) { print(n); }
            }
            print(n);
        }"#;
    assert_eq!(run(source), b"0\n100\n1\n100\n3\n100\n");
}

#[test]
fn record_fields_report_unknown_and_duplicate_labels_at_their_tokens() {
    let unknown =
        "// 🌍\nrecord Item { field: integer } bind {not_field: value} = new Item { field: 5 };";
    let problem = error(unknown, Kind::RecordField);
    assert_eq!(&unknown[problem.span.start..problem.span.end], "not_field");
    let duplicate =
        "record Item { field: integer } bind {field: a, field: b} = new Item { field: 5 };";
    let problem = error(duplicate, Kind::RecordField);
    assert_eq!(&duplicate[problem.span.start..problem.span.end], "field");
    let first = problem.related[0].span;
    assert!(first.start < problem.span.start);
    assert_eq!(&duplicate[first.start..first.end], "field");
}

#[test]
fn duplicate_bindings_are_rejected_across_different_pattern_kinds() {
    let source = r#"
        record Item { field: array<integer, 2>, other: integer }
        variant V { ItemValue(record Item), Empty }
        match on new V::Empty() {
            case Empty {}
            case ItemValue({field: [key, _], other: key}) {}
        }
    "#;
    let problem = error(source, Kind::DuplicateBinding);
    assert_eq!(&source[problem.span.start..problem.span.end], "key");
    let first = problem.related[0].span;
    assert!(first.start < problem.span.start);
    assert_eq!(&source[first.start..first.end], "key");
}

#[test]
fn an_invalid_shape_is_a_type_error_even_with_a_wildcard_or_unselected_tag() {
    let source = r#"variant V { Tuple(tuple<integer, integer>), Empty }
        match on new V::Empty() { case Tuple((only,)) {} case _ {} }"#;
    let problem = error(source, Kind::TypeMismatch);
    assert_eq!(&source[problem.span.start..problem.span.end], "(only,)");
    error("bind {x: item} = (1, 2);", Kind::TypeMismatch);
    error(
        "record R { x: array<integer, 2> } bind {x: [only]} = new R { x: [array: 1, 2] };",
        Kind::TypeMismatch,
    );
    error("variant V { One(integer), Empty } match on new V::One(1) { case One(v) {} case _({x: n}) {} }", Kind::VariantCase);
}

#[test]
fn structured_payload_coverage_is_still_tag_based() {
    let prefix =
        "variant V { Two(tuple<integer, integer>), Empty } bind event = new V::Two((2, 3));";
    error(
        &format!("{prefix} match on event {{ case Two((_, _)) {{}} }}"),
        Kind::MatchCoverage,
    );
    error(
        &format!(
            "{prefix} match on event {{ case Two((_, _)) {{}} case Two((a, b)) {{}} case _ {{}} }}"
        ),
        Kind::MatchCoverage,
    );
    assert_eq!(
        run(&format!(
            "{prefix} match on event {{ case Two((x, y)) {{ print(x + y); }} case _ {{}} }}"
        )),
        b"5\n"
    );
}

#[test]
fn selected_pattern_work_is_charged_before_output() {
    let source = r#"record Pair { data: tuple<integer, integer> }
        variant V { Yes(record Pair), No }
        bind v = new V::Yes(new Pair { data: (1, 2) });
        match on v { case Yes(PATTERN) { print("selected"); } case No {} }"#;
    let steps = |pattern: &str| {
        let source = source.replace("PATTERN", pattern);
        let result = |max_steps| {
            let options = ExecutionOptions {
                limits: ExecutionLimits {
                    max_steps,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut stdout = Vec::new();
            let result = scarp::run_with_options(&source, &mut stdout, options);
            (result, stdout)
        };
        (0..100)
            .find(|&n| {
                let (result, stdout) = result(n);
                match result {
                    Ok(()) => {
                        assert_eq!(stdout, b"selected\n");
                        true
                    }
                    Err(RunError::Diagnostic(error)) if error.kind == Kind::ExecutionLimit => {
                        assert!(stdout.is_empty());
                        false
                    }
                    error => panic!("unexpected execution result {error:?}"),
                }
            })
            .unwrap()
    };
    assert_eq!(steps("{data: (_, _)}"), steps("_") + 3);
}

#[test]
fn unselected_patterns_do_not_consume_payload_work() {
    let source = r#"record Pair { data: tuple<integer, integer> }
        variant V { Yes(record Pair), No }
        match on new V::No() { case Yes(PATTERN) { print("unused"); } case No { print("selected"); } }"#;
    let plain = source.replace("PATTERN", "_");
    let grouped = source.replace("PATTERN", "{data: (_, _)}");
    for max_steps in 0..16 {
        let options = ExecutionOptions {
            limits: ExecutionLimits {
                max_steps,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut left = Vec::new();
        let mut right = Vec::new();
        assert_eq!(
            scarp::run_with_options(&plain, &mut left, options).is_ok(),
            scarp::run_with_options(&grouped, &mut right, options).is_ok(),
        );
        assert_eq!(left, right);
    }
}

#[test]
fn record_field_and_binding_spans_survive_module_syntax_cache_relocation() {
    let mut cache = SyntaxCache::new(SyntaxCacheLimits::default());
    let module = r#"// 🧪
        pub record Item { fields: tuple<integer, integer> }
        pub variant Event { Ready(record Item), Empty }
        pub pure funcra score(event: variant Event) -> integer {
            match on event {
                case Ready({fields: (left, right)}) { return left + right; }
                case Empty { return 0; }
            }
        }"#;
    let base = r#"import "event.srp" as e; print(e.score(new e.Event::Ready(new e.Item { fields: (4, 5) })));"#;
    for prefix in ["", "// a longer entry file 🌍\n"] {
        let mut loader = MemorySourceLoader::new();
        let entry = format!("{prefix}{base}");
        loader.insert("main.srp", &entry).unwrap();
        loader.insert("event.srp", module).unwrap();
        let graph =
            modules::load_with_syntax_cache("main.srp", &loader, Default::default(), &mut cache)
                .unwrap();
        let compiled = modules::resolve_indexed(graph, Default::default()).unwrap();
        let symbol = compiled
            .index()
            .goto_definition(1, module.rfind("left").unwrap())
            .unwrap();
        assert_eq!(symbol.name.as_ref(), "left");
        let mut out = Vec::new();
        compiled
            .program()
            .execute(&mut out, &mut Vec::new(), Default::default())
            .unwrap();
        assert_eq!(out, b"9\n");

        let broken = module.replace("case Ready({fields:", "case Ready({bad:");
        let mut loader = MemorySourceLoader::new();
        loader.insert("main.srp", &entry).unwrap();
        loader.insert("event.srp", &broken).unwrap();
        for _ in 0..2 {
            let graph = modules::load_with_syntax_cache(
                "main.srp",
                &loader,
                Default::default(),
                &mut cache,
            )
            .unwrap();
            let error = modules::resolve(graph).unwrap_err();
            assert_eq!(error.diagnostic.kind, Kind::RecordField);
            let at = error.sources.locate_span(error.diagnostic.span).unwrap();
            assert_eq!(at.source, 1);
            assert_eq!(&broken[at.span.start..at.span.end], "bad");
        }
    }
    assert!(cache.stats().hits > 0);
}

#[test]
fn parser_retains_preorder_and_source_spans_for_mixed_payloads() {
    let source = "// 🧭\nvariant V { One(integer) } match on new V::One(1) { case One({items: [a, _], meta: (label,)}) {} }";
    let program = scarp::parse(source).unwrap();
    let Statement::Match(statement) = &program.statements[1] else {
        panic!("expected match")
    };
    let nodes = statement.arms[0].payload.as_ref().unwrap().nodes();
    assert_eq!(nodes.len(), 6);
    let PatternKind::Record(fields) = &nodes[0].kind else {
        panic!("expected record")
    };
    assert_eq!(fields.len(), 2);
    for field in fields {
        assert_eq!(&source[field.span.start..field.span.end], field.name);
    }
    assert!(matches!(&nodes[1].kind, PatternKind::Array(2)));
    assert!(matches!(&nodes[4].kind, PatternKind::Tuple(1)));
}

#[test]
fn malformed_or_oversized_patterns_fail_cleanly() {
    for pattern in [
        "{}",
        "{a}",
        "{a: }",
        "{a: x b: y}",
        "{a: (value)}",
        "{a: []}",
    ] {
        let source = format!("bind {pattern} = 0;");
        let problem = error(&source, Kind::UnexpectedToken);
        assert!(problem.span.end <= source.len());
    }
    let labels = (0..33)
        .map(|i| format!("f{i}: _"))
        .collect::<Vec<_>>()
        .join(",");
    error(&format!("bind {{{labels}}} = 0;"), Kind::RecordField);
    let over = "{x:".repeat(33) + "_" + &"}".repeat(33);
    error(&format!("bind {over} = 0;"), Kind::RecordField);
}

#[test]
fn native_entry_points_reject_structured_patterns() {
    let bind = "record R { x: integer } bind {x: a} = new R { x: 1 };";
    let arms = "variant V { Pair(tuple<integer, integer>) } match on new V::Pair((1,2)) { case Pair((x,y)) {} }";
    for source in [bind, arms] {
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
fn syntax_cache_counts_nested_payload_nodes_and_record_labels() {
    let source = r#"record Data { pair: tuple<integer, integer> }
        variant V { Record(record Data) }
        match on new V::Record(new Data { pair: (1, 2) }) { case Record(PATTERN) {} }"#;
    let retained = |pattern, max_ast_nodes| {
        let mut loader = MemorySourceLoader::new();
        loader
            .insert("main.srp", source.replace("PATTERN", pattern))
            .unwrap();
        let mut cache = SyntaxCache::new(SyntaxCacheLimits {
            max_ast_nodes,
            ..Default::default()
        });
        let graph =
            modules::load_with_syntax_cache("main.srp", &loader, Default::default(), &mut cache)
                .unwrap();
        modules::resolve(graph).unwrap();
        cache.stats().retained_ast_nodes
    };
    let leaf = retained("_", usize::MAX);
    assert!(leaf > 0);
    let nested = retained("{pair: (_, _)}", usize::MAX);
    assert_eq!(nested, leaf + 4);
    assert_eq!(retained("{pair: (_, _)}", nested - 1), 0);
    assert_eq!(retained("{pair: (_, _)}", nested), nested);
}

#[test]
fn maximum_record_pattern_depth_compiles_and_executes_on_a_small_stack() {
    let depth = scarp::types::MAX_COMPOSITE_DEPTH;
    let mut source = "record R0 { x: integer }\n".to_owned();
    let mut value = "new R0 { x: 91 }".to_owned();
    for index in 1..depth {
        source.push_str(&format!("record R{index} {{ x: record R{} }}\n", index - 1));
        value = format!("new R{index} {{ x: {value} }}");
    }
    let pattern = "{x: ".repeat(depth) + "result" + &"}".repeat(depth);
    source.push_str(&format!("bind {pattern} = {value}; print(result);"));
    std::thread::Builder::new()
        .stack_size(512 * 1024)
        .spawn(move || assert_eq!(run(&source), b"91\n"))
        .unwrap()
        .join()
        .unwrap();
}
