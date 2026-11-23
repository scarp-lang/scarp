use scarp::{
    diagnostics::{DiagnosticKind as Kind, Span},
    ExecutionOptions, RunError,
};

fn output(source: &str) -> String {
    let mut output = Vec::new();
    scarp::run(source, &mut output).expect("program should run");
    String::from_utf8(output).expect("UTF-8 output")
}

fn failed(source: &str, options: ExecutionOptions, kind: Kind) -> scarp::diagnostics::Diagnostic {
    let mut out = Vec::new();
    let error = scarp::run_with_options(source, &mut out, options).unwrap_err();
    let RunError::Diagnostic(diagnostic) = error else {
        panic!("expected a source diagnostic")
    };
    assert_eq!(diagnostic.kind, kind, "{source}: {diagnostic:?}");
    assert!(out.is_empty(), "failure must not emit buffered output");
    diagnostic
}

#[test]
fn mutable_index_updates_replace_the_tuple_and_preserve_existing_aliases() {
    let source = r#"
        var position = (10, 20);
        bind old = position;
        position[0] = 50;
        position[(1)] = 100;
        print(position, old);
        var pair = (1, "hello");
        bind saved = pair;
        pair[1] = "Scarp";
        print(pair, saved);
    "#;
    assert_eq!(
        output(source),
        "(50, 100) (10, 20)\n(1, \"Scarp\") (1, \"hello\")\n"
    );
}

#[test]
fn nested_replacement_and_pure_function_local_updates_work() {
    let source = r#"
        pure funcra adjust() -> tuple<tuple<integer, integer>, boolean> {
            var point = ((1, 2), true);
            point[0] = (3, 4);
            return point;
        }
        print(adjust(), adjust()[0][1]);
    "#;
    assert_eq!(output(source), "((3, 4), true) 4\n");
}

#[test]
fn replacement_expression_evaluates_exactly_once() {
    let source = r#"
        funcra replacement() -> integer { print("evaluated"); return 9; }
        var pair = (1, 2);
        pair[1] = replacement();
        print(pair);
    "#;
    assert_eq!(output(source), "evaluated\n(1, 9)\n");
}

#[test]
fn immutable_target_reports_the_original_declaration() {
    let source = "bind pair=(1,2); pair[0]=9;";
    let diagnostic = failed(
        source,
        ExecutionOptions::default(),
        Kind::ImmutableAssignment,
    );
    assert_eq!(&source[diagnostic.span.start..diagnostic.span.end], "pair");
    assert_eq!(diagnostic.related.len(), 1);
}

#[test]
fn indices_remain_literal_and_checked_at_compile_time() {
    for source in [
        "var t=(1,2); t[-1]=3;",
        "var t=(1,2); t[2]=3;",
        "var t=(1,2); var i=0; t[i]=3;",
        "var t=(1,2); t[0+0]=3;",
        "var t=(1,2); t[0.0]=3;",
    ] {
        let diagnostic = scarp::compile(source).unwrap_err();
        assert_eq!(diagnostic.kind, Kind::TupleIndex, "{source}");
        assert!(diagnostic.span.start < diagnostic.span.end);
    }
}

#[test]
fn member_types_and_target_collections_are_validated() {
    failed(
        "var t=(1, \"hi\"); t[0]=false;",
        ExecutionOptions::default(),
        Kind::TypeMismatch,
    );
    failed(
        "var t=(1, \"hi\"); t[1]=3;",
        ExecutionOptions::default(),
        Kind::TypeMismatch,
    );
    failed(
        "var t=[array: 1, 2]; t[0]=3;",
        ExecutionOptions::default(),
        Kind::TypeMismatch,
    );
    failed(
        "var t=1; t[0]=3;",
        ExecutionOptions::default(),
        Kind::TypeMismatch,
    );
    let err = scarp::parse("var t=(1,2); t[0]+=1;").unwrap_err();
    assert_eq!(err.kind, Kind::UnexpectedToken);
}

#[test]
fn replacing_member_consumes_new_tuple_quota_and_is_atomic() {
    let original = "var t=(1,2);";
    let mut low = 0usize;
    let mut high = 4096usize;
    while low < high {
        let mid = low + (high - low) / 2;
        let options = ExecutionOptions {
            max_tuple_bytes: mid,
            ..Default::default()
        };
        if scarp::check_with_options(original, options).is_ok() {
            high = mid;
        } else {
            low = mid + 1;
        }
    }
    assert!(low < 4096);
    let options = ExecutionOptions {
        max_tuple_bytes: low,
        ..Default::default()
    };
    scarp::check_with_options(original, options).unwrap();
    let source = "print(\"before\"); var t=(1,2); t[0]=3; print(t);";
    let diagnostic = failed(source, options, Kind::TupleAllocation);
    assert_eq!(
        diagnostic.span,
        Span::new(source.find('0').unwrap(), source.find('0').unwrap() + 1)
    );
}

#[test]
fn contextual_keyword_names_remain_valid_indexed_assignment_targets() {
    assert_eq!(
        output("var repeat=(1,2); repeat[0]=5; print(repeat);"),
        "(5, 2)\n"
    );
    assert_eq!(
        output("var import=(1,2); import[1]=6; print(import);"),
        "(1, 6)\n"
    );
    assert_eq!(output("var pub=(1,2); pub[0]=7; print(pub);"), "(7, 2)\n");
}

#[test]
fn indexed_updates_in_imported_modules_preserve_snapshots_and_source_spans() {
    use scarp::modules::{self, MemorySourceLoader, ModuleLimits};

    let mut loader = MemorySourceLoader::new();
    loader
        .insert(
            "main.srp",
            "// 🌍\nimport \"lib.srp\" as lib; print(lib.replace());",
        )
        .unwrap();
    loader
        .insert(
            "lib.srp",
            "pub pure funcra replace()->tuple<tuple<integer,string>,tuple<integer,string>> { var pair=(1,\"old\"); bind saved=pair; pair[0]=9; return (pair,saved); }",
        )
        .unwrap();
    let program =
        modules::compile_with_loader("main.srp", &loader, ModuleLimits::default()).unwrap();
    program.check(Default::default()).unwrap();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    program
        .execute(&mut out, &mut err, Default::default())
        .unwrap();
    assert_eq!(out, b"((9, \"old\"), (1, \"old\"))\n");
    assert!(err.is_empty());

    let mut invalid = MemorySourceLoader::new();
    invalid
        .insert(
            "main.srp",
            "import \"lib.srp\" as lib; print(lib.replace());",
        )
        .unwrap();
    invalid
        .insert(
            "lib.srp",
            "// 🌍\npub pure funcra replace()->tuple<integer> { var pair=(1,); pair[2]=9; return pair; }",
        )
        .unwrap();
    let diagnostic =
        modules::compile_with_loader("main.srp", &invalid, ModuleLimits::default()).unwrap_err();
    assert_eq!(diagnostic.diagnostic.kind, Kind::TupleIndex);
    let rendered = diagnostic.render();
    assert!(rendered.contains("lib.srp:2:"), "{rendered}");
}
