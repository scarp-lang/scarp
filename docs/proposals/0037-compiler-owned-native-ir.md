# Proposal 0037 — Compiler-owned scalar IR and freestanding object emission

**Status: design for review. Not implemented.** This proposal advances
[the native compilation milestone](https://github.com/scarp-lang/scarp/issues/128)
after the internal scalar preflight introduced by PR #130. It authorizes no tag,
prerelease, public CLI command, stable ABI, runtime change or toolchain switch.

## Problem and constraints

The experimental `c_backend::emit_c/emit_program` API lowers a bounded
integer/Boolean subset into a freestanding C11 translation unit. A separate
C compiler must still turn that unit into machine code. The published Scarp
CLI does **not** produce object files or native executables. Historical PR #22
has a hosted build driver based on an older feature stack; directly merging
it would bypass current source, type, memory and compatibility contracts.

The first compiler-owned machine-code step should reuse the current compiler's
lexing, parsing, name resolution, immutable storage plan and source diagnostics.
No independent language parser, implicit fallback to external compilers, network
fetches or opaque third-party runtime is acceptable in the production path.

## Proposed first deliverable

Produce **one experimental relocatable freestanding object**, not an executable.
The review target is ELF64 for x86-64. This is a proposed milestone target,
**not a claim that Scarp currently supports it**. Before implementation,
decide and record the exact target triple, calling convention, symbol names,
section alignment, relocations and embedding-host contract.

A successful object need not have a process entry point or operating-system
services. The consumer supplies workspace and output callbacks; running the
object requires a separately reviewed host integration. Unsupported targets,
features or calling conventions must fail explicitly rather than silently
falling back to C or launching a platform compiler.

## Phase A: typed, target-neutral lowering

Introduce a private, versioned-by-source-tree `ScalarIr` lowered **only after**
`ResolvedProgram` has passed semantic checking and backend acceptance. Avoid
exposing an unstable serialized IR or adding public AST variants.

| IR family | Source-plan meaning | Essential invariant |
| --- | --- | --- |
| `ConstI64`, `ConstBool`, `Load`, `Store` | Integer/Boolean values and resolved slots | Keep types distinct; no implicit conversions |
| `CheckedUnary`, `CheckedBinary`, `Compare` | Arithmetic, masks, shifts, comparisons | Preserve E-codes and original operator byte spans |
| `Branch`, `Jump`, `Label` | Conditions and short-circuit control flow | Skipped operands consume no runtime work |
| `RepeatInit`, `RepeatGuard`, `RepeatNext` | Counted iteration with a saved bound | Negative counts, index limits and loop jumps match the interpreter |
| `Retire` | Resolved storage lifetime end | No extra runtime work charge; no reading retired slots |
| `PrintLiteral`, `PrintValue` | Existing restricted stdout output | Exact UTF-8/LF bytes, print budgets and validation before output |

Each instruction must carry a **source byte span**, a checked static type (where
a value is produced), and explicit operands. Register/temporary numbering must
be deterministic. No pointers, host handles, raw addresses, mutable aliases,
unsupported calls or functions should be expressible in the first IR slice.

The IR verifier runs after lowering and before any emission. It checks block
targets, value types, defined-before-read registers, slot bounds, instruction
count, control-flow reachability and resource-accounting events. The linker,
source loader and existing resolver remain the sole authorities for source
meaning. Compiler errors must not expose partially lowered code.

The current scalar preflight's 256-slot, 4096-operation, 1024-per-expression,
16384-total-expression-node and 65536-literal-byte caps remain in force for
the existing C backend. New IR/encoder limits must be explicit and additive;
they are **not** an implicit promise of unlimited native compilation.

## Phase B: freestanding object encoder

Build an ELF64 `ET_REL` writer in safe Rust. Define deterministic `.text`,
data/constant, string and symbol-table layout, exact little-endian encoding,
section headers and relocation records. Reject offset arithmetic overflow,
invalid alignment, excessive section counts/sizes, overlapping regions and
unsupported relocations before publishing bytes.

Use a **caller-owned output buffer** with separate bounds for IR instructions,
compiled bytes, symbols, relocations and constant payloads. Fallible reserves
and checked arithmetic must be used before large allocations. A successful
encoding should be reproducible for identical source, options and compiler
revision without embedding timestamps, absolute filesystem paths or host IDs.

The initial object should not depend on a C compiler, linker, network or shell.
Validation tools such as `readelf`/`llvm-readobj` are independent test
oracles only, not dependencies of the Scarp toolchain.

## Phase C: runtime semantics and host ABI decision

Before considering an exported object callable, decide the lifetime, alignment
and layout of the caller-provided workspace, integer/Boolean representation,
host write callback, step/print limits, error code and span reporting, and
whether `scarp_validate` / `scarp_run` remain the internal adapter names.
Do not freeze the experimental C emitter's ABI by accident.

Match the interpreter for arithmetic overflow, division by zero,
`INT64_MIN / -1`, invalid shift counts, loop control, short-circuit evaluation,
step and print quota exhaustion, and empty-string output. A prevalidation
pass (or equivalently bounded transactional output) must ensure computation
failures produce **no program output**. Once output replay begins, host write
failure can still leave previously written bytes, as the current contract states.

## Phase D: CLI and artifact publication gate

Only after the object writer and host tests are independently reviewed should
an **experimental** `scarp compile --emit obj FILE -o OUTPUT` be proposed.
That syntax is a design candidate, not an implemented command. Keep `exec`,
`verify`, `inspect`, `guide`, `version` and deliberately retained legacy
entry points unchanged; notably, runtime `check` is not static `verify`.

The output parent must exist. Existing regular files, directories, symlinks
(including dangling symlinks) and competing outputs must never be overwritten.
Stage privately, validate before publication, reserve destinations exclusively
and handle cleanup/error paths without shell interpolation. Do not silently
produce executables or automatically run generated code.

General native executable linking, Windows PE, Mach-O, cross-target runtimes,
source modules as separately linked objects, strings-as-values, float/vector/
tile operations, user functions, arrays and GPU execution are **future reviews**.

## Verification gates

1. **IR equivalence:** deterministic fixtures and property-style generated
   valid scalar programs compare native IR interpretation against the existing
   checked interpreter and experimental C11 oracle. Check exact output,
   diagnostics, UTF-8 source spans, side-effect ordering and budgets.
2. **Invalid inputs:** malformed source or resolver failures never invoke
   native lowering. Unsupported modern expressions return E0049 without
   emitting an object, partial source file or misleading success status.
3. **Encoder integrity:** compare byte-exact ELF headers, sections,
   symbols and relocations with independent fixture decoders. Corruption and
   mutated boundary inputs must fail safely without out-of-range indexing.
4. **Resource boundaries:** test zero, one, exact maximum and over-maximum
   values for registers, instructions, object bytes, alignment, relocations,
   literal storage, work, output and recursion-free deep expressions.
5. **Cross-platform authoring:** the encoder must run under Rust 1.74 and
   stable Rust on the current Windows, Linux and macOS CI matrix, even when
   the **generated object target** is initially only freestanding x86-64 ELF.
6. **Runtime integration:** where supported, link through a trusted test
   harness and compare execution with Rust interpreter and C11 references
   at optimization-independent boundaries. Do not conflate object parsing
   with successfully running native code.
7. **Release integrity:** require all four Rust jobs and three package jobs
   on the final-main source plus separate version/source approval. No PR
   checks, object fixture or version change by itself authorizes publication.

## Implementation order and explicit non-goals

- [ ] Approve one target, ABI and logical compiler/encoder resource limits.
- [ ] Introduce verified private scalar IR and deterministic text snapshots.
- [ ] Differential-test IR execution against current scalar behavior.
- [ ] Encode and independently decode one freestanding ELF64 object format.
- [ ] Add secure object output handling and host linking validation.
- [ ] Consider an experimental CLI route and separately scoped publication.

This proposal does **not** claim that `v0.0.6-alpha.2` ships native objects,
that an LSP exists, or that the C11 emitter is a general native compiler.
Preserve the historical PR #22 discussion for its unique tests and design
ideas; determine its eventual disposition separately under issue #119.
