# Scarp

**Post-alpha.5 development branch:** the [bounded BigInt milestone](docs/bigint.md)
adds a separate exact `bigint` type with typed constructors, arithmetic and
per-run work/digit/allocation quotas. It is not in published alpha.4 or the frozen
alpha.5 source candidate. No version/tag or publication request is changed.

**0.0.6-alpha.5** is an **unpublished source candidate** with [Top-P nucleus ranking](docs/nucleus-selection.md), [Pearson correlation](docs/pearson-correlation.md), and experimental [multi-binding ELF64 integer kernels](docs/native-elf64.md). These features are **not** in published alpha.4 binaries. See [alpha.5 candidate notes](docs/releases/v0.0.6-alpha.5.md) and [release gates](docs/releases/v0.0.6-alpha.5-development-plan.md). No alpha.5 source approval or publication is implied.

**0.0.6-alpha.4** is a **published experimental GitHub prerelease**
(8 November 2026) from exact source
`f04be0758cbce405ddcdb229699b0ff83c09d207`.
The latest published prerelease is
[0.0.6-alpha.4](https://github.com/scarp-lang/scarp/releases/tag/v0.0.6-alpha.4),
with verified Windows x64, Linux x64, and Apple Silicon archives.
See the [public release notes](docs/releases/v0.0.6-alpha.4-publication.md),
[publication verification](docs/releases/v0.0.6-alpha.4-validation.md),
and historical [candidate snapshot](docs/releases/v0.0.6-alpha.4.md).
The next `v0.0.6-alpha.5` candidate is in development and must receive
a **different source-specific approval** before publication.

**0.0.6-alpha.3** is an **earlier published GitHub prerelease** (7 November
2026). It adds pure deterministic [Top-K ranking](docs/topk-selection.md),
[temperature-controlled softmax](docs/temperature-softmax.md), and
private scalar IR compiler verification. Download verified Windows x64, Linux
x64 and macOS Apple Silicon archives from the
[public release](https://github.com/scarp-lang/scarp/releases/tag/v0.0.6-alpha.3).
See [public release notes](docs/releases/v0.0.6-alpha.3-publication.md)
and the [exact-source verification record](docs/releases/v0.0.6-alpha.3-validation.md).
The [candidate notes](docs/releases/v0.0.6-alpha.3.md) remain a historical
prepublication snapshot; this is not a stable or general native compiler release.

**Build Beyond Boundaries.**

**Published in v0.0.6-alpha.4:** [sampled calculus](docs/sampled-calculus.md)
adds bounded `sample_area(x, y)`, `sample_slopes(x, y)` and
`sample_cumulative_area(x, y)` to current `main` via
[PR #153](https://github.com/scarp-lang/scarp/pull/153) and
[PR #155](https://github.com/scarp-lang/scarp/pull/155).
These kernels passed Linux, Windows, macOS and Rust 1.74 validation.
They were **not** in published v0.0.6-alpha.3 binaries; install the
v0.0.6-alpha.4 prerelease or build that immutable source tag.

**Also published in v0.0.6-alpha.4:** [cosine similarity](docs/cosine-similarity.md)
(#157) and an experimental [safe-Rust ELF64 integer-kernel writer](docs/native-elf64.md)
(#158) on current source. The ELF writer supports only a pure integer initializer
and returns relocatable object bytes; it does not produce a general Scarp
executable or introduce a public native compiler CLI.


**0.0.6-alpha.2** is an **earlier published GitHub prerelease** (6 November
2026), available for Windows x64, Linux x64 and macOS Apple Silicon. It adds
optional dependency invalidation, bounded semantic/token/AST caches, and
scoped cache eviction for Rust embedding hosts. See the
[public alpha.2 release](https://github.com/scarp-lang/scarp/releases/tag/v0.0.6-alpha.2),
[source-scoped cache contract](docs/development/cache-eviction.md), and
[publication verification](docs/releases/v0.0.6-alpha.2-validation.md).
The interpreter-backed CLI remains experimental and general native
executable compilation is not included.

**0.0.6-alpha.1** is an **earlier published GitHub prerelease** (5 November
2026), adding bounded cross-file symbol/reference navigation and finite root
mean square. See [v0.0.6-alpha.1 release notes](docs/releases/v0.0.6-alpha.1.md),
[publication verification](docs/releases/v0.0.6-alpha.1-validation.md),
[symbol APIs and JSON](docs/symbol-index.md) and [RMS](docs/root-mean-square.md).
This is not a stable language or native compiler release.

**0.0.5-alpha.5** is an [earlier published GitHub prerelease](https://github.com/scarp-lang/scarp/releases/tag/v0.0.5-alpha.5)
for Windows x64, Linux x64 and macOS Apple Silicon. It adds project-aware
module inspection and an opt-in experimental C11 scalar emitter for Rust hosts.
See [alpha.5 release notes](docs/releases/v0.0.5-alpha.5.md) and the
[publication record](docs/releases/v0.0.5-alpha.5-validation.md).

Scarp is an experimental programming language implemented in Rust. The latest
published **0.0.6-alpha.4** prerelease was published on 8 November 2026 from
exact source `f04be0758cbce405ddcdb229699b0ff83c09d207`. The compiler provides a lexer,
parser, typed resolver, interpreter, source modules, finite binary64 numerics
and a Scarp-first CLI. The bounded `create`, `launch` and project-directory
`verify` workflow remains available. Syntax and APIs are not stable.

[Static module inspection](docs/module-inspection.md) lists imports, dependencies
and typed public/private declarations without executing runtime statements.
Alpha.2 added the historical `modules` command; alpha.3 also supports
`scarp inspect modules`. See the CLI reference for version-specific spellings.

The checkout also adds [compile-time scalar constant expressions](docs/constant-expressions.md)
with checked arithmetic/logic, earlier/imported constant references and separate
compiler work limits. Try `cargo run -- run examples/constant_expressions.srp`.

Optional [declaration annotations](docs/variables.md) accept forms such as
`bind count: integer = 42;`, with exact type checking and required initialization.
The [feature matrix](docs/development/feature-matrix.md) records current gaps and
the implementation roadmap; historical native commands remain unavailable on main.

[Scalar and text built-ins](docs/scalar-text.md) provide exact numeric
conversions and budgeted Unicode length, codepoint and search queries.

[Developer inspection](docs/module-json.md) adds `modules --json`, a streaming
metadata API, `explain E####` and correctness-checked compiler/JSON benchmarks.

Use immutable `bind` and mutable `var` declarations, checked integers, finite floats, strings,
booleans, conditionals, while/repeat loops, and explicit output streams.
[Typed `funcra` functions](docs/functions.md) support named/default arguments,
recursion, unit returns, purity, contracts, and bounded heap call frames.
See the [migration guide](docs/syntax-migration.md) for older declaration syntax.

```scarp
// My first Scarp program
print("Hello from Scarp!");
```

```text
Hello from Scarp!
```

## Scarp-first CLI (v0.0.5-alpha.5 prerelease)

The alpha.5 prerelease provides `exec`, `verify FILE`, `inspect`, `guide`,
`diagnose`, and project commands `create`, `launch`, and `verify DIRECTORY`.
Alpha.3 provides the Scarp-first file commands, but not the project commands.
Older commands remain temporarily compatible. After building the intended source, use `scarp`
directly (not the Rust build tool):

```sh
scarp guide
scarp version
scarp diagnose --json
# Project commands require v0.0.5-alpha.4 or later:
scarp create hello-world
scarp launch hello-world
scarp verify hello-world
scarp inspect modules --json hello-world
scarp exec examples/hello.srp
scarp verify examples/hello.srp
scarp verify --diagnostic-format=json examples/hello.srp
scarp inspect tokens examples/hello.srp
scarp inspect ast examples/hello.srp
scarp inspect modules --json examples/module_inspection.srp
```

The `inspect ast` route prints the parser's development syntax tree
without execution, import loading or type checking. Its readable debug layout
is not a stable machine-readable schema.

The [project workflow](docs/project-workflow.md) creates a bounded `Scarp.project`
manifest and a runnable starter, verifies a project directory without running
statements, then launches via the real interpreter. This is available in the published alpha.4 packages, not in alpha.3
packages, and does not generate a native executable.

**New in published alpha.5:** `scarp inspect modules --json DIRECTORY`
resolves an explicit `Scarp.project` and inspects its checked module graph with
the same schema-v1 inventory as the contained entry file. It does not execute
program statements. This was not available in alpha.4 packages.

**Experimental alpha.5 Rust embedding feature:** The optional Rust library
`scarp::c_backend::emit_c` can generate freestanding C11 source for a tightly
restricted checked integer/Boolean subset, including bounded counted-repeat
loops. See [Proposal 0035](docs/proposals/0035-native-scalar-c11.md)
for accepted syntax, validation, output contracts and deliberate exclusions.
There is still **no** `scarp compile` command or native executable generation from arbitrary `.srp` sources.

These routes preserve the current compiler's schema-version-1
`--diagnostic-format=json` errors without executing unsupported features.
See the [CLI reference](docs/reference/cli.md) for supported options, exact
behavior and migration details. The old `run`, `check`, `tokens` and `modules`
paths remain temporarily compatible; `check` evaluates selected program paths
and should **not** be described as static-only verification. Static-only `verify` uses compiler parsing and resolution without executing runtime
statements or contracts; it does not replace the old evaluating `check`.
`compile` and native executable generation remain unsupported. `create` and
`launch` are available in published alpha.4 packages, but not in
alpha.3 packages. Alpha.1 and alpha.2 lack the Scarp-first command names.

The [documentation hub](docs/README.md) provides setup, tutorials, command and
diagnostic references, Rust embedding, testing, troubleshooting, and compatibility
guides. Start there for a reading path matched to your task.

[Source modules](docs/modules.md) organize programs with relative `.srp` imports,
explicit aliases and public functions/scalar constants. Follow the
[import tutorial](docs/tutorials/modules.md), then run the three-file example:

```sh
cargo run --locked --offline -- run examples/module_demo.srp
cargo run --locked --offline -- check examples/module_demo.srp
```

The [tile and linear-algebra guide](docs/tiles.md) covers immutable two-axis
values, transpose, matrix-vector/matrix products and tile arithmetic. Shared
vector windows keep their visible-slice boundaries through all operations.

```scarp
bind weights = tile([1.0, 2.0, -1.0, 3.0], 2, 2);
print(matvec(weights, [2.0, 1.0])); // [4.0, 1.0]
```

[Automatic value retirement](docs/memory-lifetimes.md) releases inaccessible
scope values while preserving returned values, contracts and output snapshots.
[Independent runtime budgets](docs/budgets.md) bound work, calls, slots, snapshots
and cumulative new numerical payload. These do not measure total process memory.

Existing numerical capabilities include compensated vector reductions, scaled
norms, softmax, log-space operations, deterministic selection, categorical losses,
one-hot targets and explicit loss gradients. See [vectors](docs/vectors.md),
[the numerical tutorial](docs/tutorials/numerical-program.md), and
[error repairs](docs/reference/numerical-errors.md).

The earlier published prerelease was
[0.0.6-alpha.3](https://github.com/scarp-lang/scarp/releases/tag/v0.0.6-alpha.3),
published 7 November 2026 from source commit
`ceff07d9014f466974e0c9e02fc5d23573f55777`. It provides Windows x64,
Linux x64 and macOS Apple Silicon archives with SHA-256 checksums.
See the [public release notes](docs/releases/v0.0.6-alpha.3-publication.md),
[publication verification record](docs/releases/v0.0.6-alpha.3-validation.md)
and [release engineering](docs/releasing.md) for publication gates.
Earlier prerelease records remain historical snapshots.

The maintainer transferred this repository to **scarp-lang/scarp**. The
[migration record](docs/migrations/scarp-lang-Scarp.md) documents identity,
history, release/download verification and updated clone instructions.

## Source navigation in published v0.0.6-alpha.1

Published v0.0.6-alpha.1 adds [symbol/reference navigation](docs/symbol-index.md) through
`scarp inspect symbols --json FILE` and `scarp inspect references --json FILE`
(or an explicit project directory). The Rust snapshot API provides go-to-definition
and find-references without running statements, defaults or contracts. It retains
cross-file aliases, folded constant uses and lexical shadowing under independent
index limits. Try `examples/symbol_navigation.srp` and `cargo run --example symbol_index`.
These commands are absent from published alpha.5 binaries.

## Build and run

Binary archives and checksums are available through
[GitHub Releases](https://github.com/scarp-lang/scarp/releases). See the
[latest v0.0.6-alpha.3 prerelease](https://github.com/scarp-lang/scarp/releases/tag/v0.0.6-alpha.3)
for its supported archives, checksums and source commit. The
[changelog](CHANGELOG.md) records recent language and tooling changes.

To build from source:

Install a stable Rust toolchain with Cargo using the instructions at
[rustup.rs](https://rustup.rs/). On Windows, use the MSVC toolchain and install the
C++ build tools if the Rust installer requests them.

```sh
git clone https://github.com/scarp-lang/scarp.git
cd scarp
git switch main
cargo build --locked
cargo run -- run examples/hello.srp
cargo run -- run examples/arithmetic.srp
cargo run -- tokens examples/hello.srp
cargo run -- check examples/scopes.srp
cargo run -- --help
cargo run -- --version
```

The project has no third-party Rust dependencies. After installing Rust, the
prototype can be built and tested offline with `cargo build --offline` and
`cargo test --offline`.

To install the command from this checkout:

```sh
cargo install --path . --locked
scarp run examples/hello.srp
scarp tokens examples/hello.srp
```

Filenames with spaces should be quoted. Use `scarp run -- -example.srp` for a
filename beginning with a dash. Source paths use the platform's native path
encoding; source contents must be UTF-8. The `.srp` suffix is conventional, and
the CLI does not reject other filename extensions.

## What works now

- `print("text");` statements, executed in source order.
- Multiple statements, empty strings, and empty or comment-only files.
- Signed 64-bit integers with `+`, `-`, `*`, `/`, unary minus, and parentheses.
- Operator precedence, left-associative binary operators, and checked arithmetic.
- Immutable integer/string/boolean bindings with `bind name = expression;` and resolved names.
- Explicit `var` bindings, assignment, and checked compound assignment.
- Lexical block scopes, shadowing, and nearest-binding assignment.
- Inferred primitive types, checked same-type assignment, and string value snapshots.
- Boolean comparisons and short-circuit `and`, `or`, and `not` expressions.
- Scoped `if`/`elif`/`else` statements with explicit boolean conditions.
- `while` and counted `repeat` loops, nearest-loop `break`/`continue`.
- Typed file-level `funcra`, positional/named/default arguments, forward and
  mutual recursion, explicit returns, verified purity, and executable contracts.
- Relative source modules, public/private functions and scalar constants,
  explicit filesystem/in-memory loaders, cycle diagnostics and bounded graphs.
- Heap call frames and seven deterministic CLI/embedding resource limits shared
  by `run` and `check`; see [budgets](docs/budgets.md).
- Unicode text in strings and comments, Unicode whitespace, and LF or CRLF files.
- Optional UTF-8 byte order mark at the very start of a file.
- `//` comments to the end of the line, and flexible whitespace between tokens.
- Escapes `\\` (backslash), `\"` (double quote), `\n` (LF), `\t` (tab), and `\r` (CR).
- `run`, `check`, `tokens`, `--help`/`-h`, and `--version`/`-V` commands.
- Syntax errors with a code, filename, line, column, source excerpt, and help.

`print` appends a newline; `write` omits it. `println` remains a supported alias.
Both accept zero or more comma-separated values. `eprint` and `ewrite` use stderr.
See the [output guide](docs/output.md) for exact bytes and embedding APIs. Physical line breaks inside a string are rejected;
use `\n` instead. Identifiers use ASCII letters, digits, and underscores, cannot
start with a digit, and are case-sensitive. Statements include declarations,
assignments, blocks, `if`/`elif`/`else` decisions, and `print` calls. Escapes other than the five listed
above are rejected.

The entire file is lexed, parsed, resolved, checked for primitive type consistency,
and its selected execution path is evaluated before any program output is written. Source and semantic errors
therefore produce no program output.
Output stream failures propagate as errors; an output
failure can occur after earlier statements have been written. A closed output
pipe is treated as normal CLI termination.

```scarp
print(1 + 2 * 3);           // 7
print((1 + 2) * 3);         // 9
print(-7 / 2);              // -3, division truncates toward zero
print(-9223372036854775808); // the minimum signed 64-bit integer
```

Integer overflow, division by zero, and out-of-range literals are errors in both
debug and release builds. Grouping is limited to 64 nested parentheses; long
binary chains use a flat syntax tree. Strings can be bound, copied, reassigned
with the same type, and printed. Concatenation and mixed string/integer arithmetic
are unsupported.

Booleans use `true` and `false`. Comparisons support same-type `==` and `!=`,
and same-type numeric `<`, `<=`, `>`, `>=`. Readable `not`, `and`, and `or` operators require
booleans; `and` and `or` short-circuit at runtime while all operands are checked
for names and types during compilation. See the
[boolean design](docs/proposals/0004-boolean-expressions.md).
Conditional statements select the first true `if`/`elif` body, or the optional
`else` body. Every condition and body undergoes static checks; arithmetic executes
only on the selected path. Start with the [decisions tutorial](docs/tutorials/decisions.md).

`check` validates syntax, names, mutability, scopes, primitive types, and arithmetic
without writing program output. It returns zero and remains silent on success.
It evaluates selected loops, declarations, and assignments in fresh local
storage, so reachable unused arithmetic failures are still reported. The Rust
library's `scarp::compile` only parses and resolves; it does not evaluate arithmetic.
There is no `scarp compile` CLI command.

`tokens` checks lexical validity without parsing or executing the file. It prints
one token per line, including decoded string text, one-based line/column ranges,
and half-open UTF-8 byte ranges. For the hello example, the first token is:

```text
2:1..2:6  identifier(print)  [26..31]
```

An end-of-file token is always included. Columns count Unicode scalar values;
terminal display widths for combining marks and wide characters can differ.
Tabs are expanded to four spaces in error excerpts.

```text
error[E0004]: expected ';', found <eof>
 --> example.srp:1:15
  |
1 | print("hello")
  |               ^
 = help: use print("text"); or print(1 + 2);
```

Exit codes are `0` for success, `1` for source, file, or output errors, and `2` for
invalid command-line usage. Diagnostics go to standard error; program output and
token listings go to standard output.

## Library

The `scarp` library exposes `lexer::lex`, `parser::parse`, `compile`,
`evaluator::evaluate`, `interpreter::execute`, and `run`. `parse` exposes syntax;
`compile` validates syntax and resolves names into an immutable `ResolvedProgram`.
`check` and `interpreter::validate` also evaluate without an output destination.
`evaluator::evaluate` preserves an integer result, while `evaluate_value` returns
an integer/float/string/boolean `Value`. `execute` and `run` validate all
arguments before writing and return `RunError`. The output destination is explicit, making
execution usable without redirecting process-wide standard output.

For multiple files, use `modules::compile_file` or an explicit
`modules::SourceLoader` with `modules::compile_with_loader`. `ModuleProgram`
owns its source map and offers separate check/execute methods. Source-string
APIs keep their existing signatures and never load files implicitly. See
[module embedding](docs/embedding.md#load-modules-explicitly).

```rust
let mut output = Vec::new();
scarp::run(r#"print("Hello from Scarp!");"#, &mut output).unwrap();
assert_eq!(output, b"Hello from Scarp!\n");
```

## Development checks

```sh
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo test --locked --release
cargo build --locked
cargo run --locked -- run examples/hello.srp
cargo run --locked -- tokens examples/hello.srp
```

Tests cover source spans, escapes, Unicode, missing punctuation, invalid files,
command-line usage, output failures, arithmetic boundaries, precedence, nesting,
long expression chains, and all shipped examples.
The complete check sequence, including rustdoc, is available as
`sh scripts/check.sh --offline` or `./scripts/check.ps1 -Offline`. See
[CONTRIBUTING.md](CONTRIBUTING.md) for the development and PR workflow.

## Prototype limits and planned roadmap

While loops and `break`/`continue` support iterative computations with explicit
per-call budgets. See [loops and limits](docs/proposals/0006-bounded-loops.md).

The [performance guide](docs/performance.md) provides stage benchmarks and
measured execution improvements. C/C++/Rust performance comparisons and native
code generation remain future work.

This is a small interpreter. It loads bounded source graphs into memory
and reports the first error. Functions are interpreted using heap call frames.
It has immutable numerical vectors and tiles and a first source-module system.
General-purpose collections, user-defined types, runtime module globals,
re-exports, a standard library, a package manager, native code generation and
self-hosting require further milestones. It does not execute arbitrary Rust code.
The syntax and library API are experimental and may change.

Read the [implemented language reference](docs/language.md) and
[architecture notes](docs/architecture.md) for precise rules. Runnable programs
in [`examples/`](examples/) are paired with `.stdout` files and checked by the
test suite.

Future work is **planned**, in this order:

1. Module tooling, additional conformance and source navigation.
2. Explicit constant-expression and library organization design.
3. User-defined types and callable values.
4. A compiler backend and developer tooling.
5. Domain-specific libraries.

The remaining capabilities require separate milestones. Integer expressions are implemented as
specified in the [expression design](docs/proposals/0001-integer-expressions.md).
Integer, finite float, vector, string, and boolean bindings are implemented in this checkout. Read the
[variable reference](docs/variables.md) and [staged variable design](docs/proposals/0002-variables.md).
The [preparation notes](docs/releases/v0.0.5-alpha.1.md) preserve the original
module contract; the [publication record](docs/releases/v0.0.5-alpha.1-validation.md)
confirms the released source and checksums. Tensors and
native backends require separate integration decisions.

The [function integration in PR #41](https://github.com/scarp-lang/scarp/pull/41)
integrated typed calls, defaults, contracts, and bounded heap frames into main.
Beta.3 published that work with CLI resource controls. Beta.2 binaries do not
contain functions. Release notes
describe immutable snapshots; branch names can move.

## License

Original project code is licensed under the [Apache License 2.0](LICENSE).
