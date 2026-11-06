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
    pub(crate) operation_count: usize,
    pub(crate) expression_nodes: usize,
    pub(crate) literal_bytes: usize,
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
        operation_count: program.statements.len(),
        expression_nodes,
        literal_bytes,
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
        assert_eq!(requirements.operation_count, program.statements.len());
        assert_eq!(requirements.expression_nodes, 4);
        assert_eq!(requirements.literal_bytes, 2);
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
            assert_eq!(diagnostic.kind, DiagnosticKind::NativeUnsupported, "{source}");
        }
    }

    #[test]
    fn scalar_only_entry_with_no_bindings_still_has_one_storage_slot() {
        let program = compile("print(true);").unwrap();
        let requirements = inspect(&program).unwrap();
        assert_eq!(requirements.slot_count, 1);
        assert_eq!(requirements.temporary_count, 1);
        assert_eq!(requirements.literal_bytes, 0);
    }
}
