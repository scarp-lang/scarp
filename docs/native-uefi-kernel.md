# Experimental x86-64 UEFI kernel

`scarp native-exe uefi-kernel-x86_64` builds a relocatable PE32+ image from
two independent Scarp files. One supplies a UEFI loader entry, and the other
supplies a kernel entry that runs after `ExitBootServices`. Both entries take
zero parameters and return `integer`. The compiler emits the bridge, stack
switch, machine instructions and PE sections directly.

```sh
cargo run --locked --bin scarp -- native-exe uefi-kernel-x86_64 boot kernel 10000 1024 \
  experiments/bare-metal/uefi-loader.srp experiments/bare-metal/uefi-kernel.srp \
  -o BOOTX64.EFI
```

Place the image at `EFI/BOOT/BOOTX64.EFI` on an EFI system partition for the
emulator. The existing [UEFI application target](native-uefi.md) retains its
firmware-return ABI.

## Execution stages

1. The named Scarp loader runs using the firmware's stack and console. A
   zero return requests handoff. A nonzero result or checked failure returns
   `EFI_ABORTED`.
2. The generated bridge captures the firmware map into a 128 KiB buffer.
   It checks the descriptor version, returned byte count and stride before
   requesting `ExitBootServices`. An outdated map key can trigger up to two
   retries. Before the first exit attempt, map failure returns `EFI_ABORTED`.
   A terminal error after an exit attempt halts, since firmware may already
   have partially shut down its services.
3. Successful handoff masks external interrupts, clears the direction flag
   and discards the saved firmware handle and dispatch table. The bridge
   switches to the top of a dedicated 64 KiB stack and loads a private IDT.
   It enters the second Scarp file with a fresh work budget.
4. On kernel return, the bridge halts in ring zero. RAX retains the source
   result and low RDX contains the checked native status. For checked
   failures, high RDX indexes the original kernel source spans available
   through the Rust API. A processor trap halts with status 12 and carries
   no Scarp span. The default IDT handler is terminal for every vector.

The stack limit applies independently at each stage. The loader bound
includes generated UEFI bridge and outgoing argument space. The kernel bound
covers generated Scarp frames below the dedicated stack top. Firmware's own
frames and processor-delivered exception frames have separate usage.

The PE image reserves zero-filled, writable nonexecutable storage for the
memory-map metadata, descriptors, IDT, scratch page and kernel stack. Its
code and firmware literals occupy separate executable and read-only sections.
The kernel inherits the firmware page tables and current code selector at
handoff. Kernel-owned paging, the GDT and TSS, returning interrupt handlers,
and planning physical allocations remain future parts of this target.

## Kernel-only source capabilities

The following calls require an impure function in the kernel source file.
Memory offsets are byte offsets into the compiler-owned 4096-byte page.
The checked region accepts 8-byte-aligned offsets from 0 through 4088. The
compiler charges each operation against the native work allowance and
preserves the order of its memory effects.

| Operation | Contract |
| --- | --- |
| `kernel_load64(offset)` | Read one aligned 64-bit integer from coherent RAM with x86 acquire ordering. |
| `kernel_exchange64(offset, value)` | Atomically replace the aligned cell and return its previous bits using a locked exchange. |
| `kernel_compare_exchange64(offset, expected, replacement)` | Atomically replace the cell only when it matches. Return the previously observed bits on either outcome. The locked operation orders preceding and following accesses. |
| `kernel_fence()` | Emit x86 `MFENCE` for ordering accesses to coherent RAM. |
| `kernel_map_count()` | Return the count of validated firmware descriptors retained at handoff. |
| `kernel_map_type(i)`, `kernel_map_physical(i)`, `kernel_map_virtual(i)`, `kernel_map_pages(i)`, `kernel_map_attributes(i)` | Read one field from an indexed snapshot descriptor. Wide fields are returned as 64-bit patterns and indices are checked against the descriptor count. |
| `x86_cpuid(leaf, subleaf, selector)` | Query CPUID with two unsigned 32-bit operands. Select EAX, EBX, ECX or EDX using 0 through 3. Results are zero-extended to Scarp integers. |
| `x86_read_tsc()` | Check CPUID for TSC support, emit `LFENCE; RDTSC` and return the 64-bit counter pattern. This is a CPU counter without a clock-rate or cross-core synchronization contract. |
| `x86_read_cr3()` | Read the raw current CR3 bits. The result is an integer observation of the architecture state. |
| `x86_pause()` | Emit the unprivileged `PAUSE` instruction. This is also available in impure UEFI loader code. |
| `x86_breakpoint()` | Issue an INT3 fault under the terminal exception table. |
| `qemu_debug_byte(value)` | Accept an integer from 0 through 255 and issue one byte to the fixed QEMU debug port 0xE9. Return 1 to indicate that the instruction was issued. The call is specific to this emulator diagnostic interface. |

The map and scratch page have separate capabilities. Source programs can
inspect the snapshot and access the scratch page through these checks. The
same program has no direct way to overwrite the page table, IDT, or firmware
pointers, or access arbitrary ports or MMIO. The emulated kernel fixture is
single-core and masks external interrupts.

The kernel example builds an aligned bump allocator in Scarp over the
scratch page. A compare-and-exchange publishes each cursor change.
The example checks alignment, disjoint placement, capacity exhaustion,
and independent reads from two allocated ranges. Its reserved page is
retained for the life of this kernel invocation.

Native failure status 9 covers an invalid region offset, 10 covers a map
index, 11 covers a debug byte, and 13 covers a CPU query operand or an
unavailable CPU feature. Existing arithmetic and work status codes retain
their meanings.

## Verification

`python3 experiments/bare-metal/uefi-smoke.py` boots the applications and
kernel under OVMF. It checks the debug-port transcript, the guest's exact
halt instruction, CPL and interrupt flag, its new stack and IDT, and the
checked failure statuses. The Linux ABI test supplies a mock firmware table
at a relocated address to test malformed maps, mismatched keys, retry
exhaustion, and successful transition to the privilege boundary.

Firmware ABI references [UEFI boot services](https://uefi.org/specs/UEFI/2.11/07_Services_Boot_Services.html) · [PE format](https://learn.microsoft.com/en-us/windows/win32/debug/pe-format)
