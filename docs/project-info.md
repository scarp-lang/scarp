# Read-only project information (unreleased development)

This command is implemented in the unmerged project-info development
branch, stacked on the test-runner branch (PR #218), which in turn
depends on the declarative-project foundation (PR #217).
**It is not yet available in a published Scarp compiler.**

## Inspect a project

```sh
scarp project info
scarp project info calculator
scarp project info --json calculator
scarp guide project
```

Without a directory, the command inspects the current working directory.
It reads the manifest using the same bounded, strictly validated reader
used by `scarp run`, `scarp launch` and `scarp verify`. The configured
entry must exist, be a regular `.srp` file, and resolve inside the
canonical project directory. A symlinked manifest is rejected, as is an
entrypoint symlink escaping the project root.

This command **never executes application code**, installs dependencies,
loads imported source files, compiles the module graph, or writes build
artifacts. It reports only validated manifest declarations and the
configured entry label, not inferred program capabilities.

The text format for a version-2 project looks like:

```text
Name: calculator
Version: 0.1.0
Type: application
Manifest: scarp.project (v2)
Entry: src/main.srp
```

## Machine-readable output

The `--json` option prints exactly one JSON object followed by a newline.
Schema version 1 uses the following keys:

```json
{
  "schema_version": 1,
  "name": "calculator",
  "version": "0.1.0",
  "type": "application",
  "entry": "src/main.srp",
  "manifest": "scarp.project",
  "format": "v2"
}
```

- `schema_version` is the output contract version, currently `1`.
- `name` is the validated project name, including international text.
- `version` is the declared semantic version, or JSON `null` for legacy v1.
- `type` is currently `application`; library manifests are not supported.
- `entry` is the declared relative source path, not an absolute host path.
- `manifest` is `scarp.project` or `Scarp.project`, by format.
- `format` is `v2` or `legacy-v1`.

The encoder escapes quotes, backslashes and control characters, preserving
valid Unicode. The JSON layout is deterministic and does not expose the
canonical host path. Unknown fields are **not** silently discarded.
A legacy project returns:

```json
{"schema_version":1,"name":"legacy","version":null,"type":"application","entry":"src/main.srp","manifest":"Scarp.project","format":"legacy-v1"}
```

## Errors and compatibility

For invalid project metadata, missing entries or unsafe paths, the command
returns status 1, reports a message to stderr, and writes **no JSON stdout**.
Incorrect command usage returns status 2. With `--json`, these errors
still use human-readable stderr; JSON formatting currently applies only
to successful metadata. Commands such as `scarp run`, `scarp verify`,
`scarp test`, `scarp init` and `scarp launch` retain their interfaces.

The development branch is subject to cross-platform CI and manual review,
with no authorization for merging, tagging or releasing this feature.
