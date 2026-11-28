# Experimental x86-64 UEFI applications

The native compiler can compile a single Scarp source file into a PE32+
application for x86-64 UEFI. Firmware loads the generated machine code and
provides the system table and initial stack. The image links only generated
code and read-only data, with zero import-directory entries.

```sh
cargo run --locked --bin scarp -- native-exe uefi-x86_64 boot 1000 1024 \
  experiments/bare-metal/uefi.srp -o BOOTX64.EFI
```

Select one parameterless `funcra` returning `integer` by name. The example's
entry is `boot`. Copy the image to `EFI/BOOT/BOOTX64.EFI` on an EFI system
partition that the emulator can read.

## Source and runtime contract

- The supported subset includes checked signed 64-bit integer and Boolean
  operations, bounded loops, inline local arrays, tuples and records, and
  nonrecursive direct calls with integer arguments and results. The target
  rejects host-runtime values such as BigInts, dynamic strings, and tensors.
- `x86_pause()` emits the unprivileged x86 `PAUSE` instruction. It is a
  statement in an impure function and spends one native work event. It
  carries no memory-ordering or timing guarantee.
- `print("literal")` emits one firmware console line through
  `EFI_SIMPLE_TEXT_OUTPUT_PROTOCOL.OutputString`. The literal may contain
  up to 1024 UCS-2 characters. Embedded NUL, non-BMP characters, and control
  characters other than CR, LF, tab and backspace are rejected. LF becomes
  CRLF, and the compiler appends CRLF plus a terminator. Installed console
  drivers determine glyph coverage. A missing console or nonzero
  `OutputString` status aborts the application. Output is immediate.
- A zero result returns `EFI_SUCCESS`. A nonzero result or a checked
  native failure returns `EFI_ABORTED`. All Scarp frames share the work
  allowance. The work counter meters Scarp events and does not measure
  firmware service time.
- The stack budget is measured below the incoming UEFI stack pointer. It
  covers generated frames, the entry bridge and outgoing firmware call
  overhead, including shadow space. Firmware's own frames and asynchronous
  interrupts consume additional stack. The budget is an upper bound on the
  generated portion of an application invocation, not a firmware stack
  configuration.

The entry bridge preserves the UEFI nonvolatile registers used by generated
Scarp code. Internal calls use the bounded SysV AMD64 integer convention.
The firmware console call uses the x64 firmware ABI. Its 16-bit literals
live in read-only `.rdata` and are referenced with RIP-relative addressing.
The image includes an absolute-padding relocation block so the loader can
move it. The output writer publishes only to a new path.

This application target runs during UEFI boot services and returns to
firmware. The [separate UEFI kernel target](native-uefi-kernel.md) defines
its own post-firmware stack, memory capabilities and terminal exception ABI.

## Emulator verification

Install QEMU system x86, OVMF, GNU binutils and Python 3 on a Linux development
host. Run the smoke suite from the repository root.

```sh
python3 experiments/bare-metal/uefi-smoke.py
```

Use `SCARP_OVMF_CODE` and `SCARP_OVMF_VARS` to specify a matching firmware
pair on distributions that install it outside `/usr/share/OVMF`. The test
uses fresh virtual disks and firmware variables. It checks the diagnostic
lines, the `PAUSE` encoding, successful return to OVMF, and aborts on
arithmetic failure, exhausted work and nonzero results.

References [UEFI console specification](https://uefi.org/specs/UEFI/2.11/12_Protocols_Console_Support.html) · [x64 calling convention](https://learn.microsoft.com/en-us/cpp/build/x64-calling-convention) · [PE format](https://learn.microsoft.com/en-us/windows/win32/debug/pe-format)
