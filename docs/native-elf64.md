# First compiler-owned ELF64 object writer (development only)

> **Not yet released.** This strictly scoped experimental library API is
> not in `v0.0.6-alpha.3`. It does not provide native Scarp executables,
> a public `scarp compile` command, a stable ABI, or whole-program output.

The current development branch adds `native_object::emit_elf64_integer_kernel`.
The compiler parses and resolves the *real* Scarp source, runs the existing
native scalar preflight, lowers and independently verifies its private scalar
statement and expression IR, then encodes a **freestanding x86-64 ELF64 ET_REL
relocatable object** using only safe Rust. No C compiler, linker or network
request is performed during production emission.

## Exact accepted source

A single top-level integer binding with an initializer built only from integer
literals, groups, bitwise NOT (`~`), AND (`&`), OR (`|`), and XOR (`^`):

```scarp
bind result = (42 & 15) ^ (~0 & 8);
```

Every other construct, including dynamic name loads, mathematical arithmetic,
checked shifts, branches, functions, print statements and multiple declarations,
is rejected before producing bytes with E0049. This is intentionally stricter
than the existing full private scalar IR and C11 emitter.

The generated symbol `scarp_scalar_kernel` has the **experimental** SysV AMD64
`extern "C" fn() -> i64` calling convention. It returns the initializer
value (2 in the example). This is a *kernel result ABI*, not the side effects
or stdout behavior of executing the original Scarp file.

The emitted object has deterministic sections: null, `.text`, `.symtab`,
`.strtab`, `.shstrtab`, and an empty `.note.GNU-stack`.
Its only non-null symbol is the global function in `.text`; it has **no
external references and therefore no relocations**. Full relocations,
host output callbacks and checked runtime-error ABI remain future work.
Stack temporaries are 8-byte slots owned by the callee; only caller-scratch
RAX and RCX registers are modified, and no external function calls occur.

## Rust host example

```rust
let object = scarp::native_object::emit_elf64_integer_kernel(
    "bind result = (42 & 15) ^ (~0 & 8);",
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
The accepted one-declaration kernel is smaller still. Native code has at most
200,000 bytes, the complete ELF object at most 262,144 bytes; all sections are
checked before allocation, with fallible reserves and deterministic offsets.
No source path, timestamp, username, environment variable, or build identity
is embedded in output. Unsupported input yields no object or partial file.

Regression coverage validates stable identical bytes, section-table offsets,
ELF header sizes, global symbol metadata, source-subset rejection, and an
**independent Linux x86-64 C host link-and-execute comparison**.
The encoding still needs security review and broader arithmetic/error ABI work
before being promoted beyond this narrow development API.

```sh
cargo test --locked --offline --test native_object
# The Linux x86-64 suite uses a C compiler only as an independent test oracle.
# Production object emission never invokes one.
```

See [Proposal 0037](proposals/0037-compiler-owned-native-ir.md) and
[Issue #128](https://github.com/scarp-lang/scarp/issues/128).
