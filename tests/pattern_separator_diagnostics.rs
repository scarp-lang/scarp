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

#[test]
fn malformed_group_diagnostics_identify_the_innermost_missing_delimiter() {
    for (source, found, expected) in [
        ("bind (first, [second, third)) = item;", ")", "']'"),
        ("bind {x: left, y: right] = item;", "]", "'}'"),
        ("bind [left, right = item;", "=", "']'"),
        ("// 🌍\r\nbind (outer, {x: value,", "", "'}'"),
        ("bind (value,", "", "')'"),
    ] {
        let error = scarp::parse(source).unwrap_err();
        assert_eq!(error.kind, DiagnosticKind::UnexpectedToken);
        assert_eq!(&source[error.span.start..error.span.end], found);
        if found.is_empty() {
            assert_eq!(error.span.start, source.len());
        }
        assert!(error.message.starts_with(&format!("expected {expected},")));
    }
}

#[test]
fn duplicate_commas_and_empty_groups_have_distinct_diagnostics() {
    for source in ["bind (left,, right) = item;", "bind {x: left,,} = item;"] {
        let error = scarp::parse(source).unwrap_err();
        let duplicate = source.find(",,").unwrap() + 1;
        assert_eq!(error.kind, DiagnosticKind::UnexpectedToken);
        assert_eq!(
            (error.span.start, error.span.end),
            (duplicate, duplicate + 1)
        );
        assert!(error
            .message
            .contains("where a destructuring pattern member is required"));
    }
    for source in ["bind () = item;", "bind [] = item;", "bind {} = item;"] {
        let error = scarp::parse(source).unwrap_err();
        assert_eq!(error.kind, DiagnosticKind::UnexpectedToken);
        assert!(error.message.contains("requires at least one member"));
    }
}
