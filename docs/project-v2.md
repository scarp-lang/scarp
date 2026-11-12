# Declarative Scarp projects (development branch; not released)

> Status: implemented in the associated development branch, pending full CI
> review. This document does **not** describe a published compiler release.

Scarp's new project manifest is **`scarp.project`**, a compact,
declarative format designed for Scarp. It is not TOML, JSON, Rust code, or
executable Scarp source. Reading a manifest never evaluates source code,
opens a network connection, runs dependency scripts, or installs packages.

## Quick start

```sh
scarp init hello
scarp verify hello
scarp run hello
```

The generated directory contains:

```text
hello/
├── scarp.project
├── src/
│   └── main.srp
├── tests/
├── examples/
├── README.md
└── .gitignore
```

`init` requires a *new* directory, and refuses to overwrite even an empty
existing directory. It cannot initialize the current directory in this
initial implementation. The parent directory must already exist. Successful
initialization produces a source file compiled by the actual interpreter,
not example pseudocode.

## Manifest example

```text
project "hello" {
    version: "0.1.0"
    entry: "src/main.srp"
    type: "application"
    dependencies {}
}
```

## Grammar (v2)

The following EBNF is normative for this development format:

```ebnf
manifest      = "project", string, "{", { field }, "}", EOF ;
field         = "version", ":", string
              | "entry", ":", string
              | "type", ":", string
              | "dependencies", "{", "}" ;
string        = '"', { unicode-character | '\"' | '\\' }, '"' ;
```

The lexer ignores Unicode-independent ASCII whitespace and line comments
beginning with `//` outside strings. Unescaped newlines inside strings are
illegal. The two supported escape sequences are quote (`\"`) and backslash
(`\\`); no variables, interpolation, environment substitution, expression
evaluation, or arbitrary user-defined grammar rules are available.

The `version`, `entry`, and `type` fields must occur exactly once each.
Their ordering is not significant. `dependencies {}` is optional; a
nonempty dependencies block is currently rejected with an explicit error.
Other fields and duplicate fields are errors, not silently ignored.

### Field validation

- Project name: 1–96 UTF-8 bytes, no leading/trailing whitespace,
  control characters, colon, or path separator.
- Version: semantic version `MAJOR.MINOR.PATCH`, with optional
  `-PRERELEASE` and `+BUILD` identifiers; canonical nonnegative
  components, no leading zeroes in numeric core/prerelease identifiers.
- Entry: a nonempty relative path of at most 512 UTF-8 bytes ending in
  `.srp`; forward slashes only, no empty, `.` or `..` components,
  drive prefixes, backslashes, colon, or control characters.
- Type: currently the literal `application`.
- Maximum manifest size: 4096 bytes; UTF-8 required.

The resolver canonicalizes the project directory and entry path, rejects
manifest symlinks, and rejects an entry symlink that resolves outside the
project root. It does not search ancestor directories or silently adopt
an outside source file. Compiler imports are subject to the existing
module resolver and its own safety checks.

Errors report the manifest's approximate `line:column` for lexical or
syntactic failures and maintain the compiler's established exit status and
schema-v1 JSON error envelope when requested with
`--diagnostic-format=json`.

## CLI behavior

| Command | Behavior |
| --- | --- |
| `scarp init DIRECTORY` | Create a fresh v2 project; no overwrite |
| `scarp run DIRECTORY` | Execute declared project entry using existing interpreter |
| `scarp run` | Execute the project in the current directory |
| `scarp verify DIRECTORY` | Compile and type-check project without runtime execution |
| `scarp launch DIRECTORY` | Existing explicit project command, now resolves both manifests |
| `scarp create DIRECTORY` | Retained legacy v1 project initialization |
| `scarp run FILE.srp` | Retained single-file interpreter workflow |
| `scarp exec FILE.srp` | Retained canonical explicit source execution |

`run` uses the existing execution budgets, stdout/stderr conventions, and
runtime diagnostics. `verify` does not execute application statements
or create output artifacts. A malformed manifest fails before any
application output is produced.

## Migration and compatibility

Old projects continue using `Scarp.project`:

```text
scarp-project: 1
name: legacy
entry: src/main.srp
```

This legacy manifest is still read with its original strict field order.
Both legacy `create`, `launch` and project-directory `verify` remain
supported. On case-insensitive filesystems the two filenames may refer to
the same file; the reader identifies legacy format by its header rather
than relying on filename capitalization.

For a planned migration, create a new `scarp.project` file containing
the declarative fields, verify it, then stop using the old manifest. On
case-sensitive filesystems where both names exist, the lower-case v2
name takes precedence; avoid shipping both manifests.

## Not implemented

No package registry, dependency resolution, lockfile, remote installation,
`scarp test` discovery, `scarp fmt` formatting, native build or source
workspace discovery is added here. The empty dependencies block reserves
syntax without pretending those features work. Broader standard-library
and tooling features require independent, tested development slices.

CI must pass on Windows, Linux, macOS and Rust 1.74 before this format can
be considered ready for merge. This change does not publish a release.
