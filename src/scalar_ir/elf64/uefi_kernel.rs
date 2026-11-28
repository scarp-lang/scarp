//! Verified firmware-to-kernel transition and capability-scoped x86-64 operations.

use super::{
    failure, patch_forward_branch, patch_relative_branch, uefi, Code, Condition, Diagnostic, Span,
};
use crate::{
    native_object::{
        CHECKED_KERNEL_CPU_QUERY, CHECKED_KERNEL_DEBUG_BYTE, CHECKED_KERNEL_MEMORY_MAP_INDEX,
        CHECKED_KERNEL_REGION_OFFSET, CHECKED_KERNEL_UNHANDLED_TRAP,
    },
    native_platform::{MapField, NativeValueOp},
};

const EFI_INVALID_PARAMETER: u64 = (1 << 63) | 2;
const EXIT_ATTEMPTS: u8 = 3;
const HANDOFF_CALL_FRAME: u8 = 48;
pub(super) const LOADER_FRAME_BYTES: u32 = 7 * 8;
pub(super) const MIN_LOADER_STACK: u32 = LOADER_FRAME_BYTES + HANDOFF_CALL_FRAME as u32 + 8;

// R14 indexes the retained firmware map. R15 indexes a separate scratch page
// available to source atomics. Neither capability is exposed as an address.
const MAP_CAPACITY: u32 = 128 * 1024;
const MAP_BUFFER: u32 = 4096;
const IDT: u32 = MAP_BUFFER + MAP_CAPACITY;
const IDT_BYTES: u32 = 256 * 16;
const SCRATCH: u32 = IDT + IDT_BYTES;
const SCRATCH_BYTES: u32 = 4096;
const STACK_TOP: u32 = SCRATCH + SCRATCH_BYTES + 64 * 1024;
const MAP_KEY: u8 = 8;
const MAP_DESCRIPTOR_SIZE: u8 = 16;
const MAP_DESCRIPTOR_VERSION: u8 = 24;
const MAP_COUNT: u8 = 32;
const IDTR: u8 = 64;

impl Code {
    fn cpuid_preserving_rbx(&mut self) -> Result<(), Diagnostic> {
        self.move_register(9, 3)?;
        self.push(&[0x0f, 0xa2])?; // cpuid
        self.move_register(3, 9)
    }

