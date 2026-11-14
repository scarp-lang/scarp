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

## End-of-input fallback (development main)

`read_line_or(fallback: string) -> string` is an additional impure expression
for line-oriented programs that need a sensible final value when input is
exhausted. It consumes one selected line exactly like `read_line()` and returns
the fallback **only** for E0055 (immediate EOF). A genuinely empty LF/CRLF line
returns `""`, not the fallback. Other errors (unavailable input, invalid UTF-8,
reader failure, input quota, work or text quota) remain errors. It evaluates its
string fallback argument before performing the read, even when input exists.
This form is not included in published alpha.6–alpha.8.

```scarp
bind first = read_line_or("Anonymous");
bind second = read_line_or("Guest");
print(first, second);
```

Both input expressions are rejected in pure functions, defaults and contracts.
They obey the same reader capability, exact UTF-8 handling, input limits and
no-output-on-runtime-error rule. Either intrinsic spelling may be shadowed by
an explicitly defined function; that user function does not acquire input.

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

Pure-context failures retain E0025. Both experimental native emitters reject
input with E0049 before accessing a reader. The separate
[BigInt diagnostics](reference/diagnostics.md#bigint-diagnostics)
use E0051–E0053 in alpha.9.

## Output and interactive boundary

The interpreter evaluates the program and captures events before replaying
stdout/stderr. All five output forms retain their original semantics. A source,
input or runtime failure writes no program output; already consumed input
cannot be restored. Output replay can partially write before an output I/O
failure, as before. Stream switches retain their existing flush behavior.

**Development feature, not in published v0.0.6-alpha.9:** `ask(prompt: string)
-> string` is an impure expression that writes and flushes a UTF-8 prompt
to the explicitly supplied stdout writer before reading one bounded UTF-8 line.
Unlike `write("Name: "); read_line()`, the prompt is visible immediately.
An empty prompt is valid, and prompt text has no implicit newline.

CLI execution requires an explicit `--interactive` option to enable prompts
and terminal stdin. The opt-in works with redirected stdin and also with
`--input-file PATH` for reproducible interactive program testing; without it, `ask` returns E0054 without writing a prompt or consuming
input. The Rust embedding APIs `run_with_interactive_input`,
`run_with_interactive_input_and_options`, interpreter
`execute_with_interactive_input_and_options` and module
`execute_with_interactive_input` require caller-supplied readers/writers.
There is no ambient library stdin acquisition. All existing default execution
APIs stay noninteractive, including `check` and `verify`.

```scarp
bind name: string = ask("Your name: ");
print("Welcome", name);
```

Prompt text is emitted and flushed **during** evaluation, before a potentially
blocking host read. Therefore visible prompt bytes cannot be rolled back after
EOF, invalid UTF-8, a later runtime error or a subsequent failed output replay.
Ordinary `print`/`write`/`eprint`/`ewrite` events remain buffered until
successful evaluation. For example, an ordinary `print` before `ask` will
be replayed *after* the immediate prompt. Use `ask` for prompt text.
Input quotas are preflighted before the prompt, where possible. The runtime
also checks that at least one work step remains for the first reader-buffer probe
before making the prompt visible. This check does not consume a step; the actual
read still charges it normally. Later reader/scanning work may fail after a
prompt has already appeared. E0066 reports a prompt write/flush failure with
no arbitrary host error text. Host blocking,
cancellation and deadlines remain the caller's responsibility. No password
echo control, formatting or new input data type is introduced.


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
