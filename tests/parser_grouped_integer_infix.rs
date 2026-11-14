//! Regression for the iterative, stack-bounded integer-infix grouping path.
//! The parser uses the existing infix precedence and Span semantics and falls
//! back to the full grammar for all unsupported operand forms.
use scarp::{
    ast::{ExpressionKind as E, PrintArgument, Statement},
    diagnostics::DiagnosticKind,
    parser::MAX_EXPRESSION_DEPTH,
};
fn print_expression(statement: &Statement) -> &scarp::ast::Expression {
    match statement {
        Statement::Println {
            argument: PrintArgument::Expression(expression),
            ..
        } => expression,
        Statement::Output { arguments, .. } => match &arguments[0] {
            PrintArgument::Expression(expression) => expression,
            _ => panic!("expected output expression"),
        },
        _ => panic!("expected output statement"),
    }
}

fn output(source: &str) -> Vec<u8> {
    let mut out = Vec::new();
    scarp::run(source, &mut out).unwrap();
    out
}
#[test]
fn nested_integer_binary_parentheses_preserve_or_and_precedence() {
    let source = "print((1|(2&(3|4))));";
    assert_eq!(output(source), b"3\n");
    let source = "print(((((1|2)&(7^3)) | (8 >> 2))));";
    assert_eq!(output(source), b"2\n");
}
#[test]
fn nested_groups_preserve_arithmetic_shift_precedence() {
    let source = "print(((((1+2)*3) << 2)));";
    assert_eq!(output(source), b"36\n");
    let source = "print((((100/4)+5)%7));";
    assert_eq!(output(source), b"2\n");
}
#[test]
fn pure_nested_parentheses_keep_integers_unchanged() {
    let source = "print((((((42))))));";
    assert_eq!(output(source), b"42\n");
    let source = "print(((((-42)))));";
    assert_eq!(output(source), b"-42\n");
}
#[test]
fn overlapping_binary_groups_retire_operators_in_correct_order() {
    assert_eq!(output("print((((1+2)*3)+((4+5)*6)));"), b"63\n");
    assert_eq!(output("print(((((16>>2)+3)<<1)&15));"), b"14\n");
}
#[test]
fn nested_groups_with_noninteger_atoms_fall_back_cleanly() {
    assert_eq!(output("print((((true))));"), b"true\n");
    assert_eq!(output("print((((1.5))));"), b"1.5\n");
    assert_eq!(output(r#"print((((("hello")))));"#), b"hello\n");
}
#[test]
fn nested_groups_with_calls_keep_full_function_semantics() {
    let source = r#"
        pure funcra twice(x: integer) -> integer { return x * 2; }
        print(((((twice(3))))));
    "#;
    assert_eq!(output(source), b"6\n");
}
#[test]
fn comma_tuple_forms_are_not_reinterpreted_as_arithmetic_groups() {
    let source = "print(((((1,2)))));";
    assert_eq!(output(source), b"(1, 2)\n");
}
#[test]
fn oversized_nested_expressions_retain_the_original_depth_diagnostic() {
    let source = format!(
        "print({}1{});",
        "(".repeat(MAX_EXPRESSION_DEPTH + 1),
        ")".repeat(MAX_EXPRESSION_DEPTH + 1),
    );
    let error = scarp::parse(&source).unwrap_err();
    assert_eq!(error.kind, DiagnosticKind::ExpressionNesting);
    assert_eq!(&source[error.span.start..error.span.end], "(");
}
#[test]
fn exactly_allowed_expression_depth_parses_in_explicit_groups() {
    let source = format!(
        "print({}1{});",
        "(".repeat(MAX_EXPRESSION_DEPTH),
        ")".repeat(MAX_EXPRESSION_DEPTH),
    );
    assert_eq!(output(&source), b"1\n");
}
#[test]
fn grouped_integer_grammar_rejects_missing_operand_without_panicking() {
    for source in [
        "print(((((1+)))));",
        "print(((((1|)))));",
        "print((((1|2)));",
        "print((((1 2))));",
    ] {
        assert!(scarp::parse(source).is_err(), "{source}");
    }
}
#[test]
fn nested_integer_literal_boundaries_keep_original_diagnostics() {
    let source = "print((((0x10000000000000000))));";
    let error = scarp::parse(source).unwrap_err();
    assert_eq!(error.kind, DiagnosticKind::InvalidInteger);
}
#[test]
fn original_group_ast_spans_are_preserved_under_iterative_lowering() {
    let source = "print(((((42)))));";
    let ast = scarp::parse(source).unwrap();
    let expression = print_expression(&ast.statements[0]);
    let mut root = expression.root();
    let mut groups = 0;
    while let E::Group { expression: inner } = &expression.nodes()[root].kind {
        let span = expression.nodes()[root].span;
        assert!(source[span.start..span.end].starts_with('('));
        root = *inner;
        groups += 1;
    }
    // The first '(' opens print's argument list; four are AST Group nodes.
    assert_eq!(groups, 4);
    assert!(matches!(expression.nodes()[root].kind, E::Integer(42)));
}
#[test]
fn nested_infix_groups_preserve_exact_operator_span() {
    let source = "print((((1|2))))";
    let error = scarp::parse(source).unwrap_err();
    assert_eq!(error.kind, DiagnosticKind::UnexpectedToken);
    let valid = "print((((1|2))));";
    let ast = scarp::parse(valid).unwrap();
    let expr = print_expression(&ast.statements[0]);
    let op = expr
        .nodes()
        .iter()
        .find_map(|node| match &node.kind {
            E::Binary { operator_span, .. } => Some(operator_span),
            _ => None,
        })
        .unwrap();
    assert_eq!(&valid[op.start..op.end], "|");
}
#[test]
fn grouped_path_is_stable_for_unicode_trivia_and_whitespace() {
    let source = "// 🌍\nprint(((((1 + 2))))) ;";
    assert_eq!(output(source), b"3\n");
}
