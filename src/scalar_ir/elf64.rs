//! Self-contained x86-64 SysV integer-kernel code and ELF64 ET_REL encoding.
//! Production emission uses only safe Rust, no external compiler or linker.

mod layout;

use self::layout::{
    align_up, frame_bytes, slot_offset, stack_argument_offset, INTEGER_ARGUMENT_REGISTERS,
};
use super::{
    invalid,
    program::{ProgramOp, ScalarProgramIr},
    Operation, ScalarExpressionIr, Unary,
};
use crate::{
    ast::BinaryOperator,
    diagnostics::{Diagnostic, Span},
    native_object::{
        CheckedElf64Kernel, CHECKED_KERNEL_DIVISION_BY_ZERO, CHECKED_KERNEL_INTEGER_OVERFLOW,
        CHECKED_KERNEL_INVALID_SHIFT, CHECKED_KERNEL_REPEAT_COUNT, CHECKED_KERNEL_WORK_LIMIT,
    },
};

const MAX_TEXT_BYTES: usize = 200_000;
const MAX_OBJECT_BYTES: usize = 262_144;
// Bound both the public signature and the number of caller-stack loads.
pub(super) const MAX_INTEGER_ARGUMENTS: usize = 16;
const STACK_PROBE_INTERVAL: u32 = 4096; // Linux x86-64 base page size

fn failure() -> Diagnostic {
    invalid(Span::default())
}

struct Code {
    data: Vec<u8>,
    branches_to_failure: Vec<usize>,
    failure_spans: Vec<Span>,
    // Keep the two existing kernel encodings byte-for-byte compatible.
    compact_function: bool,
    metered: bool,
}

// Second opcode byte of a near conditional branch (0F 8x rel32).
#[repr(u8)]
enum Condition {
    Overflow = 0x80,
    Equal = 0x84,
    NotEqual = 0x85,
    Above = 0x87,
    Sign = 0x88,
    GreaterEqual = 0x8d,
}

impl Code {
    fn new(steps: usize, compact_function: bool) -> Result<Self, Diagnostic> {
        // IR work events may emit no bytes. Use the estimate for reservation
        // only and enforce the actual text limit in push.
        let capacity = steps
            .saturating_mul(24)
            .saturating_add(64)
            .min(MAX_TEXT_BYTES);
        let mut data = Vec::new();
        data.try_reserve_exact(capacity).map_err(|_| failure())?;
        Ok(Self {
            data,
            branches_to_failure: Vec::new(),
            failure_spans: Vec::new(),
            compact_function,
            metered: false,
        })
    }

    fn push(&mut self, bytes: &[u8]) -> Result<(), Diagnostic> {
        if self
            .data
            .len()
            .checked_add(bytes.len())
            .filter(|n| *n <= MAX_TEXT_BYTES)
            .is_none()
        {
            return Err(failure());
        }
        // Bounded growth is fallible: an estimated capacity need not be exact.
        self.data.try_reserve(bytes.len()).map_err(|_| failure())?;
        self.data.extend_from_slice(bytes);
        Ok(())
    }

    /// Emit a register/memory operand using RSP as the base. SIB is required
    /// even for zero displacement. The short displacement is signed, so
    /// offsets 128 and above must retain the full 32-bit encoding.
    fn stack_operand(
        &mut self,
        opcode: u8,
        register: u8,
        displacement: [u8; 4],
    ) -> Result<(), Diagnostic> {
        if register >= 16 {
            return Err(failure());
        }
        let rex = 0x48 | ((register >> 3) << 2);
        let field = (register & 7) << 3;
        let offset = i32::from_le_bytes(displacement);
        if self.compact_function {
            if offset == 0 {
                return self.push(&[rex, opcode, field | 0x04, 0x24]);
            }
            if let Ok(short) = i8::try_from(offset) {
                return self.push(&[rex, opcode, field | 0x44, 0x24, short as u8]);
            }
        }
        self.push(&[rex, opcode, field | 0x84, 0x24])?;
        self.push(&displacement)
    }

    fn load_rax(&mut self, src: usize) -> Result<(), Diagnostic> {
        self.stack_operand(0x8b, 0, slot_offset(src)?)
    }

    fn load_rcx(&mut self, src: usize) -> Result<(), Diagnostic> {
        self.stack_operand(0x8b, 1, slot_offset(src)?)
    }

