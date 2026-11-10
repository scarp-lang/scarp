# Bounded program input contract

Status: **published in v0.0.6-alpha.6**, 10 November 2026.
The older alpha.5 does not support input. See [publication verification](releases/v0.0.6-alpha.6-validation.md).
This is the compiler-owned contract and regression-fixture reference. Public
reference material belongs in Scarp Docs and lessons in Scarp by Example after
integration. The preceding design is [PR #180](https://github.com/scarp-lang/scarp/pull/180).

## Language behavior

`read_line() -> string` is an impure, zero-argument expression. It reads one
selected logical line from an explicitly supplied UTF-8 byte source. It uses
the existing call syntax, string type, lexical scopes and function frames;
no new keyword, AST constructor or numeric built-in is introduced. As with
existing intrinsics, a user-defined function with this name takes precedence.

```scarp
bind name: string = read_line();
print("Hello", name);
```

A line containing `Ada` followed by LF prints `Hello Ada` followed by LF. Repeated
calls read successive lines in source evaluation order, including nested calls,
output arguments, file initializers, selected branches, loops and impure functions.
Unselected short-circuit operands, branches and unused functions consume nothing.
`pure funcra`, parameter defaults, `needs` and `ensures` reject input statically,
including unreachable branches. Module constants keep their existing prohibition
on calls. A regular entry-script `bind` initializer may read at runtime.

An LF terminates a line. Exactly one preceding CR is removed only with LF;
bare CR, spaces, NUL, combining marks and all other valid UTF-8 content remain.
An LF or CRLF alone produces the empty string. Immediate EOF reports E0055;
a final nonempty unterminated line is accepted. The next selected read encounters
EOF. Invalid UTF-8 reports E0056 without replacement. Input is never normalized,
trimmed or implicitly converted to a number. Scalar parsing is separate PR #179.

## Explicit host APIs and compatibility

- `run_with_input` and `run_with_input_and_options` compile source then execute
  using a caller-owned `BufRead` and explicit stdout/stderr writers.
- `interpreter::execute_with_input` and `execute_with_input_and_options` reuse a
  compiled `ResolvedProgram` with a fresh reader state and quotas per execution.
- `ModuleProgram::execute_with_input` retains source-map and caller-stack
  diagnostics for imported functions.

See [the executable Rust example](../examples/program_input.rs). Existing
single-writer, dual-writer, `ExecutionLimits`, `ExecutionOptions`, `RunError`,
AST and numeric `Builtin` APIs keep their shapes and behavior for existing
programs. Selecting input through a writer-only API reports E0054. Those APIs
never acquire global stdin. Standalone expression evaluation also has no reader.
New diagnostic variants require additions in exhaustive Rust host matches.
Typed expression metadata exposes an impure `() -> string` signature; indexed
symbol navigation records a built-in call without a source declaration.

## CLI and nonexecution

```sh
cargo run --locked -- exec examples/program_input.srp < examples/program_input.stdin
cargo run --locked -- verify examples/program_input.srp
cargo run --locked --example program_input
```

`exec`, legacy `run` and project `launch` use redirected/piped stdin. With a
terminal connected to stdin, a selected intrinsic reports E0054 immediately;
programs that do not select input still run. `verify` and all source inspections
never read program input. Evaluating `check` keeps running selected computation
but reports E0054 if it reaches input, even if its stdin was redirected.

A pipe or supplied host reader can still block while waiting for its producer.
The host owns reader readiness, cancellation and wall-clock deadlines. Work
quotas bound compiler-controlled work, not time spent inside a host I/O call.
Reader implementations and buffering may read ahead; raw quotas count bytes
consumed through `BufRead::consume`, not underlying operating-system reads.

## Quotas, allocation and work

Input limits are a separate public `InputLimits` value; existing public runtime
option struct literals remain valid. CLI options precede the path and accept
unsigned decimal counts, including zero, with no repeated flag.

| InputLimits field | CLI flag | Default | Meaning |
| --- | --- | --- | --- |
| `max_line_bytes` | `--max-input-line-bytes` | 65,536 | Raw bytes in one line including CR/LF |
| `max_total_bytes` | `--max-input-bytes` | 1,048,576 | Cumulative raw bytes consumed per execution |
| `max_reads` | `--max-input-reads` | 4,096 | Selected intrinsic calls, including EOF attempts |

The effective raw line cap is also bounded by the existing 1,048,576-byte string
ceiling. Thus a line at that raw ceiling must reserve space for its terminator.
Line and total limits are enforced incrementally without consuming past them.
At an exact unterminated-line boundary a buffered EOF probe distinguishes a
valid final line from an overlong line. Zero raw-byte/read quotas fail before
calling the reader. With no byte allowance for a new read, E0058 takes precedence
over probing for EOF. Quotas do not restrict a shadowing user function.

Input capture conservatively charges **raw** bytes, including stripped CR/LF,
to `ExecutionOptions.max_text_bytes` before reserving or copying. Immutable
string sharing does not recharge. Retirement does not refund allocation.
Buffers grow geometrically, capped by the effective raw line limit, using
fallible reservation. There is no unbounded `read_line`/`read_to_end` call.
Existing Arc string conversion retains its normal allocation behavior; logical
quotas are not an exact allocator-overhead or process-RAM measurement.

Work charges include existing expression/statement tasks, one unit before each
reader-buffer call (including EOF and Interrupted retries), three per scanned
raw byte, and prior payload size when growing capture storage. Scan blocks have
at most 4,096 bytes. Reader fragmentation can change call/growth work, but the
same source and same reader behavior have deterministic budgets. Work/text/input
exhaustion all retain the selected source span and prevent output replay.

| Diagnostic | Meaning |
| --- | --- |
| E0054 | No reader in this execution mode, including terminal input/check |
| E0055 | EOF before a new line |
| E0056 | Invalid UTF-8 line |
| E0057 | Reader error; arbitrary host error strings are not exposed |
| E0058 | Raw line/total/read quota or bounded reservation failure |
| E0017 / E0048 | Existing shared work/text quota |

E0051-E0053 remain unallocated on main and are used by separate draft BigInt
PR #172. Input codes avoid that integration collision. Pure-context failures
retain E0025. Both experimental native emitters reject input with E0049 without
accessing a reader.

## Output and interactive boundary

The interpreter evaluates the program and captures events before replaying
stdout/stderr. All five output forms retain their original semantics. A source,
input or runtime failure writes no program output; already consumed input
cannot be restored. Output replay can partially write before an output I/O
failure, as before. Stream switches retain their existing flush behavior.

`ask` is **not implemented**. Neither `write("Name: ")` nor `print` displays a
prompt before an input read. A future separate interactive mode or prompt API
must explicitly write and flush the prompt before waiting, and document that
visible prompt bytes cannot be rolled back after later failure. It needs its
own destination, flush/broken-pipe, EOF, nonterminal and cancellation tests.
This development slice deliberately rejects terminal input instead of hiding
that contract behind buffered output. No password input is introduced.

## Verification

`tests/program_input.rs` covers line endings, Unicode, EOF/encoding/reader
failures, purity, types, selected paths, recursion-compatible frames, zero/exact
quotas, allocation/work bounds, fragmentation, concurrent program reuse, native
rejection and atomic output. `tests/program_input_modules_cli.rs` exercises real
CLI processes, open-stdin nonblocking checks, project launch, module navigation,
source-local errors, broken pipes and the exact shipped input/output fixture.
The shared example/package runners accept `.stdin` companions and compare exact
`.stdout` bytes. The Python oracle tests 128 seeded Unicode lines, exact errors
and real Unix PTYs without providing terminal input. Windows/macOS/MSRV and
native archives must also pass the repository's existing CI matrix before
integration; a prior branch's success is not evidence for this source.
