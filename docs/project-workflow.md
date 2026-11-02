# Scarp projects (development)

Scarp has an offline, explicit project workflow. It uses the **existing Scarp
interpreter**, not a native compiler or package manager.

```sh
scarp create hello-world
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
scarp verify hello-world/src/main.srp
```

This format is an intentionally small **development manifest v1**, not a
general project/package specification. It does not yet support dependencies,
a package lock, source paths outside the project, a project build, native
artifacts, multi-entrypoint selection, workspace discovery, or project-level
verification/inspection aliases. Use `verify` on the declared `.srp` file.
The manifest format is not a commitment to its final stable spelling.
