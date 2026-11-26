//! Private, bounded scalar program IR lowered from the canonical resolved plan.
//! No interpreter fallback, target ABI, public commands, or native object output.

mod calls;
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

pub(super) use self::calls::ScalarCallGraph;

const MAX_PROGRAM_SLOTS: usize = 256;
const MAX_PROGRAM_STEPS: usize = 4096;
const MAX_LITERAL_BYTES: usize = 65536;
const MAX_EXPRESSION_NODES: usize = 16384;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::scalar_ir) struct ProgramStep {
    pub(in crate::scalar_ir) span: Span,
    /// The interpreter charges one work step per resolved operation except
    /// retirement. This is an accounting event, not a loop-iteration bound.
    pub(in crate::scalar_ir) charge_work: bool,
    pub(in crate::scalar_ir) kind: ProgramOp,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::scalar_ir) enum ProgramOp {
    Return {
        value: ScalarExpressionIr,
    },
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

impl ProgramOp {
    pub(super) fn expression(&self) -> Option<&ScalarExpressionIr> {
        match self {
            Self::Return { value }
            | Self::Store { value, .. }
            | Self::Update { value, .. }
            | Self::RepeatInit { count: value, .. }
            | Self::JumpIfFalse {
                condition: value, ..
            }
            | Self::PrintValue { value } => Some(value),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ScalarProgramIr {
    slots: Vec<ValueType>,
    steps: Vec<ProgramStep>,
    // None is a script; Some(0) is a genuine zero-argument function.
    function_parameters: Option<usize>,
    bounded: bool,
}

impl ScalarProgramIr {
    pub(super) fn lower(program: &ResolvedProgram) -> Result<Self, Diagnostic> {
        let slots = program
            .binding_types()
            .iter()
            .map(|kind| kind.as_primitive().ok_or_else(|| invalid(Span::default())))
            .collect::<Result<Vec<_>, _>>()?;
        Self::lower_operations(slots, &program.statements, None, false, None)
    }

    pub(super) fn lower_integer_function(
        program: &ResolvedProgram,
        max_parameters: usize,
    ) -> Result<Self, Diagnostic> {
        Self::lower_function(program, max_parameters, false)
    }

    pub(super) fn lower_bounded_integer_function(
        program: &ResolvedProgram,
        max_parameters: usize,
    ) -> Result<Self, Diagnostic> {
        Self::lower_function(program, max_parameters, true)
    }

    fn lower_function(
        program: &ResolvedProgram,
        max_parameters: usize,
        bounded: bool,
    ) -> Result<Self, Diagnostic> {
        if let Some(statement) = program.statements.first() {
            return Err(unsupported_function(statement.span));
        }
        if program.functions().len() != 1 || program.functions.len() != 1 {
            return Err(unsupported_function(Span::default()));
        }
        Self::lower_selected_function(program, 0, max_parameters, bounded, None)
    }

    fn lower_selected_function(
        program: &ResolvedProgram,
        id: usize,
        max_parameters: usize,
        bounded: bool,
        call_arities: Option<&[usize]>,
    ) -> Result<Self, Diagnostic> {
        let signature = &program.functions()[id];
        let function = &program.functions[id];
        let unsupported = |span| {
            if call_arities.is_some() {
                invalid(span).with_help("use pure integer functions with required integer parameters, scalar locals and integer returns")
            } else {
                unsupported_function(span)
            }
        };
        let parameters = signature.parameters.len();
        if !signature.type_parameters.is_empty() {
            return Err(
                unsupported(signature.span).with_help("run generic functions in the interpreter")
            );
        }
        if !signature.pure || signature.result != ValueType::Integer || parameters > max_parameters
        {
            return Err(unsupported(signature.span));
        }
        for parameter in &signature.parameters {
            if parameter.kind != ValueType::Integer || parameter.default.is_some() {
                return Err(unsupported(parameter.span));
            }
        }
        if let Some(contract) = signature.contracts.first() {
            return Err(unsupported(contract.span));
        }
        let len = function.statements.len();
        for (pc, step) in function.statements.iter().enumerate() {
            match &step.kind {
                ResolvedStatement::Store { .. }
                | ResolvedStatement::Update { .. }
                | ResolvedStatement::Return { value: Some(_) }
                | ResolvedStatement::Retire { .. } => {}
                ResolvedStatement::JumpIfFalse { target, .. }
                | ResolvedStatement::Jump { target }
                    if *target <= len && (bounded || *target > pc) => {}
                ResolvedStatement::LoopControl { target, .. } if bounded && *target <= len => {}
                ResolvedStatement::RepeatInit { .. } | ResolvedStatement::RepeatNext { .. }
                    if bounded => {}
                ResolvedStatement::RepeatGuard { target, .. } if bounded && *target <= len => {}
                _ => return Err(unsupported(step.span)),
            }
        }
        let slots = function
            .slot_types
            .iter()
            .map(|kind| match kind.primitive() {
                Some(kind @ (ValueType::Integer | ValueType::Boolean)) => Ok(kind),
                _ => Err(unsupported(signature.span)),
            })
            .collect::<Result<Vec<_>, _>>()?;
        Self::lower_operations(
            slots,
            &function.statements,
            Some(parameters),
            bounded,
            call_arities,
        )
    }

    fn lower_operations(
        slots: Vec<ValueType>,
        operations: &[crate::resolver::ResolvedOperation],
        function_parameters: Option<usize>,
        bounded: bool,
        call_arities: Option<&[usize]>,
    ) -> Result<Self, Diagnostic> {
        if slots.len() > MAX_PROGRAM_SLOTS || operations.len() > MAX_PROGRAM_STEPS {
            return Err(invalid(Span::default()));
        }
        let mut steps = Vec::with_capacity(operations.len());
        let mut total_nodes = 0usize;
        let mut literal_bytes = 0usize;
        let lower_expression = |value| ScalarExpressionIr::lower(value, &slots, call_arities);
        for statement in operations {
            let kind = match &statement.kind {
                ResolvedStatement::Return { value: Some(value) }
                    if function_parameters.is_some() =>
                {
                    ProgramOp::Return {
                        value: lower_expression(value)?,
                    }
                }
                ResolvedStatement::Store { slot, value } => ProgramOp::Store {
                    slot: *slot,
                    value: lower_expression(value)?,
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
                    value: lower_expression(value)?,
                },
                ResolvedStatement::RepeatInit {
                    count,
                    count_slot,
                    index_slot,
                } => ProgramOp::RepeatInit {
                    count: lower_expression(count)?,
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
                    condition: lower_expression(condition)?,
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
                        value: lower_expression(value)?,
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
            let expression_nodes = kind.expression().map_or(0, |value| value.registers.len());
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
        let ir = Self {
            slots,
            steps,
            function_parameters,
            bounded,
        };
        ir.verify_with_calls(call_arities)?;
        Ok(ir)
    }

    /// A strictly linear program of integer stores may become a freestanding
    /// object kernel. Only the LAST store's integer value is returned. This is
    /// not execution of general Scarp statements, I/O or runtime budgets.
    pub(super) fn integer_kernel(&self) -> Result<&Self, Diagnostic> {
        if self.is_function()
            || self.steps.is_empty()
            || self.slots.is_empty()
            || self.slots.iter().any(|kind| *kind != ValueType::Integer)
        {
            return Err(invalid(Span::default()));
        }
        for step in &self.steps {
            match &step.kind {
                ProgramOp::Store { slot, value }
                    if self.slots.get(*slot) == Some(&ValueType::Integer)
                        && value.registers.get(value.root) == Some(&ValueType::Integer) => {}
                _ => return Err(invalid(step.span)),
            }
        }
        Ok(self)
    }

    pub(super) fn parameters(&self) -> usize {
        self.function_parameters.unwrap_or(0)
    }

    pub(super) fn is_function(&self) -> bool {
        self.function_parameters.is_some()
    }

    pub(super) fn is_bounded(&self) -> bool {
        self.bounded
    }

    pub(super) fn operations(&self) -> &[ProgramStep] {
        &self.steps
    }

    pub(super) fn integer_slot_count(&self) -> usize {
        self.slots.len()
    }

    pub(super) fn integer_stores(&self) -> impl Iterator<Item = (usize, &ScalarExpressionIr)> {
        self.steps.iter().map(|step| match &step.kind {
            ProgramOp::Store { slot, value } => (*slot, value),
            _ => unreachable!("only invoke after checking integer_kernel()"),
        })
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
        call_arities: Option<&[usize]>,
    ) -> Result<(), Diagnostic> {
        expression.verify_with_calls(call_arities)?;
        for instruction in &expression.steps {
            if instruction.span.start < span.start || instruction.span.end > span.end {
                return Err(invalid(instruction.span));
            }
        }
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
        self.verify_with_calls(None)
    }

    fn verify_with_calls(&self, call_arities: Option<&[usize]>) -> Result<(), Diagnostic> {
        let len = self.steps.len();
        let function = self.is_function();
        let parameters = self.parameters();
        if (self.bounded && !function)
            || (call_arities.is_some() && !self.bounded)
            || len > MAX_PROGRAM_STEPS
            || self.slots.len() > MAX_PROGRAM_SLOTS
            || self.slots.get(..parameters).map_or(true, |kinds| {
                kinds.iter().any(|kind| *kind != ValueType::Integer)
            })
        {
            return Err(invalid(Span::default()));
        }
        let mut literal_bytes = 0usize;
        let mut total_nodes = 0usize;
        for (pc, instruction) in self.steps.iter().enumerate() {
            let span = instruction.span;
            if span.start > span.end
                || instruction.charge_work == matches!(instruction.kind, ProgramOp::Retire { .. })
            {
                return Err(invalid(span));
            }
            if function {
                match &instruction.kind {
                    ProgramOp::Store { slot, .. } | ProgramOp::Update { slot, .. }
                        if *slot >= parameters => {}
                    ProgramOp::JumpIfFalse { target, .. } | ProgramOp::Jump { target }
                        if self.bounded || *target > pc => {}
                    ProgramOp::LoopControl { retire, .. }
                        if self.bounded && retire.iter().all(|s| *s >= parameters) => {}
                    ProgramOp::RepeatInit {
                        count_slot,
                        index_slot,
                        ..
                    }
                    | ProgramOp::RepeatGuard {
                        count_slot,
                        index_slot,
                        ..
                    } if self.bounded && *count_slot >= parameters && *index_slot >= parameters => {
                    }
                    ProgramOp::RepeatNext { index_slot }
                        if self.bounded && *index_slot >= parameters => {}
                    ProgramOp::Retire { slots } if slots.iter().all(|s| *s >= parameters) => {}
                    ProgramOp::Return { .. } => {}
                    _ => return Err(invalid(span)),
                }
            }
            let expression = match &instruction.kind {
                ProgramOp::Return { value } if function => {
                    self.check_expression(value, Some(ValueType::Integer), span, call_arities)?;
                    Some(value)
                }
                ProgramOp::Return { .. } => return Err(invalid(span)),
                ProgramOp::Store { slot, value } => {
                    self.check_expression(value, None, span, call_arities)?;
                    self.check_slot(*slot, value.registers[value.root], span)?;
                    Some(value)
                }
                ProgramOp::Update {
                    slot,
                    operator,
                    operator_span,
                    value,
                } => {
                    if operator.is_comparison()
                        || operator_span.start > operator_span.end
                        || operator_span.start < span.start
                        || operator_span.end > span.end
                    {
                        return Err(invalid(*operator_span));
                    }
                    self.check_slot(*slot, ValueType::Integer, span)?;
                    self.check_expression(value, Some(ValueType::Integer), span, call_arities)?;
                    Some(value)
                }
                ProgramOp::RepeatInit {
                    count,
                    count_slot,
                    index_slot,
                } => {
                    if count_slot == index_slot {
                        return Err(invalid(span));
                    }
                    self.check_slot(*count_slot, ValueType::Integer, span)?;
                    self.check_slot(*index_slot, ValueType::Integer, span)?;
                    self.check_expression(count, Some(ValueType::Integer), span, call_arities)?;
                    Some(count)
                }
                ProgramOp::RepeatGuard {
                    count_slot,
                    index_slot,
                    target,
                } => {
                    if count_slot == index_slot || *target > len {
                        return Err(invalid(span));
                    }
                    self.check_slot(*count_slot, ValueType::Integer, span)?;
                    self.check_slot(*index_slot, ValueType::Integer, span)?;
                    None
                }
                ProgramOp::RepeatNext { index_slot } => {
                    self.check_slot(*index_slot, ValueType::Integer, span)?;
                    None
                }
                ProgramOp::JumpIfFalse { condition, target } => {
                    self.check_expression(condition, Some(ValueType::Boolean), span, call_arities)?;
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
                    self.check_expression(value, None, span, call_arities)?;
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

fn unsupported_function(span: Span) -> Diagnostic {
    invalid(span).with_help(
        "use one pure integer function with required integer parameters, scalar locals, forward conditionals and integer returns",
    )
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

    fn function_ir(source: &str) -> ScalarProgramIr {
        ScalarProgramIr::lower_integer_function(&compile(source).unwrap(), 6).unwrap()
    }

    #[test]
    fn bounded_function_ir_accepts_cycles_only_in_explicit_mode() {
        let source = "pure funcra f(n: integer) -> integer {
            var i = n; while i > 0 { i -= 1; } return i;
        }";
        let resolved = compile(source).unwrap();
        assert!(ScalarProgramIr::lower_integer_function(&resolved, 16).is_err());
        let mut ir = ScalarProgramIr::lower_bounded_integer_function(&resolved, 16).unwrap();
        assert!(ir.is_bounded());
        assert!(ir.verify().is_ok());
        ir.bounded = false;
        assert!(ir.verify().is_err());
    }

    #[test]
    fn zero_argument_function_is_not_a_script_and_cannot_fall_through() {
        let ir = function_ir("pure funcra answer() -> integer { return 42; }");
        assert!(ir.is_function());
        assert_eq!(ir.parameters(), 0);
        assert!(ir.integer_kernel().is_err());
        let mut malformed = ir.clone();
        let end = malformed.steps.len();
        malformed.steps[0].kind = ProgramOp::Jump { target: end };
        assert!(malformed.verify().is_err());
        let mut script = ir;
        script.function_parameters = None;
        assert!(script.verify().is_err());
    }

    #[test]
    fn verifier_rejects_spans_outside_the_enclosing_statement() {
        let source = "// नमस्ते 🌏\r\nvar n = 4; n += 2; print(n);";
        let ir = program(source);

        let mut malformed = ir.clone();
        let ProgramOp::Store { value, .. } = &mut malformed.steps[0].kind else {
            unreachable!()
        };
        let outside = Span::new(0, 2);
        value.steps[0].span = outside;
        assert_eq!(malformed.verify().unwrap_err().span, outside);

        let mut malformed = ir;
        let ProgramOp::Update { operator_span, .. } = &mut malformed.steps[1].kind else {
            unreachable!()
        };
        let outside = Span::new(source.len(), source.len() + 1);
        *operator_span = outside;
        assert_eq!(malformed.verify().unwrap_err().span, outside);
    }

    #[test]
    fn function_verifier_requires_typed_returns_and_forbids_fallthrough() {
        let source = "pure funcra choose(x: integer) -> integer {
            if x < 0 { return x; } return 0;
        }";
        let ir = function_ir(source);
        let mut malformed = ir.clone();
        let conditional = malformed
            .steps
            .iter_mut()
            .find(|step| matches!(step.kind, ProgramOp::JumpIfFalse { .. }))
            .unwrap();
        let ProgramOp::JumpIfFalse { target, .. } = &mut conditional.kind else {
            unreachable!()
        };
        *target = ir.steps.len();
        assert!(malformed.verify().is_err());
        *malformed
            .steps
            .iter_mut()
            .find_map(|step| match &mut step.kind {
                ProgramOp::JumpIfFalse { target, .. } => Some(target),
                _ => None,
            })
            .unwrap() = 0;
        assert!(malformed.verify().is_err());

        let mut malformed = ir;
        let ProgramOp::JumpIfFalse { condition, .. } = &malformed.steps[0].kind else {
            unreachable!()
        };
        let condition = condition.clone();
        malformed.steps[1].kind = ProgramOp::Return { value: condition };
        assert!(malformed.verify().is_err());
    }

    #[test]
    fn function_verifier_requires_local_initialization_and_protects_parameters() {
        let source = "pure funcra f(x: integer) -> integer { bind local = x; return local; }";
        let ir = function_ir(source);
        let mut malformed = ir.clone();
        malformed.steps[0].kind = ProgramOp::Jump { target: 1 };
        assert!(malformed.verify().is_err());

        let mut malformed = ir;
        let ProgramOp::Store { slot, .. } = &mut malformed.steps[0].kind else {
            unreachable!()
        };
        *slot = 0;
        assert!(malformed.verify().is_err());
        let ProgramOp::Store { slot, .. } = &mut malformed.steps[0].kind else {
            unreachable!()
        };
        *slot = usize::MAX;
        assert!(malformed.verify().is_err());
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
    fn repeat_count_and_index_cannot_share_a_slot() {
        let intact = program("repeat 2 as i { print(i); }");
        for corrupt_guard in [false, true] {
            let mut ir = intact.clone();
            let span = ir
                .steps
                .iter_mut()
                .find_map(|step| {
                    let (count_slot, index_slot) = match &mut step.kind {
                        ProgramOp::RepeatInit {
                            count_slot,
                            index_slot,
                            ..
                        } if !corrupt_guard => (count_slot, index_slot),
                        ProgramOp::RepeatGuard {
                            count_slot,
                            index_slot,
                            ..
                        } if corrupt_guard => (count_slot, index_slot),
                        _ => return None,
                    };
                    *count_slot = *index_slot;
                    Some(step.span)
                })
                .unwrap();

            let diagnostic = ir.verify().unwrap_err();
            assert_eq!(diagnostic.kind, DiagnosticKind::NativeUnsupported);
            assert_eq!(diagnostic.span, span);
        }
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