    fn store_rax(&mut self, dst: usize) -> Result<(), Diagnostic> {
        self.stack_operand(0x89, 0, slot_offset(dst)?)
    }

    fn store_argument(&mut self, index: usize, register: u8) -> Result<(), Diagnostic> {
        self.stack_operand(0x89, register, slot_offset(index)?)
    }

    fn copy_stack_argument(
        &mut self,
        index: usize,
        stack_index: usize,
        frame: u32,
    ) -> Result<(), Diagnostic> {
        self.stack_operand(0x8b, 0, stack_argument_offset(frame, stack_index)?)?;
        self.store_rax(index)
    }

    /// MOV does not change arithmetic flags. Writing EAX zero-extends; C7
    /// with REX.W sign-extends its immediate. All other values need imm64.
    fn integer_rax(&mut self, value: i64) -> Result<(), Diagnostic> {
        if self.compact_function {
            if let Ok(unsigned) = u32::try_from(value) {
                self.push(&[0xb8])?; // mov eax, imm32
                return self.push(&unsigned.to_le_bytes());
            }
            if let Ok(signed) = i32::try_from(value) {
                self.push(&[0x48, 0xc7, 0xc0])?; // mov rax, sign-extended imm32
                return self.push(&signed.to_le_bytes());
            }
        }
        self.push(&[0x48, 0xb8])?; // movabs rax, imm64
        self.push(&value.to_le_bytes())
    }

    fn restore_stack_and_return(&mut self, bytes: u32) -> Result<(), Diagnostic> {
        self.push(&[0x48, 0x81, 0xc4])?; // add rsp, imm32
        self.push(&bytes.to_le_bytes())?;
        self.push(&[0xc3])
    }

    fn jump(&mut self) -> Result<usize, Diagnostic> {
        self.push(&[0xe9])?; // jmp rel32
        let displacement = self.data.len();
        self.push(&[0; 4])?;
        Ok(displacement)
    }

    fn allocate_stack(&mut self, mut bytes: u32) -> Result<(), Diagnostic> {
        // Touch each full page crossed so a large frame cannot jump a guard
        // page. The final adjustment is less than one page and all slots are
        // initialized by the verified IR before they are loaded.
        while bytes >= STACK_PROBE_INTERVAL {
            self.push(&[0x48, 0x81, 0xec])?; // sub rsp, imm32
            self.push(&STACK_PROBE_INTERVAL.to_le_bytes())?;
            self.push(&[0xc6, 0x04, 0x24, 0x00])?; // mov byte ptr [rsp], 0
            bytes -= STACK_PROBE_INTERVAL;
        }
        if bytes != 0 {
            self.push(&[0x48, 0x81, 0xec])?;
            self.push(&bytes.to_le_bytes())?;
        }
        Ok(())
    }

    /// R10 is caller-saved and unused by the scalar encoder. It is loaded
    /// once per invocation and never exposed as an argument or result.
    fn start_meter(&mut self, max_work: u32) -> Result<(), Diagnostic> {
        self.push(&[0x41, 0xba])?; // mov r10d, imm32 (zero extends)
        self.push(&max_work.to_le_bytes())?;
        self.metered = true;
        Ok(())
    }

    fn charge_work(&mut self, span: Span) -> Result<(), Diagnostic> {
        if !self.metered {
            return Ok(());
        }
        let id = self.record_failure_span(span)?;
        self.push(&[0x4d, 0x85, 0xd2])?; // test r10, r10
        self.branch_to_failure(Condition::Equal, CHECKED_KERNEL_WORK_LIMIT, id)?;
        self.push(&[0x49, 0xff, 0xca]) // dec r10
    }

    fn guard_repeat_count(&mut self, span: Span) -> Result<(), Diagnostic> {
        let span_id = self.record_failure_span(span)?;
        self.push(&[0x48, 0x85, 0xc0])?; // test rax, rax
        self.branch_to_failure(Condition::Sign, CHECKED_KERNEL_REPEAT_COUNT, span_id)
    }

    fn guard_overflow(&mut self, span: Span) -> Result<(), Diagnostic> {
        let span_id = self.record_failure_span(span)?;
        self.branch_to_failure(
            Condition::Overflow,
            CHECKED_KERNEL_INTEGER_OVERFLOW,
            span_id,
        )
    }

    fn record_failure_span(&mut self, span: Span) -> Result<u32, Diagnostic> {
        let span_id = u32::try_from(self.failure_spans.len()).map_err(|_| failure())?;
        self.failure_spans.try_reserve(1).map_err(|_| failure())?;
        self.failure_spans.push(span);
        Ok(span_id)
    }

