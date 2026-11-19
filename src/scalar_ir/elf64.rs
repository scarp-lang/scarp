//! Self-contained x86-64 SysV integer-kernel code and ELF64 ET_REL encoding.
//! Production emission uses only safe Rust, no external compiler or linker.

use super::{invalid, program::ScalarProgramIr, Operation, Unary};
use crate::{
    ast::BinaryOperator,
    diagnostics::{Diagnostic, Span},
    native_object::{
        CheckedElf64Kernel, CHECKED_KERNEL_DIVISION_BY_ZERO, CHECKED_KERNEL_INTEGER_OVERFLOW,
        CHECKED_KERNEL_INVALID_SHIFT,
    },
};

const MAX_TEXT_BYTES: usize = 200_000;
const MAX_OBJECT_BYTES: usize = 262_144;

fn failure() -> Diagnostic {
    invalid(Span::default())
}

struct Code {
    data: Vec<u8>,
    branches_to_failure: Vec<usize>,
    failure_spans: Vec<Span>,
}

// Second opcode byte of a near conditional branch (0F 8x rel32).
#[repr(u8)]
enum Condition {
    Overflow = 0x80,
    Equal = 0x84,
    NotEqual = 0x85,
    Above = 0x87,
}

impl Code {
    fn new(steps: usize) -> Result<Self, Diagnostic> {
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

    fn slot(index: usize) -> Result<[u8; 4], Diagnostic> {
        index
            .checked_mul(8)
            .and_then(|x| u32::try_from(x).ok())
            .map(u32::to_le_bytes)
            .ok_or_else(failure)
    }

    fn load_rax(&mut self, src: usize) -> Result<(), Diagnostic> {
        self.push(&[0x48, 0x8b, 0x84, 0x24])?;
        self.push(&Self::slot(src)?)
    }

    fn load_rcx(&mut self, src: usize) -> Result<(), Diagnostic> {
        self.push(&[0x48, 0x8b, 0x8c, 0x24])?;
        self.push(&Self::slot(src)?)
    }

    fn store_rax(&mut self, dst: usize) -> Result<(), Diagnostic> {
        self.push(&[0x48, 0x89, 0x84, 0x24])?;
        self.push(&Self::slot(dst)?)
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
    let mut code = Code::new(total_steps)?;
    // sub rsp, imm32: no calls, all integer slots and temporaries are local.
    code.push(&[0x48, 0x81, 0xec])?;
    code.push(&stack_size.to_le_bytes())?;

    for (slot, expr) in program.integer_stores() {
        for step in &expr.steps {
            match &step.kind {
                Operation::Work => {}
                Operation::Integer { dst, value } => {
                    code.push(&[0x48, 0xb8])?; // movabs rax, imm64
                    code.push(&value.to_le_bytes())?;
                    code.store_rax(temporary(slot_count, *dst)?)?;
                }
                Operation::Load { dst, slot } => {
                    code.load_rax(*slot)?;
                    code.store_rax(temporary(slot_count, *dst)?)?;
                }
                Operation::Move { dst, src } => {
                    code.load_rax(temporary(slot_count, *src)?)?;
                    code.store_rax(temporary(slot_count, *dst)?)?;
                }
                Operation::Unary {
                    dst,
                    src,
                    op: Unary::BitNot,
                } => {
                    code.load_rax(temporary(slot_count, *src)?)?;
                    code.push(&[0x48, 0xf7, 0xd0])?; // not rax
                    code.store_rax(temporary(slot_count, *dst)?)?;
                }
                Operation::Unary {
                    dst,
                    src,
                    op: Unary::Negate,
                } if checked_arithmetic => {
                    code.load_rax(temporary(slot_count, *src)?)?;
                    code.push(&[0x48, 0xf7, 0xd8])?; // neg rax
                    code.guard_overflow(step.span)?;
                    code.store_rax(temporary(slot_count, *dst)?)?;
                }
                Operation::Binary {
                    dst,
                    left,
                    right,
                    op,
                } => {
                    code.load_rax(temporary(slot_count, *left)?)?;
                    code.load_rcx(temporary(slot_count, *right)?)?;
                    code.binary(*op, checked_arithmetic, step.span)?;
                    code.store_rax(temporary(slot_count, *dst)?)?;
                }
                _ => return Err(invalid(step.span)),
            }
        }
        code.load_rax(temporary(slot_count, expr.root)?)?;
        code.store_rax(slot)?;
    }

    if checked_arithmetic {
        code.finish_checked_return()?;
    }
    // Both return paths restore the same stack frame and leave callee-saved
    // registers untouched. On success RAX still holds the final store value.
    code.push(&[0x48, 0x81, 0xc4])?;
    code.push(&stack_size.to_le_bytes())?;
    code.push(&[0xc3])?;
    Ok(code)
}

fn aligned(value: usize, alignment: usize) -> Result<usize, Diagnostic> {
    value
        .checked_add(alignment - 1)
        .map(|x| x & !(alignment - 1))
        .ok_or_else(failure)
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

fn object(machine: &[u8], strings: &[u8]) -> Result<Vec<u8>, Diagnostic> {
    // Offsets are into shstrtab, starting with the required NUL name.
    const SHSTR: &[u8] = b"\0.text\0.symtab\0.strtab\0.shstrtab\0.note.GNU-stack\0";
    let text_offset = aligned(64, 16)?;
    let sym_offset = aligned(
        text_offset.checked_add(machine.len()).ok_or_else(failure)?,
        8,
    )?;
    let str_offset = sym_offset.checked_add(48).ok_or_else(failure)?;
    let shstr_offset = str_offset.checked_add(strings.len()).ok_or_else(failure)?;
    let header_offset = aligned(
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