    pub(super) fn kernel_value(&mut self, op: NativeValueOp, span: Span) -> Result<(), Diagnostic> {
        match op {
            NativeValueOp::Load64
            | NativeValueOp::Exchange64
            | NativeValueOp::CompareExchange64 => {
                let span_id = self.record_failure_span(span)?;
                self.push(&[0x48, 0x81, 0xf9])?; // cmp rcx, last valid byte offset
                self.push(&(SCRATCH_BYTES - 8).to_le_bytes())?;
                self.branch_to_failure(Condition::Above, CHECKED_KERNEL_REGION_OFFSET, span_id)?;
                self.push(&[0xf6, 0xc1, 7])?; // test cl, 7 (natural alignment)
                self.branch_to_failure(Condition::NotEqual, CHECKED_KERNEL_REGION_OFFSET, span_id)?;
                match op {
                    NativeValueOp::Load64 => self.push(&[0x49, 0x8b, 0x04, 0x0f]),
                    NativeValueOp::Exchange64 => self.push(&[0x49, 0x87, 0x04, 0x0f]),
                    // lock cmpxchg [r15 + rcx], r8, with expected and observed in RAX
                    _ => self.push(&[0xf0, 0x4d, 0x0f, 0xb1, 0x04, 0x0f]),
                }
            }
            NativeValueOp::MapCount => self.push(&[0x49, 0x8b, 0x46, MAP_COUNT]),
            NativeValueOp::Map(field) => {
                let span_id = self.record_failure_span(span)?;
                self.push(&[0x49, 0x3b, 0x4e, MAP_COUNT])?; // cmp rcx, [r14 + MAP_COUNT]
                self.branch_to_failure(
                    Condition::AboveEqual,
                    CHECKED_KERNEL_MEMORY_MAP_INDEX,
                    span_id,
                )?;
                // The bridge verified count * stride == size <= MAP_CAPACITY.
                self.push(&[0x49, 0x0f, 0xaf, 0x4e, MAP_DESCRIPTOR_SIZE])?;
                let (width, offset) = match field {
                    MapField::Type => (0x41, 0), // zero-extend the UINT32 type field
                    MapField::Physical => (0x49, 8),
                    MapField::Virtual => (0x49, 16),
                    MapField::Pages => (0x49, 24),
                    MapField::Attributes => (0x49, 32),
                };
                self.push(&[width, 0x8b, 0x84, 0x0e])?; // mov (r|e)ax, [r14 + rcx + disp32]
                self.push(&(MAP_BUFFER + offset).to_le_bytes())
            }
            NativeValueOp::DebugByte => {
                let span_id = self.record_failure_span(span)?;
                self.push(&[0x48, 0x81, 0xf9, 0xff, 0, 0, 0])?; // cmp rcx, 255
                self.branch_to_failure(Condition::Above, CHECKED_KERNEL_DEBUG_BYTE, span_id)?;
                self.push(&[0x89, 0xc8, 0xe6, 0xe9])?; // mov eax, ecx; out 0xe9, al
                self.integer_rax(1) // one byte accepted by the output instruction
            }
            NativeValueOp::CpuId => {
                let span_id = self.record_failure_span(span)?;
                self.move_register(2, 1)?;
                self.push(&[0x48, 0x09, 0xc2, 0x48, 0xc1, 0xea, 32])?; // or rdx, rax; shr rdx, 32
                self.branch_to_failure(Condition::NotEqual, CHECKED_KERNEL_CPU_QUERY, span_id)?;
                self.push(&[0x49, 0x83, 0xf8, 3])?; // cmp r8, 3
                self.branch_to_failure(Condition::Above, CHECKED_KERNEL_CPU_QUERY, span_id)?;
                self.push(&[0x91])?; // xchg eax, ecx (leaf, subleaf)
                self.move_register(9, 3)?;
                self.push(&[0x0f, 0xa2])?;
                for (selector, register) in [(1, 3), (2, 1), (3, 2)] {
                    self.push(&[0x41, 0x83, 0xf8, selector])?; // cmp r8d, selector
                    self.push(&[0x48, 0x0f, 0x44, 0xc0 | register])?; // cmove rax, register
                }
                self.move_register(3, 9)
            }
            NativeValueOp::Timestamp => {
                let span_id = self.record_failure_span(span)?;
                self.push(&[0x31, 0xc0, 0x31, 0xc9])?; // CPUID leaf 0, subleaf 0
                self.cpuid_preserving_rbx()?;
                self.push(&[0x85, 0xc0])?;
                self.branch_to_failure(Condition::Equal, CHECKED_KERNEL_CPU_QUERY, span_id)?;
                self.integer_rax(1)?;
                self.push(&[0x31, 0xc9])?;
                self.cpuid_preserving_rbx()?;
                self.push(&[0xf7, 0xc2, 16, 0, 0, 0])?; // test edx, TSC feature bit
                self.branch_to_failure(Condition::Equal, CHECKED_KERNEL_CPU_QUERY, span_id)?;
                self.push(&[0x0f, 0xae, 0xe8, 0x0f, 0x31])?; // lfence; rdtsc
                self.push(&[0x48, 0xc1, 0xe2, 32, 0x48, 0x09, 0xd0]) // combine EDX:EAX
            }
            NativeValueOp::ReadCr3 => self.push(&[0x0f, 0x20, 0xd8]),
        }
    }
}

struct Bridge {
    code: Vec<u8>,
    loader_call: usize,
    kernel_call: usize,
    storage: usize,
}

fn call_forward(code: &mut Code) -> Result<usize, Diagnostic> {
    code.push(&[0xe8])?;
    let displacement = code.data.len();
    code.push(&[0; 4])?;
    Ok(displacement)
}

fn install_exception_table(code: &mut Code) -> Result<usize, Diagnostic> {
    code.push(&[0x49, 0x8d, 0xbe])?; // lea rdi, [r14 + IDT]
    code.push(&IDT.to_le_bytes())?;
    code.push(&[0x49, 0x89, 0x7e, IDTR + 2])?; // packed IDTR base
    code.push(&[0x66, 0x41, 0xc7, 0x46, IDTR, 0xff, 0x0f])?; // IDTR limit
    code.push(&[0x48, 0x8d, 0x15])?; // lea rdx, [rip + unhandled trap]
    let trap_address = code.data.len();
    code.push(&[0; 4])?;
    code.push(&[0x66, 0x8c, 0xc8])?; // mov ax, cs
    code.move_register(9, 2)?;
    code.push(&[0x49, 0xc1, 0xe9, 16])?; // shr r9, 16 (middle entry bits)
    code.move_register(8, 9)?;
    code.push(&[0x49, 0xc1, 0xe8, 16])?; // shr r8, 16 (high entry bits)
    code.push(&[0xb9, 0x00, 0x01, 0, 0])?; // mov ecx, 256
    let gate = code.data.len();
    code.push(&[0x66, 0x89, 0x17])?; // low address
    code.push(&[0x66, 0x89, 0x47, 2])?; // current kernel code selector
    code.push(&[0x66, 0xc7, 0x47, 4, 0, 0x8e])?; // IST 0, DPL 0, interrupt gate
    code.push(&[0x66, 0x44, 0x89, 0x4f, 6])?;
    code.push(&[0x44, 0x89, 0x47, 8])?;
    code.push(&[0xc7, 0x47, 12, 0, 0, 0, 0])?;
    code.push(&[0x48, 0x83, 0xc7, 16, 0xff, 0xc9])?; // advance one gate; dec ecx
    let more = code.near_branch(Condition::NotEqual)?;
    patch_relative_branch(&mut code.data, more, gate)?;
    code.push(&[0x41, 0x0f, 0x01, 0x5e, IDTR])?; // lidt [r14 + IDTR]
    Ok(trap_address)
}