    fn branch_to_failure(
        &mut self,
        condition: Condition,
        status: u32,
        span_id: u32,
    ) -> Result<(), Diagnostic> {
        self.branches_to_failure
            .try_reserve(1)
            .map_err(|_| failure())?;
        // The second SysV return eightbyte contains status and span ID. MOV
        // preserves the flags from the preceding operation or comparison.
        self.push(&[0x48, 0xba])?; // movabs rdx, imm64
        self.push(&status.to_le_bytes())?;
        self.push(&span_id.to_le_bytes())?;
        let displacement = self.near_branch(condition)?;
        self.branches_to_failure.push(displacement);
        Ok(())
    }

    fn near_branch(&mut self, condition: Condition) -> Result<usize, Diagnostic> {
        self.push(&[0x0f, condition as u8])?;
        let displacement = self.data.len();
        self.push(&[0; 4])?;
        Ok(displacement)
    }

    fn signed_division(&mut self, remainder: bool, span: Span) -> Result<(), Diagnostic> {
        let span_id = self.record_failure_span(span)?;
        self.push(&[0x48, 0x85, 0xc9])?; // test rcx, rcx
        self.branch_to_failure(Condition::Equal, CHECKED_KERNEL_DIVISION_BY_ZERO, span_id)?;

        // A 64-bit signed quotient can overflow only for i64::MIN / -1.
        // IDIV also requires the quotient to fit when only RDX is wanted.
        self.push(&[0x48, 0x83, 0xf9, 0xff])?; // cmp rcx, -1
        let ordinary_divisor = self.near_branch(Condition::NotEqual)?;
        self.push(&[0x48, 0xba])?; // movabs rdx, i64::MIN
        self.push(&i64::MIN.to_le_bytes())?;
        self.push(&[0x48, 0x39, 0xd0])?; // cmp rax, rdx
        self.branch_to_failure(Condition::Equal, CHECKED_KERNEL_INTEGER_OVERFLOW, span_id)?;
        let division = self.data.len();
        patch_forward_branch(&mut self.data, ordinary_divisor, division)?;
        self.push(&[0x48, 0x99])?; // cqo (sign-extend RAX into RDX:RAX)
        self.push(&[0x48, 0xf7, 0xf9])?; // idiv rcx
        if remainder {
            self.push(&[0x48, 0x89, 0xd0])?; // mov rax, rdx
        }
        Ok(())
    }

    fn binary(&mut self, op: BinaryOperator, checked: bool, span: Span) -> Result<(), Diagnostic> {
        let (instruction, can_overflow): (&[u8], bool) = match op {
            BinaryOperator::BitAnd => (&[0x48, 0x21, 0xc8], false),
            BinaryOperator::BitOr => (&[0x48, 0x09, 0xc8], false),
            BinaryOperator::BitXor => (&[0x48, 0x31, 0xc8], false),
            BinaryOperator::Add if checked => (&[0x48, 0x01, 0xc8], true),
            BinaryOperator::Subtract if checked => (&[0x48, 0x29, 0xc8], true),
            BinaryOperator::Multiply if checked => (&[0x48, 0x0f, 0xaf, 0xc1], true),
            BinaryOperator::Divide | BinaryOperator::Remainder if checked => {
                return self.signed_division(op == BinaryOperator::Remainder, span);
            }
            BinaryOperator::ShiftLeft if checked => return self.shift(0xe0, span), // shl rax, cl
            BinaryOperator::ShiftRight if checked => return self.shift(0xf8, span), // sar rax, cl
            BinaryOperator::UnsignedRight if checked => return self.shift(0xe8, span), // shr rax, cl
            _ => return Err(invalid(span)),
        };
        self.push(instruction)?;
        if can_overflow {
            self.guard_overflow(span)?;
        }
        Ok(())
    }

    fn shift(&mut self, mod_rm: u8, span: Span) -> Result<(), Diagnostic> {
        let span_id = self.record_failure_span(span)?;
        // Compare the full count as unsigned. Negative counts also exceed
        // 63, and the guard runs before x86 can mask the count to six bits.
        self.push(&[0x48, 0x83, 0xf9, 63])?; // cmp rcx, 63
        self.branch_to_failure(Condition::Above, CHECKED_KERNEL_INVALID_SHIFT, span_id)?;
        self.push(&[0x48, 0xd3, mod_rm])
    }

