//! Linux ELF64 process envelope for already verified position-independent text.

use super::{failure, put, u16at, u32at, u64at, Diagnostic, MAX_TEXT_BYTES};

const IMAGE_ADDRESS: u64 = 0x0040_0000;
const PAGE_BYTES: usize = 4096;
const ELF_HEADER_BYTES: usize = 64;
const PROGRAM_HEADER_BYTES: usize = 56;
const PROGRAM_HEADERS: usize = 2;
const CODE_OFFSET: usize = ELF_HEADER_BYTES + PROGRAM_HEADERS * PROGRAM_HEADER_BYTES;

// Linux process entry aligns the incoming stack for a SysV call. On return,
// EDX contains the native status (the upper half of RDX is the source span).
// Successful values 0..125 map to themselves. Other values map to 126, and
// checked runtime failures map to 128 + status. No output or input syscalls.
const START: [u8; 43] = [
    0x48, 0x83, 0xe4, 0xf0, // and rsp, -16
    0xe8, 0x22, 0x00, 0x00, 0x00, // call native entry immediately after shim
    0x85, 0xd2, // test edx, edx
    0x75, 0x0f, // jne error
    0xbf, 0x7e, 0x00, 0x00, 0x00, // mov edi, 126
    0x48, 0x83, 0xf8, 0x7d, // cmp rax, 125
    0x77, 0x0a, // ja exit (also catches negative signed values)
    0x89, 0xc7, // mov edi, eax
    0xeb, 0x06, // jmp exit
    0x8d, 0xba, 0x80, 0x00, 0x00, 0x00, // error: lea edi, [rdx + 128]
    0xb8, 0x3c, 0x00, 0x00, 0x00, // exit: mov eax, 60
    0x0f, 0x05, // syscall
    0x0f, 0x0b, // ud2 if the exit syscall returns
];

pub(super) fn image(machine: &[u8]) -> Result<Vec<u8>, Diagnostic> {
    if !(1..=MAX_TEXT_BYTES).contains(&machine.len()) {
        return Err(failure());
    }
    let total = CODE_OFFSET + START.len() + machine.len();
    let mut image = Vec::new();
    image.try_reserve_exact(total).map_err(|_| failure())?;
    image.resize(total, 0);
    put(&mut image, 0, b"\x7fELF\x02\x01\x01")?;
    u16at(&mut image, 16, 2)?; // ET_EXEC
    u16at(&mut image, 18, 62)?; // EM_X86_64
    u32at(&mut image, 20, 1)?;
    u64at(&mut image, 24, IMAGE_ADDRESS + CODE_OFFSET as u64)?;
    u64at(&mut image, 32, ELF_HEADER_BYTES as u64)?;
    u16at(&mut image, 52, ELF_HEADER_BYTES as u16)?;
    u16at(&mut image, 54, PROGRAM_HEADER_BYTES as u16)?;
    u16at(&mut image, 56, PROGRAM_HEADERS as u16)?;

    let load = &mut image[ELF_HEADER_BYTES..ELF_HEADER_BYTES + PROGRAM_HEADER_BYTES];
    u32at(load, 0, 1)?; // PT_LOAD at file offset zero
    u32at(load, 4, 5)?; // PF_R | PF_X
    u64at(load, 16, IMAGE_ADDRESS)?;
    u64at(load, 24, IMAGE_ADDRESS)?;
    u64at(load, 32, total as u64)?;
    u64at(load, 40, total as u64)?;
    u64at(load, 48, PAGE_BYTES as u64)?;

    let stack = &mut image[ELF_HEADER_BYTES + PROGRAM_HEADER_BYTES..CODE_OFFSET];
    u32at(stack, 0, 0x6474_e551)?; // PT_GNU_STACK
    u32at(stack, 4, 6)?; // PF_R | PF_W

    put(&mut image, CODE_OFFSET, &START)?;
    put(&mut image, CODE_OFFSET + START.len(), machine)?;
    Ok(image)
}
