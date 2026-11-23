# Scarp project workflow

On the development branch, `scarp.project` (lowercase `s`) is the canonical
manifest filename. Both `scarp create` and `scarp init` now create the same
declarative v2 project, rather than mixing two different manifest formats.

```sh
scarp create hello-world
scarp verify hello-world
scarp exec hello-world
scarp project info --json hello-world
```

The generated files are:

```text
hello-world/
  scarp.project
  src/
    main.srp
  tests/
  examples/
  README.md
  .gitignore
```

The generated declarative manifest is Scarp's own format, **not TOML or JSON**:

```text
project "hello-world" {
    version: "0.1.0"
    entry: "src/main.srp"
    type: "application"
    dependencies {}
}
```

Both commands refuse existing destinations (including empty directories), do
not install dependencies or execute code, and require an existing parent
directory. `verify` is static; `exec` and `launch` use the interpreter.
See [the manifest specification](project-v2.md) for exact grammar, size,
path containment, and security rules.

## Existing projects remain readable

Published older `create` versions generated `Scarp.project` with the v1
manifest grammar:

```text
scarp-project: 1
name: hello-world
entry: src/main.srp
```

Existing uppercase manifests continue to work; the loader still chooses v1
or v2 based on the header. Changing filename case alone does not convert
a v1 manifest into v2. For migration, write a valid `scarp.project` v2
manifest, verify it, then remove the uppercase file. Do not keep both:
the lowercase filename takes priority on case-sensitive filesystems; on
case-insensitive systems both spellings may resolve to the same file.

The earliest create/launch workflow shipped in v0.0.5-alpha.4, and module
inspection arrived in v0.0.5-alpha.5. Those binaries are not changed by
this development-branch update; a future release is required for the new
`create` default.
