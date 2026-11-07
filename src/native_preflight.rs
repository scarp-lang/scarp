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
            ResolvedStatement::Store { slot, .. }
            | ResolvedStatement::Update { slot, .. } => *slot < slot_count,
            ResolvedStatement::RepeatInit {
                count_slot,
                index_slot,
                ..
            } => *count_slot < slot_count && *index_slot < slot_count,
            ResolvedStatement::RepeatGuard {
                count_slot,
                index_slot,
                target,
            } => {
                *count_slot < slot_count
                    && *index_slot < slot_count
                    && *target <= operation_count
            }
            ResolvedStatement::RepeatNext { index_slot } => *index_slot < slot_count,
            ResolvedStatement::JumpIfFalse { target, .. }
            | ResolvedStatement::Jump { target } => *target <= operation_count,
            ResolvedStatement::LoopControl { target, retire } => {
                *target <= operation_count && retire.iter().all(|slot| *slot < slot_count)
            }
            ResolvedStatement::Retire { slots } => {
                slots.iter().all(|slot| *slot < slot_count)
            }
            _ => true,
        };
        if !valid {
            return Err(unsupported(operation.span));
        }
    }
    Ok(())
}

/// A resolved expression is an immutable postorder tree. Verify the indices
/// used by the existing C emitter before it can index or unwrap them.
fn verify_expression_shape(
    expression: &crate::resolver::ResolvedExpression,
    slot_count: usize,
) -> Result<(), Diagnostic> {
    let nodes = expression.syntax.nodes();
    if expression.syntax.root() >= nodes.len() {
        return Err(unsupported(Span::default()));
    }
    for (index, node) in nodes.iter().enumerate() {
        if node.span.start > node.span.end {
            return Err(unsupported(node.span));
        }
        let valid = match &node.kind {
            ExpressionKind::Name(_) => expression
                .slots
                .get(index)
                .and_then(|slot| *slot)
                .is_some_and(|slot| slot < slot_count),
            ExpressionKind::Negate { operand, .. }
            | ExpressionKind::BitNot { operand, .. }
            | ExpressionKind::Not { operand, .. } => *operand < index,
            ExpressionKind::Group { expression, .. } => *expression < index,
            ExpressionKind::Binary { left, right, .. }
            | ExpressionKind::Logical { left, right, .. } => *left < index && *right < index,
            _ => true,
        };
        if !valid {
            return Err(unsupported(node.span));
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
            verify_expression_shape(expression, program.binding_count())?;
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
        assert_eq!(inspect(&program).unwrap_err().kind, DiagnosticKind::NativeUnsupported);
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
        assert_eq!(inspect(&program).unwrap_err().kind, DiagnosticKind::NativeUnsupported);
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
    fn scalar_only_entry_with_no_bindings_still_has_one_storage_slot() {
        let program = compile("print(true);").unwrap();
        let requirements = inspect(&program).unwrap();
        assert_eq!(requirements.slot_count, 1);
        assert_eq!(requirements.temporary_count, 1);
    }
}
