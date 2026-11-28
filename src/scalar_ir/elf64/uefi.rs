//! x86-64 UEFI ABI bridge and PE32+ image for verified native text.

use super::{
    failure, layout::align_up, patch_forward_branch, put, u16at, u32at, u64at, Code, Condition,
    Diagnostic, Span, MAX_TEXT_BYTES,
};

const FILE_ALIGNMENT: usize = 512;
const SECTION_ALIGNMENT: usize = 4096;
const PE_OFFSET: usize = 0x80;
const OPTIONAL_HEADER: usize = PE_OFFSET + 4 + 20;
const OPTIONAL_SIZE: usize = 240;
const SECTION_TABLE: usize = OPTIONAL_HEADER + OPTIONAL_SIZE;
const SECTION_HEADER_BYTES: usize = 40;
const IMAGE_BASE: u64 = 0x0040_0000;
const TEXT_RVA: usize = SECTION_ALIGNMENT;

pub(super) const ENTRY_FRAME_BYTES: u32 = 3 * 8;
// Shadow space, saved R10, alignment padding, and the firmware return address.
const CONSOLE_FRAME_BYTES: u8 = 48;
pub(super) const CONSOLE_CALL_BYTES: u32 = CONSOLE_FRAME_BYTES as u32 + 8;
const CONSOLE_FAILURE: u32 = 8;
const EFI_ABORTED: u64 = (1 << 63) | 21;

#[derive(Default)]
pub(super) struct ConsoleLiterals {
    bytes: Vec<u8>,
    fixups: Vec<(usize, usize)>,
}

impl ConsoleLiterals {
    fn append(&mut self, text: &[u8], displacement: usize) -> Result<(), Diagnostic> {
        let text = std::str::from_utf8(text).map_err(|_| failure())?;
        // UTF-8 byte length bounds the number of UCS-2 code units. Account
        // for possible CR insertion, the final CRLF, and the terminator.
        let reserve = text.len() * 4 + 6;
        if reserve > MAX_TEXT_BYTES.saturating_sub(self.bytes.len()) {
            return Err(failure());
        }
        self.bytes.try_reserve(reserve).map_err(|_| failure())?;
        self.fixups.try_reserve(1).map_err(|_| failure())?;
        self.fixups.push((displacement, self.bytes.len()));
        let mut previous = 0;
        for unit in text.encode_utf16() {
            if unit == 10 && previous != 13 {
                self.bytes.extend_from_slice(&13u16.to_le_bytes());
            }
            self.bytes.extend_from_slice(&unit.to_le_bytes());
            previous = unit;
        }
        self.bytes.extend_from_slice(&[13, 0, 10, 0, 0, 0]);
        Ok(())
    }
}

impl Code {
    pub(super) fn firmware_print(&mut self, text: &[u8], span: Span) -> Result<(), Diagnostic> {
        let span_id = self.record_failure_span(span)?;
        self.push(&[0x4d, 0x85, 0xff])?; // test r15, r15 (SystemTable->ConOut)
        self.branch_to_failure(Condition::Equal, CONSOLE_FAILURE, span_id)?;
        self.move_register(1, 15)?; // mov rcx, r15 (This)
        self.push(&[0x48, 0x8d, 0x15])?; // lea rdx, [rip + literal]
        let displacement = self.data.len();
        self.push(&[0; 4])?;
        self.console
            .as_mut()
            .ok_or_else(failure)?
            .append(text, displacement)?;

        // The internal frame is already 16-byte aligned. The firmware may
        // clobber volatile R10, so save the shared work meter above shadow space.
        self.push(&[0x48, 0x83, 0xec, CONSOLE_FRAME_BYTES])?; // sub rsp, 48
        self.push(&[0x4c, 0x89, 0x54, 0x24, 0x20])?; // mov [rsp + 32], r10
        self.push(&[0x41, 0xff, 0x57, 0x08])?; // call [r15 + 8] (OutputString)
        self.push(&[0x4c, 0x8b, 0x54, 0x24, 0x20])?; // mov r10, [rsp + 32]
        self.push(&[0x48, 0x83, 0xc4, CONSOLE_FRAME_BYTES])?; // add rsp, 48
        self.push(&[0x48, 0x85, 0xc0])?; // test rax, rax (EFI_STATUS)
        self.branch_to_failure(Condition::NotEqual, CONSOLE_FAILURE, span_id)
    }
}

fn startup() -> Result<Vec<u8>, Diagnostic> {
    let mut code = Code::new(0, true)?;
    // EFIAPI passes ImageHandle in RCX and SystemTable in RDX. SysV
    // code may clobber RDI/RSI. Preserve those and the new ConOut register.
    // Three pushes align RSP to 16 before the internal SysV call.
    code.push(&[0x57, 0x56, 0x41, 0x57])?; // push rdi; push rsi; push r15
    code.push(&[0x48, 0x85, 0xd2])?; // test rdx, rdx
    let no_table = code.near_branch(Condition::Equal)?;
    code.push(&[0x4c, 0x8b, 0x7a, 0x40])?; // mov r15, [rdx + 64] (ConOut)
    code.push(&[0xe8])?; // call the native entry immediately after the bridge
    let call_entry = code.data.len();
    code.push(&[0; 4])?;
    code.push(&[0x85, 0xd2])?; // test edx, edx (native checked status)
    let checked_failure = code.near_branch(Condition::NotEqual)?;
    code.push(&[0x48, 0x85, 0xc0])?; // test rax, rax (Scarp result)
    let success = code.near_branch(Condition::Equal)?;
    let return_error = code.data.len();
    code.integer_rax(EFI_ABORTED as i64)?;
    let restore = code.data.len();
    code.push(&[0x41, 0x5f, 0x5e, 0x5f, 0xc3])?; // pop r15; pop rsi; pop rdi; ret
    let entry = code.data.len();
    patch_forward_branch(&mut code.data, no_table, return_error)?;
    patch_forward_branch(&mut code.data, checked_failure, return_error)?;
    patch_forward_branch(&mut code.data, success, restore)?;
    patch_forward_branch(&mut code.data, call_entry, entry)?;
    Ok(code.data)
}

