//! Shared, target-neutral preflight for the experimental scalar code generator.
//!
//! This is deliberately not a native compiler or a complete intermediate
//! representation. It validates the current resolver's immutable typed plan
//! without invoking a C compiler or creating artifacts, so subsequent native
//! backends can share exactly the same initial acceptance and resource gate.

use crate::{
    ast::{ExpressionKind, OutputTarget},
    diagnostics::{Diagnostic, DiagnosticKind, Span},
    resolver::{ResolvedArgument, ResolvedProgram, ResolvedStatement},
    value::ValueType,
};

const MAX_SCALAR_SLOTS: usize = 256;
const MAX_OPERATIONS: usize = 4096;
const MAX_EXPRESSION_NODES: usize = 1024;
const MAX_TOTAL_EXPRESSION_NODES: usize = 16384;
const MAX_LITERAL_BYTES: usize = 65536;

/// Backend workspace requirements after checking the exact supported subset.
/// These are logical counts, not a process-heap allocation bound.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ScalarRequirements {
    pub(crate) slot_count: usize,
    pub(crate) temporary_count: usize,
}

fn unsupported(span: Span) -> Diagnostic {
    Diagnostic::new(
        DiagnosticKind::NativeUnsupported,
        span,
        "program exceeds the experimental scalar C11 subset or its resource limits",
    )
    .with_help(
        "use scalar integer/boolean expressions and one-argument stdout print; see docs/proposals/0035-native-scalar-c11.md",
    )
}

fn verify_plan_shape(program: &ResolvedProgram) -> Result<(), Diagnostic> {
    let slot_count = program.binding_count();
    let operation_count = program.statements.len();
    for operation in &program.statements {
        if operation.span.start > operation.span.end {
            return Err(unsupported(operation.span));
        }
        let valid = match &operation.kind {
            ResolvedStatement::Store { slot, .. } | ResolvedStatement::Update { slot, .. } => {
                *slot < slot_count
            }
            ResolvedStatement::RepeatInit {
                count_slot,
                index_slot,
                ..
            } => *count_slot < slot_count && *index_slot < slot_count,
            ResolvedStatement::RepeatGuard {
                count_slot,
                index_slot,
                target,
            } => *count_slot < slot_count && *index_slot < slot_count && *target <= operation_count,
            ResolvedStatement::RepeatNext { index_slot } => *index_slot < slot_count,
            ResolvedStatement::JumpIfFalse { target, .. } | ResolvedStatement::Jump { target } => {
                *target <= operation_count
            }
            ResolvedStatement::LoopControl { target, retire } => {
                *target <= operation_count && retire.iter().all(|slot| *slot < slot_count)
            }
            ResolvedStatement::Retire { slots } => slots.iter().all(|slot| *slot < slot_count),
            _ => true,
        };
        if !valid {
            return Err(unsupported(operation.span));
        }
    }
    Ok(())
}

/// A resolved expression is an immutable postorder tree. The native backend
/// must not trust a plan whose scalar types, indices or side tables disagree.
/// This is a defensive verifier for an already type-checked plan, not a second
/// source-language resolver.
fn operand_type(types: &[ValueType], child: usize, span: Span) -> Result<ValueType, Diagnostic> {
    types.get(child).copied().ok_or_else(|| unsupported(span))
}

