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

## Discovery and structured reports (unreleased stacked PR)

The additional test-reporting branch adds two optional flags:

```sh
scarp test --list calculator        # Read-only test discovery
scarp test --json calculator        # Execute and return JSON
scarp test --json --list calculator # Read-only discovery as JSON
```

`--list` inspects the same validated manifest, entrypoint and immediate
`tests/*.srp` files as a normal test run, including symlink and 64-test
limits. It **does not compile or execute any Scarp test source**. Plain-text
listing prints the sorted relative filenames followed by the test count.
For empty suites it prints an explicit zero-discovered message.

`--json` produces one JSON object on stdout, followed by a newline.
Schema-version-1 example for a successful and failed test:

```json
{
  "schema_version": 1,
  "mode": "run",
  "discovered": 2,
  "passed": 1,
  "failed": 1,
  "tests": [
    { "path": "tests/add.srp", "status": "passed" },
    { "path": "tests/error.srp", "status": "failed", "error": "bounded compiler or runtime diagnostic" }
  ]
}
```

The mode is `"run"` or `"list"`. Listing entries have status
`"discovered"` and counts of zero passed/failed. JSON paths are project-root
relative and properly escaped. When a test fails, its diagnostic is truncated
to at most 1600 Unicode scalar values and stored in its `error` field.
The test program's own output is captured, **not inserted into JSON**;
per-test runtime/compile failures return status 1 but still produce a complete
result object. An empty suite reports discovered=0 and returns status 0;
CI users who require tests must explicitly check this count.

Unsafe project/test paths, malformed manifests, missing tests directories
and invalid CLI arguments produce an error on stderr and **no JSON stdout**.
The `--json` flag does not make these setup errors machine-readable yet.
The normal human-readable `scarp test` format is unchanged.

This development work does not implement an assertion language construct,
custom execution-budget flags, retries, package installation, or a release.

## CI guard: require at least one test (unreleased stacked PR)

The optional `--require-tests` flag makes a valid but empty suite fail.
It prevents a new project from receiving a green CI status before any
test programs have been written.

```sh
scarp test --require-tests calculator
scarp test --require-tests --json calculator
scarp test --require-tests --list calculator
```

When `tests/` contains zero immediate `.srp` files, this flag returns
exit status **1**, in both execution and read-only listing modes.
The usual zero-test human-readable message or complete JSON report is
still written to stdout; diagnostics and setup errors remain on stderr.
A nonempty suite retains normal pass/fail exit statuses. This option
is safe to combine with `--json` or `--list`, and duplicate flags are
invalid usage.

The default `scarp test` remains backward compatible: an empty,
well-formed suite still reports zero tests and exits successfully.
The option does not change filename ordering, symlink restrictions,
source execution behavior, discovery limits, or the JSON v1 schema.
A missing `tests/` directory is still a separate setup error.

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
