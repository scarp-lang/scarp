//! Private, bounded scalar program IR lowered from the canonical resolved plan.
//! No interpreter fallback, target ABI, public commands, or native object output.

mod definite_slots;

#[cfg(test)]
mod execute;

use super::{invalid, ScalarExpressionIr};
use crate::{
    ast::{BinaryOperator, OutputTarget},
    diagnostics::{Diagnostic, Span},
    resolver::{ResolvedArgument, ResolvedProgram, ResolvedStatement},
    value::ValueType,
};

const MAX_PROGRAM_STEPS: usize = 4096;
const MAX_LITERAL_BYTES: usize = 65536;
const MAX_EXPRESSION_NODES: usize = 16384;

#[derive(Debug, Clone, PartialEq, Eq)]
struct ProgramStep {
    span: Span,
    /// The interpreter charges one work step per resolved operation except
    /// retirement. This is an accounting event, not a loop-iteration bound.
    charge_work: bool,
    kind: ProgramOp,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ProgramOp {
    Store {
        slot: usize,
        value: ScalarExpressionIr,
    },
    Update {
        slot: usize,
        operator: BinaryOperator,
        operator_span: Span,
        value: ScalarExpressionIr,
    },
    RepeatInit {
        count: ScalarExpressionIr,
        count_slot: usize,
        index_slot: usize,
    },
    RepeatGuard {
        count_slot: usize,
        index_slot: usize,
        target: usize,
    },
    RepeatNext {
        index_slot: usize,
    },
    JumpIfFalse {
        condition: ScalarExpressionIr,
        target: usize,
    },
    Jump {
        target: usize,
    },
    LoopControl {
        target: usize,
        retire: Vec<usize>,
    },
    Retire {
        slots: Vec<usize>,
    },
    PrintValue {
        value: ScalarExpressionIr,
    },
    PrintLiteral {
        text: Vec<u8>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ScalarProgramIr {
    slots: Vec<ValueType>,
    steps: Vec<ProgramStep>,
}

impl ScalarProgramIr {
    fn expression(
        expression: &crate::resolver::ResolvedExpression,
        slots: &[ValueType],
    ) -> Result<ScalarExpressionIr, Diagnostic> {
        ScalarExpressionIr::lower(expression, slots)
    }

    pub(super) fn lower(program: &ResolvedProgram) -> Result<Self, Diagnostic> {
        let slots = program.binding_types().to_vec();
        if slots.len() > 256 || program.statements.len() > MAX_PROGRAM_STEPS {
            return Err(invalid(Span::default()));
        }
        let mut steps = Vec::with_capacity(program.statements.len());
        let mut total_nodes = 0usize;
        let mut literal_bytes = 0usize;
        for statement in &program.statements {
            let kind = match &statement.kind {
                ResolvedStatement::Store { slot, value } => ProgramOp::Store {
                    slot: *slot,
                    value: Self::expression(value, &slots)?,
                },
                ResolvedStatement::Update {
                    slot,
                    operator,
                    operator_span,
                    value,
                } => ProgramOp::Update {
                    slot: *slot,
                    operator: *operator,
                    operator_span: *operator_span,
                    value: Self::expression(value, &slots)?,
                },
                ResolvedStatement::RepeatInit {
                    count,
                    count_slot,
                    index_slot,
                } => ProgramOp::RepeatInit {
                    count: Self::expression(count, &slots)?,
                    count_slot: *count_slot,
                    index_slot: *index_slot,
                },
                ResolvedStatement::RepeatGuard {
                    count_slot,
                    index_slot,
                    target,
                } => ProgramOp::RepeatGuard {
                    count_slot: *count_slot,
                    index_slot: *index_slot,
                    target: *target,
                },
                ResolvedStatement::RepeatNext { index_slot } => ProgramOp::RepeatNext {
                    index_slot: *index_slot,
                },
                ResolvedStatement::JumpIfFalse { condition, target } => ProgramOp::JumpIfFalse {
                    condition: Self::expression(condition, &slots)?,
                    target: *target,
                },
                ResolvedStatement::Jump { target } => ProgramOp::Jump { target: *target },
                ResolvedStatement::LoopControl { target, retire } => ProgramOp::LoopControl {
                    target: *target,
                    retire: retire.clone(),
                },
                ResolvedStatement::Retire { slots } => ProgramOp::Retire {
                    slots: slots.clone(),
                },
                ResolvedStatement::Output {
                    arguments,
                    target: OutputTarget::Stdout,
                    newline: true,
                } if arguments.len() == 1 => match &arguments[0] {
                    ResolvedArgument::Expression(value) => ProgramOp::PrintValue {
                        value: Self::expression(value, &slots)?,
                    },
                    ResolvedArgument::String(text) => {
                        literal_bytes = literal_bytes
                            .checked_add(text.len())
                            .ok_or_else(|| invalid(statement.span))?;
                        if literal_bytes > MAX_LITERAL_BYTES {
                            return Err(invalid(statement.span));
                        }
                        ProgramOp::PrintLiteral {
                            text: text.as_bytes().to_vec(),
                        }
                    }
                },
                _ => return Err(invalid(statement.span)),
            };
            let expression_nodes = match &kind {
                ProgramOp::Store { value, .. }
                | ProgramOp::Update { value, .. }
                | ProgramOp::RepeatInit { count: value, .. }
                | ProgramOp::PrintValue { value } => value.registers.len(),
                ProgramOp::JumpIfFalse { condition, .. } => condition.registers.len(),
                _ => 0,
            };
            total_nodes = total_nodes
                .checked_add(expression_nodes)
                .ok_or_else(|| invalid(statement.span))?;
            if total_nodes > MAX_EXPRESSION_NODES {
                return Err(invalid(statement.span));
            }
            let charge_work = !matches!(kind, ProgramOp::Retire { .. });
            steps.push(ProgramStep {
                span: statement.span,
                charge_work,
                kind,
            });
        }
        let ir = Self { slots, steps };
        ir.verify()?;
        Ok(ir)
    }

    /// Only one declaration's pure integer initializer is currently eligible
    /// for the experimental ELF kernel ABI; this is not whole-program emission.
    pub(super) fn single_integer_kernel(&self) -> Result<&ScalarExpressionIr, Diagnostic> {
        if self.slots != [ValueType::Integer] || self.steps.len() != 1 {
            return Err(invalid(Span::default()));
        }
        match &self.steps[0].kind {
            ProgramOp::Store { slot: 0, value }
                if value.registers.get(value.root) == Some(&ValueType::Integer) =>
            {
                Ok(value)
            }
            _ => Err(invalid(self.steps[0].span)),
        }
    }

    fn check_slot(&self, slot: usize, kind: ValueType, span: Span) -> Result<(), Diagnostic> {
        if self.slots.get(slot) == Some(&kind) {
            Ok(())
        } else {
            Err(invalid(span))
        }
    }

    fn check_expression(
        &self,
        expression: &ScalarExpressionIr,
        kind: Option<ValueType>,
        span: Span,
    ) -> Result<(), Diagnostic> {
        expression.verify()?;
        if expression.slots != self.slots
            || kind.is_some_and(|expected| expression.registers[expression.root] != expected)
        {
            return Err(invalid(span));
        }
        Ok(())
    }

    /// Check every operation, including currently unreachable source code.
    /// Then walk possible statement edges without executing user expressions.
    pub(super) fn verify(&self) -> Result<(), Diagnostic> {
        let len = self.steps.len();
        if len > MAX_PROGRAM_STEPS || self.slots.len() > 256 {
            return Err(invalid(Span::default()));
        }
        let mut literal_bytes = 0usize;
        let mut total_nodes = 0usize;
        for instruction in &self.steps {
            let span = instruction.span;
            if span.start > span.end
                || instruction.charge_work == matches!(instruction.kind, ProgramOp::Retire { .. })
            {
                return Err(invalid(span));
            }
            let expression = match &instruction.kind {
                ProgramOp::Store { slot, value } => {
                    self.check_expression(value, None, span)?;
                    self.check_slot(*slot, value.registers[value.root], span)?;
                    Some(value)
                }
                ProgramOp::Update {
                    slot,
                    operator,
                    operator_span,
                    value,
                } => {
                    if operator.is_comparison() || operator_span.start > operator_span.end {
                        return Err(invalid(span));
                    }
                    self.check_slot(*slot, ValueType::Integer, span)?;
                    self.check_expression(value, Some(ValueType::Integer), span)?;
                    Some(value)
                }
                ProgramOp::RepeatInit {
                    count,
                    count_slot,
                    index_slot,
                } => {
                    self.check_slot(*count_slot, ValueType::Integer, span)?;
                    self.check_slot(*index_slot, ValueType::Integer, span)?;
                    self.check_expression(count, Some(ValueType::Integer), span)?;
                    Some(count)
                }
                ProgramOp::RepeatGuard {
                    count_slot,
                    index_slot,
                    target,
                } => {
                    self.check_slot(*count_slot, ValueType::Integer, span)?;
                    self.check_slot(*index_slot, ValueType::Integer, span)?;
                    if *target > len {
                        return Err(invalid(span));
                    }
                    None
                }
                ProgramOp::RepeatNext { index_slot } => {
                    self.check_slot(*index_slot, ValueType::Integer, span)?;
                    None
                }
                ProgramOp::JumpIfFalse { condition, target } => {
                    self.check_expression(condition, Some(ValueType::Boolean), span)?;
                    if *target > len {
                        return Err(invalid(span));
                    }
                    Some(condition)
                }
                ProgramOp::Jump { target } => {
                    if *target > len {
                        return Err(invalid(span));
                    }
                    None
                }
                ProgramOp::LoopControl { target, retire } => {
                    if *target > len || retire.iter().any(|slot| *slot >= self.slots.len()) {
                        return Err(invalid(span));
                    }
                    None
                }
                ProgramOp::Retire { slots } => {
                    if slots.iter().any(|slot| *slot >= self.slots.len()) {
                        return Err(invalid(span));
                    }
                    None
                }
                ProgramOp::PrintValue { value } => {
                    self.check_expression(value, None, span)?;
                    Some(value)
                }
                ProgramOp::PrintLiteral { text } => {
                    literal_bytes = literal_bytes
                        .checked_add(text.len())
                        .ok_or_else(|| invalid(span))?;
                    if literal_bytes > MAX_LITERAL_BYTES || std::str::from_utf8(text).is_err() {
                        return Err(invalid(span));
                    }
                    None
                }
            };
            if let Some(value) = expression {
                total_nodes = total_nodes
                    .checked_add(value.registers.len())
                    .ok_or_else(|| invalid(span))?;
                if total_nodes > MAX_EXPRESSION_NODES {
                    return Err(invalid(span));
                }
            }
        }

        // The definite-slot dataflow performs bounded CFG reachability,
        // intersects all predecessors, and rejects reads that can bypass a
        // Store/RepeatInit or follow binding retirement.
        self.verify_definite_slots()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{compile, diagnostics::DiagnosticKind};

    fn program(source: &str) -> ScalarProgramIr {
        let resolved = compile(source).unwrap();
        crate::native_preflight::inspect(&resolved).unwrap();
        ScalarProgramIr::lower(&resolved).unwrap()
    }

    #[test]
    fn deterministic_control_flow_and_utf8_literals() {
        let source =
            "var n=0; while n<3 { n+=1; if n==2 { continue; } print(n); } print(\"नमस्ते\");";
        let a = program(source);
        assert_eq!(a, program(source));
        assert!(a
            .steps
            .iter()
            .any(|step| matches!(step.kind, ProgramOp::JumpIfFalse { .. })));
        assert!(a
            .steps
            .iter()
            .any(|step| matches!(step.kind, ProgramOp::LoopControl { .. })));
        assert!(a.steps.iter().any(|step| matches!(
            &step.kind,
            ProgramOp::PrintLiteral { text } if text == "नमस्ते".as_bytes()
        )));
        a.verify().unwrap();
    }

    #[test]
    fn counted_repeat_and_scope_retirement_keep_explicit_charges() {
        let ir = program("repeat 4 as i { bind copy=i; print(copy); }");
        assert!(ir
            .steps
            .iter()
            .any(|step| matches!(step.kind, ProgramOp::RepeatGuard { .. })));
        assert!(ir
            .steps
            .iter()
            .any(|step| matches!(step.kind, ProgramOp::RepeatNext { .. })));
        assert!(ir
            .steps
            .iter()
            .any(|step| { matches!(step.kind, ProgramOp::Retire { .. }) && !step.charge_work }));
        assert!(ir
            .steps
            .iter()
            .all(|step| { step.charge_work == !matches!(step.kind, ProgramOp::Retire { .. }) }));
    }

    #[test]
    fn corrupt_branch_and_retirement_slots_fail_without_panicking() {
        let mut ir = program("var i=0; while i<2 { bind j=i; i+=1; }");
        let count = ir.steps.len();
        for step in &mut ir.steps {
            if let ProgramOp::JumpIfFalse { target, .. } = &mut step.kind {
                *target = count + 1;
                break;
            }
        }
        assert_eq!(
            ir.verify().unwrap_err().kind,
            DiagnosticKind::NativeUnsupported
        );

        let mut ir = program("{ bind x=1; }");
        for step in &mut ir.steps {
            if let ProgramOp::Retire { slots } = &mut step.kind {
                slots.push(usize::MAX);
            }
        }
        assert_eq!(
            ir.verify().unwrap_err().kind,
            DiagnosticKind::NativeUnsupported
        );

        let mut ir = program("print(1);");
        ir.steps[0].charge_work = false;
        assert_eq!(
            ir.verify().unwrap_err().kind,
            DiagnosticKind::NativeUnsupported
        );
    }

    #[test]
    fn rejects_corrupted_print_payload_and_expression_registers() {
        let mut ir = program("print(\"é\");");
        for step in &mut ir.steps {
            if let ProgramOp::PrintLiteral { text } = &mut step.kind {
                text[0] = 0xff;
            }
        }
        assert_eq!(
            ir.verify().unwrap_err().kind,
            DiagnosticKind::NativeUnsupported
        );

        let mut ir = program("print(true and false);");
        if let ProgramOp::PrintValue { value } = &mut ir.steps[0].kind {
            value.root = usize::MAX;
        }
        assert_eq!(
            ir.verify().unwrap_err().kind,
            DiagnosticKind::NativeUnsupported
        );
    }

    #[test]
    fn matches_supported_scalar_program_families() {
        for source in [
            "",
            "print(1+2*3);",
            "print(true == false);",
            "print(false and (1/0 == 1));",
            "var sum=0; repeat 3 as i { sum+=i; } print(sum);",
            "var x=0; while x<2 { { bind y=x; print(y); } x+=1; }",
            "if true { print(\"नमस्ते\"); } else { print(3); }",
            "repeat 3 as i { if i == 1 { continue; } if i == 2 { break; } print(i); }",
        ] {
            let resolved = compile(source).unwrap();
            crate::native_preflight::inspect(&resolved).unwrap();
            ScalarProgramIr::lower(&resolved).unwrap();
        }
    }
}
