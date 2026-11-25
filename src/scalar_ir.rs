//! Private, target-neutral scalar expression IR (first native IR slice).
//!
//! This lowers already-resolved integer/boolean expressions, never reparses
//! source, and does not emit machine code or introduce a public ABI. Future
//! statement and object lowering will build on its checked control-flow form.

mod elf64;
mod program;

use crate::{
    ast::{BinaryOperator, ExpressionKind, LogicalOperator, Statement},
    diagnostics::{Diagnostic, DiagnosticKind, Span},
    resolver::{ResolvedCall, ResolvedExpression, ResolvedProgram},
    value::ValueType,
};
use std::borrow::Cow;

const MAX_IR_REGISTERS: usize = 1024;
const MAX_IR_STEPS: usize = 8192;

fn invalid(span: Span) -> Diagnostic {
    Diagnostic::new(
        DiagnosticKind::NativeUnsupported,
        span,
        "the private scalar IR cannot verify this resolved expression",
    )
    .with_help("only bounded, well-typed integer/boolean scalar expressions are supported")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Unary {
    Negate,
    BitNot,
    Not,
}

/// Every instruction owns its original UTF-8 source byte span.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Instruction {
    kind: Operation,
    span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Operation {
    /// One deterministic interpreter-equivalent work charge.
    Work,
    Integer {
        dst: usize,
        value: i64,
    },
    Boolean {
        dst: usize,
        value: bool,
    },
    Load {
        dst: usize,
        slot: usize,
    },
    Move {
        dst: usize,
        src: usize,
    },
    Unary {
        dst: usize,
        src: usize,
        op: Unary,
    },
    Binary {
        dst: usize,
        left: usize,
        right: usize,
        op: BinaryOperator,
    },
    /// Arguments are already evaluated in source order. This list is in ABI
    /// parameter order, using the resolver's explicit-argument mapping.
    Call {
        dst: usize,
        callee: usize,
        arguments: Vec<usize>,
    },
    /// Both paths are conservatively considered possible by the verifier.
    /// The target must be forward and may point exactly past the last step.
    Branch {
        condition: usize,
        when_true: bool,
        target: usize,
    },
    /// Skip the remaining arms of a conditional expression after selecting a value.
    Jump {
        target: usize,
    },
}

impl Operation {
    fn successors(&self, pc: usize) -> (usize, Option<usize>) {
        match *self {
            Self::Branch { target, .. } => (pc + 1, Some(target)),
            Self::Jump { target } => (target, None),
            _ => (pc + 1, None),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ScalarExpressionIr {
    registers: Vec<ValueType>,
    slots: Vec<ValueType>,
    root: usize,
    steps: Vec<Instruction>,
}

enum Task {
    Visit(usize),
    Finish(usize),
    FinishCall(usize),
    Logical(usize),
    CloseLogical {
        node: usize,
        branch: usize,
    },
    NextConditionalArm {
        node: usize,
        arm: usize,
        exits: Vec<usize>,
    },
    ConditionalGuard {
        node: usize,
        arm: usize,
        exits: Vec<usize>,
    },
    FinishConditionalArm {
        node: usize,
        result: usize,
        next_arm: Option<(usize, usize)>,
        exits: Vec<usize>,
    },
}

fn register_type(
    types: &[ValueType],
    index: usize,
    expected: ValueType,
    span: Span,
) -> Result<(), Diagnostic> {
    if types.get(index) == Some(&expected) {
        Ok(())
    } else {
        Err(invalid(span))
    }
}

fn require_read(
    initialized: &[bool],
    types: &[ValueType],
    index: usize,
    expected: ValueType,
    span: Span,
) -> Result<(), Diagnostic> {
    register_type(types, index, expected, span)?;
    if initialized.get(index).copied() == Some(true) {
        Ok(())
    } else {
        Err(invalid(span))
    }
}

fn join(destination: &mut Option<Vec<bool>>, incoming: Cow<'_, [bool]>) {
    match destination {
        Some(existing) => {
            for (bit, other) in existing.iter_mut().zip(incoming.iter()) {
                *bit &= *other;
            }
        }
        None => *destination = Some(incoming.into_owned()),
    }
}

impl ScalarExpressionIr {
    fn patch_target(
        &mut self,
        step: usize,
        destination: usize,
        span: Span,
    ) -> Result<(), Diagnostic> {
        match self
            .steps
            .get_mut(step)
            .map(|instruction| &mut instruction.kind)
        {
            Some(Operation::Branch { target, .. } | Operation::Jump { target }) => {
                *target = destination;
                Ok(())
            }
            _ => Err(invalid(span)),
        }
    }

    fn emit(&mut self, span: Span, kind: Operation) -> Result<usize, Diagnostic> {
        if self.steps.len() >= MAX_IR_STEPS || span.start > span.end {
            return Err(invalid(span));
        }
        let index = self.steps.len();
        self.steps.push(Instruction { kind, span });
        Ok(index)
    }

    fn lower(
        expression: &ResolvedExpression,
        slots: &[ValueType],
        call_arities: Option<&[usize]>,
    ) -> Result<Self, Diagnostic> {
        let nodes = expression.syntax.nodes();
        if nodes.is_empty()
            || nodes.len() > MAX_IR_REGISTERS
            || expression.syntax.root() >= nodes.len()
            || expression.types.len() != nodes.len()
            || expression.slots.len() != nodes.len()
            || expression.calls.len() != nodes.len()
        {
            return Err(invalid(Span::default()));
        }
        let mut registers = Vec::with_capacity(nodes.len());
        for (index, node) in nodes.iter().enumerate() {
            let kind = if expression.types[index] == ValueType::Integer.id() {
                ValueType::Integer
            } else if expression.types[index] == ValueType::Boolean.id() {
                ValueType::Boolean
            } else {
                return Err(invalid(node.span));
            };
            registers.push(kind);
        }
        let mut ir = Self {
            registers,
            slots: slots.to_vec(),
            root: expression.syntax.root(),
            steps: Vec::new(),
        };
        let mut tasks = vec![Task::Visit(ir.root)];
        while let Some(task) = tasks.pop() {
            match task {
                Task::Visit(index) => {
                    let node = nodes.get(index).ok_or_else(|| invalid(Span::default()))?;
                    ir.emit(node.span, Operation::Work)?;
                    match &node.kind {
                        ExpressionKind::Integer(value) => {
                            ir.emit(
                                node.span,
                                Operation::Integer {
                                    dst: index,
                                    value: *value,
                                },
                            )?;
                        }
                        ExpressionKind::Boolean(value) => {
                            ir.emit(
                                node.span,
                                Operation::Boolean {
                                    dst: index,
                                    value: *value,
                                },
                            )?;
                        }
                        ExpressionKind::Name(_) => {
                            let slot = expression.slots[index].ok_or_else(|| invalid(node.span))?;
                            ir.emit(node.span, Operation::Load { dst: index, slot })?;
                        }
                        ExpressionKind::Group { expression: child }
                        | ExpressionKind::Negate { operand: child, .. }
                        | ExpressionKind::BitNot { operand: child, .. }
                        | ExpressionKind::Not { operand: child, .. } => {
                            tasks.push(Task::Finish(index));
                            tasks.push(Task::Visit(*child));
                        }
                        ExpressionKind::Binary { left, right, .. } => {
                            tasks.push(Task::Finish(index));
                            tasks.push(Task::Visit(*right));
                            tasks.push(Task::Visit(*left));
                        }
                        ExpressionKind::Logical { left, .. } => {
                            tasks.push(Task::Logical(index));
                            tasks.push(Task::Visit(*left));
                        }
                        ExpressionKind::Conditional { .. } => {
                            tasks.push(Task::NextConditionalArm {
                                node: index,
                                arm: 0,
                                exits: Vec::new(),
                            });
                        }
                        ExpressionKind::UserCall { arguments, .. } if call_arities.is_some() => {
                            tasks.push(Task::FinishCall(index));
                            for argument in arguments.iter().rev() {
                                tasks.push(Task::Visit(argument.expression));
                            }
                        }
                        _ => return Err(invalid(node.span)),
                    }
                }
                Task::FinishCall(index) => {
                    let node = &nodes[index];
                    let (
                        ExpressionKind::UserCall { arguments, .. },
                        Some(ResolvedCall::User {
                            function,
                            parameters,
                        }),
                    ) = (&node.kind, &expression.calls[index])
                    else {
                        return Err(invalid(node.span));
                    };
                    let arity = call_arities
                        .and_then(|counts| counts.get(*function).copied())
                        .ok_or_else(|| invalid(node.span))?;
                    if arguments.len() != arity || parameters.len() != arity {
                        return Err(invalid(node.span));
                    }
                    let mut ordered = vec![None; arity];
                    for (argument, &parameter) in arguments.iter().zip(parameters) {
                        let destination = ordered
                            .get_mut(parameter)
                            .ok_or_else(|| invalid(node.span))?;
                        if destination.replace(argument.expression).is_some() {
                            return Err(invalid(node.span));
                        }
                    }
                    let arguments = ordered
                        .into_iter()
                        .collect::<Option<Vec<_>>>()
                        .ok_or_else(|| invalid(node.span))?;
                    ir.emit(node.span, Operation::Work)?;
                    ir.emit(
                        node.span,
                        Operation::Call {
                            dst: index,
                            callee: *function,
                            arguments,
                        },
                    )?;
                }
                Task::Finish(index) => {
                    let node = &nodes[index];
                    match &node.kind {
                        ExpressionKind::Group { expression: child } => {
                            ir.emit(
                                node.span,
                                Operation::Move {
                                    dst: index,
                                    src: *child,
                                },
                            )?;
                        }
                        ExpressionKind::Negate {
                            operand,
                            operator_span,
                        } => {
                            ir.emit(*operator_span, Operation::Work)?;
                            ir.emit(
                                *operator_span,
                                Operation::Unary {
                                    dst: index,
                                    src: *operand,
                                    op: Unary::Negate,
                                },
                            )?;
                        }
                        ExpressionKind::BitNot {
                            operand,
                            operator_span,
                        } => {
                            ir.emit(*operator_span, Operation::Work)?;
                            ir.emit(
                                *operator_span,
                                Operation::Unary {
                                    dst: index,
                                    src: *operand,
                                    op: Unary::BitNot,
                                },
                            )?;
                        }
                        ExpressionKind::Not {
                            operand,
                            operator_span,
                        } => {
                            ir.emit(*operator_span, Operation::Work)?;
                            ir.emit(
                                *operator_span,
                                Operation::Unary {
                                    dst: index,
                                    src: *operand,
                                    op: Unary::Not,
                                },
                            )?;
                        }
                        ExpressionKind::Binary {
                            left,
                            right,
                            operator,
                            operator_span,
                        } => {
                            ir.emit(*operator_span, Operation::Work)?;
                            ir.emit(
                                *operator_span,
                                Operation::Binary {
                                    dst: index,
                                    left: *left,
                                    right: *right,
                                    op: *operator,
                                },
                            )?;
                        }
                        _ => return Err(invalid(node.span)),
                    }
                }
                Task::Logical(index) => {
                    let node = &nodes[index];
                    let ExpressionKind::Logical {
                        left,
                        right,
                        operator,
                        operator_span,
                    } = &node.kind
                    else {
                        return Err(invalid(node.span));
                    };
                    // Charge the first logical operator evaluation after its left
                    // operand, then initialize the result for the skipped path.
                    ir.emit(*operator_span, Operation::Work)?;
                    ir.emit(
                        *operator_span,
                        Operation::Move {
                            dst: index,
                            src: *left,
                        },
                    )?;
                    let branch = ir.emit(
                        *operator_span,
                        Operation::Branch {
                            condition: *left,
                            when_true: *operator == LogicalOperator::Or,
                            target: usize::MAX,
                        },
                    )?;
                    tasks.push(Task::CloseLogical {
                        node: index,
                        branch,
                    });
                    tasks.push(Task::Visit(*right));
                }
                Task::CloseLogical {
                    node: index,
                    branch,
                } => {
                    let node = &nodes[index];
                    let ExpressionKind::Logical {
                        right,
                        operator_span,
                        ..
                    } = &node.kind
                    else {
                        return Err(invalid(node.span));
                    };
                    // This charge is reachable only if the right side executed.
                    ir.emit(*operator_span, Operation::Work)?;
                    ir.emit(
                        *operator_span,
                        Operation::Move {
                            dst: index,
                            src: *right,
                        },
                    )?;
                    ir.patch_target(branch, ir.steps.len(), node.span)?;
                }
                Task::NextConditionalArm { node, arm, exits } => {
                    let ExpressionKind::Conditional {
                        branches,
                        otherwise,
                    } = &nodes[node].kind
                    else {
                        return Err(invalid(nodes[node].span));
                    };
                    if let Some(branch) = branches.get(arm) {
                        tasks.push(Task::ConditionalGuard { node, arm, exits });
                        tasks.push(Task::Visit(branch.condition));
                    } else {
                        tasks.push(Task::FinishConditionalArm {
                            node,
                            result: *otherwise,
                            next_arm: None,
                            exits,
                        });
                        tasks.push(Task::Visit(*otherwise));
                    }
                }
                Task::ConditionalGuard { node, arm, exits } => {
                    let ExpressionKind::Conditional { branches, .. } = &nodes[node].kind else {
                        return Err(invalid(nodes[node].span));
                    };
                    let branch = &branches[arm];
                    let span = nodes[branch.condition].span;
                    // The interpreter charges once for deciding a reached guard.
                    ir.emit(span, Operation::Work)?;
                    let skip = ir.emit(
                        span,
                        Operation::Branch {
                            condition: branch.condition,
                            when_true: false,
                            target: usize::MAX,
                        },
                    )?;
                    tasks.push(Task::FinishConditionalArm {
                        node,
                        result: branch.result,
                        next_arm: Some((arm + 1, skip)),
                        exits,
                    });
                    tasks.push(Task::Visit(branch.result));
                }
                Task::FinishConditionalArm {
                    node,
                    result,
                    next_arm,
                    mut exits,
                } => {
                    let span = nodes[node].span;
                    // The selected result defines the root on this path. Moving
                    // it and skipping other arms add no source work events.
                    ir.emit(
                        span,
                        Operation::Move {
                            dst: node,
                            src: result,
                        },
                    )?;
                    if let Some((arm, skip)) = next_arm {
                        exits.push(ir.emit(span, Operation::Jump { target: usize::MAX })?);
                        ir.patch_target(skip, ir.steps.len(), span)?;
                        tasks.push(Task::NextConditionalArm { node, arm, exits });
                    } else {
                        for exit in exits {
                            ir.patch_target(exit, ir.steps.len(), span)?;
                        }
                    }
                }
            }
        }
        ir.verify_with_calls(call_arities)?;
        Ok(ir)
    }

    /// Verify forward-only CFG edges, definite assignment at every join,
    /// scalar types and bounded memory before any backend consumes this IR.
    fn verify(&self) -> Result<(), Diagnostic> {
        self.verify_with_calls(None)
    }

    fn verify_with_calls(&self, call_arities: Option<&[usize]>) -> Result<(), Diagnostic> {
        if self.registers.is_empty()
            || self.registers.len() > MAX_IR_REGISTERS
            || self.steps.len() > MAX_IR_STEPS
            || self.root >= self.registers.len()
            || self.slots.len() > 256
            || self
                .registers
                .iter()
                .chain(self.slots.iter())
                .any(|kind| !matches!(kind, ValueType::Integer | ValueType::Boolean))
        {
            return Err(invalid(Span::default()));
        }
        let mut incoming: Vec<Option<Vec<bool>>> = vec![None; self.steps.len() + 1];
        incoming[0] = Some(vec![false; self.registers.len()]);
        for (pc, instruction) in self.steps.iter().enumerate() {
            if instruction.span.start > instruction.span.end {
                return Err(invalid(instruction.span));
            }
            // Every edge moves forward, so this state has reached its last use.
            let mut initialized = incoming[pc]
                .take()
                .ok_or_else(|| invalid(instruction.span))?;
            let types = &self.registers;
            let span = instruction.span;
            match &instruction.kind {
                Operation::Work | Operation::Jump { .. } => {}
                Operation::Integer { dst, .. } => {
                    register_type(types, *dst, ValueType::Integer, span)?;
                    initialized[*dst] = true;
                }
                Operation::Boolean { dst, .. } => {
                    register_type(types, *dst, ValueType::Boolean, span)?;
                    initialized[*dst] = true;
                }
                Operation::Load { dst, slot } => {
                    let kind = *self.slots.get(*slot).ok_or_else(|| invalid(span))?;
                    register_type(types, *dst, kind, span)?;
                    initialized[*dst] = true;
                }
                Operation::Move { dst, src } => {
                    let kind = *types.get(*dst).ok_or_else(|| invalid(span))?;
                    require_read(&initialized, types, *src, kind, span)?;
                    initialized[*dst] = true;
                }
                Operation::Unary { dst, src, op } => {
                    let kind = if *op == Unary::Not {
                        ValueType::Boolean
                    } else {
                        ValueType::Integer
                    };
                    register_type(types, *dst, kind, span)?;
                    require_read(&initialized, types, *src, kind, span)?;
                    initialized[*dst] = true;
                }
                Operation::Binary {
                    dst,
                    left,
                    right,
                    op,
                } => {
                    let kind = *types.get(*left).ok_or_else(|| invalid(span))?;
                    if !matches!(op, BinaryOperator::Equal | BinaryOperator::NotEqual)
                        && kind != ValueType::Integer
                    {
                        return Err(invalid(span));
                    }
                    require_read(&initialized, types, *left, kind, span)?;
                    require_read(&initialized, types, *right, kind, span)?;
                    let result_kind = if op.is_comparison() {
                        ValueType::Boolean
                    } else {
                        ValueType::Integer
                    };
                    register_type(types, *dst, result_kind, span)?;
                    initialized[*dst] = true;
                }
                Operation::Branch { condition, .. } => {
                    require_read(&initialized, types, *condition, ValueType::Boolean, span)?;
                }
                Operation::Call {
                    dst,
                    callee,
                    arguments,
                } => {
                    if call_arities.and_then(|counts| counts.get(*callee)) != Some(&arguments.len())
                    {
                        return Err(invalid(span));
                    }
                    for &argument in arguments {
                        require_read(&initialized, types, argument, ValueType::Integer, span)?;
                    }
                    register_type(types, *dst, ValueType::Integer, span)?;
                    initialized[*dst] = true;
                }
            }
            let (next, alternative) = instruction.kind.successors(pc);
            let valid_target = |target| target > pc && target <= self.steps.len();
            if !valid_target(next) || alternative.is_some_and(|target| !valid_target(target)) {
                return Err(invalid(span));
            }
            if let Some(target) = alternative.filter(|&target| target != next) {
                join(&mut incoming[target], Cow::Borrowed(&initialized));
            }
            join(&mut incoming[next], Cow::Owned(initialized));
        }
        if incoming[self.steps.len()]
            .as_ref()
            .and_then(|known| known.get(self.root))
            .copied()
            != Some(true)
        {
            return Err(invalid(Span::default()));
        }
        Ok(())
    }
}

/// Compile a strictly restricted linear sequence of integer initializers to a self-contained
/// x86-64 ELF64 relocatable kernel. The ABI is experimental and is not the
/// observable Scarp stdout/program-execution contract.
pub(crate) fn emit_integer_kernel(source: &str) -> Result<Vec<u8>, Diagnostic> {
    let ir = lower_integer_kernel(source)?;
    elf64::emit(&ir)
}

pub(crate) fn emit_checked_integer_kernel(
    source: &str,
) -> Result<crate::native_object::CheckedElf64Kernel, Diagnostic> {
    let ir = lower_integer_kernel(source)?;
    elf64::emit_checked(&ir)
}

pub(crate) fn emit_checked_integer_function(
    source: &str,
) -> Result<crate::native_object::CheckedElf64Kernel, Diagnostic> {
    emit_integer_function_with_layout(source, None).map(|(kernel, _)| kernel)
}

pub(crate) fn emit_bounded_integer_function(
    source: &str,
    max_work: u32,
) -> Result<crate::native_object::CheckedElf64Kernel, Diagnostic> {
    emit_integer_function_with_layout(source, Some(max_work)).map(|(kernel, _)| kernel)
}

pub(crate) fn emit_integer_function_with_layout(
    source: &str,
    budget: Option<u32>,
) -> Result<
    (
        crate::native_object::CheckedElf64Kernel,
        crate::native_object::FreestandingCallLayout,
    ),
    Diagnostic,
> {
    let resolved = crate::compile(source)?;
    let ir = if budget.is_some() {
        program::ScalarProgramIr::lower_bounded_integer_function(
            &resolved,
            elf64::MAX_INTEGER_ARGUMENTS,
        )?
    } else {
        program::ScalarProgramIr::lower_integer_function(&resolved, elf64::MAX_INTEGER_ARGUMENTS)?
    };
    elf64::emit_function_with_layout(&ir, budget)
}

pub(crate) fn emit_bounded_integer_entry(
    source: &str,
    entry: &str,
    max_work: u32,
    max_stack_bytes: u32,
) -> Result<crate::native_object::FreestandingEntry, Diagnostic> {
    let resolved = crate::compile(source)?;
    let ir = program::ScalarCallGraph::lower(&resolved, entry, elf64::MAX_INTEGER_ARGUMENTS)?;
    elf64::emit_bounded_entry(&ir, max_work, max_stack_bytes)
}

fn lower_integer_kernel(source: &str) -> Result<program::ScalarProgramIr, Diagnostic> {
    let syntax = crate::parse(source)?;
    // The resolved plan uses the same Store for declarations and plain
    // assignments. Check the source shape before resolving erases it.
    let unsupported = syntax
        .statements
        .iter()
        .find(|statement| !matches!(statement, Statement::Declaration { mutable: false, .. }))
        .map(Statement::span);
    let program = crate::resolver::resolve(syntax)?;
    crate::native_preflight::inspect(&program)?;
    if let Some(span) = unsupported {
        return Err(invalid(span).with_help("use only immutable integer bind declarations"));
    }
    let ir = program::ScalarProgramIr::lower(&program)?;
    ir.integer_kernel()?;
    Ok(ir)
}

/// Verify a private whole-program scalar IR, including all lowered expression
/// CFGs and the resolved operation/jump/retirement plan. The public C11 API is
/// unchanged; object emission and a native compile CLI are out of scope.
pub(crate) fn verify_program(program: &ResolvedProgram) -> Result<(), Diagnostic> {
    program::ScalarProgramIr::lower(program).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        compile,
        resolver::{ResolvedArgument, ResolvedStatement},
    };

    fn lower(source: &str) -> ScalarExpressionIr {
        let program = compile(source).unwrap();
        let expression = program
            .statements
            .iter()
            .find_map(|statement| match &statement.kind {
                ResolvedStatement::Output { arguments, .. } => {
                    arguments.iter().find_map(|arg| match arg {
                        ResolvedArgument::Expression(value) => Some(value),
                        _ => None,
                    })
                }
                _ => None,
            })
            .expect("source must contain a scalar output expression");
        ScalarExpressionIr::lower(
            expression,
            &program
                .binding_types()
                .iter()
                .map(|kind| kind.as_primitive().expect("scalar test type"))
                .collect::<Vec<_>>(),
            None,
        )
        .unwrap()
    }

    #[test]
    fn deterministic_typed_expression_ir_with_unicode_spans() {
        let source = "print(\"नमस्ते\"); print(1 + (2 * 3));";
        let a = lower(source);
        let b = lower(source);
        assert_eq!(a, b);
        assert!(a.steps.iter().any(|instruction| matches!(
            instruction.kind,
            Operation::Binary {
                op: BinaryOperator::Multiply,
                ..
            }
        )));
        assert!(a.steps[0].span.start > "print(\"नमस्ते\"); ".len() - 1);
        a.verify().unwrap();
    }

    #[test]
    fn short_circuit_branches_skip_the_right_hand_work() {
        for source in [
            "print(false and (1 / 0 == 1));",
            "print(true or (1 / 0 == 1));",
            "print(true and (false or false));",
            "print(not false and (2 << 3 == 16));",
        ] {
            let ir = lower(source);
            assert!(
                ir.steps
                    .iter()
                    .any(|step| matches!(step.kind, Operation::Branch { .. })),
                "{source}"
            );
            ir.verify().unwrap();
        }
    }

    #[test]
    fn standalone_verifier_rejects_mutated_targets_and_registers() {
        let mut ir = lower("print(false and (1 + 2 == 3));");
        let branch = ir
            .steps
            .iter_mut()
            .find_map(|instruction| match &mut instruction.kind {
                Operation::Branch { target, .. } => Some(target),
                _ => None,
            })
            .unwrap();
        *branch = 0;
        assert_eq!(
            ir.verify().unwrap_err().kind,
            DiagnosticKind::NativeUnsupported
        );

        let mut ir = lower("print(1 + 2);");
        let root = ir.root;
        for instruction in &mut ir.steps {
            if let Operation::Binary { left, .. } = &mut instruction.kind {
                *left = root;
            }
        }
        assert_eq!(
            ir.verify().unwrap_err().kind,
            DiagnosticKind::NativeUnsupported
        );

        let mut ir = lower("print(1 + 2);");
        ir.registers[ir.root] = ValueType::Boolean;
        assert_eq!(
            ir.verify().unwrap_err().kind,
            DiagnosticKind::NativeUnsupported
        );
    }

    #[test]
    fn branch_join_requires_assignment_on_both_paths() {
        let span = Span::new(2, 3);
        let instruction = |kind| Instruction { kind, span };
        let mut ir = ScalarExpressionIr {
            registers: vec![ValueType::Boolean, ValueType::Integer],
            slots: Vec::new(),
            root: 1,
            steps: vec![
                instruction(Operation::Boolean {
                    dst: 0,
                    value: true,
                }),
                instruction(Operation::Branch {
                    condition: 0,
                    when_true: true,
                    target: 3,
                }),
                instruction(Operation::Integer { dst: 1, value: 7 }),
                instruction(Operation::Move { dst: 1, src: 1 }),
            ],
        };
        let error = ir.verify().unwrap_err();
        assert_eq!(error.kind, DiagnosticKind::NativeUnsupported);
        assert_eq!(error.span, span);

        // With the branch and fallthrough pointing at the same instruction,
        // both paths initialize the integer before the join.
        let Operation::Branch { target, .. } = &mut ir.steps[1].kind else {
            unreachable!("test branch")
        };
        *target = 2;
        ir.verify().unwrap();
    }

    #[test]
    fn conditional_join_requires_a_value_from_each_selected_arm() {
        let intact = lower("print(if true { 1 } elif false { 2 } else { 3 });");
        let first_jump = intact
            .steps
            .iter()
            .position(|step| matches!(step.kind, Operation::Jump { .. }))
            .unwrap();
        let mut missing_value = intact.clone();
        missing_value.steps[first_jump - 1].kind = Operation::Work;
        assert_eq!(
            missing_value.verify().unwrap_err().kind,
            DiagnosticKind::NativeUnsupported
        );
        for target in [first_jump, intact.steps.len() + 1] {
            let mut bad_target = intact.clone();
            let span = bad_target.steps[first_jump].span;
            bad_target.steps[first_jump].kind = Operation::Jump { target };
            assert_eq!(bad_target.verify().unwrap_err().span, span);
        }
    }

    #[test]
    fn verifies_current_scalar_statement_expressions_without_new_cli() {
        for source in [
            "",
            "print(\"हिन्दी\");",
            "var x=1; while x<3 { x+=1; } print(x);",
            "repeat 3 as n { print(n); }",
            "if true { print(false and (1/0==1)); } else { print(42); }",
            "print(~0); print(-1 >>> 1); print(true == false);",
        ] {
            let program = compile(source).unwrap();
            crate::native_preflight::inspect(&program).unwrap();
            verify_program(&program).unwrap();
        }
    }
}
