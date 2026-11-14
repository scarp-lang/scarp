//! Source-relative AST span relocation for the opt-in parse cache.
//! Keep the exhaustive variant matches updated when the AST expands.

use super::{
    ConditionalBranch, Expression, ExpressionKind, FunctionDeclaration, PrintArgument, Program,
    Statement,
};
use crate::diagnostics::Span;

/// Relocate every span and return the number of statement/expression nodes.
/// Parser depth limits bound the recursive statement walk; expressions are flat.
pub(crate) fn relocate(program: &mut Program, base: usize, to_local: bool) -> usize {
    fn span(value: &mut Span, base: usize, to_local: bool) {
        let move_one = |point: usize| {
            if to_local {
                point
                    .checked_sub(base)
                    .expect("parser span precedes source")
            } else {
                point.checked_add(base).expect("source-map span overflow")
            }
        };
        value.start = move_one(value.start);
        value.end = move_one(value.end);
    }
    fn expr(value: &mut Expression, base: usize, to_local: bool, nodes: &mut usize) {
        *nodes = nodes.saturating_add(value.nodes.len());
        for node in &mut value.nodes {
            span(&mut node.span, base, to_local);
            match &mut node.kind {
                ExpressionKind::Index { operator_span, .. }
                | ExpressionKind::Negate { operator_span, .. }
                | ExpressionKind::BitNot { operator_span, .. }
                | ExpressionKind::Not { operator_span, .. }
                | ExpressionKind::Logical { operator_span, .. }
                | ExpressionKind::Binary { operator_span, .. } => {
                    span(operator_span, base, to_local)
                }
                ExpressionKind::UserCall {
                    function_span,
                    arguments,
                    ..
                } => {
                    span(function_span, base, to_local);
                    for argument in arguments {
                        if let Some((_, label_span)) = &mut argument.label {
                            span(label_span, base, to_local);
                        }
                    }
                }
                ExpressionKind::Conditional { .. }
                | ExpressionKind::Tuple { .. }
                | ExpressionKind::Array { .. }
                | ExpressionKind::Integers { .. }
                | ExpressionKind::Vector { .. }
                | ExpressionKind::Integer(_)
                | ExpressionKind::Float(_)
                | ExpressionKind::Boolean(_)
                | ExpressionKind::String(_)
                | ExpressionKind::Name(_)
                | ExpressionKind::Group { .. } => {}
            }
        }
    }
    fn prints(args: &mut [PrintArgument], base: usize, to_local: bool, nodes: &mut usize) {
        for arg in args {
            match arg {
                PrintArgument::String(_) => {}
                PrintArgument::Expression(value) => expr(value, base, to_local, nodes),
            }
        }
    }
    fn statements(items: &mut [Statement], base: usize, to_local: bool, nodes: &mut usize) {
        for item in items {
            *nodes = nodes.saturating_add(1);
            match item {
                Statement::Import {
                    path_span,
                    alias_span,
                    span: whole,
                    ..
                } => {
                    span(path_span, base, to_local);
                    span(alias_span, base, to_local);
                    span(whole, base, to_local);
                }
                Statement::Export {
                    declaration,
                    span: whole,
                } => {
                    span(whole, base, to_local);
                    statements(
                        std::slice::from_mut(declaration.as_mut()),
                        base,
                        to_local,
                        nodes,
                    );
                }
                Statement::Function(FunctionDeclaration {
                    name_span,
                    parameters,
                    contracts,
                    statements: body,
                    span: whole,
                    ..
                }) => {
                    span(name_span, base, to_local);
                    span(whole, base, to_local);
                    for parameter in parameters {
                        span(&mut parameter.span, base, to_local);
                        if let Some(default) = &mut parameter.default {
                            expr(default, base, to_local, nodes);
                        }
                    }
                    for contract in contracts {
                        span(&mut contract.span, base, to_local);
                        expr(&mut contract.expression, base, to_local, nodes);
                    }
                    statements(body, base, to_local, nodes);
                }
                Statement::Return { value, span: whole } => {
                    span(whole, base, to_local);
                    if let Some(value) = value {
                        expr(value, base, to_local, nodes);
                    }
                }
                Statement::Call {
                    expression,
                    span: whole,
                }
                | Statement::Assert {
                    condition: expression,
                    span: whole,
                    ..
                } => {
                    span(whole, base, to_local);
                    expr(expression, base, to_local, nodes);
                }
                Statement::Repeat {
                    count,
                    index_span,
                    statements: body,
                    span: whole,
                    ..
                } => {
                    span(index_span, base, to_local);
                    span(whole, base, to_local);
                    expr(count, base, to_local, nodes);
                    statements(body, base, to_local, nodes);
                }
                Statement::While {
                    condition,
                    statements: body,
                    span: whole,
                } => {
                    span(whole, base, to_local);
                    expr(condition, base, to_local, nodes);
                    statements(body, base, to_local, nodes);
                }
                Statement::Break { span: whole } | Statement::Continue { span: whole } => {
                    span(whole, base, to_local);
                }
                Statement::If {
                    branches,
                    otherwise,
                    span: whole,
                } => {
                    span(whole, base, to_local);
                    for ConditionalBranch {
                        condition,
                        statements: body,
                        span: branch_span,
                    } in branches
                    {
                        span(branch_span, base, to_local);
                        expr(condition, base, to_local, nodes);
                        statements(body, base, to_local, nodes);
                    }
                    statements(otherwise, base, to_local, nodes);
                }
                Statement::Block {
                    statements: body,
                    span: whole,
                } => {
                    span(whole, base, to_local);
                    statements(body, base, to_local, nodes);
                }
                Statement::Println {
                    argument,
                    span: whole,
                } => {
                    span(whole, base, to_local);
                    prints(std::slice::from_mut(argument), base, to_local, nodes);
                }
                Statement::Output {
                    arguments,
                    span: whole,
                    ..
                } => {
                    span(whole, base, to_local);
                    prints(arguments, base, to_local, nodes);
                }
                Statement::Declaration {
                    name_span,
                    annotation,
                    initializer,
                    span: whole,
                    ..
                } => {
                    span(name_span, base, to_local);
                    span(whole, base, to_local);
                    if let Some(annotation) = annotation {
                        span(&mut annotation.span, base, to_local);
                    }
                    expr(initializer, base, to_local, nodes);
                }
                Statement::Destructure {
                    bindings,
                    initializer,
                    span: whole,
                    ..
                } => {
                    span(whole, base, to_local);
                    for (_, name_span) in bindings {
                        span(name_span, base, to_local);
                    }
                    expr(initializer, base, to_local, nodes);
                }
                Statement::Assign {
                    name_span,
                    operator_span,
                    value,
                    span: whole,
                    ..
                } => {
                    span(name_span, base, to_local);
                    span(operator_span, base, to_local);
                    span(whole, base, to_local);
                    expr(value, base, to_local, nodes);
                }
            }
        }
    }
    let mut nodes = 0;
    statements(&mut program.statements, base, to_local, &mut nodes);
    nodes
}

#[cfg(test)]
mod tests {
    use super::relocate;
    use crate::{lexer, parser};

    #[test]
    fn relocation_round_trips_nested_control_flow_and_all_expression_fields() {
        let source = r#"
import "x.srp" as x;
pub pure funcra calculate(n: integer, factor: integer=n)->integer
 needs n >= 0; ensures result >= n; {
    var value: integer = n + factor;
    if value > 0 { value += 1; } elif false { value = 3; } else { value = 4; }
    repeat 2 as index { if index == 1 { continue; } }
    while false { break; }
    return value;
}
bind values=[1,2,3];
bind array:array<array<integer, 0>, 1>=[array: [array: <integer>]];
print(array[0]);
print(values[1] + calculate(3, factor: 4));
"#;
        let original = parser::parse(&lexer::lex(source).unwrap()).unwrap();
        let mut shifted = original.clone();
        let nodes = relocate(&mut shifted, 317, false);
        assert!(nodes > 20);
        assert_ne!(shifted, original);
        assert_eq!(relocate(&mut shifted, 317, true), nodes);
        assert_eq!(shifted, original);
    }
}
