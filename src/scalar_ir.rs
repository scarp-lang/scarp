//! Private, target-neutral scalar expression IR (first native IR slice).
//!
//! This lowers already-resolved integer/boolean expressions, never reparses
//! source, and does not emit machine code or introduce a public ABI. Future
//! statement and object lowering will build on its checked control-flow form.

mod elf64;
mod program;

use crate::{
    ast::{BinaryOperator, ExpressionKind, LogicalOperator},
    diagnostics::{Diagnostic, DiagnosticKind, Span},
    resolver::{ResolvedExpression, ResolvedProgram},
    value::ValueType,
};

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
    /// Both paths are conservatively considered possible by the verifier.
    /// The target must be forward and may point exactly past the last step.
    Branch {
        condition: usize,
        when_true: bool,
        target: usize,
    },
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
    Logical(usize),
    CloseLogical { node: usize, branch: usize },
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

fn join(destination: &mut Option<Vec<bool>>, incoming: &[bool]) {
    match destination {
        Some(existing) => {
            for (bit, other) in existing.iter_mut().zip(incoming) {
                *bit &= *other;
            }
        }
        None => *destination = Some(incoming.to_vec()),
    }
}

impl ScalarExpressionIr {
    fn emit(&mut self, span: Span, kind: Operation) -> Result<usize, Diagnostic> {
        if self.steps.len() >= MAX_IR_STEPS || span.start > span.end {
            return Err(invalid(span));
        }
        let index = self.steps.len();
        self.steps.push(Instruction { kind, span });
        Ok(index)
    }

    fn lower(expression: &ResolvedExpression, slots: &[ValueType]) -> Result<Self, Diagnostic> {
        let nodes = expression.syntax.nodes();
        if nodes.is_empty()
            || nodes.len() > MAX_IR_REGISTERS
            || expression.syntax.root() >= nodes.len()
            || expression.types.len() != nodes.len()
            || expression.slots.len() != nodes.len()
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
                        _ => return Err(invalid(node.span)),
                    }
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
                    let end = ir.steps.len();
                    if let Operation::Branch { target, .. } = &mut ir.steps[branch].kind {
                        *target = end;
                    } else {
                        return Err(invalid(node.span));
                    }
                }
            }
        }
        ir.verify()?;
        Ok(ir)
    }

    /// Verify forward-only CFG edges, definite assignment at every join,
    /// scalar types and bounded memory before any backend consumes this IR.
    fn verify(&self) -> Result<(), Diagnostic> {
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
            let mut initialized = incoming[pc]
                .clone()
                .ok_or_else(|| invalid(instruction.span))?;
            let mut branch = None;
            let types = &self.registers;
            let span = instruction.span;
            match &instruction.kind {
                Operation::Work => {}
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
                Operation::Branch {
                    condition, target, ..
                } => {
                    require_read(&initialized, types, *condition, ValueType::Boolean, span)?;
                    if *target <= pc || *target > self.steps.len() {
                        return Err(invalid(span));
                    }
                    branch = Some(*target);
                }
            }
            join(&mut incoming[pc + 1], &initialized);
            if let Some(target) = branch {
                join(&mut incoming[target], &initialized);
            }
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

/// Compile a strictly restricted pure integer initializer to a self-contained
/// x86-64 ELF64 relocatable kernel. The ABI is experimental and is not the
/// observable Scarp stdout/program-execution contract.
pub(crate) fn emit_integer_kernel(source: &str) -> Result<Vec<u8>, Diagnostic> {
    let program = crate::compile(source)?;
    crate::native_preflight::inspect(&program)?;
    let ir = program::ScalarProgramIr::lower(&program)?;
    elf64::emit(ir.single_integer_kernel()?)
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
        crate::native_preflight::inspect(&program).unwrap();
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
        ScalarExpressionIr::lower(expression, program.binding_types()).unwrap()
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
