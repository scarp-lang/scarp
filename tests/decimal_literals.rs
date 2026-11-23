//! End-to-end opt-in decimal-literal syntax and binary64 compatibility.
use scarp::{
    diagnostics::{DiagnosticKind, Span},
    lexer::{lex, TokenKind},
    modules::{self, MemorySourceLoader, ModuleLimits},
    RunError,
};

fn output(source: &str) -> String {
    let mut out = Vec::new();
    scarp::run(source, &mut out).expect("valid exact-decimal program");
    String::from_utf8(out).unwrap()
}

fn error(source: &str) -> (DiagnosticKind, Span) {
    let mut out = Vec::new();
    let RunError::Diagnostic(problem) = scarp::run(source, &mut out).unwrap_err() else {
        panic!("expected typed diagnostic");
    };
    assert!(out.is_empty(), "failed execution must not replay buffered output");
    (problem.kind, problem.span)
}

#[test]
fn decimal_addition_and_loop_increments_are_exact() {
    let source = r#"
        print(0.1d + 0.2d, 0.3d == 0.1d + 0.2d);
        var x = 1.0d;
        while x <= 5.0d { x += 0.1d; }
        print(x);
        print(0.1 + 0.2);
    "#;
    assert_eq!(output(source), "0.3 true\n5.1\n0.30000000000000004\n");
    assert_eq!(
        output("print(-1.20d, 1.20d == 1.2d, 10d / 8d);"),
        "-1.2 true 1.25\n"
    );
}

#[test]
fn literal_and_constructor_are_exact_but_do_not_coerce_to_float() {
    assert_eq!(
        output(r#"print(0.1d + decimal("0.2"), decimal_to_text(42d));"#),
        "0.3 42\n"
    );
    assert_eq!(error("print(0.1d + 0.2);").0, DiagnosticKind::TypeMismatch);
    assert_eq!(error("print(1d + 1);").0, DiagnosticKind::TypeMismatch);
}

#[test]
fn decimal_tokens_and_diagnostics_keep_source_spans() {
    let tokens = lex("0.1d + 42d + 0.1").unwrap();
    assert_eq!(tokens[0].kind, TokenKind::Decimal("0.1".into()));
    assert_eq!((tokens[0].span.start, tokens[0].span.end), (0, 4));
    assert_eq!(tokens[2].kind, TokenKind::Decimal("42".into()));
    assert_eq!((tokens[2].span.start, tokens[2].span.end), (7, 10));
    assert_eq!(tokens[4].kind, TokenKind::Float("0.1".into()));

    for (source, expected) in [
        ("print(0.1234567890123456789d);", DiagnosticKind::DecimalLimit),
        ("print(12345678901234567890123456789d);", DiagnosticKind::DecimalLimit),
        ("print(1.2d_more);", DiagnosticKind::InvalidDecimal),
        ("print(1e3d);", DiagnosticKind::InvalidFloat),
    ] {
        let (kind, span) = error(source);
        assert_eq!(kind, expected, "{source}");
        assert!(source.is_char_boundary(span.start));
        assert!(source.is_char_boundary(span.end));
        assert!(span.end <= source.len());
    }
}

#[test]
fn modules_can_export_exact_decimal_scalar_constants() {
    let mut loader = MemorySourceLoader::new();
    loader.insert("rates.srp", "pub bind rate: decimal = 0.1d;").unwrap();
    loader
        .insert(
            "main.srp",
            r#"import "rates.srp" as rates; print(rates.rate + 0.2d);"#,
        )
        .unwrap();
    let program =
        modules::compile_with_loader("main.srp", &loader, ModuleLimits::default()).unwrap();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    program
        .execute(&mut out, &mut err, Default::default())
        .unwrap();
    assert_eq!(out, b"0.3\n");
    assert!(err.is_empty());
}
