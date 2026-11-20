# Compiler-owned ELF64 object writer

The experimental `native_object::emit_elf64_integer_kernel` Rust API first
shipped in alpha.4 for one integer binding. Alpha.5 added dependent integer
bindings. The API emits an object for a single kernel with an experimental ABI.
The Scarp CLI runs programs through the interpreter.
The compiler parses and resolves the *real* Scarp source, runs the existing
native scalar preflight, lowers and independently verifies its private scalar
statement and expression IR, then encodes a **freestanding x86-64 ELF64 ET_REL
relocatable object** using only safe Rust. No C compiler, linker or network
request is performed during production emission.

## Exact accepted source

The bitwise kernel accepts a bounded linear sequence of immutable integer
`bind` declarations, where initializers use integer literals, groups, references to
earlier initialized integer bindings, bitwise NOT (`~`), AND (`&`),
OR (`|`) or XOR (`^`):

```scarp
bind input = 42;
bind mask = 15;
bind result = (input & mask) ^ (~0 & 8);
```

The kernel returns the **last binding's value** (2 above). Binding loads are
resolved through the verified scalar IR, not source-variable name lookup in the
encoder. A separate contiguous region of 8-byte stack slots stores each
binding; another region holds reusable per-expression temporaries. The
private dataflow verifier proves every native load has been initialized.

Other statements, including checked arithmetic, shifts, loops, branches,
functions, calls, output, updates, floats and modules, remain rejected with
E0049 before any object bytes are returned. This is intentionally stricter
than the existing full private scalar IR and C11 emitter.

The generated symbol `scarp_scalar_kernel` has the **experimental** SysV AMD64
`extern "C" fn() -> i64` calling convention. It returns the last initializer value (2 in the example). This is a *kernel result ABI*, not the side effects
or stdout behavior of executing the original Scarp file.

The emitted object has deterministic sections: null, `.text`, `.symtab`,
`.strtab`, `.shstrtab`, and an empty `.note.GNU-stack`.
Its only non-null symbol is the global function in `.text`; it has **no
external references and therefore no relocations**. Full relocations,
host output callbacks and checked runtime-error ABI remain future work.
Binding slots and reusable expression temporaries are 8-byte stack cells owned
by the callee. Only caller-scratch RAX and RCX are modified, and no external
function calls occur. No native work-budget or runtime-error ABI is claimed.

## Rust host example

```rust
let object = scarp::native_object::emit_elf64_integer_kernel(
    "bind input = 42; bind mask = 15; bind result = (input & mask) ^ (~0 & 8);",
)?;
// Keep ownership of the returned byte buffer. Use OpenOptions::create_new(true)
// if writing it to a file, so existing paths cannot be overwritten.
# Ok::<(), scarp::diagnostics::Diagnostic>(())
```

The returned byte buffer has no filesystem side effects. Hosts may explicitly
save the object under a *new* path and link it with their own test harnesses.
The target is always ELF64 x86-64 even when encoding on Windows or macOS; the
current API has no cross-platform executable linking or target-selector flag.

## Resource and correctness limits

The input must pass the existing preflight: at most 256 scalar slots, 4096
resolved statements, 1024 nodes per expression and 16,384 total nodes.
The accepted binding-only sequence is stricter than general programs.
The encoder checks actual machine-code growth against 200,000 bytes and caps
the complete ELF object at 262,144 bytes. It uses fallible reserves and
deterministic section offsets.
The alpha.16 candidate probes every full 4 KiB stack page crossed while
allocating a large frame. Both return paths restore the complete frame.
No source path, timestamp, username, environment variable, or build identity
is embedded in output. Unsupported input yields no object or partial file.

## Experimental checked integer kernel

`native_object::emit_elf64_checked_integer_kernel` emits a separate kernel
for the same linear binding subset and adds checked signed `+`, `-`, `*`,
unary `-`, division `/`, and remainder `%`. Its distinct symbol is
`scarp_checked_scalar_kernel`. The existing bitwise-only API and symbol retain
their original signatures.

The checked symbol uses the SysV AMD64 calling convention and takes no
arguments. Its return value has this C layout:

```c
#include <stdint.h>
struct scarp_checked_result {
    int64_t value;
    uint32_t status;
    uint32_t span_id;
};
extern struct scarp_checked_result scarp_checked_scalar_kernel(void);
```

Status zero returns the last binding value and a zero span ID.

| Status | Rust constant | Meaning |
| --- | --- | --- |
| 1 | `CHECKED_KERNEL_INTEGER_OVERFLOW` | Signed overflow, including `i64::MIN / -1` and `i64::MIN % -1` |
| 2 | `CHECKED_KERNEL_DIVISION_BY_ZERO` | Zero divisor in division or remainder |
| 3 | `CHECKED_KERNEL_INVALID_SHIFT` | Shift count outside 0 through 63, available in alpha.15 |

Division truncates toward zero and nonzero remainder takes the sign of the
dividend. Alpha.15 checks the full shift count before executing `<<`, `>>` or
`>>>`. Left shift discards bits past the 64-bit width, arithmetic right shift
extends the sign bit, and logical right shift fills with zeros.

```scarp
bind bits = -8;
bind count = 2;
bind result = (bits >> count) ^ (bits >>> count);
```

Failure returns a zero value and the index of the first failing operator in
the Rust result's `failure_spans` table. The table uses half-open UTF-8 byte
offsets in the original source. A host consults it only when status is nonzero.
The checked emitter preserves interpreter operand order, so an earlier failure
terminates before a later operator can run. Both return paths restore the same
stack frame.

The checked emitter applies the same immutable-binding, preflight and object
size limits. It is an experimental Rust API for self-contained Linux x86-64
ELF64 kernels with a fixed integer expression subset.

Regression coverage validates stable identical bytes, section-table offsets,
ELF header sizes, global symbol metadata, source-subset rejection, and an
**independent Linux x86-64 C host link-and-execute comparisons**, including
multi-binding dependencies, arithmetic failures and invalid shift counts.

```sh
cargo test --locked --offline --test native_object
cargo test --locked --offline --test native_object_checked
# The Linux x86-64 suite uses a C compiler only as an independent test oracle.
# Production object emission never invokes one.
```

See [Proposal 0037](proposals/0037-compiler-owned-native-ir.md) and
[Issue #128](https://github.com/scarp-lang/scarp/issues/128).
