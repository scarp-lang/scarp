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

## Experimental parameterized integer function

`native_object::emit_elf64_checked_integer_function` accepts one `pure funcra`
with zero to sixteen required `integer` parameters and an `integer` return type.
Zero-argument and seven-to-sixteen-argument functions are supported on the
development branch for the next pre-release, not in the v0.0.6-alpha.20
published ABI.
It supports scalar integer and Boolean binding storage, plain assignments,
integer compound assignments, forward `if`/`elif`/`else`, short-circuit
Boolean expressions and early returns. Source entry operations, loops, calls,
contracts, optional parameters and heap-backed values are rejected.

The new symbol is `scarp_checked_integer_function`. Declare exactly as many
`int64_t` parameters as the Scarp function has. They arrive in RDI, RSI, RDX,
RCX, R8 and R9 in order for the first six; arguments seven through sixteen
are read from caller-owned stack slots according to SysV AMD64. The callee
copies them into its own verified frame without modifying the caller's frame.
The return uses the same 16-byte result layout as the
checked zero-argument kernel. For a zero-argument function, declare the
parameter list as `(void)` in C.
For the two-argument example:

```c
#include <stdint.h>
struct scarp_result { int64_t value; uint32_t status; uint32_t span_id; };
extern struct scarp_result scarp_checked_integer_function(int64_t lane, int64_t stride);
```

The bounded frame stores parameters before arithmetic can overwrite argument
registers. Lexical retirement ends slot visibility in the verifier. The same
frame is restored on success and on checked arithmetic failure. Source spans use
UTF-8 byte offsets and each failure ID is local to the generated object.

Run the [Rust object emission example](../examples/checked_elf64_function.rs)
with `cargo run --locked --example checked_elf64_function`. The
[Scarp implementation](../examples/checked_elf64_function.srp) is also runnable
through the interpreter and has a checked output fixture.

## Experimental compact function encoding

The next pre-release development branch selects smaller x86-64 instruction
forms for the checked integer-function API. Stack operands use no
displacement for offset zero, a signed 8-bit displacement when possible,
and the full 32-bit displacement otherwise. Integer immediates use a
zero-extending 32-bit move or sign-extending 32-bit move where exact;
values outside those ranges retain the 64-bit immediate.

The two original binding-only kernel encodings remain unchanged.
Function calling conventions, stack probes, branch resolution, overflow
checks and failure-span indexing are unchanged. The code-density regression
verifies 37 bytes of machine code rather than the previous 48 bytes for
`pure funcra f() -> integer { return 42; }`. This is a static size target,
not a measured runtime speedup. Independent native execution tests cover
signed and unsigned immediate boundaries.

## Experimental bounded native loops

The development-only Rust API
`native_object::emit_elf64_bounded_integer_function(source, max_work)`
emits the distinct Linux x86-64 SysV symbol
`scarp_bounded_integer_function`. It accepts the checked scalar function
subset plus while, counted repeat, break and continue, including nested loops. Required
integer parameters use the same register/stack calling convention and
the same checked result structure.

The caller selects a fixed unsigned 32-bit native IR work allowance when
emitting the object. It is initialized anew on each invocation, not shared
between calls. Every visited non-retirement statement and each executed
expression Work event consumes one unit. A zero balance rejects the next
charged event before its work, returning value zero, status 4, and the
index of that event's UTF-8 source span. Unselected short-circuit branches
consume no expression events. This native metric is distinct from interpreter
execution accounting and does not claim wall-clock or process-memory limits.

Backward statement edges are accepted only in the bounded mode. The
existing verifier checks definite slot initialization and retirement at
control-flow joins and rejects reachable fallthrough. Expression branches,
arithmetic failure branches and unmetered function branches retain the
forward-only rule. Calls, default arguments, contracts,
heap-backed values, and general CLI native compilation remain unsupported.

```rust
let source = "pure funcra sum(n: integer) -> integer {
    var i = n;
    var total = 0;
    while i > 0 { total += i; i -= 1; }
    return total;
}";
let artifact = scarp::native_object::emit_elf64_bounded_integer_function(
    source, 1000,
)?;
assert!(!artifact.object.is_empty());
# Ok::<(), scarp::diagnostics::Diagnostic>(())
```

The public alpha.20 release does not include this API. A C host must use the
exact arity and symbol documented above. Tests link and execute emitted ELF
objects on Linux x86-64 using an independent C compiler.

### Counted native repeat

The bounded API also accepts `repeat count as index { ... }`. It evaluates
the count once before entering the loop, snapshots it in a compiler-owned
integer slot, initializes the index to zero, and checks the signed index
against the saved count before each iteration. Updating a source variable
used to compute the count does not change the saved bound. Zero skips the
body. Negative counts return status 5
(`CHECKED_KERNEL_REPEAT_COUNT`) with the repeat initialization source span.
Index progression is checked for integer overflow. Break and continue use
the existing resolved control-flow targets and retirement verifier.

The native work allowance applies to initialization, guard, index advancement
and body operations. Huge positive counts do not bypass the allowance.
The unmetered integer-function API still rejects loops.

```scarp
pure funcra triangular(n: integer) -> integer {
    var total = 0;
    repeat n as i {
        total += i + 1;
    }
    return total;
}
```

The native host regression suite compares zero, one and multiple iterations,
nested loops, early returns, break/continue, count snapshots and negative-count
diagnostics against interpreter execution; it also tests exhaustion before
negative-count evaluation and with a maximum signed 64-bit iteration count.