fn verify_expression_shape(
    expression: &crate::resolver::ResolvedExpression,
    binding_types: &[ValueType],
) -> Result<(), Diagnostic> {
    let nodes = expression.syntax.nodes();
    if nodes.len() > MAX_EXPRESSION_NODES
        || expression.syntax.root() >= nodes.len()
        || expression.slots.len() != nodes.len()
        || expression.types.len() != nodes.len()
        || expression.calls.len() != nodes.len()
    {
        return Err(unsupported(Span::default()));
    }

    let mut inferred = Vec::with_capacity(nodes.len());
    for (index, node) in nodes.iter().enumerate() {
        if node.span.start > node.span.end
            || expression.calls[index].is_some()
            || (!matches!(node.kind, ExpressionKind::Name(_))
                && expression.slots[index].is_some())
        {
            return Err(unsupported(node.span));
        }
        let kind = match &node.kind {
            ExpressionKind::Integer(_) => ValueType::Integer,
            ExpressionKind::Boolean(_) => ValueType::Boolean,
            ExpressionKind::Name(_) => {
                let slot = expression.slots[index].ok_or_else(|| unsupported(node.span))?;
                *binding_types
                    .get(slot)
                    .ok_or_else(|| unsupported(node.span))?
            }
            ExpressionKind::Group { expression: child } => {
                operand_type(&inferred, *child, node.span)?
            }
            ExpressionKind::Negate {
                operand,
                operator_span,
            }
            | ExpressionKind::BitNot {
                operand,
                operator_span,
            } => {
                if operator_span.start > operator_span.end
                    || operand_type(&inferred, *operand, node.span)? != ValueType::Integer
                {
                    return Err(unsupported(node.span));
                }
                ValueType::Integer
            }
            ExpressionKind::Not {
                operand,
                operator_span,
            } => {
                if operator_span.start > operator_span.end
                    || operand_type(&inferred, *operand, node.span)? != ValueType::Boolean
                {
                    return Err(unsupported(node.span));
                }
                ValueType::Boolean
            }
            ExpressionKind::Logical {
                left,
                right,
                operator_span,
                ..
            } => {
                if operator_span.start > operator_span.end
                    || operand_type(&inferred, *left, node.span)? != ValueType::Boolean
                    || operand_type(&inferred, *right, node.span)? != ValueType::Boolean
                {
                    return Err(unsupported(node.span));
                }
                ValueType::Boolean
            }
            ExpressionKind::Binary {
                left,
                right,
                operator,
                operator_span,
            } => {
                if operator_span.start > operator_span.end {
                    return Err(unsupported(node.span));
                }
                let left_kind = operand_type(&inferred, *left, node.span)?;
                let right_kind = operand_type(&inferred, *right, node.span)?;
                if left_kind != right_kind
                    || (!matches!(
                        operator,
                        crate::ast::BinaryOperator::Equal | crate::ast::BinaryOperator::NotEqual
                    ) && left_kind != ValueType::Integer)
                {
                    return Err(unsupported(node.span));
                }
                if operator.is_comparison() {
                    ValueType::Boolean
                } else {
                    ValueType::Integer
                }
            }
            _ => return Err(unsupported(node.span)),
        };
        if expression.types[index] != kind.id() {
            return Err(unsupported(node.span));
        }
        inferred.push(kind);
    }
    if inferred.get(expression.syntax.root()).copied() != Some(expression.kind) {
        return Err(unsupported(Span::default()));
    }
    Ok(())
}

/// The resolver already checked statements, but a native backend must reject
/// corrupted expression-to-slot and guard types before producing any output.
fn verify_operation_types(program: &ResolvedProgram) -> Result<(), Diagnostic> {
    let slots = program.binding_types();
    for operation in &program.statements {
        let valid = match &operation.kind {
            ResolvedStatement::Store { slot, value } => slots.get(*slot) == Some(&value.kind),
            ResolvedStatement::Update {
                slot,
                value,
                operator,
                operator_span,
            } => {
                slots.get(*slot) == Some(&ValueType::Integer)
                    && value.kind == ValueType::Integer
                    && !operator.is_comparison()
                    && operator_span.start <= operator_span.end
            }
            ResolvedStatement::RepeatInit {
                count,
                count_slot,
                index_slot,
            } => {
                count.kind == ValueType::Integer
                    && slots.get(*count_slot) == Some(&ValueType::Integer)
                    && slots.get(*index_slot) == Some(&ValueType::Integer)
            }
            ResolvedStatement::RepeatGuard {
                count_slot,
                index_slot,
                ..
            } => {
                slots.get(*count_slot) == Some(&ValueType::Integer)
                    && slots.get(*index_slot) == Some(&ValueType::Integer)
            }
            ResolvedStatement::RepeatNext { index_slot } => {
                slots.get(*index_slot) == Some(&ValueType::Integer)
            }
            ResolvedStatement::JumpIfFalse { condition, .. } => {
                condition.kind == ValueType::Boolean
            }
            _ => true,
        };
        if !valid {
            return Err(unsupported(operation.span));
        }
    }
    Ok(())
}