    fn compare(&mut self, op: BinaryOperator, span: Span) -> Result<(), Diagnostic> {
        let opcode = match op {
            BinaryOperator::Equal => 0x94,
            BinaryOperator::NotEqual => 0x95,
            BinaryOperator::Less => 0x9c,
            BinaryOperator::LessEqual => 0x9e,
            BinaryOperator::Greater => 0x9f,
            BinaryOperator::GreaterEqual => 0x9d,
            _ => return Err(invalid(span)),
        };
        self.push(&[0x48, 0x39, 0xc8])?; // cmp rax, rcx
        self.push(&[0x0f, opcode, 0xc0])?; // setcc al
        self.push(&[0x0f, 0xb6, 0xc0]) // movzx eax, al
    }

    fn finish_checked_return(&mut self) -> Result<(), Diagnostic> {
        self.push(&[0x31, 0xd2])?; // success: xor edx, edx
        if self.branches_to_failure.is_empty() {
            return Ok(());
        }
        let clear_failed_value = [0x31, 0xc0]; // failure: xor eax, eax
        self.push(&[0xeb, clear_failed_value.len() as u8])?; // success skips failure
        let failure_target = self.data.len();
        self.push(&clear_failed_value)?;
        for &displacement in &self.branches_to_failure {
            patch_forward_branch(&mut self.data, displacement, failure_target)?;
        }
        Ok(())
    }
}

fn patch_forward_branch(
    code: &mut [u8],
    displacement: usize,
    target: usize,
) -> Result<(), Diagnostic> {
    if target > code.len() {
        return Err(failure());
    }
    let offset = displacement
        .checked_add(4)
        .and_then(|end| target.checked_sub(end))
        .and_then(|offset| i32::try_from(offset).ok())
        .ok_or_else(failure)?;
    put(code, displacement, &offset.to_le_bytes())
}

/// Only bounded statement CFG edges may point backward. Expression and
/// failure edges retain their original forward-only patching.
fn patch_relative_branch(
    code: &mut [u8],
    displacement: usize,
    target: usize,
) -> Result<(), Diagnostic> {
    let end = displacement.checked_add(4).ok_or_else(failure)?;
    if target > code.len() || end > code.len() {
        return Err(failure());
    }
    let target = i64::try_from(target).map_err(|_| failure())?;
    let end = i64::try_from(end).map_err(|_| failure())?;
    let offset = i32::try_from(target - end).map_err(|_| failure())?;
    put(code, displacement, &offset.to_le_bytes())
}

fn temporary(base: usize, register: usize) -> Result<usize, Diagnostic> {
    base.checked_add(register).ok_or_else(failure)
}

/// Keep final user bindings in a separate stack region from the ephemeral
/// registers reused for each initializer. The verified IR guarantees
/// definite initialization before any Load; no native input pointer exists.
fn text(program: &ScalarProgramIr, checked_arithmetic: bool) -> Result<Code, Diagnostic> {
    let slot_count = program.integer_slot_count();
    let mut max_registers = 0usize;
    let mut total_steps = 0usize;
    for (_, expr) in program.integer_stores() {
        expr.verify()?;
        max_registers = max_registers.max(expr.registers.len());
        total_steps = total_steps
            .checked_add(expr.steps.len())
            .ok_or_else(failure)?;
    }
    let stack_size = slot_count
        .checked_add(max_registers)
        .and_then(|n| n.checked_mul(8))
        .and_then(|n| u32::try_from(n).ok())
        .ok_or_else(failure)?;
    let mut code = Code::new(total_steps, false)?;
    code.allocate_stack(stack_size)?;

    for (slot, expr) in program.integer_stores() {
        expression(&mut code, expr, slot_count, checked_arithmetic, false)?;
        code.store_rax(slot)?;
    }

    if checked_arithmetic {
        code.finish_checked_return()?;
    }
    code.restore_stack_and_return(stack_size)?;
    Ok(code)
}

