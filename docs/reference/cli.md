# Command-line reference

## Project commands in v0.0.6-alpha.8

| Command | Behavior |
| --- | --- |
| `scarp init DIRECTORY` | Scaffold a new project with a declarative manifest and starter source |
| `scarp run [DIRECTORY]` | Execute the project entrypoint with the interpreter |
| `scarp verify [DIRECTORY]` | Type-check the entrypoint and imports without executing runtime statements |
| `scarp project info [--json] [DIRECTORY]` | Inspect the validated manifest and entrypoint |
| `scarp test [--list] [--json] [--require-tests] [--filter SUBSTRING] [DIRECTORY]` | Discover or run project tests |

The optional directory defaults to the current project. `run FILE` and
`verify FILE` continue to accept standalone source files. See the
[manifest format and migration](../project-v2.md), [test report and fixture
contracts](../project-testing.md), and [assertion syntax](../assertions.md).

Scarp v0.0.6-alpha.9 adds `scarp test --json=v2`
for [structured diagnostics](../project-testing.md#structured-failure-reports).

**Development candidate only (not in published alpha.9):** `scarp exec
--input-file DATA.txt app.srp` selects a UTF-8 file as the program's explicit
input source. `run FILE`, `run PROJECT` and `launch PROJECT` accept the
same option. An input file takes priority over ambient stdin; normal input
quotas, UTF-8 errors and transactional output remain unchanged. The path is
relative to the process working directory, not automatically to the Scarp
source directory. Static commands reject the flag. Input is streamed through
a buffered file reader rather than loaded into memory all at once.

**Integration candidate (not in published alpha.9):** `--interactive`
allows `ask(prompt)` and `ask_or(prompt, fallback)` to flush a prompt before
reading a line from stdin or an input file. `--interactive --input-file DATA.txt` makes
interactive programs reproducible without reading ambient stdin; prompts are
visible even if later input or runtime execution fails. Without `--interactive`,
both prompt built-ins report E0054. `ask_or` uses the fallback only at EOF.

Development builds accept `--max-array-bytes` on `exec`, `run`, `launch`, and
`check`. The default cumulative fixed-array payload limit is 8,388,608 bytes.
See [fixed arrays](../fixed-arrays.md) for the backing-storage charge.

**v0.0.6-alpha.6 published prerelease:** `exec`/`run`/`launch` support redirected
UTF-8 `read_line()` with `--max-input-line-bytes`, `--max-input-bytes` and
`--max-input-reads`. `--max-tuple-bytes` limits new tuple payload. Terminal reads
and evaluating `check` report E0054; `verify` remains static. See the
[input contract](../program-input.md) and [tuple contract](../tuples.md).

**v0.0.6-alpha.5** is an earlier published experimental GitHub prerelease,
released on **9 November 2026** from exact source
`205e92f166f186c3905e5880d41c31717b697b8e`.
See the [public release](https://github.com/scarp-lang/scarp/releases/tag/v0.0.6-alpha.5)
and [source/asset verification](../releases/v0.0.6-alpha.5-validation.md).
Top-P and Pearson are pure built-ins; ELF64 multi-binding generation remains
experimental Rust-hosted code only. It adds no CLI command, executable linker
or `scarp compile`.

**v0.0.6-alpha.4** is a published experimental GitHub prerelease.
It adds scientific numerical functions and the first native ELF64 object
writer exposed only as an experimental Rust library API.
It does **not** add `scarp compile` or a native executable compiler.
CLI execution remains interpreter-based. See the [verified release](../releases/v0.0.6-alpha.4-validation.md).

The **published `v0.0.6-alpha.3` prerelease** adds numerical built-ins but
**no new CLI commands**. The interpreter-backed command vocabulary
remains compatible with published `v0.0.6-alpha.2`. See
[alpha.3 publication verification](../releases/v0.0.6-alpha.3-validation.md).

## Scarp-first CLI (retained by published v0.0.6-alpha.6)

The published alpha.2 prerelease adds no CLI commands: its new cache and
dependency-invalidation features are opt-in Rust embedding APIs. The command
set remains the same as published alpha.1.

Published v0.0.6-alpha.1 adds static `inspect symbols` and `inspect references`
commands, which are absent from older alpha.5 packages.

The latest published prerelease is
[v0.0.6-alpha.9](https://github.com/scarp-lang/scarp/releases/tag/v0.0.6-alpha.9).
It retains project-directory module inspection. The experimental C11 scalar
source emitter is a Rust embedding API, **not** a Scarp CLI command;
`scarp compile` still does not exist.

The Scarp-first commands and offline `diagnose` shipped in published alpha.3.
Alpha.1/alpha.2 binaries retain the older vocabulary. `create`, `launch`, and
`verify DIRECTORY` are included in **published v0.0.5-alpha.4 packages**,
not alpha.3 packages. Confirm the exact release tag/commit and use
`scarp version` or `scarp --version` to check an installed binary.

| Scarp-first command | Actual behavior |
| --- | --- |
| `scarp`, `scarp guide` | Print current supported command guidance |
| `scarp guide exec` | Show contextual execution help |
| `scarp create DIRECTORY` | From alpha.4: create a fresh offline project, refusing existing paths |
| `scarp launch [budgets] DIRECTORY` | From alpha.4: run the contained manifest entrypoint via the interpreter |
| `scarp diagnose [--json]` | Show actual running compiler version, host OS/architecture and supported capability inventory without network access |
| `scarp guide inspect tokens` | Show lexer inspection help |
| `scarp guide inspect modules` | Show checked-module inspection help |
| `scarp version` | Print the same actual package version as `--version` |
| `scarp exec [budgets] FILE` | Execute using the existing interpreter with the same diagnostics and budgets as the historical `run` command |
| `scarp verify FILE` | Statically resolve source/imports, names and types without executing runtime statements |
| `scarp verify DIRECTORY` | From alpha.4: validate `Scarp.project` and check its entrypoint statically |
| `scarp inspect tokens FILE` | List lexer tokens and real source locations |
| `scarp inspect ast FILE` | Parse just the entry source and display its actual frontend syntax tree without execution |
| `scarp inspect modules [--json] FILE` | Inspect statically checked modules; JSON retains schema version 1 |
| `scarp inspect symbols [--json] FILE|DIRECTORY` | From v0.0.6-alpha.1: show typed symbol definitions in a statically checked source graph |
| `scarp inspect references [--json] FILE|DIRECTORY` | From v0.0.6-alpha.1: show source references and matching definitions without running statements |
| `scarp explain E####` | Explain an existing compiler diagnostic |

For the alpha.4 release, examples include:

```sh
scarp guide
scarp create hello-world
scarp launch hello-world
scarp verify hello-world
scarp exec examples/hello.srp
scarp verify examples/hello.srp
scarp inspect tokens examples/hello.srp
scarp inspect ast examples/hello.srp
scarp inspect modules --json examples/module_inspection.srp
scarp guide inspect modules
```

`exec` accepts exactly one source filename, with an optional `--` before a
dash-leading filename. **It does not forward program arguments**; project-mode
execution via `launch` is supported from alpha.4 using an explicit
`Scarp.project`; standalone `exec` does not forward program arguments. The
`inspect` command accepts `tokens`, `ast`, `modules`, `symbols`, and
`references`. The latter three support schema-v1 `--json` output; the AST
debug view is not JSON. Missing/unsupported topics and options
return exit 2.

**Legacy migration window:** Existing `run`, `check`, `tokens`, and
`modules` invocations remain usable for old scripts, examples and extension
integrations. They are not encouraged as the new public vocabulary. Their removal
requires a separate approved migration decision and coordinated tooling updates.
The legacy `check` command evaluates selected runtime paths silently and is
**not** equivalent to the proposed static-only `verify` command.

The static-only `verify` uses the real parser and module resolver, evaluates
only compile-time scalar module constants, and produces no runtime artifacts.
In alpha.4, an explicit directory argument resolves
the bounded `Scarp.project` manifest through the same path validation as
`launch`. A source-file argument retains its existing behavior. Invalid
manifests return status 1 and honor `--diagnostic-format=json` without
partial output.
It does **not** evaluate runtime arithmetic, program statements or contracts.
There is no `--strict` or success-`--json` support for `verify` yet.
The new source commands accept `--diagnostic-format=json|text` for compiler
errors using existing diagnostic schema version 1. For `inspect modules`,
`--json` selects successful module metadata; `--diagnostic-format=json`
selects error output only. `exec` retains program stdout/stderr behavior.
The nested `inspect` command forwards diagnostic flags to the same real
lexer and module compiler without changing the error schema.

**Alpha.5 published extension (not in older alpha.4 packages):**
`scarp inspect modules [--json] PROJECT_DIRECTORY` also accepts an explicit
project directory, resolving its `Scarp.project` to
the same contained entrypoint used by `launch` and `verify`. Source and project
inventories are identical when they resolve to the same canonical entry file.
This is static inspection: it does not execute program statements or produce
native artifacts. Invalid manifests return status 1 with no partial output;
`--diagnostic-format=json` applies to these errors as well.

`compile`, `polish`, and package or registry management are not available.
Alpha.4 `create` and `launch` use the real interpreter; see
[Scarp projects](../project-workflow.md). No native Scarp executable is generated;
Rust's `cargo build` builds the compiler itself.

### Development AST inspection

```sh
scarp inspect ast examples/hello.srp
scarp inspect ast --diagnostic-format=json examples/hello.srp
```

`inspect ast` uses the actual Scarp lexer and parser. It **does not**
resolve imported files, check types, or execute statements and therefore does
not replace `verify`. Successful output is a human-readable **development
debug representation**, not a frozen AST serialization, parser API contract,
or schema-versioned JSON. `--json` is deliberately unavailable for successful
AST output. The existing `--diagnostic-format=json|text` option applies only
to parser/lexing diagnostics on stderr. The source path supports the normal
`--` separator, Unicode and filenames containing spaces. There is no new
top-level `scarp ast` alias.

## Historical and existing CLI contracts

Since alpha.2, `scarp modules --json [--] <path>` emits metadata with file-local
UTF-8 byte spans. Primitive signatures use schema 1, tuple signatures use schema
2, and signatures containing a fixed array use schema 3 on development builds.
`scarp explain E####` prints a code explanation without
source I/O. See [JSON inspection and explanations](../module-json.md) for schema,
ordering, flags, writer errors and status rules. Both commands are implemented;
build/compile/fmt/test/bench/doc/doctor remain unavailable CLI commands.

The alpha.2 module-inspection milestone added `scarp modules [--] <path>` and per-command help.
It loads, links and statically checks files, then reports imports, dependencies
and typed public/private declarations. It never executes runtime statements,
defaults or contracts, and rejects runtime-budget flags. Restricted scalar module
constants evaluate during compilation with separate work limits. Loading limits still
apply. Failure writes a diagnostic with no partial inventory; successful
inventory goes to stdout. See [module inspection](../module-inspection.md).
Released alpha.1 has no modules command; alpha.3 additionally provides
`scarp inspect modules`.

The 0.0.5-alpha.1 prerelease keeps these commands, options and exit codes. Tile and
matrix calls work through `run` and `check`; `tokens` only lexes their source.
`--max-vector-bytes` counts new vector and tile element buffers together, including
matrix outputs, and retirement does not refund the cumulative quota. E0017
identifies work exhaustion, while E0036 identifies payload exhaustion. Example:

```sh
scarp run --max-steps 2000000 --max-vector-bytes 1024 "examples/matrix_forward.srp"
scarp check examples/tile_windows.srp
```

Use this page to select a command, interpret its streams and status, and pass
paths reliably. Availability differs between the published release and the
development stack; see [compatibility](../compatibility.md).

## Commands and availability

| Invocation | Purpose | v0.0.1 | v0.0.2 through beta.3 |
| --- | --- | --- | --- |
| `scarp run <path>` | Read, validate, and execute a source file | Yes | Yes |
| `scarp tokens <path>` | Print lexer tokens without parsing | Yes | Yes |
| `scarp check <path>` | Validate names, types, and arithmetic without printing | No | Yes |
| `scarp --help` or `scarp -h` | Show usage | Yes | Yes |
| `scarp --version` or `scarp -V` | Print the package version | Yes | Yes |
| `scarp run --help` | Show usage before requiring a file | No | Yes |

The `tokens` and `check` commands also accept `--help` and `-h` after the
command name. Only one source path is accepted. There is no stdin input mode,
watch mode, import search-path option, or `scarp build` command.
`exec`, `verify`, `inspect`, and `compile` are not accepted commands;
proposed CLI designs do not change the shipped command contract.

v0.0.3 keeps these spellings and adds contextual guidance for run/check/tokens.
Run/check help shows budget placement, defaults, and iteration examples. Check
still evaluates selected loops and arithmetic; tokens does not execute code.

From a source checkout, place Cargo's separator before the Scarp arguments:

```sh
cargo run -- run examples/hello.srp
cargo run -- tokens examples/hello.srp
cargo run -- check examples/scopes.srp
```

`cargo build` builds the Rust interpreter. It does not compile a `.srp` file to a
native executable.

## What each file command validates

`run` validates the complete source graph before writing program output. Validation
includes names, scopes, mutability, primitive types, and typed function bodies.
Selected calls, defaults, contracts and arithmetic
are checked before any pending prints are written.

`check` uses the same preparation pass as execution, discarding each print value
immediately instead of retaining pending output. It evaluates declarations and assignments in fresh local storage, so
reachable unused initializers can still fail. Unselected conditional bodies do
not evaluate arithmetic, but they still undergo static checks. A successful check is silent. It does not
format or write the program's print values.

`tokens` only checks lexical validity. It can succeed when a file has missing
punctuation, unknown variables, or arithmetic failures. It never executes the
program or follows imports. Use `check` rather than a token listing to validate
development source.

## Paths and encoding

Since beta.1, `run` and `check` accept `--max-steps <count>` and `--max-prints <count>`
before the path. Defaults are 1,000,000 work steps and 100,000 selected prints.
Each option can occur once; unsigned decimal zero is allowed. Budget failures use
status 1 and leave output empty; malformed options use status 2. See
[loops and budgets](../proposals/0006-bounded-loops.md) for exact accounting.

Beta.3 also accepts `--max-call-depth`, `--max-calls`, `--max-live-slots`, and
`--max-capture-bytes`. They follow the same placement, decimal-count, duplicate,
separator, and usage-error rules as steps/prints. Alpha.3 adds
`--max-vector-bytes`; current `tokens` rejects all seven runtime options.
Defaults and exact accounting are in the [budget guide](../budgets.md).

The [budget guide](../budgets.md) provides copy-paste invocations and explains
zero limits, E0017/E0018, and embedding APIs.

Relative paths are resolved from the process working directory. Quote paths
containing spaces:

```sh
scarp run "my examples/hello.srp"
```

Use an explicit separator for a filename beginning with a dash:

```sh
scarp run -- -hello.srp
scarp tokens -- -hello.srp
scarp check -- -hello.srp
```

The last command requires v0.0.2 or later. The separator belongs after the
Scarp command; when using Cargo, that means `cargo run -- check -- -hello.srp`.

Paths use the operating system's native encoding. Source contents must be UTF-8.
A single leading UTF-8 byte order mark is accepted. LF and CRLF files work.
Missing files, directories used as files, unreadable files, and invalid UTF-8
contents produce read errors with a nonzero status. The entry file's `.srp`
extension is conventional, not required.

Starting in 0.0.5-alpha.1, `run` and `check` load imports relative to each
importing file and trust the entry file's parent directory. Import strings require
portable `/`-separated relative `.srp` paths; escaping the root, absolute paths,
backslashes and drive/URI prefixes are rejected. Canonical sources are loaded
once and cycles fail before execution. No network/search-path loading occurs.
Default source limits are 128 files, 32 import edges per path, 1 MiB per file,
8 MiB total, 1,024 imports and 8 MiB expansion text. These are fixed CLI defaults,
independent of the seven runtime flags; Rust hosts can configure `ModuleLimits`.
Read [modules](../modules.md) for exact semantics and filesystem trust assumptions.
Imported-file load/path failures use E0038/E0043 at the importing source span.

## Standard streams and status

| Result | stdout | stderr | Exit code |
| --- | --- | --- | --- |
| Successful run | Selected stdout output | Selected `eprint`/`ewrite` output | `0` |
| Successful check | Empty | Empty | `0` |
| Successful token listing | Tokens | Empty | `0` |
| Help or version | Requested information | Empty | `0` |
| Source, evaluation, or budget error | Empty | Diagnostic, without user-program stderr | `1` |
| File-read error | Empty | Read error | `1` |
| Output I/O error | Earlier bytes may remain | Earlier user bytes and/or an error if writable | `1` except broken pipe |
| Invalid command-line usage | Empty | Error and usage | `2` |

Output I/O failures can occur after some bytes have been written. The CLI treats
a broken output pipe as normal termination; library callers receive the output
error. Source and semantic errors do not produce partial program output.
An `eprint` message is user-program output, not automatically a failure:
inspect the exit code to distinguish it from a diagnostic. Successful `check`
suppresses both stdout and user-program stderr. Budget flags are rejected by
`tokens`; it performs no execution to budget.

In a POSIX shell, inspect the previous command's status with `echo $?`. In
PowerShell, inspect `$LASTEXITCODE`. Read stderr as well as the status when
diagnosing a failure.

## Token format

The hello example starts with this token:

```text
2:1..2:6  identifier(print)  [26..31]
```

The first range is one-based line/column information. The bracketed range is a
half-open UTF-8 byte span in the original source. String tokens show decoded
values using escaped debugging text. EOF is an empty span after all source
bytes, including trailing comments or whitespace.

Columns count Unicode scalar values rather than bytes or terminal cells. See
the [diagnostic reference](diagnostics.md) for caret interpretation and the
[troubleshooting guide](../troubleshooting.md) for version/path problems.

Development output statements can write to stdout or stderr; see the
[output guide](../output.md). Successful `check` stays silent on both streams.

## Published v0.0.6-alpha.1 symbol and reference inspection

Current main supports `inspect symbols [--json] FILE_OR_PROJECT_DIRECTORY` and
`inspect references [--json] FILE_OR_PROJECT_DIRECTORY`. Both are static complete-
graph inspections; runtime budgets do not apply. They accept the existing
`--diagnostic-format=json` error format and `--` filename separator. No top-level
`symbols` or `references` aliases are introduced. See the [navigation contract](../symbol-index.md)
for snapshot-local IDs, precise source positions, limits, schema-v1 fixtures,
implicit definitions and failure behavior. These commands are not in alpha.5.

## BigInt flags

Scarp v0.0.6-alpha.9 adds [BigInt](../bigint.md) flags `--max-bigint-digits` (default 10,000,
maximum 1,000,000) and `--max-bigint-bytes` (default 8,388,608). `exec`/`run` and
evaluating `check` accept them before the filename; static `verify` rejects
runtime flags. E0051 diagnoses malformed decimal text, E0052 digit/payload
quota exhaustion, and E0053 limb reservation failure. `explain` documents all
three codes. [BigInt integration PR #200](https://github.com/scarp-lang/scarp/pull/200)
