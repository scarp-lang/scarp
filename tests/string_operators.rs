use scarp::{diagnostics::DiagnosticKind as Kind, ExecutionOptions, RunError};

fn run(source: &str, bytes: usize) -> Vec<u8> {
    let options = ExecutionOptions {
        max_text_bytes: bytes,
        ..ExecutionOptions::default()
    };
    scarp::check_with_options(source, options).unwrap();
    let mut out = Vec::new();
    scarp::run_with_options(source, &mut out, options).unwrap();
    out
}

#[test]
fn concatenation_newlines_and_repetition() {
    let source = r#"
        bind name: string = "Sanskar";
        bind language: string = "Scarp Programming Language";
        print(name + "\n" + language);
        print("é🌍" * 2 + "!");
        print("" * 9223372036854775807, "x" * 0);
    "#;
    assert_eq!(
        run(source, 1024),
        "Sanskar\nScarp Programming Language\né🌍é🌍!\n \n".as_bytes()
    );
}

#[test]
fn raw_and_multiline_literals_preserve_bytes() {
    let source = r#"
        bind raw = r"a\nb";
        bind lines = """one
two""";
        print(raw, lines);
        print("\u{1F30D}\t\\\"");
    "#;
    assert_eq!(run(source, 128), "a\\nb one\ntwo\n🌍\t\\\"\n".as_bytes());
}

#[test]
fn compound_updates_preserve_previous_values() {
    let source = r#"
        var value = "a";
        bind copy = value;
        value += "é";
        value *= 2;
        print(copy, value);
    "#;
    assert_eq!(run(source, 32), "a aéaé\n".as_bytes());
}

#[test]
fn quota_and_type_errors_leave_output_empty() {
    let source = r#"bind a="é"; bind b=a+"!"; print(b);"#;
    assert_eq!(run(source, 6), "é!\n".as_bytes());
    let options = ExecutionOptions {
        max_text_bytes: 5,
        ..ExecutionOptions::default()
    };
    let expected = scarp::check_with_options(source, options).unwrap_err();
    assert_eq!(expected.kind, Kind::TextConstruction);
    let mut out = Vec::new();
    let RunError::Diagnostic(actual) =
        scarp::run_with_options(source, &mut out, options).unwrap_err()
    else {
        panic!("expected diagnostic")
    };
    assert_eq!(actual, expected);
    assert!(out.is_empty());
    for source in [
        r#"print("a"+1);"#,
        r#"print("a"*true);"#,
        r#"print(2*"a");"#,
        r#"print("a"/2);"#,
    ] {
        assert_eq!(scarp::check(source).unwrap_err().kind, Kind::TypeMismatch);
    }
    assert_eq!(
        scarp::check(r#"print("a" * -1);"#).unwrap_err().kind,
        Kind::TextConstruction
    );
}

#[test]
fn operators_do_not_call_shadowed_builtins() {
    let source = r#"
        pure funcra text_concat(a: integer,b: integer)->integer{return a+b;}
        print("a" + "b", text_concat(2,3));
    "#;
    assert_eq!(run(source, 32), b"ab 5\n");
}
