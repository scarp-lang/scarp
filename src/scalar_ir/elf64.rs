//! Self-contained x86-64 SysV integer-kernel code and ELF64 ET_REL encoding.
//! Production emission uses only safe Rust, no external compiler or linker.

use super::{invalid, Operation, ScalarExpressionIr, Unary};
use crate::{
    ast::BinaryOperator,
    diagnostics::{Diagnostic, Span},
    value::ValueType,
};

const MAX_TEXT_BYTES: usize = 200_000;
const MAX_OBJECT_BYTES: usize = 262_144;

fn failure() -> Diagnostic {
    invalid(Span::default())
}

struct Code {
    data: Vec<u8>,
}

impl Code {
    fn new(steps: usize) -> Result<Self, Diagnostic> {
        let maximum = steps
            .checked_mul(24)
            .and_then(|n| n.checked_add(64))
            .filter(|n| *n <= MAX_TEXT_BYTES)
            .ok_or_else(failure)?;
        let mut data = Vec::new();
        data.try_reserve_exact(maximum).map_err(|_| failure())?;
        Ok(Self { data })
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
        // The initial reservation covers every possible supported instruction.
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
}

fn text(ir: &ScalarExpressionIr) -> Result<Vec<u8>, Diagnostic> {
    ir.verify()?;
    if ir.registers.get(ir.root) != Some(&ValueType::Integer) {
        return Err(failure());
    }
    let stack_size = ir
        .registers
        .len()
        .checked_mul(8)
        .and_then(|x| u32::try_from(x).ok())
        .ok_or_else(failure)?;
    let mut code = Code::new(ir.steps.len())?;
    // sub rsp, imm32; no calls are made and all temporaries live on our stack.
    code.push(&[0x48, 0x81, 0xec])?;
    code.push(&stack_size.to_le_bytes())?;
    for step in &ir.steps {
        match &step.kind {
            Operation::Work => {}
            Operation::Integer { dst, value } => {
                code.push(&[0x48, 0xb8])?; // movabs rax, imm64
                code.push(&value.to_le_bytes())?;
                code.store_rax(*dst)?;
            }
            Operation::Move { dst, src } => {
                code.load_rax(*src)?;
                code.store_rax(*dst)?;
            }
            Operation::Unary {
                dst,
                src,
                op: Unary::BitNot,
            } => {
                code.load_rax(*src)?;
                code.push(&[0x48, 0xf7, 0xd0])?; // not rax
                code.store_rax(*dst)?;
            }
            Operation::Binary {
                dst,
                left,
                right,
                op,
            } => {
                code.load_rax(*left)?;
                code.load_rcx(*right)?;
                let instruction = match op {
                    BinaryOperator::BitAnd => [0x48, 0x21, 0xc8],
                    BinaryOperator::BitOr => [0x48, 0x09, 0xc8],
                    BinaryOperator::BitXor => [0x48, 0x31, 0xc8],
                    _ => return Err(invalid(step.span)),
                };
                code.push(&instruction)?;
                code.store_rax(*dst)?;
            }
            _ => return Err(invalid(step.span)),
        }
    }
    code.load_rax(ir.root)?;
    code.push(&[0x48, 0x81, 0xc4])?; // add rsp, imm32
    code.push(&stack_size.to_le_bytes())?;
    code.push(&[0xc3])?; // ret
    Ok(code.data)
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

pub(super) fn emit(ir: &ScalarExpressionIr) -> Result<Vec<u8>, Diagnostic> {
    let machine = text(ir)?;
    // Offsets are into shstrtab, starting with the required NUL name.
    const SHSTR: &[u8] = b"\0.text\0.symtab\0.strtab\0.shstrtab\0.note.GNU-stack\0";
    const STR: &[u8] = b"\0scarp_scalar_kernel\0";
    let text_offset = aligned(64, 16)?;
    let sym_offset = aligned(
        text_offset.checked_add(machine.len()).ok_or_else(failure)?,
        8,
    )?;
    let str_offset = sym_offset.checked_add(48).ok_or_else(failure)?;
    let shstr_offset = str_offset.checked_add(STR.len()).ok_or_else(failure)?;
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

    put(&mut object, text_offset, &machine)?;
    let mut symbols = [0u8; 48]; // null symbol and one global function
    u32at(&mut symbols, 24, 1)?; // name offset in .strtab
    symbols[28] = 0x12; // STB_GLOBAL | STT_FUNC
    u16at(&mut symbols, 30, 1)?; // .text section
    u64at(&mut symbols, 40, machine.len() as u64)?;
    put(&mut object, sym_offset, &symbols)?;
    put(&mut object, str_offset, STR)?;
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
        STR.len(),
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
