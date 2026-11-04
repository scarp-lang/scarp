# Command-line reference

## Scarp-first CLI (v0.0.5-alpha.4 published prerelease)

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
| `scarp inspect modules [--json] FILE_OR_DIRECTORY` | Inspect statically checked modules or an explicit project directory; JSON retains schema version 1 |
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
scarp inspect modules --json hello-world
scarp guide inspect modules
```

`exec` accepts exactly one source filename, with an optional `--` before a
dash-leading filename. **It does not forward program arguments**; project-mode
execution via `launch` is supported from alpha.4 using an explicit
`Scarp.project`; standalone `exec` does not forward program arguments. The
`inspect` command accepts `tokens`, `ast`, and `modules` in this branch;
`--json` is supported only for modules. Missing/unsupported topics and options
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
lexer and module compiler without changing the error schema. `inspect modules`
also accepts an explicit project directory, resolving its `Scarp.project` to
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

Since alpha.2, `scarp modules --json [--] <path>` emits schema-version-1 metadata with
file-local UTF-8 byte spans. `scarp explain E####` prints a code explanation without
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
