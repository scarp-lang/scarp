# Experimental Scarp bare-metal smoke harness

This opt-in experiment exercises the compiler-owned ELF64 emitter without a
host OS or libc **inside the guest**. It reuses the real Scarp
[checked integer function](../../examples/checked_elf64_function.srp);
the existing Rust example emits its standalone relocatable machine code.
A second example emits a bounded integer loop from `bounded_sum.srp`.

## Run on a Linux development host

Install a supported Rust toolchain and Cargo, GNU x86-64 binutils, GRUB
utilities with the i386-pc BIOS modules, xorriso, mtools, QEMU system x86 and GNU
coreutils. Run from the repository root:

```sh
bash experiments/bare-metal/smoke.sh
```

The script uses a temporary directory, validates the Multiboot2 header,
creates a BIOS-only ISO using the GRUB i386-pc platform at
`/usr/lib/grub/i386-pc`, reports GRUB messages over the serial console, and boots only an emulated machine using TCG.
It needs no root privileges, does not write a physical disk, and removes the
temporary outputs on exit. A successful guest reports QEMU debug-exit
status 33 and the complete marker sequence, translated into host exit status zero. Guest failure, timeout,
tool failure or unexpected exit status is a failed smoke test.

## Boot-stage diagnostics

The BIOS GRUB configuration loads the image directly rather than waiting in
a menu. The Multiboot2 header explicitly declares the 32-bit entry address.
The QEMU debug console captures one character per completed stage:

| Marker | Stage |
| --- | --- |
| B | Multiboot2 magic checked |
| G | GDT loaded |
| P | PAE enabled and page table root loaded |
| E | Long mode enabled in EFER |
| V | Paging enabled |
| L | Entered 64-bit code |
| S | Stack and segment registers initialized |
| 1 | Positive multiplication checked |
| 2 | Negative-input return checked |
| 3 | Checked overflow verified |
| 4 | Bounded loop returns 55 for input 10 and restores the stack |
| 5 | Work allowance exhaustion returns status 4 and restores the stack |
| 6 | Subsequent invocation returns 6 with a fresh allowance and restored stack |
| X / F | Invalid boot magic / failed function check |

Success requires exactly `BGPEVLS123456` and exit status 33.
On failure the runner prints the last 80 lines of QEMU exception and reset
diagnostics. A CPU triple fault is not kept running by `-no-shutdown`.
No marker means the assembly entry was not reached or the boot magic failed
before the debug device could be used; GRUB output and the QEMU log provide
additional evidence.

## Guest contract

- GRUB enters the assembly shim in 32-bit protected mode.
- The shim installs a minimal GDT and three levels of paging, identity
  mapping the first GiB with 2 MiB pages, then enters 64-bit mode.
- The shim owns a 64 KiB stack, aligns it for SysV AMD64, and leaves interrupts
  disabled. The linker keeps executable and writable load segments separate;
  the temporary page tables do not yet enforce those permissions.
- The emitted Scarp function is linked directly, with no libc or interpreter.
  Two normal calls must return 112 and -1 respectively, with zero status and
  span fields. A multiplication overflow must return value zero and status 1.
- A second emitted function uses a 1000-event native IR work allowance. It
  checks finite loop execution, controlled exhaustion for a large input, and
  a successful fresh call after failure. No interpreter runs in the guest.
- The emulator-specific debug-exit device reports pass/fail; it is not a
  portable hardware interface.

## Scope and next steps

This is a bootstrap integration experiment, not a new Scarp language
feature, bootable OS distribution, stable kernel ABI, independent bootloader,
or proof of a fully native Scarp compiler. GRUB and an assembly shim are
temporary development dependencies; Rust runs only on the development host.
The emitted function retains the existing experimental checked integer ABI.

Before a general bare-metal target is declared supported, add verified
cross-target code generation, explicit fixed-width/address types, controlled
memory and volatile I/O operations, calling conventions, relocations and
linking, stack/frame bounds, interrupt-safe code generation, and freestanding
libraries. Maintain emulator CI and require a successful boot on every supported
target before claiming bare-metal execution is verified.

