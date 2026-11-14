//! Regression coverage for iterative consecutive-brace parsing.
//! These are semantic equivalence tests: compact brace-only chains must behave
//! exactly like ordinary nested statement blocks with real source byte spans.
use scarp::{
    ast::Statement,
    diagnostics::DiagnosticKind,
    parser::MAX_BLOCK_DEPTH,
};

fn output(source: &str) -> Vec<u8> {
    let mut bytes = Vec::new();
    scarp::run(source, &mut bytes).unwrap();
    bytes
}

#[test]
fn compact_nested_blocks_preserve_every_delimiter_byte_span() {
    let source = "{{{print(42);}}}";
    let program = scarp::parse(source).unwrap();
    assert_eq!(program.statements.len(), 1);
    let mut statement = &program.statements[0];
    for opening in 0..3 {
        let Statement::Block { statements, span } = statement else {
            panic!("expected nested block at layer {opening}");
        };
        assert_eq!(span.start, opening);
        assert_eq!(span.end, source.len() - opening);
        assert_eq!(statements.len(), 1);
        statement = &statements[0];
    }
    assert!(matches!(statement, Statement::Println { .. } | Statement::Output { .. }));
}
#[test]
fn exactly_maximum_nested_braces_are_accepted() {
    let source = format!(
        "{}print(42);{}",
        "{".repeat(MAX_BLOCK_DEPTH),
        "}".repeat(MAX_BLOCK_DEPTH),
    );
    let program = scarp::parse(&source).unwrap();
    assert_eq!(program.statements.len(), 1);
    assert_eq!(output(&source), b"42\n");
}
#[test]
fn one_excessive_block_is_rejected_with_existing_diagnostic() {
    let source = format!(
        "{}print(42);{}",
        "{".repeat(MAX_BLOCK_DEPTH + 1),
        "}".repeat(MAX_BLOCK_DEPTH + 1),
    );
    let error = scarp::parse(&source).unwrap_err();
    assert_eq!(error.kind, DiagnosticKind::BlockNesting);
    assert_eq!(
        &source[error.span.start..error.span.end],
        "{",
    );
}
#[test]
fn brace_chain_with_outer_siblings_uses_general_statement_parser() {
    let source = "{{print(1);} print(2);}";
    assert_eq!(output(source), b"1\n2\n");
    let program = scarp::parse(source).unwrap();
    let Statement::Block { statements, .. } = &program.statements[0] else {
        panic!("expected outer block");
    };
    assert_eq!(statements.len(), 2);
    assert!(matches!(statements[0], Statement::Block { .. }));
    assert!(matches!(statements[1], Statement::Output { .. } | Statement::Println { .. }));
}
#[test]
fn brace_chain_with_nested_conditionals_preserves_all_branches() {
    let source = "{{ if true { print(1); } else { print(99); } print(2); }}";
    assert_eq!(output(source), b"1\n2\n");
}
#[test]
fn nested_blocks_with_destructuring_preserve_scope_retirement() {
    let source = "{{ {bind (a, b) = (3, 4); print(a+b);} } print(7);";
    assert_eq!(output(source), b"7\n7\n");
}
#[test]
fn all_nested_blocks_without_statements_are_valid() {
    assert!(scarp::parse("{{{{}}}}").is_ok());
    assert_eq!(output("{{{{}}}}"), b"");
}
#[test]
fn missing_closing_delimiters_still_error_cleanly() {
    for source in [
        "{{ print(1); }",
        "{{{ print(1); }}",
        "{{{ print(1); }",
    ] {
        assert!(scarp::parse(source).is_err(), "{source}");
    }
}
#[test]
fn unicode_trivia_does_not_shift_the_nested_byte_spans() {
    let source = "// 🌍\n{{print(\"हिंदी\");}}";
    let ast = scarp::parse(source).unwrap();
    let Statement::Block { statements, span } = &ast.statements[0] else {
        panic!("expected outer block");
    };
    assert_eq!(span.start, source.find('{').unwrap());
    assert_eq!(span.end, source.len());
    let Statement::Block { span, .. } = &statements[0] else {
        panic!("expected inner block");
    };
    assert_eq!(span.start, source.find("{{").unwrap() + 1);
    assert_eq!(span.end, source.len() - 1);
    assert_eq!(output(source), "हिंदी\n".as_bytes());
}
