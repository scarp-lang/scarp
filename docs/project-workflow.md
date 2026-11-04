# Scarp projects (v0.0.5-alpha.4 prerelease)

The published alpha.4 prerelease has an offline, explicit project workflow.
It uses the **real Scarp compiler and interpreter**, not a native code
generator or package manager. These directory commands are not available in
alpha.3 packages.

```sh
scarp create hello-world
scarp verify hello-world
scarp launch hello-world
scarp guide launch
```

`create` fails if the selected directory already exists, including an empty
directory. It does not install dependencies, connect to a registry, silently
adopt a preexisting source tree, or overwrite user files. The parent directory
must already exist. The new directory contains:

```text
hello-world/
  Scarp.project
  src/
    main.srp
```

The manifest format is intentionally minimal and strictly versioned:

```text
scarp-project: 1
name: hello-world
entry: src/main.srp
```

It is UTF-8, exactly three lines in this order, with a maximum size of
4096 bytes. A name must have 1–96 UTF-8 bytes without control characters,
leading/trailing whitespace, or path separators/colon. The entry must be a
relative `.srp` file inside the selected project root, contain only normal
slash-separated path components, and have no `.`, `..`, drive prefix, colon,
backslash, or control characters. Manifest symlinks are disallowed. An entry
symlink is accepted only if its canonical target remains inside the canonical
project directory. Invalid and unknown/duplicate fields fail rather than
being ignored.

`launch` never searches parent directories for a manifest, and always loads
the explicit project directory passed to it. It loads one manifest/entrypoint
and executes using exactly the same interpreter, module imports, budgets,
stdout/stderr, runtime diagnostics, and exit codes as `exec`. Its budget
flags are `--max-steps`, `--max-prints`, `--max-call-depth`,
`--max-calls`, `--max-live-slots`, `--max-capture-bytes`,
`--max-vector-bytes`, and `--max-text-bytes`. Use
`--diagnostic-format=json` for the compiler's existing structured error
format. A malformed manifest is reported with status 1; invalid CLI
invocations have status 2. `--` allows a dash-leading directory argument.

```sh
scarp launch --max-steps 500000 hello-world
scarp launch --diagnostic-format=json hello-world
scarp verify hello-world
scarp verify --diagnostic-format=json hello-world
scarp verify hello-world/src/main.srp
```

The `verify PROJECT_DIRECTORY` command reuses these manifest/entrypoint checks
and invokes the real static compiler, including imports and compile-time
constants, without running statements or writing files. It is silent on success.
Unlike `launch`, `verify` does not accept execution-budget flags. File-based
`verify FILE.srp` remains unchanged. Manifest failures return status 1 and
support `--diagnostic-format=json`.

**Unreleased development-source extension (not in the published alpha.4 packages):**

```sh
scarp inspect modules --json hello-world
```

`inspect modules PROJECT_DIRECTORY` (optionally with `--json`) resolves the
same explicit `Scarp.project` entrypoint and compiles its full module graph
without running statements. It produces the same text or schema-version-1 JSON
inventory as inspecting the canonical entry file directly. Manifest errors
return status 1 with no partial inventory; `--diagnostic-format=json` uses the
existing structured error format. This also applies to the historical
`modules PROJECT_DIRECTORY` spelling; prefer `inspect modules` for new tools.
No ancestor search, runtime-budget options, native build, or new manifest
fields are introduced.

This format is an intentionally small **development manifest v1**, not a
general project/package specification. It does not yet support dependencies,
a package lock, source paths outside the project, a project build, native
artifacts, multi-entrypoint selection, workspace discovery or other project-level
inspection commands. The manifest's stable spelling is not guaranteed.
Project-directory verification is not available in published alpha.3 packages.
