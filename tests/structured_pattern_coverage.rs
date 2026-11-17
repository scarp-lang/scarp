//! Source-level regression coverage for nominal and nested binding patterns.
//! Exercise the public compiler/runtime APIs, including diagnostic identity
//! and exact UTF-8 byte spans, not merely whether compilation failed.

use scarp::diagnostics::DiagnosticKind;

#[test]
fn structured_pattern_fixture_executes_with_deterministic_output() {
    let source = include_str!("../examples/structured_pattern_coverage.srp");
    let mut stdout = Vec::new();
    scarp::run(source, &mut stdout).expect("structured patterns must compile and execute");
    assert_eq!(
        stdout,
        include_bytes!("../examples/structured_pattern_coverage.stdout")
    );
}

#[test]
fn duplicate_names_in_nested_tuple_bindings_are_rejected_at_second_name() {
    let source = "// नमस्ते 🌍\nbind ((same, _), same) = ((1, 2), 3);";
    let error = scarp::compile(source).unwrap_err();
    assert_eq!(error.kind, DiagnosticKind::DuplicateBinding);
    assert_eq!(&source[error.span.start..error.span.end], "same");
    assert_eq!(error.span.start, source.rfind("same").unwrap());
    assert_eq!(error.related.len(), 1);
    let first = error.related[0].span;
    assert_eq!(&source[first.start..first.end], "same");
    assert_eq!(first.start, source.find("same").unwrap());
}

#[test]
fn unknown_record_field_is_rejected_in_an_unselected_variant_arm() {
    let source = r#"
        record Point { x: integer, y: integer }
        variant Event { Move(record Point), Idle }
        match on new Event::Idle() {
            case Move({missing: value}) { print(value); }
            case Idle { print(1); }
        }
    "#;
    let error = scarp::compile(source).unwrap_err();
    assert_eq!(error.kind, DiagnosticKind::RecordField);
    assert_eq!(&source[error.span.start..error.span.end], "missing");
}

#[test]
fn tuple_pattern_arity_mismatch_reports_the_pattern_not_initializer() {
    let source = "bind (first, second) = (1, 2, 3); print(first);";
    let error = scarp::compile(source).unwrap_err();
    assert_eq!(error.kind, DiagnosticKind::TypeMismatch);
    assert_eq!(
        &source[error.span.start..error.span.end],
        "(first, second)"
    );
    assert!(error.message.contains("requires 2 elements, found 3"));
}

#[test]
fn selected_payload_bindings_remain_immutable() {
    let source = r#"
        record Point { x: integer }
        variant Event { Move(record Point), Idle }
        match on new Event::Move(new Point { x: 7 }) {
            case Move({x: local}) { local = 10; }
            case Idle {}
        }
    "#;
    let error = scarp::compile(source).unwrap_err();
    assert_eq!(error.kind, DiagnosticKind::ImmutableAssignment);
    assert_eq!(&source[error.span.start..error.span.end], "local");
}

#[test]
fn selected_payload_names_do_not_escape_their_match_arm() {
    let source = r#"
        variant Event { Move(integer), Idle }
        match on new Event::Idle() {
            case Move(value) {}
            case Idle {}
        }
        print(value);
    "#;
    let error = scarp::compile(source).unwrap_err();
    assert_eq!(error.kind, DiagnosticKind::UnknownName);
    assert_eq!(&source[error.span.start..error.span.end], "value");
}
