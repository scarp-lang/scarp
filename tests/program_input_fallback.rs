use std::io::Cursor;

use scarp::{
    diagnostics::{DiagnosticKind as Kind, Diagnostic},
    types::{FunctionEffect, TypeKind},
    InputLimits, ExecutionOptions, RunError,
};

fn output(source: &str, input: &[u8]) -> (Vec<u8>, Vec<u8>) {
    let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
    scarp::run_with_input(
        source,
        &mut Cursor::new(input),
        &mut stdout,
        &mut stderr,
    )
    .unwrap();
    (stdout, stderr)
}

fn error(source: &str, input: &[u8]) -> Kind {
    let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
    let result = scarp::run_with_input(
        source,
        &mut Cursor::new(input),
        &mut stdout,
        &mut stderr,
    );
    assert!(stdout.is_empty() && stderr.is_empty());
    match result.unwrap_err() {
        RunError::Diagnostic(diagnostic) => diagnostic.kind,
        other => panic!("expected diagnostic, got {other:?}"),
    }
}

#[test]
fn eof_fallback_is_distinct_from_empty_or_unterminated_lines() {
    let source = r#"print(read_line_or("fallback"));"#;
    assert_eq!(output(source, b""), (b"fallback\n".to_vec(), Vec::new()));
    assert_eq!(output(source, b"\n"), (b"\n".to_vec(), Vec::new()));
    assert_eq!(output(source, b"\r\n"), (b"\n".to_vec(), Vec::new()));
    assert_eq!(output(source, b"final"), (b"final\n".to_vec(), Vec::new()));
    assert_eq!(
        output(r#"print(read_line_or("missing"), read_line_or("missing"));"#, b"Ada\n"),
        (b"Ada missing\n".to_vec(), Vec::new())
    );
}

#[test]
fn eof_is_the_only_error_the_fallback_handles() {
    let source = r#"print("before"); eprint(read_line_or("fallback"));"#;
    assert_eq!(error(source, b"\xff\n"), Kind::InputEncoding);
    let no_reader = scarp::check(r#"print(read_line_or("fallback"));"#).unwrap_err();
    assert_eq!(no_reader.kind, Kind::InputUnavailable);
    let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
    let failure = scarp::run_with_input_and_options(
        source,
        &mut Cursor::new(b""),
        &mut stdout,
        &mut stderr,
        ExecutionOptions::default(),
        InputLimits { max_reads: 0, ..Default::default() },
    ).unwrap_err();
    assert!(matches!(failure, RunError::Diagnostic(Diagnostic { kind: Kind::InputLimit, .. })));
    assert!(stdout.is_empty() && stderr.is_empty());
}

#[test]
fn fallback_is_typed_eager_impure_and_shadowable() {
    for source in [
        r#"print(read_line_or());"#,
        r#"print(read_line_or("x", "y"));"#,
        r#"print(read_line_or(default: "x"));"#,
    ] {
        assert_eq!(scarp::compile(source).unwrap_err().kind, Kind::CallArguments);
    }
    assert_eq!(
        scarp::compile("print(read_line_or(4));").unwrap_err().kind,
        Kind::TypeMismatch
    );
    assert_eq!(
        scarp::compile(r#"pure funcra f() -> string { return read_line_or("x"); }"#)
            .unwrap_err().kind,
        Kind::ImpureFunction
    );
    assert_eq!(
        output(r#"pure funcra read_line_or(default: string) -> string { return default; } print(read_line_or("shadow"));"#, b""),
        (b"shadow\n".to_vec(), Vec::new())
    );
    // Eager fallback evaluation consumes a line before the actual read.
    assert_eq!(
        output(r#"print(read_line_or(read_line()));"#, b"first\nsecond\n"),
        (b"second\n".to_vec(), Vec::new())
    );
    assert_eq!(
        output(r#"print(read_line_or(read_line()));"#, b"first\n"),
        (b"first\n".to_vec(), Vec::new())
    );
}

#[test]
fn typed_metadata_exposes_the_impure_string_to_string_signature() {
    let program = scarp::compile(r#"print(read_line_or("fallback"));"#).unwrap();
    let signature = program
        .typed_expressions()
        .find_map(|expression| expression.nodes().find_map(|node| node.call_signature))
        .expect("read_line_or signature");
    assert!(matches!(
        program.type_registry().get(signature),
        Some(TypeKind::Function { parameters, result, effect })
            if parameters.as_ref() == &[scarp::value::ValueType::String.id()]
                && *result == scarp::value::ValueType::String.id()
                && *effect == FunctionEffect::Impure
    ));
}