struct Section {
    name: &'static [u8],
    rva: usize,
    offset: usize,
    size: usize,
    raw_size: usize,
    flags: u32,
}

impl Section {
    fn new(
        name: &'static [u8],
        rva: usize,
        offset: usize,
        size: usize,
        flags: u32,
    ) -> Result<Self, Diagnostic> {
        Ok(Self {
            name,
            rva,
            offset,
            size,
            raw_size: align_up(size, FILE_ALIGNMENT)?,
            flags,
        })
    }

    fn next_rva(&self) -> Result<usize, Diagnostic> {
        align_up(self.rva + self.size, SECTION_ALIGNMENT)
    }

    fn write(&self, image: &mut [u8], index: usize, bytes: &[u8]) -> Result<(), Diagnostic> {
        let header = SECTION_TABLE + index * SECTION_HEADER_BYTES;
        put(image, header, self.name)?;
        u32at(image, header + 8, self.size as u32)?;
        u32at(image, header + 12, self.rva as u32)?;
        u32at(image, header + 16, self.raw_size as u32)?;
        u32at(image, header + 20, self.offset as u32)?;
        u32at(image, header + 36, self.flags)?;
        put(image, self.offset, bytes)
    }
}

pub(super) fn image(mut code: Code) -> Result<Vec<u8>, Diagnostic> {
    let console = code.console.take().ok_or_else(failure)?;
    let mut machine = startup()?;
    let bridge_size = machine.len();
    machine
        .try_reserve_exact(code.data.len())
        .map_err(|_| failure())?;
    machine.extend_from_slice(&code.data);

    let section_count = if console.bytes.is_empty() { 2 } else { 3 };
    let headers = align_up(
        SECTION_TABLE + section_count * SECTION_HEADER_BYTES,
        FILE_ALIGNMENT,
    )?;
    let text = Section::new(b".text", TEXT_RVA, headers, machine.len(), 0x6000_0020)?;
    let data = Section::new(
        b".rdata",
        text.next_rva()?,
        text.offset + text.raw_size,
        console.bytes.len(),
        0x4000_0040,
    )?;
    for (displacement, offset) in console.fixups {
        let displacement = bridge_size + displacement;
        // Both the use and the literal move with the firmware's image base.
        let rip = text.rva + displacement + 4;
        let relative =
            i32::try_from((data.rva + offset) as i64 - rip as i64).map_err(|_| failure())?;
        put(&mut machine, displacement, &relative.to_le_bytes())?;
    }

    // All generated text is position-independent. A base-relocation block
    // with only ABSOLUTE padding entries lets firmware load it at any base.
    let mut reloc = [0; 12];
    u32at(&mut reloc, 0, text.rva as u32)?;
    u32at(&mut reloc, 4, 12)?;
    let relocation = Section::new(
        b".reloc",
        data.next_rva()?,
        data.offset + data.raw_size,
        reloc.len(),
        0x4200_0040,
    )?;
    let size = relocation.offset + relocation.raw_size;
    let mut result = Vec::new();
    result.try_reserve_exact(size).map_err(|_| failure())?;
    result.resize(size, 0);

    put(&mut result, 0, b"MZ")?;
    u32at(&mut result, 0x3c, PE_OFFSET as u32)?;
    put(&mut result, PE_OFFSET, b"PE\0\0")?;
    u16at(&mut result, PE_OFFSET + 4, 0x8664)?; // AMD64
    u16at(&mut result, PE_OFFSET + 6, section_count as u16)?;
    u16at(&mut result, PE_OFFSET + 20, OPTIONAL_SIZE as u16)?;
    u16at(&mut result, PE_OFFSET + 22, 0x0022)?; // executable, large-address aware

    let optional = &mut result[OPTIONAL_HEADER..SECTION_TABLE];
    u16at(optional, 0, 0x20b)?; // PE32+
    u32at(optional, 4, text.raw_size as u32)?;
    u32at(optional, 8, (data.raw_size + relocation.raw_size) as u32)?;
    u32at(optional, 16, text.rva as u32)?; // AddressOfEntryPoint
    u32at(optional, 20, text.rva as u32)?; // BaseOfCode
    u64at(optional, 24, IMAGE_BASE)?;
    u32at(optional, 32, SECTION_ALIGNMENT as u32)?;
    u32at(optional, 36, FILE_ALIGNMENT as u32)?;
    u32at(optional, 56, relocation.next_rva()? as u32)?;
    u32at(optional, 60, headers as u32)?;
    u16at(optional, 68, 10)?; // EFI application subsystem
    u16at(optional, 70, 0x0140)?; // dynamic base and NX compatible
    u32at(optional, 108, 16)?; // number of data directories
    u32at(optional, 152, relocation.rva as u32)?; // base relocation directory
    u32at(optional, 156, relocation.size as u32)?;

    text.write(&mut result, 0, &machine)?;
    if !console.bytes.is_empty() {
        data.write(&mut result, 1, &console.bytes)?;
    }
    relocation.write(&mut result, section_count - 1, &reloc)?;
    Ok(result)
}
