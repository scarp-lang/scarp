use scarp::diagnostics::DiagnosticKind;

fn output(source: &str) -> String {
    let mut out = Vec::new();
    scarp::run(source, &mut out).unwrap();
    String::from_utf8(out).unwrap()
}

fn rejection(source: &str, kind: DiagnosticKind, text: &str) {
    let error = scarp::compile(source).unwrap_err();
    assert_eq!(error.kind, kind, "{source}: {error:?}");
    assert!(
        error.message.contains(text),
        "unexpected error for {source}: {error:?}"
    );
}

#[test]
fn integer_literal_cases_dispatch_with_default_and_signed_values() {
    let source = r#"
        match on -10 {
            case 10 { print("wrong"); }
            case -10 { print("negative"); }
            case _ { print("default"); }
        }
        match on 0x2a {
            case 42 { print("hex matched"); }
            case _ { print("unexpected"); }
        }
        match on 8 {
            case 0 { print("zero"); }
            case 1 { print("one"); }
            case 2 { print("two"); }
            case _ { print("other"); }
        }
    "#;
    assert_eq!(output(source), "negative\nhex matched\nother\n");
}

#[test]
fn text_literal_cases_are_decoded_and_preserve_unicode() {
    let source = r#"
        match on "नमस्ते" {
            case "hello" { print("unexpected"); }
            case "नमस्ते" { print("greeting"); }
            case _ { print("unmatched"); }
        }
        match on "" {
            case "" { print("empty"); }
            case _ { print("nonempty"); }
        }
        match on "a\nb" {
            case "a\nb" { print("escaped"); }
            case _ { print("unexpected"); }
        }
    "#;
    assert_eq!(output(source), "greeting\nempty\nescaped\n");
}

#[test]
fn exact_case_and_fallback_have_separate_scopes() {
    let source = r#"
        pure funcra label(value: integer) -> string {
            match on value {
                case 1 {
                    bind local = "one";
                    return local;
                }
                case 2 { return "two"; }
                case _ { return "other"; }
            }
        }
        print(label(1));
        print(label(2));
        print(label(900));
    "#;
    assert_eq!(output(source), "one\ntwo\nother\n");
}

#[test]
fn match_subject_evaluates_once_and_later_branches_do_not_run() {
    let source = r#"
        funcra fetch() -> string {
            print("fetched");
            return "stop";
        }
        match on fetch() {
            case "start" { print("start"); }
            case "stop" { print("selected"); }
            case _ { print("fallback"); }
        }
    "#;
    assert_eq!(output(source), "fetched\nselected\n");
}

#[test]
fn scalar_match_requires_an_exhaustive_wildcard() {
    for source in [
        "match on 1 { case 1 {} }",
        "match on \"x\" { case \"x\" {} }",
        "match on -1 { case -1 {} }",
    ] {
        rejection(source, DiagnosticKind::MatchCoverage, "requires case _");
    }
}

#[test]
fn alias_spellings_of_one_integer_case_are_duplicates() {
    for source in [
        "match on 10 { case 10 {} case 0xa {} case _ {} }",
        "match on 0 { case -0 {} case 0 {} case _ {} }",
        "match on 1 { case 1 {} case 0b1 {} case _ {} }",
    ] {
        rejection(source, DiagnosticKind::MatchCoverage, "duplicate scalar literal");
    }
}

#[test]
fn decoded_duplicate_text_literal_patterns_are_rejected() {
    rejection(
        "match on \"hello\" { case \"hello\" {} case \"hello\" {} case _ {} }",
        DiagnosticKind::MatchCoverage,
        "duplicate scalar literal",
    );
}

#[test]
fn wrong_type_invalid_tag_and_payload_patterns_fail_at_compile_time() {
    rejection(
        "match on 2 { case \"2\" {} case _ {} }",
        DiagnosticKind::TypeMismatch,
        "does not match",
    );
    rejection(
        "match on \"2\" { case 2 {} case _ {} }",
        DiagnosticKind::TypeMismatch,
        "does not match",
    );
    rejection(
        "match on 1 { case name {} case _ {} }",
        DiagnosticKind::VariantCase,
        "exact literal",
    );
    rejection(
        "match on 1 { case 1(x) {} case _ {} }",
        DiagnosticKind::VariantCase,
        "cannot bind a payload",
    );
    rejection(
        "match on 1 { case _ {} case 1 {} }",
        DiagnosticKind::MatchCoverage,
        "unreachable after the wildcard",
    );
}

#[test]
fn integer_overflow_is_reported_in_source_literal_span() {
    let source = "match on 0 { case 9223372036854775808 {} case _ {} }";
    let error = scarp::compile(source).unwrap_err();
    assert_eq!(error.kind, DiagnosticKind::InvalidInteger);
    assert_eq!(
        &source[error.span.start..error.span.end],
        "9223372036854775808"
    );
    let source = "match on 0 { case -9223372036854775809 {} case _ {} }";
    let error = scarp::compile(source).unwrap_err();
    assert_eq!(error.kind, DiagnosticKind::InvalidInteger);
    assert_eq!(
        &source[error.span.start..error.span.end],
        "-9223372036854775809"
    );
}

#[test]
fn minimum_i64_case_remains_representable() {
    let source = r#"
        match on -9223372036854775808 {
            case -9223372036854775808 { print("minimum"); }
            case _ { print("fallback"); }
        }
    "#;
    assert_eq!(output(source), "minimum\n");
}

#[test]
fn literal_diagnostic_spans_are_utf8_byte_exact() {
    let source = "// नमस्ते\nmatch on 7 { case \"wrong\" {} case _ {} }";
    let error = scarp::compile(source).unwrap_err();
    assert_eq!(error.kind, DiagnosticKind::TypeMismatch);
    assert_eq!(&source[error.span.start..error.span.end], "\"wrong\"");
}

#[test]
fn native_scalar_backend_rejects_literal_dispatch() {
    let program = scarp::compile("match on 7 { case 7 {} case _ {} }").unwrap();
    assert_eq!(
        scarp::c_backend::emit_program(&program).unwrap_err().kind,
        DiagnosticKind::NativeUnsupported
    );
}