fn bridge() -> Result<Bridge, Diagnostic> {
    let mut code = Code::new(0, true)?;
    // Seven saves align the firmware's incoming stack. R12 keeps ImageHandle,
    // R13 keeps BootServices, R14 holds the map and RBX counts exit attempts.
    code.push(&[
        0x57, 0x56, 0x53, 0x41, 0x54, 0x41, 0x55, 0x41, 0x56, 0x41, 0x57,
    ])?;
    code.push(&[0x48, 0x85, 0xc9])?; // test rcx, rcx
    let no_handle = code.near_branch(Condition::Equal)?;
    code.push(&[0x48, 0x85, 0xd2])?; // test rdx, rdx
    let no_table = code.near_branch(Condition::Equal)?;
    code.move_register(12, 1)?;
    code.push(&[0x4c, 0x8b, 0x6a, 0x60])?; // mov r13, [rdx + 96] (BootServices)
    code.push(&[0x4c, 0x8b, 0x7a, 0x40])?; // mov r15, [rdx + 64] (ConOut)
    code.push(&[0x4d, 0x85, 0xed])?; // test r13, r13
    let no_services = code.near_branch(Condition::Equal)?;
    code.push(&[0x4c, 0x8d, 0x35])?; // lea r14, [rip + map storage]
    let storage = code.data.len();
    code.push(&[0; 4])?;
    let loader_call = call_forward(&mut code)?;
    code.push(&[0x85, 0xd2])?; // test edx, edx (checked status)
    let loader_failed = code.near_branch(Condition::NotEqual)?;
    code.push(&[0x48, 0x85, 0xc0])?; // test rax, rax (permission to hand off)
    let loader_declined = code.near_branch(Condition::NotEqual)?;

    code.push(&[0x48, 0x83, 0xec, HANDOFF_CALL_FRAME])?;
    code.push(&[0x31, 0xdb])?; // xor ebx, ebx
    let retry = code.data.len();
    code.push(&[0x49, 0xc7, 0x06])?; // mov qword [r14], MAP_CAPACITY
    code.push(&MAP_CAPACITY.to_le_bytes())?;
    code.push(&[0x49, 0x8d, 0x0e])?; // lea rcx, [r14] (MemoryMapSize)
    code.push(&[0x49, 0x8d, 0x96])?; // lea rdx, [r14 + MAP_BUFFER]
    code.push(&MAP_BUFFER.to_le_bytes())?;
    code.push(&[0x4d, 0x8d, 0x46, MAP_KEY])?;
    code.push(&[0x4d, 0x8d, 0x4e, MAP_DESCRIPTOR_SIZE])?;
    code.push(&[0x49, 0x8d, 0x46, MAP_DESCRIPTOR_VERSION])?;
    code.push(&[0x48, 0x89, 0x44, 0x24, 0x20])?; // fifth argument above shadow space
    code.push(&[0x41, 0xff, 0x55, 0x38])?; // call [r13 + 56] (GetMemoryMap)
    code.push(&[0x48, 0x85, 0xc0])?;
    let map_call_failed = code.near_branch(Condition::NotEqual)?;

    code.push(&[0x49, 0x8b, 0x06, 0x48, 0x3d])?; // mov rax, [r14]; cmp rax, cap
    code.push(&MAP_CAPACITY.to_le_bytes())?;
    let map_too_large = code.near_branch(Condition::Above)?;
    code.push(&[0x49, 0x8b, 0x4e, MAP_DESCRIPTOR_SIZE, 0x48, 0x83, 0xf9, 40])?;
    let stride_too_small = code.near_branch(Condition::Below)?;
    code.push(&[0x41, 0x83, 0x7e, MAP_DESCRIPTOR_VERSION, 1])?;
    let map_version = code.near_branch(Condition::NotEqual)?;
    code.push(&[0x31, 0xd2, 0x48, 0xf7, 0xf1])?; // xor edx, edx; div rcx
    code.push(&[0x48, 0x85, 0xd2])?;
    let partial_descriptor = code.near_branch(Condition::NotEqual)?;
    code.push(&[0x48, 0x85, 0xc0])?;
    let empty_map = code.near_branch(Condition::Equal)?;
    code.push(&[0x49, 0x89, 0x46, MAP_COUNT])?;

    code.push(&[0xff, 0xc3])?; // inc ebx
    code.move_register(1, 12)?;
    code.push(&[0x49, 0x8b, 0x56, MAP_KEY])?;
    code.push(&[0x41, 0xff, 0x95, 0xe8, 0, 0, 0])?; // call [r13 + 232] (ExitBootServices)
    code.push(&[0x48, 0x85, 0xc0])?;
    let exited = code.near_branch(Condition::Equal)?;
    code.push(&[0x48, 0xba])?; // movabs rdx, EFI_INVALID_PARAMETER
    code.push(&EFI_INVALID_PARAMETER.to_le_bytes())?;
    code.push(&[0x48, 0x39, 0xd0])?; // cmp rax, rdx
    let fatal_status = code.near_branch(Condition::NotEqual)?;
    code.push(&[0x83, 0xfb, EXIT_ATTEMPTS])?;
    let retry_stale_key = code.near_branch(Condition::Below)?;
    let attempts_exhausted = code.jump()?;

    let invalid_map = code.data.len();
    code.push(&[0x85, 0xdb])?; // after an exit attempt, the caller cannot use boot services
    let unsafe_return = code.near_branch(Condition::NotEqual)?;
    code.push(&[0x48, 0x83, 0xc4, HANDOFF_CALL_FRAME])?;
    let abort = code.data.len();
    code.integer_rax(uefi::EFI_ABORTED as i64)?;
    code.push(&[
        0x41, 0x5f, 0x41, 0x5e, 0x41, 0x5d, 0x41, 0x5c, 0x5b, 0x5e, 0x5f, 0xc3,
    ])?;

    let enter_kernel = code.data.len();
    code.push(&[0xfa, 0xfc])?; // cli; cld
    code.push(&[0x49, 0x8d, 0xa6])?; // lea rsp, [r14 + STACK_TOP]
    code.push(&STACK_TOP.to_le_bytes())?;
    code.push(&[0x4d, 0x8d, 0xbe])?; // lea r15, [r14 + SCRATCH]
    code.push(&SCRATCH.to_le_bytes())?;
    code.push(&[0x45, 0x31, 0xe4, 0x45, 0x31, 0xed])?; // clear firmware handle and dispatch table
    let trap_address = install_exception_table(&mut code)?;
    let kernel_call = call_forward(&mut code)?;
    let halt = code.data.len();
    code.push(&[0xfa, 0xf4, 0xeb, 0xfd])?; // cli; hlt; jmp back to hlt
    let trap = code.data.len();
    code.push(&[0x31, 0xc0, 0xba])?; // xor eax, eax; mov edx, native trap status
    code.push(&CHECKED_KERNEL_UNHANDLED_TRAP.to_le_bytes())?;
    let halt_trap = code.jump()?;

    for displacement in [
        no_handle,
        no_table,
        no_services,
        loader_failed,
        loader_declined,
    ] {
        patch_forward_branch(&mut code.data, displacement, abort)?;
    }
    for displacement in [
        map_call_failed,
        map_too_large,
        stride_too_small,
        map_version,
        partial_descriptor,
        empty_map,
    ] {
        patch_forward_branch(&mut code.data, displacement, invalid_map)?;
    }
    for displacement in [fatal_status, attempts_exhausted, unsafe_return] {
        patch_forward_branch(&mut code.data, displacement, halt)?;
    }
    patch_forward_branch(&mut code.data, exited, enter_kernel)?;
    patch_relative_branch(&mut code.data, retry_stale_key, retry)?;
    patch_forward_branch(&mut code.data, trap_address, trap)?;
    patch_relative_branch(&mut code.data, halt_trap, halt)?;
    Ok(Bridge {
        code: code.data,
        loader_call,
        kernel_call,
        storage,
    })
}

pub(super) fn image(mut loader: Code, kernel: Code) -> Result<(Vec<u8>, Vec<Span>), Diagnostic> {
    let Bridge {
        mut code,
        loader_call,
        kernel_call,
        storage,
    } = bridge()?;
    let loader_start = code.len();
    code.try_reserve_exact(loader.data.len() + kernel.data.len())
        .map_err(|_| failure())?;
    code.extend_from_slice(&loader.data);
    let kernel_start = code.len();
    code.extend_from_slice(&kernel.data);
    patch_forward_branch(&mut code, loader_call, loader_start)?;
    patch_forward_branch(&mut code, kernel_call, kernel_start)?;
    let console = loader.console.take().ok_or_else(failure)?;
    let image = uefi::write_image(
        code,
        console,
        loader_start,
        STACK_TOP as usize,
        &[(storage, 0)],
    )?;
    Ok((image, kernel.failure_spans))
}
