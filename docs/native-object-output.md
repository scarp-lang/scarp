# Native object output and proposed compile command

The experimental Rust API writes a completed ELF64 object at an explicit
destination. The object emitter performs compilation before the writer
touches the output directory.

```rust
let object = scarp::native_object::emit_elf64_integer_kernel("bind value = 42;")?;
scarp::native_object::write_elf64_object_new("value.o", &object)?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

The writer checks the Linux x86-64 relocatable header, writes and flushes
the bytes in a scratch directory beside the destination, and publishes
them with a no-overwrite hard link. The output appears after the full
object is written. Existing regular files, directories and symlinks keep
their previous contents or targets. The staging guard removes its paths
on normal return and I/O error. The destination parent must already
exist and its filesystem must support hard links. The caller controls
that directory. Directory durability after a crash and reaping staging
paths left by process termination are separate concerns.

## Proposed first CLI contract

- Accept one UTF-8 Scarp source file and require `--target x86_64-linux`,
  `--entry NAME`, `--max-work COUNT`, and `-o PATH`. The allowance is an
  unsigned 32-bit count per entry invocation. Project input and other
  targets can be separate modes once their import and ABI rules are defined.
- Emit one Linux x86-64 ELF64 relocatable object. Use the checked scalar
  function entry ABI, a bounded native work allowance, and the existing
  diagnostics and byte spans for unsupported source or compilation errors.
- Return 0 when the object is written, 2 for invalid arguments or an
  unsupported target selector, and 1 for unreadable input, a compilation
  diagnostic, or an output failure. Diagnostics distinguish each stage.
- Complete compilation before staging and publish through the
  no-overwrite writer. Return a diagnostic on an unsupported filesystem
  or target. Keep execution, user input and host linking outside this
  object's compilation operation.

The existing CLI commands keep their own contracts. This proposal does
not add a command or an executable linker.