fn expression(
    code: &mut Code,
    expr: &ScalarExpressionIr,
    slot_count: usize,
    checked: bool,
    control_flow: bool,
) -> Result<(), Diagnostic> {
    let mut offsets = Vec::new();
    let mut branches = Vec::new();
    let register = |index| temporary(slot_count, index);
    for step in &expr.steps {
        if control_flow {
            offsets.push(code.data.len());
        }
        match step.kind {
            Operation::Work => code.charge_work(step.span)?,
            Operation::Integer { dst, value } => {
                code.integer_rax(value)?;
                code.store_rax(register(dst)?)?;
            }
            Operation::Boolean { dst, value } if control_flow => {
                code.push(&[0xb8, u8::from(value), 0, 0, 0])?; // mov eax, imm32
                code.store_rax(register(dst)?)?;
            }
            Operation::Load { dst, slot } => {
                code.load_rax(slot)?;
                code.store_rax(register(dst)?)?;
            }
            Operation::Move { dst, src } => {
                code.load_rax(register(src)?)?;
                code.store_rax(register(dst)?)?;
            }
            Operation::Unary { dst, src, op } => {
                code.load_rax(register(src)?)?;
                match op {
                    Unary::BitNot => code.push(&[0x48, 0xf7, 0xd0])?, // not rax
                    Unary::Negate if checked => {
                        code.push(&[0x48, 0xf7, 0xd8])?; // neg rax
                        code.guard_overflow(step.span)?;
                    }
                    // A verified Boolean is canonical zero or one.
                    Unary::Not if control_flow => code.push(&[0x83, 0xf0, 0x01])?,
                    _ => return Err(invalid(step.span)),
                }
                code.store_rax(register(dst)?)?;
            }
            Operation::Binary {
                dst,
                left,
                right,
                op,
            } => {
                code.load_rax(register(left)?)?;
                code.load_rcx(register(right)?)?;
                if control_flow && op.is_comparison() {
                    code.compare(op, step.span)?;
                } else {
                    code.binary(op, checked, step.span)?;
                }
                code.store_rax(register(dst)?)?;
            }
            Operation::Branch {
                condition,
                when_true,
                target,
            } if control_flow => {
                code.load_rax(register(condition)?)?;
                code.push(&[0x48, 0x85, 0xc0])?; // test rax, rax
                let condition = if when_true {
                    Condition::NotEqual
                } else {
                    Condition::Equal
                };
                branches.push((code.near_branch(condition)?, target));
            }
            _ => return Err(invalid(step.span)),
        }
    }
    if control_flow {
        offsets.push(code.data.len());
        resolve_branches(&mut code.data, &offsets, branches)?;
    }
    code.load_rax(register(expr.root)?)
}

fn resolve_branches(
    data: &mut [u8],
    offsets: &[usize],
    branches: Vec<(usize, usize)>,
) -> Result<(), Diagnostic> {
    for (displacement, target) in branches {
        let offset = offsets.get(target).ok_or_else(failure)?;
        patch_forward_branch(data, displacement, *offset)?;
    }
    Ok(())
}

