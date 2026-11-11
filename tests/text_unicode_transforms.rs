use scarp::{
    diagnostics::{DiagnosticKind as Kind, Span},
    numeric::{self, Builtin as B},
    value::{Value, ValueType, MAX_TEXT_BYTES},
    ExecutionOptions, RunError,
};

fn host(
    function: B,
    source: &str,
    steps: u64,
    bytes: usize,
) -> Result<Value, scarp::diagnostics::Diagnostic> {
    numeric::evaluate_with_text_limit(function, &[Value::from(source)], steps, bytes)
}

fn run(source: &str) -> String {
    let mut output = Vec::new();
    scarp::run_with_options(source, &mut output, ExecutionOptions::default()).unwrap();
    String::from_utf8(output).unwrap()
}

#[test]
fn three_text_transforms_have_exact_public_signatures() {
    for (name, function) in [
        ("text_lower", B::TextLower),
        ("text_upper", B::TextUpper),
        ("text_reverse", B::TextReverse),
    ] {
        assert_eq!(B::from_name(name), Some(function));
        assert_eq!(function.parameter_types(), &[ValueType::String]);
        assert_eq!(function.result_type(), ValueType::String);
        assert_eq!(
            numeric::evaluate(function, &[Value::Integer(1)], 100)
                .unwrap_err()
                .kind,
            Kind::TypeMismatch,
        );
        assert_eq!(
            numeric::evaluate(function, &[], 100).unwrap_err().kind,
            Kind::CallArguments,
        );
    }
}

#[test]
fn unicode_case_conversion_is_scalar_based_and_may_expand() {
    for (original, lower, upper) in [
        ("", "", ""),
        ("abc XYZ", "abc xyz", "ABC XYZ"),
        ("Straße", "straße", "STRASSE"),
        ("İ", "i\u{307}", "İ"),
        ("É", "é", "É"),
        ("🌍123", "🌍123", "🌍123"),
    ] {
        assert_eq!(
            host(B::TextLower, original, 10_000, 100).unwrap().as_str(),
            Some(lower)
        );
        assert_eq!(
            host(B::TextUpper, original, 10_000, 100).unwrap().as_str(),
            Some(upper)
        );
    }
    // Per-scalar Unicode mappings do not promise locale-specific case folding
    // or normalization of decomposed characters.
    assert_eq!(
        host(B::TextLower, "A\u{301}", 100, 100).unwrap().as_str(),
        Some("a\u{301}")
    );
}

#[test]
fn reverse_preserves_valid_utf8_and_reverses_scalar_not_grapheme_order() {
    for source in ["", "x", "abc", "aé🌍", "a\u{301}b", "\0\r\n"] {
        let reversed = host(B::TextReverse, source, 100, 100).unwrap();
        let expected: String = source.chars().rev().collect();
        assert_eq!(reversed.as_str(), Some(expected.as_str()));
        assert_eq!(
            host(B::TextReverse, reversed.as_str().unwrap(), 100, 100)
                .unwrap()
                .as_str(),
            Some(source),
        );
    }
    assert_eq!(
        host(B::TextReverse, "a\u{301}b", 100, 100)
            .unwrap()
            .as_str(),
        Some("b\u{301}a"),
    );
}

#[test]
fn transforms_work_in_typed_functions_and_preserve_user_shadowing() {
    assert_eq!(
        run(
            "pure funcra transform(s:string)->string{return text_lower(text_reverse(s));}
             bind x=transform(\"ABÉ\"); print(x,text_upper(x));",
        ),
        "éba ÉBA\n",
    );
    assert_eq!(
        run("pure funcra text_upper(s:string)->string{return s;} print(text_upper(\"abc\"));"),
        "abc\n",
    );
    for source in [
        "print(text_lower(7));",
        "print(text_upper(true));",
        "print(text_reverse(1.5));",
    ] {
        assert_eq!(scarp::compile(source).unwrap_err().kind, Kind::TypeMismatch);
    }
}

#[test]
fn case_expansion_charges_exact_output_bytes() {
    let source = "İ"; // two UTF-8 input bytes, three lowercase output bytes
    assert_eq!(
        host(B::TextLower, source, 7, 3).unwrap().as_str(),
        Some("i\u{307}")
    );
    assert_eq!(
        host(B::TextLower, source, 6, 3).unwrap_err().kind,
        Kind::ExecutionLimit
    );
    assert_eq!(
        host(B::TextLower, source, 7, 2).unwrap_err().kind,
        Kind::TextConstruction
    );
    assert_eq!(host(B::TextUpper, "ß", 6, 2).unwrap().as_str(), Some("SS"));
}

#[test]
fn reverse_charges_work_and_exact_output_bytes() {
    assert_eq!(host(B::TextReverse, "é", 4, 2).unwrap().as_str(), Some("é"));
    assert_eq!(
        host(B::TextReverse, "é", 3, 2).unwrap_err().kind,
        Kind::ExecutionLimit
    );
    assert_eq!(
        host(B::TextReverse, "é", 4, 1).unwrap_err().kind,
        Kind::TextConstruction
    );
    // The empty result still charges the minimum one unit per work phase.
    assert_eq!(host(B::TextReverse, "", 2, 0).unwrap().as_str(), Some(""));
    assert_eq!(
        host(B::TextReverse, "", 1, 0).unwrap_err().kind,
        Kind::ExecutionLimit
    );
}

#[test]
fn expanding_case_map_rejects_overlarge_result_before_allocation() {
    // U+0130: input uses 2 bytes, lowercase produces 3 bytes.
    let source = "İ".repeat(MAX_TEXT_BYTES / 2);
    let error = numeric::evaluate_with_text_limit(
        B::TextLower,
        &[Value::from(source)],
        u64::MAX,
        usize::MAX,
    )
    .unwrap_err();
    assert_eq!(error.kind, Kind::TextConstruction);
    assert!(error.message.contains("per-value limit"));
}

#[test]
fn transformed_strings_count_as_new_payloads_without_mutating_inputs() {
    let source = "bind a=\"ABC\"; bind b=text_lower(a); print(a,b);";
    let options = ExecutionOptions {
        max_text_bytes: 6,
        ..ExecutionOptions::default()
    };
    let mut out = Vec::new();
    scarp::run_with_options(source, &mut out, options).unwrap();
    assert_eq!(out, b"ABC abc\n");
    let options = ExecutionOptions {
        max_text_bytes: 5,
        ..ExecutionOptions::default()
    };
    assert_eq!(
        scarp::check_with_options(source, options).unwrap_err().kind,
        Kind::TextConstruction
    );
}

#[test]
fn failed_expansion_discards_both_streams_and_preserves_diagnostic() {
    let source = "print(\"ok\"); eprint(\"err\"); print(text_lower(\"İ\"));";
    let options = ExecutionOptions {
        max_text_bytes: 9,
        ..ExecutionOptions::default()
    };
    let expected = scarp::check_with_options(source, options).unwrap_err();
    assert_eq!(expected.kind, Kind::TextConstruction);
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let RunError::Diagnostic(actual) =
        scarp::run_with_io_and_options(source, &mut out, &mut err, options).unwrap_err()
    else {
        panic!("expected a typed compiler diagnostic");
    };
    assert_eq!(actual, expected);
    assert!(out.is_empty() && err.is_empty());
    assert!(expected.span != Span::default());
}
