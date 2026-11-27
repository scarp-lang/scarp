//! Self-contained x86-64 SysV integer-kernel code and ELF64 ET_REL encoding.
//! Production emission uses only safe Rust, no external compiler or linker.

mod layout;
mod temporaries;

use self::layout::{
    frame_bytes, slot_offset, stack_argument_offset, FileRegion, ObjectLayout, ELF_HEADER_BYTES,
    INTEGER_ARGUMENT_REGISTERS, MAX_TEXT_BYTES, SECTION_COUNT, SECTION_HEADER_BYTES, SYMBOL_BYTES,
};
use self::temporaries::{TemporaryHome, TemporaryLayout};
use super::{
    invalid,
    program::{ProgramOp, ScalarCallGraph, ScalarProgramIr},
    Operation, ScalarExpressionIr, Unary,
};
use crate::{
    ast::BinaryOperator,
    diagnostics::{Diagnostic, Span},
    native_object::{
        CheckedElf64Kernel, FreestandingCallLayout, FreestandingEntry, CHECKED_KERNEL_ARRAY_INDEX,
        CHECKED_KERNEL_DIVISION_BY_ZERO, CHECKED_KERNEL_INTEGER_OVERFLOW,
        CHECKED_KERNEL_INVALID_SHIFT, CHECKED_KERNEL_REPEAT_COUNT, CHECKED_KERNEL_WORK_LIMIT,
    },
};
use std::borrow::Cow;

// Bound both the public signature and the number of caller-stack loads.
pub(super) const MAX_INTEGER_ARGUMENTS: usize = 16;
const REGISTER_ARGUMENTS: usize = INTEGER_ARGUMENT_REGISTERS.len();
const STACK_PROBE_INTERVAL: u32 = 4096; // Linux x86-64 base page size
const TEMPORARY_REGISTER: u8 = 11; // R11, caller-saved by x86-64 SysV

fn failure() -> Diagnostic {
    invalid(Span::default())
}

struct Code {
    data: Vec<u8>,
    branches_to_failure: Vec<usize>,
    failure_spans: Vec<Span>,
    call_fixups: Vec<(usize, usize)>,
    // Keep the two existing kernel encodings byte-for-byte compatible.
    compact_function: bool,
    metered: bool,
}

// Second opcode byte of a near conditional branch (0F 8x rel32).
#[repr(u8)]
enum Condition {
    Overflow = 0x80,
    Below = 0x82,
    AboveEqual = 0x83,
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
            call_fixups: Vec::new(),
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

    fn move_register(&mut self, destination: u8, source: u8) -> Result<(), Diagnostic> {
        if destination >= 16 || source >= 16 {
            return Err(failure());
        }
        let rex = 0x48 | ((source >> 3) << 2) | (destination >> 3);
        let operand = 0xc0 | ((source & 7) << 3) | (destination & 7);
        self.push(&[rex, 0x89, operand])
    }

    fn load_temporary(&mut self, home: TemporaryHome, register: u8) -> Result<(), Diagnostic> {
        match home {
            TemporaryHome::Stack(slot) => self.stack_operand(0x8b, register, slot_offset(slot)?),
            TemporaryHome::R11 => self.move_register(register, TEMPORARY_REGISTER),
        }
    }