fn function_text(program: &ScalarProgramIr, budget: Option<u32>) -> Result<Code, Diagnostic> {
    program.verify()?;
    if !program.is_function() || program.is_bounded() != budget.is_some() {
        return Err(failure());
    }
    let parameters = program.parameters();
    if parameters > MAX_INTEGER_ARGUMENTS {
        return Err(failure());
    }
    let register_count = parameters.min(INTEGER_ARGUMENT_REGISTERS.len());
    let registers = &INTEGER_ARGUMENT_REGISTERS[..register_count];
    let mut max_registers = 0;
    let mut estimated_steps = program.operations().len();
    for step in program.operations() {
        let value = match &step.kind {
            ProgramOp::Return { value }
            | ProgramOp::Store { value, .. }
            | ProgramOp::Update { value, .. }
            | ProgramOp::RepeatInit { count: value, .. }
            | ProgramOp::JumpIfFalse {
                condition: value, ..
            } => value,
            _ => continue,
        };
        max_registers = max_registers.max(value.registers.len());
        estimated_steps = estimated_steps
            .checked_add(value.steps.len())
            .ok_or_else(failure)?;
    }
    let slot_count = program.integer_slot_count();
    let stack_size = frame_bytes(slot_count, max_registers)?;
    let mut code = Code::new(estimated_steps, true)?;
    code.allocate_stack(stack_size)?;
    for (slot, register) in registers.iter().copied().enumerate() {
        code.store_argument(slot, register)?;
    }
    // Copy caller-owned arguments into ordinary verified local slots; never
    // write into the caller's frame or consume its return address as a value.
    for slot in register_count..parameters {
        code.copy_stack_argument(slot, slot - register_count, stack_size)?;
    }

    if let Some(max_work) = budget {
        code.start_meter(max_work)?;
    }

    let mut offsets = Vec::with_capacity(program.operations().len() + 1);
    let mut branches = Vec::new();
    let mut returns = Vec::new();
    for step in program.operations() {
        offsets.push(code.data.len());
        if step.charge_work {
            code.charge_work(step.span)?;
        }
        match &step.kind {
            ProgramOp::Store { slot, value } => {
                expression(&mut code, value, slot_count, true, true)?;
                code.store_rax(*slot)?;
            }
            ProgramOp::Update {
                slot,
                operator,
                operator_span,
                value,
            } => {
                // The RHS cannot call or write slots. Scarp snapshots the LHS
                // first, which is equivalent to reading the same slot here.
                expression(&mut code, value, slot_count, true, true)?;
                code.push(&[0x48, 0x89, 0xc1])?; // mov rcx, rax
                code.load_rax(*slot)?;
                code.binary(*operator, true, *operator_span)?;
                code.store_rax(*slot)?;
            }
            ProgramOp::Return { value } => {
                expression(&mut code, value, slot_count, true, true)?;
                returns.push(code.jump()?);
            }
            ProgramOp::JumpIfFalse { condition, target } => {
                expression(&mut code, condition, slot_count, true, true)?;
                code.push(&[0x48, 0x85, 0xc0])?; // test rax, rax
                branches.push((code.near_branch(Condition::Equal)?, *target));
            }
            ProgramOp::Jump { target } => branches.push((code.jump()?, *target)),
            ProgramOp::RepeatInit {
                count,
                count_slot,
                index_slot,
            } if code.metered => {
                expression(&mut code, count, slot_count, true, true)?;
                code.guard_repeat_count(step.span)?;
                code.store_rax(*count_slot)?;
                code.integer_rax(0)?;
                code.store_rax(*index_slot)?;
            }
            ProgramOp::RepeatGuard {
                count_slot,
                index_slot,
                target,
            } if code.metered => {
                code.load_rax(*index_slot)?;
                code.load_rcx(*count_slot)?;
                code.push(&[0x48, 0x39, 0xc8])?; // cmp rax, rcx
                branches.push((code.near_branch(Condition::GreaterEqual)?, *target));
            }
            ProgramOp::RepeatNext { index_slot } if code.metered => {
                code.load_rax(*index_slot)?;
                code.push(&[0x48, 0x83, 0xc0, 0x01])?; // add rax, 1
                code.guard_overflow(step.span)?;
                code.store_rax(*index_slot)?;
            }
            ProgramOp::LoopControl { target, .. } if code.metered => {
                branches.push((code.jump()?, *target));
            }
            ProgramOp::Retire { .. } => {
                // Scalar slots own no heap payload. Definite initialization
                // prevents any read following their lexical retirement.
            }
            _ => return Err(invalid(step.span)),
        }
    }
    let success = code.data.len();
    offsets.push(success);
    if code.metered {
        for (displacement, target) in branches {
            let offset = *offsets.get(target).ok_or_else(failure)?;
            patch_relative_branch(&mut code.data, displacement, offset)?;
        }
    } else {
        resolve_branches(&mut code.data, &offsets, branches)?;
    }
    for displacement in returns {
        patch_forward_branch(&mut code.data, displacement, success)?;
    }
    code.finish_checked_return()?;
    code.restore_stack_and_return(stack_size)?;
    Ok(code)
}

fn put(target: &mut [u8], at: usize, bytes: &[u8]) -> Result<(), Diagnostic> {
    let end = at.checked_add(bytes.len()).ok_or_else(failure)?;
    target
        .get_mut(at..end)
        .ok_or_else(failure)?
        .copy_from_slice(bytes);
    Ok(())
}
fn u16at(target: &mut [u8], at: usize, value: u16) -> Result<(), Diagnostic> {
    put(target, at, &value.to_le_bytes())
}
fn u32at(target: &mut [u8], at: usize, value: u32) -> Result<(), Diagnostic> {
    put(target, at, &value.to_le_bytes())
}
fn u64at(target: &mut [u8], at: usize, value: u64) -> Result<(), Diagnostic> {
    put(target, at, &value.to_le_bytes())
}

