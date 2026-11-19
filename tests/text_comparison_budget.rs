use scarp::{
    diagnostics::{DiagnosticKind, Span},
    ExecutionLimits, RunError,
};

#[test]
fn string_comparison_charges_utf8_bytes_before_publishing_output() {
    let common = "é".repeat(64);
    for (operator, suffix, expected) in [("==", "!", "true"), ("!=", "?", "true")] {
        let source = format!(
            "bind a = \"{common}!\"; bind b = \"{common}{suffix}\"; \
             eprint(\"pending\"); print(a {operator} b);"
        );
        let position = source.find(operator).unwrap();
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut limits = ExecutionLimits {
            max_steps: 12,
            max_prints: 2,
        };
        let error =
            scarp::run_with_io_and_limits(&source, &mut stdout, &mut stderr, limits).unwrap_err();
        let RunError::Diagnostic(error) = error else {
            panic!("expected a source diagnostic");
        };
        assert_eq!(error.kind, DiagnosticKind::ExecutionLimit);
        assert_eq!(error.span, Span::new(position, position + 2));
        assert!(stdout.is_empty() && stderr.is_empty());
        assert_eq!(
            scarp::check_with_limits(&source, limits).unwrap_err(),
            error
        );

        limits.max_steps += 1;
        scarp::run_with_io_and_limits(&source, &mut stdout, &mut stderr, limits).unwrap();
        assert_eq!(stdout, format!("{expected}\n").as_bytes());
        assert_eq!(stderr, b"pending\n");
        scarp::check_with_limits(&source, limits).unwrap();
    }
}

#[test]
fn string_comparison_of_different_lengths_and_empty_strings_does_not_scan_bytes() {
    let long = "x".repeat(129);
    for (left, right, expected) in [(&long[..], "x", "false"), ("", "", "true")] {
        let source = format!("print(\"{left}\" == \"{right}\");");
        let mut stdout = Vec::new();
        scarp::run_with_limits(
            &source,
            &mut stdout,
            ExecutionLimits {
                max_steps: 5,
                max_prints: 1,
            },
        )
        .unwrap();
        assert_eq!(stdout, format!("{expected}\n").as_bytes());
    }
}