    fn store_temporary(&mut self, home: TemporaryHome) -> Result<(), Diagnostic> {
        match home {
            TemporaryHome::Stack(slot) => self.store_rax(slot),
            TemporaryHome::R11 => self.move_register(TEMPORARY_REGISTER, 0),
        }
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

    fn call(&mut self, callee: usize) -> Result<(), Diagnostic> {
        self.call_fixups.try_reserve(1).map_err(|_| failure())?;
        self.branches_to_failure
            .try_reserve(1)
            .map_err(|_| failure())?;
        self.push(&[0xe8])?; // call rel32
        let displacement = self.data.len();
        self.push(&[0; 4])?;
        self.call_fixups.push((displacement, callee));
        // Preserve the callee's packed RDX failure while unwinding. Only
        // the low status word is tested, and this branch does not charge work.
        self.push(&[0x85, 0xd2])?; // test edx, edx
        let failure = self.near_branch(Condition::NotEqual)?;
        self.branches_to_failure.push(failure);
        Ok(())
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

    /// R10 is caller-saved and reserved as the shared native work meter.
    /// Only external entries initialize it, and generated callees debit it.
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
        // Unsigned subtraction borrows exactly when the old allowance was zero.
        // The failure branch preserves CF and returns before the wrapped value is used.
        self.push(&[0x41, 0x83, 0xea, 0x01])?; // sub r10d, 1
        self.branch_to_failure(Condition::Below, CHECKED_KERNEL_WORK_LIMIT, id)
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
        for displacement in self.branches_to_failure.drain(..) {
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

#[derive(Clone, Copy)]
struct StackSlots {
    /// Slots below the locals are reserved for arguments passed to callees.
    local_base: usize,
    temporary_base: usize,
}

impl StackSlots {
    fn local(self, index: usize) -> Result<usize, Diagnostic> {
        self.local_base.checked_add(index).ok_or_else(failure)
    }

    fn temporary(self, index: usize) -> Result<usize, Diagnostic> {
        self.temporary_base.checked_add(index).ok_or_else(failure)
    }
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

    let slots = StackSlots {
        local_base: 0,
        temporary_base: slot_count,
    };
    for (slot, expr) in program.integer_stores() {
        expression(&mut code, expr, slots, checked_arithmetic, None, None)?;
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
    slots: StackSlots,
    checked: bool,
    layout: Option<&TemporaryLayout>,
    destination: Option<usize>,
) -> Result<(), Diagnostic> {
    let mut offsets = Vec::new();
    let mut branches = Vec::new();
    // Binding-only kernels keep their original byte-for-byte layout. Only
    // the function backends opt in to verified temporary slot reuse.
    let control_flow = layout.is_some();
    let home = |index| {
        let home = match layout {
            Some(layout) => layout.home(index)?,
            None => TemporaryHome::Stack(index),
        };
        match home {
            TemporaryHome::Stack(slot) => Ok(TemporaryHome::Stack(slots.temporary(slot)?)),
            TemporaryHome::R11 => Ok(TemporaryHome::R11),
        }
    };
    for step in &expr.steps {
        if control_flow {
            offsets.push(code.data.len());
        }
        match step.kind {
            Operation::Work => code.charge_work(step.span)?,
            Operation::Integer { dst, value } => {
                code.integer_rax(value)?;
                code.store_temporary(home(dst)?)?;
            }
            Operation::Boolean { dst, value } if control_flow => {
                code.push(&[0xb8, u8::from(value), 0, 0, 0])?; // mov eax, imm32
                code.store_temporary(home(dst)?)?;
            }
            Operation::Load { dst, slot } => {
                code.load_rax(slots.local(slot)?)?;
                code.store_temporary(home(dst)?)?;
            }
            Operation::Move { dst, src } => {
                code.load_temporary(home(src)?, 0)?;
                code.store_temporary(home(dst)?)?;
            }
            Operation::Unary { dst, src, op } => {
                code.load_temporary(home(src)?, 0)?;
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
                code.store_temporary(home(dst)?)?;
            }
            Operation::Binary {
                dst,
                left,
                right,
                op,
            } => {
                code.load_temporary(home(left)?, 0)?;
                code.load_temporary(home(right)?, 1)?;
                if control_flow && op.is_comparison() {
                    code.compare(op, step.span)?;
                } else {
                    code.binary(op, checked, step.span)?;
                }
                code.store_temporary(home(dst)?)?;
            }
            Operation::Index {
                dst,
                index,
                ref elements,
            } if code.metered => {
                code.load_temporary(home(index)?, 1)?;
                // Unsigned comparison also rejects negative signed indices.
                code.push(&[0x48, 0x83, 0xf9, elements.len() as u8])?; // cmp rcx, len
                let span_id = code.record_failure_span(step.span)?;
                code.branch_to_failure(Condition::AboveEqual, CHECKED_KERNEL_ARRAY_INDEX, span_id)?;
                code.load_temporary(home(elements[0])?, 0)?;
                for (position, &element) in elements.iter().enumerate().skip(1) {
                    code.load_temporary(home(element)?, 2)?;
                    code.push(&[0x48, 0x83, 0xf9, position as u8])?; // cmp rcx, position
                    code.push(&[0x48, 0x0f, 0x44, 0xc2])?; // cmove rax, rdx
                }
                code.store_temporary(home(dst)?)?;
            }
            Operation::Branch {
                condition,
                when_true,
                target,
            } if control_flow => {
                code.load_temporary(home(condition)?, 0)?;
                code.push(&[0x48, 0x85, 0xc0])?; // test rax, rax
                let condition = if when_true {
                    Condition::NotEqual
                } else {
                    Condition::Equal
                };
                branches.push((code.near_branch(condition)?, target));
            }
            Operation::Jump { target } if control_flow => {
                branches.push((code.jump()?, target));
            }
            Operation::Call {
                dst,
                callee,
                ref arguments,
            } if control_flow && code.metered => {
                let outgoing = arguments.len().saturating_sub(REGISTER_ARGUMENTS);
                if arguments.len() > MAX_INTEGER_ARGUMENTS || outgoing > slots.local_base {
                    return Err(invalid(step.span));
                }
                // RSP is already aligned and points at argument seven's slot
                // at the call site. Copy from the disjoint temporary region.
                for (stack_index, &argument) in
                    arguments.iter().skip(REGISTER_ARGUMENTS).enumerate()
                {
                    code.load_rax(home(argument)?.stack_slot()?)?;
                    code.store_rax(stack_index)?;
                }
                for (&argument, &hardware) in arguments.iter().zip(&INTEGER_ARGUMENT_REGISTERS) {
                    code.stack_operand(
                        0x8b,
                        hardware,
                        slot_offset(home(argument)?.stack_slot()?)?,
                    )?;
                }
                code.call(callee)?;
                code.store_rax(home(dst)?.stack_slot()?)?;
            }
            _ => return Err(invalid(step.span)),
        }
    }
    if control_flow {
        offsets.push(code.data.len());
        resolve_branches(&mut code.data, &offsets, branches)?;
    }
    if let Some(destination) = destination {
        for (offset, register) in expr.results().enumerate() {
            code.load_temporary(home(register)?, 0)?;
            code.store_rax(slots.local(destination + offset)?)?;
        }
        Ok(())
    } else {
        code.load_temporary(home(expr.root)?, 0)
    }
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

struct FunctionLayout<'a> {
    expressions: Vec<Option<(Cow<'a, ScalarExpressionIr>, TemporaryLayout)>>,
    slots: StackSlots,
    frame_bytes: u32,
    estimated_steps: usize,
}

fn plan_function<'a>(
    program: &'a ScalarProgramIr,
    call_arities: Option<&[usize]>,
) -> Result<FunctionLayout<'a>, Diagnostic> {
    if !program.is_function() || program.parameters() > MAX_INTEGER_ARGUMENTS {
        return Err(failure());
    }
    let mut max_registers = 0;
    let mut outgoing = 0;
    let mut estimated_steps = program.operations().len();
    let mut expressions = Vec::with_capacity(program.operations().len());
    for step in program.operations() {
        let Some(value) = step.kind.expression() else {
            expressions.push(None);
            continue;
        };
        let value = value.optimize_verified(call_arities)?;
        let mut layout = TemporaryLayout::plan_verified(&value)?;
        if call_arities.is_some() {
            layout.allocate_r11(&value);
        }
        max_registers = max_registers.max(layout.count());
        for instruction in &value.steps {
            if let Operation::Call { arguments, .. } = &instruction.kind {
                outgoing = outgoing.max(arguments.len().saturating_sub(REGISTER_ARGUMENTS));
            }
        }
        estimated_steps = estimated_steps
            .checked_add(value.steps.len())
            .ok_or_else(failure)?;
        expressions.push(Some((value, layout)));
    }
    let slots = StackSlots {
        local_base: outgoing,
        temporary_base: outgoing
            .checked_add(program.integer_slot_count())
            .ok_or_else(failure)?,
    };
    Ok(FunctionLayout {
        expressions,
        slots,
        frame_bytes: frame_bytes(slots.temporary_base, max_registers)?,
        estimated_steps,
    })
}

fn append_function(
    code: &mut Code,
    program: &ScalarProgramIr,
    layout: &FunctionLayout<'_>,
    initial_budget: Option<u32>,
) -> Result<(), Diagnostic> {
    if program.is_bounded() != code.metered {
        return Err(failure());
    }
    let parameters = program.parameters();
    let register_count = parameters.min(REGISTER_ARGUMENTS);
    let registers = &INTEGER_ARGUMENT_REGISTERS[..register_count];
    let stack_size = layout.frame_bytes;
    let slots = layout.slots;
    code.allocate_stack(stack_size)?;
    for (slot, register) in registers.iter().copied().enumerate() {
        code.store_argument(slots.local(slot)?, register)?;
    }
    // Copy caller-owned arguments into ordinary verified local slots; never
    // write into the caller's frame or consume its return address as a value.
    for slot in register_count..parameters {
        code.copy_stack_argument(slots.local(slot)?, slot - register_count, stack_size)?;
    }

    if let Some(max_work) = initial_budget {
        code.start_meter(max_work)?;
    }

    let mut offsets = Vec::with_capacity(program.operations().len() + 1);
    let mut branches = Vec::new();
    let mut returns = Vec::new();
    for (step, planned) in program.operations().iter().zip(&layout.expressions) {
        offsets.push(code.data.len());
        if step.charge_work {
            code.charge_work(step.span)?;
        }
        if let Some((value, layout)) = planned {
            let destination = match step.kind {
                ProgramOp::Store { slot, .. } => Some(slot),
                _ => None,
            };
            expression(code, value, slots, true, Some(layout), destination)?;
        }
        match &step.kind {
            ProgramOp::Store { .. } => {}
            ProgramOp::Update {
                slot,
                operator,
                operator_span,
                ..
            } => {
                // Pure calls cannot write their caller's slots. Reading the
                // LHS here yields the same value as the interpreter snapshot.
                code.push(&[0x48, 0x89, 0xc1])?; // mov rcx, rax
                let slot = slots.local(*slot)?;
                code.load_rax(slot)?;
                code.binary(*operator, true, *operator_span)?;
                code.store_rax(slot)?;
            }
            ProgramOp::Return { .. } => {
                returns.push(code.jump()?);
            }
            ProgramOp::JumpIfFalse { target, .. } => {
                code.push(&[0x48, 0x85, 0xc0])?; // test rax, rax
                branches.push((code.near_branch(Condition::Equal)?, *target));
            }
            ProgramOp::Jump { target } => branches.push((code.jump()?, *target)),
            ProgramOp::RepeatInit {
                count_slot,
                index_slot,
                ..
            } if code.metered => {
                code.guard_repeat_count(step.span)?;
                code.store_rax(slots.local(*count_slot)?)?;
                code.integer_rax(0)?;
                code.store_rax(slots.local(*index_slot)?)?;
            }
            ProgramOp::RepeatGuard {
                count_slot,
                index_slot,
                target,
            } if code.metered => {
                code.load_rax(slots.local(*index_slot)?)?;
                code.load_rcx(slots.local(*count_slot)?)?;
                code.push(&[0x48, 0x39, 0xc8])?; // cmp rax, rcx
                branches.push((code.near_branch(Condition::GreaterEqual)?, *target));
            }
            ProgramOp::RepeatNext { index_slot } if code.metered => {
                let index_slot = slots.local(*index_slot)?;
                code.load_rax(index_slot)?;
                code.push(&[0x48, 0x83, 0xc0, 0x01])?; // add rax, 1
                code.guard_overflow(step.span)?;
                code.store_rax(index_slot)?;
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
    Ok(())
}

fn function_text(
    program: &ScalarProgramIr,
    budget: Option<u32>,
) -> Result<(Code, FreestandingCallLayout), Diagnostic> {
    program.verify()?;
    let layout = plan_function(program, None)?;
    let mut code = Code::new(layout.estimated_steps, true)?;
    code.metered = budget.is_some();
    append_function(&mut code, program, &layout, budget)?;
    Ok((
        code,
        FreestandingCallLayout {
            frame_bytes: layout.frame_bytes,
            integer_arguments: program.parameters(),
        },
    ))
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
    table: FileRegion,
    index: usize,
    name: u32,
    kind: u32,
    flags: u64,
    region: Option<FileRegion>,
    link: u32,
    info: u32,
    entry_size: u64,
) -> Result<(), Diagnostic> {
    if index >= SECTION_COUNT
        || table.size() != SECTION_COUNT * SECTION_HEADER_BYTES
        || table.end()? > output.len()
    {
        return Err(failure());
    }
    let (offset, size, alignment) = region.map_or((0, 0, 1), |region| {
        (region.offset(), region.size(), region.alignment())
    });
    let mut header = [0u8; SECTION_HEADER_BYTES];
    u32at(&mut header, 0, name)?;
    u32at(&mut header, 4, kind)?;
    u64at(&mut header, 8, flags)?;
    u64at(&mut header, 24, offset as u64)?;
    u64at(&mut header, 32, size as u64)?;
    u32at(&mut header, 40, link)?;
    u32at(&mut header, 44, info)?;
    u64at(&mut header, 48, alignment as u64)?;
    u64at(&mut header, 56, entry_size)?;
    put(
        output,
        table.offset() + index * SECTION_HEADER_BYTES,
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

pub(super) fn emit_function_with_layout(
    program: &ScalarProgramIr,
    budget: Option<u32>,
) -> Result<(CheckedElf64Kernel, FreestandingCallLayout), Diagnostic> {
    let (code, layout) = function_text(program, budget)?;
    let strings: &[u8] = if budget.is_some() {
        b"\0scarp_bounded_integer_function\0"
    } else {
        b"\0scarp_checked_integer_function\0"
    };
    let object = object(&code.data, strings)?;
    Ok((
        CheckedElf64Kernel {
            object,
            failure_spans: code.failure_spans,
        },
        layout,
    ))
}

pub(super) fn emit_bounded_entry(
    graph: &ScalarCallGraph,
    max_work: u32,
    max_stack_bytes: u32,
) -> Result<FreestandingEntry, Diagnostic> {
    let bodies = graph.bodies();
    let entry = graph.entry();
    let arities: Vec<_> = bodies.iter().map(ScalarProgramIr::parameters).collect();
    let layouts = bodies
        .iter()
        .map(|body| plan_function(body, Some(&arities)))
        .collect::<Result<Vec<_>, _>>()?;
    let frame_sizes = layouts
        .iter()
        .map(|layout| layout.frame_bytes)
        .collect::<Vec<_>>();
    let peak_stack_bytes = graph.verify_stack(&frame_sizes, max_stack_bytes)?;
    let entry_layout = FreestandingCallLayout {
        frame_bytes: frame_sizes[entry],
        integer_arguments: bodies[entry].parameters(),
    };
    let order = graph.emission_order();
    let steps = order.iter().try_fold(0usize, |sum, &id| {
        sum.checked_add(layouts[id].estimated_steps)
            .ok_or_else(failure)
    })?;
    let mut code = Code::new(steps, true)?;
    code.metered = true;
    let mut targets = vec![None; bodies.len()];
    let mut entry_bytes = 0;
    for id in order {
        let is_entry = id == entry;
        // Calls to the named entry from another source function must not reset
        // the shared meter. Its internal target starts after the external meter
        // setup, at the body's own prologue.
        if is_entry {
            code.start_meter(max_work)?;
        }
        targets[id] = Some(code.data.len());
        append_function(&mut code, &bodies[id], &layouts[id], None)?;
        if is_entry {
            entry_bytes = code.data.len();
        }
    }
    for (displacement, callee) in code.call_fixups {
        let target = targets.get(callee).copied().flatten().ok_or_else(failure)?;
        patch_relative_branch(&mut code.data, displacement, target)?;
    }
    let object = object_with_entry(&code.data, b"\0scarp_bounded_integer_entry\0", entry_bytes)?;
    Ok(FreestandingEntry {
        kernel: CheckedElf64Kernel {
            object,
            failure_spans: code.failure_spans,
        },
        entry_layout,
        max_active_frames: graph.entry_depth(),
        peak_stack_bytes,
    })
}

fn object(machine: &[u8], strings: &[u8]) -> Result<Vec<u8>, Diagnostic> {
    object_with_entry(machine, strings, machine.len())
}

fn object_with_entry(
    machine: &[u8],
    strings: &[u8],
    entry_bytes: usize,
) -> Result<Vec<u8>, Diagnostic> {
    if !(1..=machine.len()).contains(&entry_bytes) {
        return Err(failure());
    }
    // Offsets are into shstrtab, starting with the required NUL name.
    const SHSTR: &[u8] = b"\0.text\0.symtab\0.strtab\0.shstrtab\0.note.GNU-stack\0";
    let [0, symbol_name @ .., 0] = strings else {
        return Err(failure());
    };
    if !symbol_name
        .iter()
        .all(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
    {
        return Err(failure());
    }
    let layout = ObjectLayout::plan(machine.len(), strings.len(), SHSTR.len())?;
    let total = layout.section_headers.end()?;

    let mut object = Vec::new();
    object.try_reserve_exact(total).map_err(|_| failure())?;
    object.resize(total, 0);
    put(&mut object, 0, b"\x7fELF")?;
    put(&mut object, 4, &[2, 1, 1])?; // ELFCLASS64, ELFDATA2LSB, EV_CURRENT
    u16at(&mut object, 16, 1)?; // ET_REL
    u16at(&mut object, 18, 62)?; // EM_X86_64
    u32at(&mut object, 20, 1)?;
    u64at(&mut object, 40, layout.section_headers.offset() as u64)?;
    u16at(&mut object, 52, ELF_HEADER_BYTES as u16)?;
    u16at(&mut object, 58, SECTION_HEADER_BYTES as u16)?;
    u16at(&mut object, 60, SECTION_COUNT as u16)?;
    u16at(&mut object, 62, 4)?; // .shstrtab index

    put(&mut object, layout.text.offset(), machine)?;
    let mut symbols = [0u8; 2 * SYMBOL_BYTES]; // null symbol and one global function
    let global = &mut symbols[SYMBOL_BYTES..];
    u32at(global, 0, 1)?; // name offset in .strtab
    global[4] = 0x12; // STB_GLOBAL | STT_FUNC
    u16at(global, 6, 1)?; // .text section
    u64at(global, 16, entry_bytes as u64)?;
    put(&mut object, layout.symbols.offset(), &symbols)?;
    put(&mut object, layout.strings.offset(), strings)?;
    put(&mut object, layout.section_names.offset(), SHSTR)?;

    // Null, executable .text, symtab, strtab, shstrtab, GNU noexec-stack.
    section(
        &mut object,
        layout.section_headers,
        1,
        1,
        1,
        6,
        Some(layout.text),
        0,
        0,
        0,
    )?;
    section(
        &mut object,
        layout.section_headers,
        2,
        7,
        2,
        0,
        Some(layout.symbols),
        3,
        1,
        SYMBOL_BYTES as u64,
    )?;
    section(
        &mut object,
        layout.section_headers,
        3,
        15,
        3,
        0,
        Some(layout.strings),
        0,
        0,
        0,
    )?;
    section(
        &mut object,
        layout.section_headers,
        4,
        23,
        3,
        0,
        Some(layout.section_names),
        0,
        0,
        0,
    )?;
    section(
        &mut object,
        layout.section_headers,
        5,
        33,
        1,
        0,
        None,
        0,
        0,
        0,
    )?;
    Ok(object)
}

#[cfg(test)]
mod encoding_tests {
    use super::*;

    #[test]
    fn compact_prologue_probes_all_full_pages_and_restores_the_entire_frame() {
        let mut code = Code::new(0, true).unwrap();
        let size = 2 * STACK_PROBE_INTERVAL + 24;
        code.allocate_stack(size).unwrap();
        code.restore_stack_and_return(size).unwrap();
        let probe = [0x48, 0x81, 0xec, 0, 0x10, 0, 0, 0xc6, 0x04, 0x24, 0];
        assert_eq!(&code.data[..11], &probe);
        assert_eq!(&code.data[11..22], &probe);
        assert_eq!(&code.data[22..29], &[0x48, 0x81, 0xec, 24, 0, 0, 0]);
        assert_eq!(&code.data[29..32], &[0x48, 0x81, 0xc4]);
        assert_eq!(&code.data[32..36], &size.to_le_bytes());
        assert_eq!(&code.data[36..], &[0xc3]);
    }

    #[test]
    fn object_rejects_missing_text_and_invalid_symbol_tables() {
        assert!(object(&[], b"\0f\0").is_err());
        for strings in [
            &b""[..],
            b"\0\0",
            b"f\0",
            b"\0f",
            b"\0f\0g\0",
            b"\0f.\0",
            b"\0\xff\0",
        ] {
            assert!(object(&[0xc3], strings).is_err());
        }
        assert!(object(&[0xc3], b"\0scarp_test\0").is_ok());
    }

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