#[allow(clippy::too_many_arguments)]
fn section(
    output: &mut [u8],
    offset: usize,
    index: usize,
    name: u32,
    kind: u32,
    flags: u64,
    file_offset: usize,
    size: usize,
    link: u32,
    info: u32,
    alignment: u64,
    entry_size: u64,
) -> Result<(), Diagnostic> {
    let mut header = [0u8; 64];
    u32at(&mut header, 0, name)?;
    u32at(&mut header, 4, kind)?;
    u64at(&mut header, 8, flags)?;
    u64at(&mut header, 24, file_offset as u64)?;
    u64at(&mut header, 32, size as u64)?;
    u32at(&mut header, 40, link)?;
    u32at(&mut header, 44, info)?;
    u64at(&mut header, 48, alignment)?;
    u64at(&mut header, 56, entry_size)?;
    put(
        output,
        offset
            .checked_add(index.checked_mul(64).ok_or_else(failure)?)
            .ok_or_else(failure)?,
        &header,
    )
}

pub(super) fn emit(program: &ScalarProgramIr) -> Result<Vec<u8>, Diagnostic> {
    let code = text(program, false)?;
    object(&code.data, b"\0scarp_scalar_kernel\0")
}

pub(super) fn emit_checked(program: &ScalarProgramIr) -> Result<CheckedElf64Kernel, Diagnostic> {
    let code = text(program, true)?;
    let object = object(&code.data, b"\0scarp_checked_scalar_kernel\0")?;
    Ok(CheckedElf64Kernel {
        object,
        failure_spans: code.failure_spans,
    })
}

pub(super) fn emit_function(program: &ScalarProgramIr) -> Result<CheckedElf64Kernel, Diagnostic> {
    let code = function_text(program, None)?;
    let object = object(&code.data, b"\0scarp_checked_integer_function\0")?;
    Ok(CheckedElf64Kernel {
        object,
        failure_spans: code.failure_spans,
    })
}

pub(super) fn emit_bounded_function(
    program: &ScalarProgramIr,
    max_work: u32,
) -> Result<CheckedElf64Kernel, Diagnostic> {
    let code = function_text(program, Some(max_work))?;
    let object = object(&code.data, b"\0scarp_bounded_integer_function\0")?;
    Ok(CheckedElf64Kernel {
        object,
        failure_spans: code.failure_spans,
    })
}

fn object(machine: &[u8], strings: &[u8]) -> Result<Vec<u8>, Diagnostic> {
    // Offsets are into shstrtab, starting with the required NUL name.
    const SHSTR: &[u8] = b"\0.text\0.symtab\0.strtab\0.shstrtab\0.note.GNU-stack\0";
    let text_offset = align_up(64, 16)?;
    let sym_offset = align_up(
        text_offset.checked_add(machine.len()).ok_or_else(failure)?,
        8,
    )?;
    let str_offset = sym_offset.checked_add(48).ok_or_else(failure)?;
    let shstr_offset = str_offset.checked_add(strings.len()).ok_or_else(failure)?;
    let header_offset = align_up(
        shstr_offset.checked_add(SHSTR.len()).ok_or_else(failure)?,
        8,
    )?;
    let total = header_offset
        .checked_add(6 * 64)
        .filter(|n| *n <= MAX_OBJECT_BYTES)
        .ok_or_else(failure)?;

    let mut object = Vec::new();
    object.try_reserve_exact(total).map_err(|_| failure())?;
    object.resize(total, 0);
    put(&mut object, 0, b"\x7fELF")?;
    put(&mut object, 4, &[2, 1, 1])?; // ELFCLASS64, ELFDATA2LSB, EV_CURRENT
    u16at(&mut object, 16, 1)?; // ET_REL
    u16at(&mut object, 18, 62)?; // EM_X86_64
    u32at(&mut object, 20, 1)?;
    u64at(&mut object, 40, header_offset as u64)?;
    u16at(&mut object, 52, 64)?;
    u16at(&mut object, 58, 64)?;
    u16at(&mut object, 60, 6)?;
    u16at(&mut object, 62, 4)?; // .shstrtab index

    put(&mut object, text_offset, machine)?;
    let mut symbols = [0u8; 48]; // null symbol and one global function
    u32at(&mut symbols, 24, 1)?; // name offset in .strtab
    symbols[28] = 0x12; // STB_GLOBAL | STT_FUNC
    u16at(&mut symbols, 30, 1)?; // .text section
    u64at(&mut symbols, 40, machine.len() as u64)?;
    put(&mut object, sym_offset, &symbols)?;
    put(&mut object, str_offset, strings)?;
    put(&mut object, shstr_offset, SHSTR)?;

    // Null, executable .text, symtab, strtab, shstrtab, GNU noexec-stack.
    section(
        &mut object,
        header_offset,
        1,
        1,
        1,
        6,
        text_offset,
        machine.len(),
        0,
        0,
        16,
        0,
    )?;
    section(
        &mut object,
        header_offset,
        2,
        7,
        2,
        0,
        sym_offset,
        symbols.len(),
        3,
        1,
        8,
        24,
    )?;
    section(
        &mut object,
        header_offset,
        3,
        15,
        3,
        0,
        str_offset,
        strings.len(),
        0,
        0,
        1,
        0,
    )?;
    section(
        &mut object,
        header_offset,
        4,
        23,
        3,
        0,
        shstr_offset,
        SHSTR.len(),
        0,
        0,
        1,
        0,
    )?;
    section(&mut object, header_offset, 5, 33, 1, 0, 0, 0, 0, 0, 1, 0)?;
    Ok(object)
}

