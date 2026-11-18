//! Regression tests for punctuation diagnostics in nested binding patterns.
//! The parser must name the missing separator instead of incorrectly asking
//! for the enclosing tuple, fixed-array, or record closing delimiter.

use scarp::diagnostics::DiagnosticKind;

#[test]
fn missing_pattern_commas_report_the_separator_at_utf8_byte_exact_spans() {
    for (source, token) in [
        ("// नमस्ते 🌍\nbind (first second) = (1, 2);", "second"),
        ("bind [left right] = [array: 1, 2];", "right"),
        ("bind {x: one y: two} = item;", "y"),
        ("bind (outer, {x: inner y: other}) = item;", "y"),
    ] {
        let error = scarp::parse(source).unwrap_err();
        assert_eq!(
            error.kind,
            DiagnosticKind::UnexpectedToken,
            "unexpected diagnostic: {error:?}"
        );
        assert_eq!(&source[error.span.start..error.span.end], token);
        assert!(
            error
                .message
                .contains("expected ',' between destructuring pattern members"),
            "the diagnostic must identify the missing comma: {error:?}"
        );
    }
}

#[test]
fn valid_comma_delimited_and_trailing_comma_patterns_remain_accepted() {
    for source in [
        "bind (first, second) = (1, 2);",
        "bind (single,) = (1,);",
        "bind [first, second] = [array: 1, 2];",
        "record Point { x: integer, y: integer } bind {x: a, y: b,} = new Point { x: 1, y: 2 };",
    ] {
        scarp::parse(source).expect("valid destructuring punctuation");
    }
}
