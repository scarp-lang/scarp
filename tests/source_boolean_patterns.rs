use scarp::diagnostics::DiagnosticKind;

fn run(source: &str) -> String {
    let mut output = Vec::new();
    scarp::run(source, &mut output).unwrap();
    String::from_utf8(output).unwrap()
}

fn rejects(source: &str, kind: DiagnosticKind, phrase: &str) {
    let err = scarp::compile(source).unwrap_err();
    assert_eq!(err.kind, kind, "{source}: {err:?}");
    assert!(err.message.contains(phrase), "{err:?}");
}

#[test]
fn finite_boolean_literals_dispatch_exhaustively() {
    let source = r#"
        match on false {
            case true { print("unexpected"); }
            case false { print("false"); }
        }
        match on true {
            case false { print("unexpected"); }
            case true { print("true"); }
        }
        match on false {
            case true { print("unexpected"); }
            case _ { print("fallback"); }
        }
    "#;
    assert_eq!(run(source), "false\ntrue\nfallback\n");
}

#[test]
fn boolean_case_coverage_and_redundancy_are_checked() {
    rejects(
        "match on true { case true {} }",
        DiagnosticKind::MatchCoverage,
        "missing false",
    );
    rejects(
        "match on false { case false {} case false {} case true {} }",
        DiagnosticKind::MatchCoverage,
        "duplicate case",
    );
    rejects(
        "match on true { case true {} case false {} case _ {} }",
        DiagnosticKind::MatchCoverage,
        "wildcard is unreachable",
    );
    rejects(
        "match on true { case _ {} case false {} }",
        DiagnosticKind::MatchCoverage,
        "unreachable after the wildcard",
    );
    rejects(
        "match on true { case missing {} case _ {} }",
        DiagnosticKind::VariantCase,
        "no literal case",
    );
    rejects(
        "match on true { case true(_) {} case false {} }",
        DiagnosticKind::VariantCase,
        "cannot bind a payload",
    );
}

#[test]
fn boolean_match_subject_evaluates_once_with_scoped_branches() {
    let source = r#"
        funcra read() -> boolean {
            print("called");
            return true;
        }
        match on read() {
            case false { print("unreachable"); }
            case true {
                bind message = "selected";
                print(message);
            }
        }
    "#;
    assert_eq!(run(source), "called\nselected\n");
}

#[test]
fn pure_functions_can_return_from_exhaustive_boolean_match() {
    let source = r#"
        pure funcra choose(value: boolean) -> integer {
            match on value {
                case true { return 7; }
                case false { return 9; }
            }
        }
        print(choose(true));
        print(choose(false));
    "#;
    assert_eq!(run(source), "7\n9\n");
}

#[test]
fn ordinary_source_variants_keep_their_own_case_names() {
    let source = r#"
        variant Switch { true, false }
        match on new Switch::false() {
            case true { print("not chosen"); }
            case false { print("variant"); }
        }
    "#;
    assert_eq!(run(source), "variant\n");
}

#[test]
fn boolean_diagnostics_preserve_utf8_byte_spans() {
    let source = "// 🧪\nmatch on true { case invalid {} case false {} }";
    let err = scarp::compile(source).unwrap_err();
    assert_eq!(err.kind, DiagnosticKind::VariantCase);
    assert_eq!(&source[err.span.start..err.span.end], "invalid");
}

#[test]
fn non_boolean_scalar_subject_still_requires_a_variant() {
    rejects(
        "match on 5 { case true {} case false {} }",
        DiagnosticKind::VariantCase,
        "needs a variant",
    );
}

#[test]
fn boolean_match_is_not_silently_lowered_to_native_scalar_code() {
    let compiled = scarp::compile("match on true { case true {} case false {} }").unwrap();
    let err = scarp::c_backend::emit_program(&compiled).unwrap_err();
    assert_eq!(err.kind, DiagnosticKind::NativeUnsupported);
}
