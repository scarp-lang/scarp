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

#[test]
fn literal_tokens_keep_original_utf8_spans_and_line_endings() {
    use scarp::lexer::{lex, TokenKind};

    let source = "// é\r\nr\"\\n🌍\" \"\"\"a\r\nb\"\"\"";
    let tokens = lex(source).unwrap();
    assert_eq!(tokens[0].kind, TokenKind::String("\\n🌍".into()));
    assert_eq!(tokens[1].kind, TokenKind::String("a\r\nb".into()));
    assert_eq!(
        &source[tokens[0].span.start..tokens[0].span.end],
        "r\"\\n🌍\""
    );
    assert_eq!(
        &source[tokens[1].span.start..tokens[1].span.end],
        "\"\"\"a\r\nb\"\"\""
    );
    for source in ["r\"🌍", "r\"a\nb\"", "\"\"\"a\n🌍"] {
        let error = lex(source).unwrap_err();
        assert_eq!(error.kind, Kind::UnterminatedString);
        assert!(source.is_char_boundary(error.span.start));
        assert!(source.is_char_boundary(error.span.end));
    }
    assert_eq!(run("bind r=1; print(r, r\"// raw\");", 32), b"1 // raw\n");
}

#[test]
fn string_operators_in_functions_and_numeric_precedence() {
    let source = r#"
        pure funcra format(s: string, n: integer=2)->string {
            return "[" + s * n + "]";
        }
        print(format("é"), format("x",3), 2+3*4);
    "#;
    assert_eq!(run(source, 128), "[éé] [xxx] 14\n".as_bytes());
}

#[test]
fn native_scalar_kernel_rejects_string_operators() {
    use scarp::native_object::emit_elf64_integer_kernel;
    for source in [
        r#"bind x="a"+"b";"#,
        r#"bind x="a"*3;"#,
        r#"var x="a"; x+="b";"#,
    ] {
        let error = emit_elf64_integer_kernel(source).unwrap_err();
        assert_eq!(error.kind, Kind::NativeUnsupported);
    }
}

#[test]
fn repetition_and_concatenation_match_explicit_builtins() {
    for value in ["", "a", "é", "🌍", "नमस्ते"] {
        for n in 0..5 {
            let source = format!(
                "bind x=\"{value}\"; print(x*{n}==text_repeat(x,{n}),x+\"!\"==text_concat(x,\"!\"));"
            );
            assert_eq!(run(&source, 256), b"true true\n");
        }
    }
}
