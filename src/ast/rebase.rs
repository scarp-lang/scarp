//! Source-relative AST span relocation for the opt-in parse cache.
//! Keep the exhaustive variant matches updated when the AST expands.

use super::{
    ConditionalBranch, Expression, ExpressionKind, FunctionDeclaration, PrintArgument, Program,
    Statement,
};
use crate::diagnostics::Span;

/// Relocate every span and return the number of statement, expression and pattern nodes.
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
                ExpressionKind::Variant(constructor) => {
                    span(&mut constructor.name_span, base, to_local);
                    span(&mut constructor.tag_span, base, to_local);
                }
                ExpressionKind::Record {
                    name_span, fields, ..
                } => {
                    span(name_span, base, to_local);
                    *nodes = nodes.saturating_add(fields.len());
                    for field in fields {
                        if let Some((_, label_span)) = &mut field.label {
                            span(label_span, base, to_local);
                        }
                    }
                }
                ExpressionKind::Field { name_span, .. } => span(name_span, base, to_local),
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
                    type_arguments,
                    ..
                } => {
                    span(function_span, base, to_local);
                    if let Some(type_arguments) = type_arguments {
                        for argument in std::sync::Arc::make_mut(type_arguments) {
                            span(&mut argument.span, base, to_local);
                            *nodes = nodes.saturating_add(argument.kind.node_count());
                        }
                    }
                    for argument in arguments {
                        if let Some((_, label_span)) = &mut argument.label {
                            span(label_span, base, to_local);
                        }
                    }
                }
                ExpressionKind::Tensor { kind, .. } => {
                    *nodes = nodes.saturating_add(kind.node_count());
                }
                ExpressionKind::Conditional { .. }
                | ExpressionKind::Tuple { .. }
                | ExpressionKind::Array { .. }
                | ExpressionKind::List { .. }
                | ExpressionKind::Map { .. }
                | ExpressionKind::Integers { .. }
                | ExpressionKind::Vector { .. }
                | ExpressionKind::Integer(_)
                | ExpressionKind::Float(_)
                | ExpressionKind::Decimal(_)
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
    fn pattern(value: &mut super::BindingPattern, base: usize, to_local: bool, nodes: &mut usize) {
        *nodes = nodes.saturating_add(value.nodes.len());
        for node in &mut value.nodes {
            span(&mut node.span, base, to_local);
            if let super::PatternKind::Record(fields) = &mut node.kind {
                *nodes = nodes.saturating_add(fields.len());
                for field in fields {
                    span(&mut field.span, base, to_local);
                }
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
                Statement::Variant(variant) => {
                    span(&mut variant.name_span, base, to_local);
                    span(&mut variant.span, base, to_local);
                    *nodes = nodes.saturating_add(variant.cases.len());
                    for case in &mut variant.cases {
                        span(&mut case.span, base, to_local);
                        if let Some(ty) = &mut case.payload {
                            span(&mut ty.span, base, to_local);
                        }
                    }
                }
                Statement::Match(statement) => {
                    span(&mut statement.span, base, to_local);
                    expr(&mut statement.subject, base, to_local, nodes);
                    *nodes = nodes.saturating_add(statement.arms.len());
                    for arm in &mut statement.arms {
                        span(&mut arm.span, base, to_local);
                        span(&mut arm.tag_span, base, to_local);
                        if let Some(payload) = &mut arm.payload {
                            pattern(payload, base, to_local, nodes);
                        }
                        statements(&mut arm.statements, base, to_local, nodes);
                    }
                }
                Statement::Record(record) => {
                    span(&mut record.name_span, base, to_local);
                    span(&mut record.span, base, to_local);
                    *nodes = nodes.saturating_add(record.fields.len());
                    for (_, name_span, ty) in &mut record.fields {
                        span(name_span, base, to_local);
                        span(&mut ty.span, base, to_local);
                    }
                }
                Statement::Function(function) => {
                    let FunctionDeclaration {
                        name_span,
                        parameters,
                        type_parameters,
                        contracts,
                        statements: body,
                        span: whole,
                        ..
                    } = function.as_mut();
                    span(name_span, base, to_local);
                    span(whole, base, to_local);
                    *nodes = nodes.saturating_add(type_parameters.len());
                    for parameter in type_parameters {
                        span(&mut parameter.span, base, to_local);
                    }
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
                    propagate,
                    span: whole,
                    ..
                } => {
                    span(name_span, base, to_local);
                    span(whole, base, to_local);
                    if let Some(marker) = propagate {
                        span(marker, base, to_local);
                    }
                    if let Some(annotation) = annotation {
                        span(&mut annotation.span, base, to_local);
                    }
                    expr(initializer, base, to_local, nodes);
                }
                Statement::Destructure {
                    pattern: binding,
                    initializer,
                    span: whole,
                    ..
                } => {
                    span(whole, base, to_local);
                    pattern(binding, base, to_local, nodes);
                    expr(initializer, base, to_local, nodes);
                }
                Statement::Assign {
                    name_span,
                    index,
                    operator_span,
                    value,
                    span: whole,
                    ..
                } => {
                    span(name_span, base, to_local);
                    span(operator_span, base, to_local);
                    span(whole, base, to_local);
                    if let Some(index) = index {
                        expr(index, base, to_local, nodes);
                    }
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