/// Reject unsupported constructs and bound native scalar workspace BEFORE
/// generating any code. This consumes only the current typed resolver plan:
/// no separate lexer, parser, type checker, loader or runtime evaluation.
pub(crate) fn inspect(program: &ResolvedProgram) -> Result<ScalarRequirements, Diagnostic> {
    if !program.functions.is_empty()
        || program.binding_count() > MAX_SCALAR_SLOTS
        || program.statements.len() > MAX_OPERATIONS
        || program
            .binding_types()
            .iter()
            .any(|kind| !matches!(kind, ValueType::Integer | ValueType::Boolean))
    {
        return Err(unsupported(Span::default()));
    }

    verify_plan_shape(program)?;
    verify_operation_types(program)?;

    let mut temporary_count = 1usize;
    let mut expression_nodes = 0usize;
    let mut literal_bytes = 0usize;
    for operation in &program.statements {
        let expression = match &operation.kind {
            ResolvedStatement::Store { value, .. }
            | ResolvedStatement::Update { value, .. }
            | ResolvedStatement::RepeatInit { count: value, .. } => Some(value),
            ResolvedStatement::JumpIfFalse { condition, .. } => Some(condition),
            ResolvedStatement::Output {
                arguments,
                target: OutputTarget::Stdout,
                newline: true,
            } if arguments.len() == 1 => match &arguments[0] {
                ResolvedArgument::Expression(value) => Some(value),
                ResolvedArgument::String(value) => {
                    literal_bytes = literal_bytes
                        .checked_add(value.len())
                        .ok_or_else(|| unsupported(operation.span))?;
                    None
                }
            },
            ResolvedStatement::Jump { .. }
            | ResolvedStatement::LoopControl { .. }
            | ResolvedStatement::Retire { .. }
            | ResolvedStatement::RepeatGuard { .. }
            | ResolvedStatement::RepeatNext { .. } => None,
            _ => return Err(unsupported(operation.span)),
        };
        if let Some(expression) = expression {
            verify_expression_shape(expression, program.binding_types())?;
            if !matches!(expression.kind, ValueType::Integer | ValueType::Boolean) {
                return Err(unsupported(operation.span));
            }
            for node in expression.syntax.nodes() {
                if !matches!(
                    node.kind,
                    ExpressionKind::Integer(_)
                        | ExpressionKind::Boolean(_)
                        | ExpressionKind::Name(_)
                        | ExpressionKind::Negate { .. }
                        | ExpressionKind::BitNot { .. }
                        | ExpressionKind::Not { .. }
                        | ExpressionKind::Binary { .. }
                        | ExpressionKind::Logical { .. }
                        | ExpressionKind::Group { .. }
                ) {
                    return Err(unsupported(node.span));
                }
            }
            let count = expression.syntax.nodes().len();
            temporary_count = temporary_count.max(count);
            expression_nodes = expression_nodes
                .checked_add(count)
                .ok_or_else(|| unsupported(operation.span))?;
        }
        if temporary_count > MAX_EXPRESSION_NODES
            || expression_nodes > MAX_TOTAL_EXPRESSION_NODES
            || literal_bytes > MAX_LITERAL_BYTES
        {
            return Err(unsupported(operation.span));
        }
    }
    Ok(ScalarRequirements {
        slot_count: program.binding_count().max(1),
        temporary_count,
    })
}

#[cfg(test)]
mod tests {
    use super::inspect;
    use crate::{compile, diagnostics::DiagnosticKind};

    #[test]
    fn counts_scalar_storage_and_utf8_literals_without_execution() {
        let program = compile("var count=7; print(count+1); print(\"é\");").unwrap();
        let requirements = inspect(&program).unwrap();
        assert_eq!(requirements.slot_count, 1);
        assert_eq!(requirements.temporary_count, 3);
    }

    #[test]
    fn rejects_unsupported_values_and_output_before_emission() {
        for source in [
            "print(1.5);",
            "print();",
            "eprint(1);",
            "write(1);",
            "print(1, 2);",
        ] {
            let program = compile(source).unwrap();
            let diagnostic = inspect(&program).unwrap_err();
            assert_eq!(
                diagnostic.kind,
                DiagnosticKind::NativeUnsupported,
                "{source}"
            );
        }
    }

    #[test]
    fn corrupted_jump_and_store_slots_fail_before_c_emission() {
        use crate::resolver::ResolvedStatement;

        let mut program = compile("var count=0; while(count<2) { count+=1; }").unwrap();
        let beyond_end = program.statements.len() + 1;
        let jump = program
            .statements
            .iter_mut()
            .find(|operation| matches!(operation.kind, ResolvedStatement::Jump { .. }))
            .expect("while loop must contain a jump");
        if let ResolvedStatement::Jump { target } = &mut jump.kind {
            *target = beyond_end;
        }
        assert_eq!(
            inspect(&program).unwrap_err().kind,
            DiagnosticKind::NativeUnsupported
        );
        assert_eq!(
            crate::c_backend::emit_program(&program).unwrap_err().kind,
            DiagnosticKind::NativeUnsupported
        );

        let mut program = compile("bind count=1; print(count);").unwrap();
        let out_of_bounds = program.binding_count();
        let store = program
            .statements
            .iter_mut()
            .find(|operation| matches!(operation.kind, ResolvedStatement::Store { .. }))
            .expect("binding must be stored");
        if let ResolvedStatement::Store { slot, .. } = &mut store.kind {
            *slot = out_of_bounds;
        }
        assert_eq!(
            inspect(&program).unwrap_err().kind,
            DiagnosticKind::NativeUnsupported
        );
    }

