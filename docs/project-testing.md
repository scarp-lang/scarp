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
assert(math.add(2, 3) == 5, "math.add must produce 5");
```

The native `assert` statement belongs to the **unreleased, stacked assertion
development branch**, not the published compiler. Its
[syntax and runtime behavior](assertions.md) use the real Scarp parser,
type checker and interpreter. Earlier project-test-runner branches do not
yet support this assertion form; there, use a standalone test source and
check its exit status.

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

The test-reporting implementation adds two optional flags:

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
compiler/runtime diagnostic text may still include an excerpt of the
failing source line, including string literals written in that source.
This is not a secrecy boundary for source code.
per-test runtime/compile failures return status 1 but still produce a complete
result object. An empty suite reports discovered=0 and returns status 0;
CI users who require tests must explicitly check this count.

Unsafe project/test paths, malformed manifests, missing tests directories
and invalid CLI arguments produce an error on stderr and **no JSON stdout**.
The `--json` flag does not make these setup errors machine-readable yet.
The normal human-readable `scarp test` format is unchanged.

The assertion statement shown in the example is implemented in this combined
unreleased development candidate, not in the published compiler. Custom
execution-budget flags, retries, package installation and releases are not
introduced by the test-reporting layer.

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

## Filename substring selection (separate unreleased stacked development PR)

An optional filter lets a developer run or list only tests whose **filename**
(including the `.srp` suffix) contains a literal, case-sensitive UTF-8
substring. This is not a glob, regex or path selector:

```sh
scarp test --filter math calculator
scarp test --filter हिंदी --list calculator
scarp test --json --require-tests --filter math calculator
```

The filter is 1–128 UTF-8 bytes and cannot begin with `-` or contain
slashes, backslashes, colons or control characters. It is a single CLI
argument following `--filter`; a missing, empty, malformed or duplicate
filter is a usage error (exit status 2). The filter can also occur after
the directory argument, before any `--` separator.

All direct `tests/*.srp` entries are checked for symlinks, safe filenames
and the 64-test maximum **before** filtering. Nonmatching sources are not
compiled, executed or included in the report. Both human-readable
`--list` and schema-version-1 JSON list/run results count selected tests
only, in the same lexicographical order. A valid filter matching nothing
returns the existing zero-test report; with `--require-tests`, its status
is 1. Without `--filter`, behavior and JSON schema remain unchanged.

This filter is not in published alpha.7 and is kept separate from the
original CI-qualified alpha.8 integration candidate.

## Optional per-test stdin fixtures (separate unreleased stacked development PR)

Scarp already has the `read_line()` intrinsic and a bounded
`execute_with_input` embedding API. In the development-only test runner, a
matching sibling fixture now supplies input to an individual test:

```text
calculator/
  tests/
    greeting.srp
    greeting.stdin
    arithmetic.srp
```

`tests/greeting.srp`:

```scarp
bind name = read_line();
assert(name == "नमस्ते", "input fixture must match");
```

`tests/greeting.stdin` contains `नमस्ते` followed by an LF newline.
Running `scarp test calculator` makes the fixture available **only** to
`greeting.srp`, not to `arithmetic.srp`. A test with no fixture retains the
existing no-input behavior: calling `read_line()` raises the compiler's
normal input-unavailable diagnostic. Test processes never consume shell stdin.

- Fixtures are optional `tests/NAME.stdin` companions to immediate
  `tests/NAME.srp` files, with the same exact filename stem.
- Each fixture must be an ordinary, contained file, not a symlink, directory,
  or file outside the canonical project root. Bad fixtures fail their selected
  test with a bounded diagnostic and preserve the complete JSON report.
- Fixture size is limited to **1 MiB**, including line terminators; fixture
  growth is checked during bounded reads. The interpreter independently
  enforces its standard 64 KiB per-line, 1 MiB cumulative and 4096-read limits,
  plus existing execution, text and allocation budgets.
- Input bytes are interpreted by the existing Scarp `read_line()` behavior;
  LF/CRLF, UTF-8 validation, EOF and all other runtime errors are unchanged.
- `scarp test --list` remains read-only and does **not** open or inspect
  `.stdin` files. Filename filtering examines and validates all `.srp`
  entries first; only selected tests read their matching input fixtures.
- `--json`, `--filter`, `--require-tests` and schema-version-1 reports
  preserve their existing semantics. Fixture contents are never emitted as
  program output into trusted PASS/FAIL or structured JSON streams. Diagnostics
  may still contain source-line excerpts.

This capability is **not** in published alpha.7 or the separately qualified
alpha.8 project integration PR #225. It requires review and integration of its
stacked dependencies before any release.

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
- Process stdin is never passed to tests; only an explicit validated sibling
  `.stdin` fixture can supply input. Ordinary program output is captured rather
  than mixed with the trusted PASS/FAIL result stream.
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
