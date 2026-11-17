# Checked scalar ELF64 kernel design

Status: proposed for issue [#128](https://github.com/scarp-lang/scarp/issues/128).
The published alpha.12 compiler provides the existing experimental
[ELF64 integer kernel API](../native-elf64.md). The checked ABI below requires
a separate implementation and review.

## Current output

`emit_elf64_integer_kernel(&str)` lowers one source file through the normal
frontend, native preflight and private scalar IR. It emits little-endian
`ET_REL` bytes for `x86_64-unknown-linux-gnu`. The single exported SysV AMD64
symbol `scarp_scalar_kernel` takes no parameters and returns the last integer
binding in RAX. The emitter accepts immutable integer bindings, references
to earlier bindings and a bounded bitwise expression subset. All storage is
in the callee's stack frame. The object contains `.text`, `.symtab`,
`.strtab`, `.shstrtab` and an empty `.note.GNU-stack` section. It has no
relocations, callbacks, process entry point or filesystem output.

[Proposal 0037](../proposals/0037-compiler-owned-native-ir.md) describes the
typed IR. The expression/control-flow lowering, verifier, and differential
oracle landed through PRs #142–#145. The C11 emitter in
[Proposal 0035](../proposals/0035-native-scalar-c11.md) remains an oracle.
Generated native code can use the existing verifier without expanding the
source parser.

## First fallible kernel

Keep the old symbol and Rust API. A separate experimental checked emitter
would accept loop-free, output-free integer bindings with checked addition,
subtraction, multiplication, signed division/remainder and shifts.
Add Boolean comparisons and forward-only branches after arithmetic has an
independent oracle. Reject functions, recursion, aggregates, imports, host
calls and external symbols during preflight.

Export a distinct no-argument SysV AMD64 symbol, proposed as
`scarp_checked_scalar_kernel`. Return a C-layout 16-byte structure with
`int64_t value`, `uint32_t status` and `uint32_t span_id` in that order.
The two eightbytes use RAX and RDX. Status zero indicates success. A nonzero
status names an explicitly mapped arithmetic or budget failure and returns
a deterministic span-table index. Return zero for value on failure. Rust
emission returns the object and a bounded source-span table to the caller.
The table contains original file-local UTF-8 ranges and excludes paths and
build identities. The first object remains freestanding and relocation-free.

Use the interpreter's signed operation contracts. Signed overflow, division
by zero, minimum integer divided by −1, and invalid shift widths take explicit
error exits. A valid shift preserves the interpreter's existing bit behavior.
Every emitted load requires a verified initialized slot. Conditional
branches preserve lazy operand evaluation. Track executed work from existing
IR accounting events, including work preceding an arithmetic error. Set a
fixed frame limit from the verified slot and temporary counts. Prove the
single terminal return structure, stack balance and saved-register behavior
on both success and error paths before writing the object.

## Verification and scope

1. Add deterministic IR snapshots and malformed internal-plan checks for
   undefined values, bad joins, wrong types, out-of-range targets, retired
   slots and missing or duplicate charge events.
2. Add independent Linux x86-64 object parsing, symbol/section checks and C
   harness execution for supported expressions. Compare results, diagnostic
   codes and source-byte spans against the interpreter and C11 oracle.
3. Cover integer endpoints, zero divisors, minimum integer divided by −1,
   shifts at −1, 0, 63 and 64, lazy branches, and zero, exact and excessive
   resource limits. Record which oracle tests actually executed.
4. Require a separate CLI review for object-file output. Validate before any
   write, require an explicit destination, stage within its trusted parent,
   reserve against competing files, reject symlinks and clean up failures.
   Preserve static verification and the existing interpreter commands.

The next hosted-runtime design must define process entry, stdout/stderr
replay, host capabilities and a checked failure boundary. For managed
values, start from immutable acyclic snapshots with shared ownership and
bounded logical payload accounting. Keep host resources out of Scarp values
until their lifetime, cycles and cleanup have an approved design. Windows
x64 COFF/PE and Apple Silicon Mach-O require their own ABI and object tests.