#[cfg(test)]
mod encoding_tests {
    use super::*;

    #[test]
    fn bounded_statement_branches_encode_signed_relative_offsets() {
        let mut bytes = [0u8; 16];
        patch_relative_branch(&mut bytes, 4, 0).unwrap();
        assert_eq!(i32::from_le_bytes(bytes[4..8].try_into().unwrap()), -8);
        patch_relative_branch(&mut bytes, 4, 16).unwrap();
        assert_eq!(i32::from_le_bytes(bytes[4..8].try_into().unwrap()), 8);
        assert!(patch_relative_branch(&mut bytes, 14, 0).is_err());
        assert!(patch_relative_branch(&mut bytes, 4, 17).is_err());
        assert!(patch_forward_branch(&mut bytes, 4, 0).is_err());
    }

    #[test]
    fn compact_stack_operands_use_the_smallest_valid_displacement() {
        for (slot, expected) in [
            (0, vec![0x48, 0x8b, 0x04, 0x24]),
            (1, vec![0x48, 0x8b, 0x44, 0x24, 8]),
            (15, vec![0x48, 0x8b, 0x44, 0x24, 120]),
            (16, vec![0x48, 0x8b, 0x84, 0x24, 128, 0, 0, 0]),
        ] {
            let mut code = Code::new(1, true).unwrap();
            code.load_rax(slot).unwrap();
            assert_eq!(code.data, expected);
        }
        let mut code = Code::new(1, true).unwrap();
        code.store_argument(0, 9).unwrap();
        assert_eq!(code.data, [0x4c, 0x89, 0x0c, 0x24]);
        assert!(code.store_argument(0, 16).is_err());

        let mut code = Code::new(1, false).unwrap();
        code.load_rax(0).unwrap();
        assert_eq!(code.data, [0x48, 0x8b, 0x84, 0x24, 0, 0, 0, 0]);
    }

    #[test]
    fn compact_immediates_preserve_signed_and_unsigned_boundaries() {
        for (value, prefix, len) in [
            (0, &[0xb8][..], 5),
            (i64::from(u32::MAX), &[0xb8][..], 5),
            (i64::from(u32::MAX) + 1, &[0x48, 0xb8][..], 10),
            (-1, &[0x48, 0xc7, 0xc0][..], 7),
            (i64::from(i32::MIN), &[0x48, 0xc7, 0xc0][..], 7),
            (i64::from(i32::MIN) - 1, &[0x48, 0xb8][..], 10),
            (i64::MIN, &[0x48, 0xb8][..], 10),
            (i64::MAX, &[0x48, 0xb8][..], 10),
        ] {
            let mut code = Code::new(1, true).unwrap();
            code.integer_rax(value).unwrap();
            assert_eq!(code.data.len(), len);
            assert!(code.data.starts_with(prefix));
            let decoded = match len {
                5 => i64::from(u32::from_le_bytes(code.data[1..].try_into().unwrap())),
                7 => i64::from(i32::from_le_bytes(code.data[3..].try_into().unwrap())),
                _ => i64::from_le_bytes(code.data[2..].try_into().unwrap()),
            };
            assert_eq!(decoded, value);
        }
        let mut code = Code::new(1, false).unwrap();
        code.integer_rax(1).unwrap();
        assert_eq!(code.data, [0x48, 0xb8, 1, 0, 0, 0, 0, 0, 0, 0]);
    }
}
