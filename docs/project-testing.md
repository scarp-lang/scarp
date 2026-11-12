# Project test runner (unreleased development)

This page describes the development-only **`scarp test`** command in the
stacked test-runner branch. The published Scarp compiler does not include
this new command. It depends on the unmerged declarative-project foundation
PR #217; neither change is a release or pre-release.

## Usage

```sh
scarp init calculator
# Create one or more .srp files inside calculator/tests/
scarp test calculator
cd calculator
scarp test
```

The command validates the project's `scarp.project` manifest (or legacy
`Scarp.project`) and its configured entrypoint first. It then enumerates
direct `.srp` files under `tests/`, sorts their UTF-8 filenames
lexicographically and compiles/executes each test with the real module
resolver and interpreter. Test files in nested directories are not run.
A legacy v1 project may create its own `tests/` directory.

Each test is **an ordinary, standalone Scarp program**, not an invented
test-specific language or a Rust unit test. A successful execution is a pass;
a compile-time or runtime diagnostic is a failure. A passing test can import
source modules within the same trusted project root, including imports from
`tests/` into `src/`.

Example files:

```text
calculator/
  scarp.project
  src/
    main.srp
    math.srp
  tests/
    addition.srp
    boundary.srp
```

`src/math.srp`:

```scarp
pub pure funcra add(a: integer, b: integer) -> integer {
    return a + b;
}
```

`tests/addition.srp`:

```scarp
import "../src/math.srp" as math;
if math.add(2, 3) != 5 {
    print(1 / 0);
}
```

The intentional error in the failure branch is a temporary assertion
pattern, **not** a new `assert` built-in. This syntax and imported-function
execution use existing Scarp capabilities. A future assertion API deserves
its own documented semantics and dedicated tests.

Example status:

```text
PASS tests/addition.srp
PASS tests/boundary.srp
2 passed, 0 failed
```

When a test fails, its line is reported as `FAIL` and a bounded diagnostic
is written to stderr. Failure status is exit code 1. Invalid CLI arguments
return exit code 2. No tests found reports an explicit zero-test message and
returns success; CI consumers should check this message if a nonempty suite
is required. A missing `tests/` directory is an error.

## Limits and security

- Discovery is nonrecursive and limited to 64 immediate `.srp` files.
- The `tests/` directory must be a real directory, not a symlink. Each
  discovered `.srp` must be a regular file, not a symlink or directory.
- Module loads use the same project-root containment checks as the existing
  source loader. Out-of-root imports are rejected.
- Every test is subject to the interpreter's existing deterministic default
  execution, text, vector and captured-output budgets.
- No subprocess, registry, install hook or network lookup is introduced.
- Tests **do execute Scarp code** and may perform any capabilities available
  to that code. Only run test files you trust.
- Interactive stdin is not passed to test programs. Ordinary program output
  is captured rather than mixed with the trusted PASS/FAIL result stream.
- No test-specific per-case timeout or custom CLI budget flags are available
  yet; these must be added in a separate reviewed change.

## Compatibility

The `scarp run FILE.srp`, `scarp exec FILE.srp`,
`scarp launch DIRECTORY` and `scarp verify DIRECTORY` commands
remain unchanged by this test runner. Existing `Scarp.project` files
are supported without conversion. No automated project discovery in
ancestor directories is performed.

The new command must pass Rust, Windows, Linux and macOS CI before review
and cannot be described as published until a separately authorized release.
