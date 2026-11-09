# Getting started

Use this guide to choose a Scarp version, build it, and confirm that the command
you are running matches the documentation. Start with
[compatibility](compatibility.md) if you already have a downloaded executable.

## Choose a version

| Goal | Choose | What to expect |
| --- | --- | --- |
| Use the latest **published** experimental prerelease | [v0.0.6-alpha.5](releases/v0.0.6-alpha.5-publication.md) | Deterministic Top-P, Pearson correlation and dependent ELF64 integer-kernel Rust API |
| Reproduce sampled calculus and cosine similarity milestone | [v0.0.6-alpha.4](releases/v0.0.6-alpha.4-publication.md) | Published earlier release with single-binding ELF64 object API |
| Reproduce Top-K and temperature softmax | v0.0.6-alpha.3 | Deterministic Top-K and private scalar IR |
| Follow the historical module milestone | v0.0.5-alpha.1 | Explicit imports, public/private functions and scalar constants |
| Reproduce the numerical milestone | v0.0.4-alpha.4 | Finite scalars, vectors, tiles and matrix operations |
| Reproduce the function beta | v0.0.3-beta.3 | Typed functions, contracts, bind/var, output streams and loops; no floats |
| Reproduce the iteration beta | v0.0.3-beta.1 | Original let/let mut and println, loops, budgets and escapes |
| Reproduce the variables milestone | v0.0.2 | Variables, scopes, strings, booleans and decisions |
| Reproduce the first interpreter | v0.0.1 | Printing and checked integer expressions |

As of 9 November 2026, `Cargo.toml` on `main` declares
`0.0.6-alpha.5`, which has been published as a GitHub experimental
prerelease from [the exact reviewed source](releases/v0.0.6-alpha.5-validation.md).
The [public tag and seven verified uploaded assets](https://github.com/scarp-lang/scarp/releases/tag/v0.0.6-alpha.5)
are separate from later maintenance commits on `main`.
A Cargo version alone does not prove source identity, and language syntax
and APIs are not stable. Alpha.5 capabilities are unavailable in earlier
alpha.4 downloads.

## Build the development checkout

Install Rust with Cargo using the instructions linked from the
[project README](../README.md). The declared minimum Rust version is 1.74;
the project also validates a current stable toolchain. Scarp has no third-party
Rust dependencies.

```sh
git clone https://github.com/scarp-lang/scarp.git
cd scarp
git switch main
cargo build --locked --offline
cargo run -- version
```

Expected version for the current alpha.5 checkout or published binary:

```text
scarp 0.0.6-alpha.5
```

Run commands from the repository root so relative example paths resolve:

```sh
cargo run -- exec examples/hello.srp
cargo run -- exec examples/string_bindings.srp
cargo run -- verify examples/scopes.srp
cargo run -- inspect tokens examples/arithmetic.srp
cargo run -- exec examples/module_demo.srp
```

`verify` performs static validation without running a program and is silent
on success. `inspect tokens` displays lexer tokens. The legacy `run`, `check`
and `tokens` commands remain temporarily available, but `check` is not static
`verify`. See the [CLI reference](reference/cli.md) for exit codes and paths.

## Create a source file

Save this as a UTF-8 file named `demo.srp`:

```scarp
print("Hello from Scarp!");
print(1 + 2 * 3);
```

Output:

```text
Hello from Scarp!
7
```

Execute it with `cargo run -- exec demo.srp`. For v0.0.1 through beta.1, use
`println` instead of `print`; those releases do not recognize `print`.
The `.srp` suffix is conventional; the CLI validates file contents rather than
requiring a particular extension.

## Use an installed command

From the selected checkout, `cargo install --path . --locked` installs `scarp`
through Cargo. Verify the result with `scarp version`. If several copies are on
your `PATH`, the first matching command may be an older release. During source
development, `cargo run -- ...` selects the current checkout explicitly.

Downloaded release archives already contain an executable and do not require
Rust for running programs. For the latest **published** prerelease, consult the
[v0.0.6-alpha.5 verification record](releases/v0.0.6-alpha.5-validation.md).
Select Windows x64 MSVC,
Linux x64 GNU, or Apple Silicon macOS to match the host. Verify the archive's
SHA-256 against the sidecar from that **same release** before extracting. CI
candidate artifacts are not public release downloads. Keep the supplied
`examples/` and `docs/` directories, and run commands from the extracted root.
Use `./scarp.exe` on Windows or `./scarp` on Unix when the directory is not on
`PATH`. A source checkout includes the Rust sources and tests; a binary archive
contains the runnable interpreter, documentation, and examples.

Next, follow [your first program](tutorials/first-program.md), then the
[variable tutorial](tutorials/variables.md) on main. Older packages use the
original syntax described in their release notes.