    #[test]
    fn corrupted_expression_references_fail_before_c_emission() {
        use crate::{
            ast::ExpressionKind,
            resolver::{ResolvedArgument, ResolvedStatement},
        };

        let mut program = compile("bind count=1; print(count);").unwrap();
        let invalid_slot = program.binding_count();
        for operation in &mut program.statements {
            if let ResolvedStatement::Output { arguments, .. } = &mut operation.kind {
                if let ResolvedArgument::Expression(expression) = &mut arguments[0] {
                    let name = expression
                        .syntax
                        .nodes()
                        .iter()
                        .position(|node| matches!(node.kind, ExpressionKind::Name(_)))
                        .expect("output reads a binding");
                    expression.slots[name] = Some(invalid_slot);
                }
            }
        }
        assert_eq!(
            crate::c_backend::emit_program(&program).unwrap_err().kind,
            DiagnosticKind::NativeUnsupported
        );

        let mut program = compile("print(1+2);").unwrap();
        for operation in &mut program.statements {
            if let ResolvedStatement::Output { arguments, .. } = &mut operation.kind {
                if let ResolvedArgument::Expression(expression) = &mut arguments[0] {
                    let bad_index = expression.syntax.nodes().len();
                    let binary = expression
                        .syntax
                        .nodes
                        .iter_mut()
                        .find(|node| matches!(node.kind, ExpressionKind::Binary { .. }))
                        .expect("output has a binary expression");
                    if let ExpressionKind::Binary { left, .. } = &mut binary.kind {
                        *left = bad_index;
                    }
                }
            }
        }
        assert_eq!(
            crate::c_backend::emit_program(&program).unwrap_err().kind,
            DiagnosticKind::NativeUnsupported
        );
    }

    #[test]
    fn corrupted_expression_type_metadata_is_rejected_before_emission() {
        use crate::{
            resolver::{ResolvedArgument, ResolvedStatement},
            value::ValueType,
        };

        for corruption in 0..6 {
            let mut program = compile("bind value=1; print(value + 2);").unwrap();
            let expression = program
                .statements
                .iter_mut()
                .find_map(|operation| match &mut operation.kind {
                    ResolvedStatement::Output { arguments, .. } => match &mut arguments[0] {
                        ResolvedArgument::Expression(value) => Some(value),
                        _ => None,
                    },
                    _ => None,
                })
                .unwrap();
            match corruption {
                0 => expression.kind = ValueType::Boolean,
                1 => expression.types[0] = ValueType::Boolean.id(),
                2 => {
                    expression.slots.clear();
                }
                3 => {
                    expression.calls.pop();
                }
                4 => {
                    expression.syntax.root = expression.syntax.nodes().len();
                }
                5 => {
                    expression.slots[1] = Some(0);
                }
                _ => unreachable!(),
            }
            assert_eq!(
                crate::c_backend::emit_program(&program).unwrap_err().kind,
                DiagnosticKind::NativeUnsupported,
                "corruption {corruption}"
            );
        }
    }

    #[test]
    fn corrupted_scalar_statement_types_fail_closed() {
        use crate::{ast::BinaryOperator, resolver::ResolvedStatement, value::ValueType};

        let mut program = compile("var count=1; count+=2;").unwrap();
        for operation in &mut program.statements {
            if let ResolvedStatement::Update { operator, .. } = &mut operation.kind {
                *operator = BinaryOperator::Equal;
            }
        }
        assert_eq!(
            inspect(&program).unwrap_err().kind,
            DiagnosticKind::NativeUnsupported
        );

        let mut program = compile("if true { print(1); }").unwrap();
        for operation in &mut program.statements {
            if let ResolvedStatement::JumpIfFalse { condition, .. } = &mut operation.kind {
                condition.kind = ValueType::Integer;
            }
        }
        assert_eq!(
            inspect(&program).unwrap_err().kind,
            DiagnosticKind::NativeUnsupported
        );

        let mut program = compile("repeat 2 as index { print(index); }").unwrap();
        for operation in &mut program.statements {
            if let ResolvedStatement::RepeatInit { count, .. } = &mut operation.kind {
                count.kind = ValueType::Boolean;
            }
        }
        assert_eq!(
            inspect(&program).unwrap_err().kind,
            DiagnosticKind::NativeUnsupported
        );
    }

    #[test]
    fn scalar_boolean_comparisons_and_short_circuit_remain_accepted() {
        for source in [
            "print(true == false);",
            "print(1 < 2);",
            "print(not false and (true or false));",
            "print(false and (1 / 0 == 1));",
            "print(4 & 3); print(5 >> 1);",
            "repeat 2 as i { print(i == 1); }",
        ] {
            let program = compile(source).unwrap();
            inspect(&program).unwrap();
            crate::c_backend::emit_program(&program).unwrap();
        }
    }

    #[test]
    fn scalar_only_entry_with_no_bindings_still_has_one_storage_slot() {
        let program = compile("print(true);").unwrap();
        let requirements = inspect(&program).unwrap();
        assert_eq!(requirements.slot_count, 1);
        assert_eq!(requirements.temporary_count, 1);
    }
}
