# Proposal 0035 — Experimental scalar C11 emission

**Status:** experimental Rust embedding API published in
`v0.0.5-alpha.5` and retained in the current compiler. It is **not** a
`scarp compile` command or a general native executable compiler. This is a
deliberately restricted port of historical PR #20, **not a merge of its old
compiler tree**.

## Purpose and invocation

Expose a small, inspectable native-backend vertical slice without publishing
a native executable command or guessing a stable ABI:

```rust
let c = scarp::c_backend::emit_c("var x = 4; x *= 3; print(x);")?;
// The caller may compile this string with an independently selected C11 toolchain.
```

For hosts with a resolved program, `c_backend::emit_program(&program)` accepts
the current compiler's immutable, typed slot/operation plan. Both return the
**entire** C source or a source diagnostic. No CLI route, C compiler process,
output path, registry or network action is created here.

## Accepted source subset

- Scalar i64 integers and booleans, with checked arithmetic, bit operations,
  comparison and short-circuit boolean logic.
- `bind` / `var` slots, compound assignments, blocks, `if` / `while`, and
  `repeat COUNT as index` counted loops (one-time count snapshot, zero-based
  immutable index, bounded advancement). `break` / `continue` follow the
  nearest loop. Scalar retirement does not consume work steps or need a C heap.
- Exactly one output argument on stdout with terminating newline
  (`print` / `println`). The argument is a scalar expression or a literal
  UTF-8 string. Strings in bindings or computed text are unsupported.

All unsupported forms fail with `E0049` before emission: functions,
unsupported imports, floats, vectors, tiles, arrays, calls,
text construction, stderr, `write`, and multiple/zero output arguments.
Frontend source errors keep their original diagnostic code.

The current implementation first applies a **crate-internal native scalar
preflight** to the already resolved program, before generating any C source.
It validates operation and expression kinds, logical workspace size and
resource quotas in one pass. The C emitter then consumes the checked workspace
counts. This preparatory boundary does **not** introduce a separately supported
native IR, object writer, compiler invocation or a new Rust public API.
Any future machine-code emitter must pass its own reviewed target/ABI and
resource checks; see [the native pipeline plan](https://github.com/scarp-lang/scarp/issues/128).

The initial caps are 256 scalar slots, 4096 operations, 1024 nodes
per expression, 16384 total expression nodes, and 65536 bytes of
literal string payload. These are checked before emission.

## Generated C contract

The emitter generates one freestanding C11 translation unit with
caller-owned `struct scarp_workspace` and `struct scarp_host`.
`scarp_validate` checks execution under explicit work/print budgets
without writing. `scarp_run` makes a dry pass before replaying stdout
to the supplied callback. No global streams or C compiler are used.
Runtime errors preserve E-code numbers and UTF-8 byte spans, including
E0046 for invalid shift counts. Empty string payloads never cause a
zero-byte host callback; only nonempty output is forwarded to the host.
A failing callback returns the internal C sentinel 65535. Once output replay begins, a callback failure may
leave previously written bytes.

This **experimental ABI** does not replicate every `ExecutionOptions` field:
C entrypoints accept only work/print budgets. The host must select
trusted limits and supply a synchronous callback. No C allocator, Rust
ABI, native executable, GPU backend, or C-to-Scarp calls are implied.

## Validation and next phases

1. Differential tests compare stdout, error codes, source spans and work/
   print budgets between the interpreter and emitted C at `-O0` and `-O2`,
   including nested counted-repeat loops, negative counts, break/continue,
   snapshots, oversized counts and the interpreter's exact E0019 code.
   Include repeated workspace use, callback failures and rejected forms.
2. Require a working C11 compiler in stable CI on Linux, macOS and Windows,
   rather than silently skipping C differential tests; retain minimum Rust 1.74.
   On Linux, explicitly compile freestanding x86-64 and AArch64 ELF objects
   and check that they contain no undefined symbols with `readelf`.
3. Only after independent review, propose CLI integration against the current
   compiler. Specify artifact publication, compiler discovery, output safety,
   cross-compilation and full option parity before advertising `scarp compile`.

No published release, tag or package changes with this experiment.
