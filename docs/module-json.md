# Module JSON inspection and diagnostic explanations

The module inventory reports imports, dependencies and declarations with
file-local spans. Build the current checkout to inspect fixed-array signatures.

```sh
cargo run -- modules --json examples/inspection_json.srp
cargo run -- modules --json examples/constant_expressions.srp
cargo run -- explain E0044
cargo run -- explain --help
```

The [runnable JSON example](../examples/inspection_json.srp) prints `42` when run.
Its [in-memory JSON snapshot](../examples/inspection_json.modules.json) is checked
exactly by tests. Filesystem path labels differ from that virtual-loader snapshot.
Ordinary `scarp modules <path>` retains its existing text inventory format.

## Schema versions

The JSON writer emits one compact UTF-8 object followed by LF. Object field order
and array order are deterministic for the same compiled sources and loader.
Check `schema_version` before interpreting type strings.

| Version | Types serialized by the inventory |
| --- | --- |
| 1 | Primitive types only |
| 2 | At least one tuple type, with no arrays in any serialized type |
| 3 | At least one fixed array, including an array nested inside a tuple |

Private symbols participate in version selection. Arrays used inside function
bodies or local bindings leave the version determined by the declarations.
Version 3 uses canonical strings such as `array<integer, 2>` and
`tuple<array<string, 0>, integer>`. The object fields below apply to all versions.

| Object | Fields |
| --- | --- |
| Top level | schema_version, entry=0, modules, constant_dependencies |
| Module | id, path, imports, dependencies, dependents, symbols |
| Import | path, alias, target, span, path_span, alias_span |
| Constant symbol | name, public, span, kind="constant", type |
| Function symbol | name, public, span, kind="function", pure, result, parameters |
| Parameter | name, type, has_default, span |
| Constant reference | source, declaration, target, target_declaration, span |

`path` on a module is its loader-provided display label; on an import it is the
source specifier. Native non-Unicode display labels may be lossy, while actual
loader identity is preserved internally. Modules follow discovery order; imports,
symbols and references follow their existing source order. Direct dependency IDs
deduplicate aliases; dependents follow discovery order. Private symbols are
inspectable metadata and remain inaccessible to importing code.

Every span is `[start,end]`, a half-open **file-local UTF-8 byte range**. Import and
symbol/parameter spans belong to their containing module. Constant-reference span
and declaration belong to source; target_declaration belongs to target. Symbol and
parameter spans identify their original names. IDs belong to this compilation,
not a persistent cross-run cache. The writer computes offsets from source starts,
without repeatedly scanning source prefixes for line/columns. Editors can map
local offsets to their own coordinate system; Rust hosts can use the existing
SourceMap API and retained source snapshots.

No source contents, constant values, function bodies, default expressions or
internal mangled function names are emitted. has_default reports presence only.
Ordinary entry-script local bindings are not module symbols and remain omitted.
JSON quoting escapes control characters, quotes and backslashes; Unicode text
otherwise remains UTF-8. Readers should check schema_version before consuming it.

## Compilation and command behavior

`modules --json` uses the same trusted-root/module/constant limits and type checker
as text inspection. Restricted module constants evaluate during compilation;
entry statements, user calls, defaults and contracts never execute. Therefore an
invalid constant fails, while a runtime `print(1/0);` can be inspected.

Source failures return 1, write diagnostics to stderr and emit no partial JSON.
Invalid/duplicate flags return usage status 2 before source I/O. Runtime-limit
flags remain restricted to run/check. Place --json before the path; use `--` before
a filename beginning with a dash: `scarp modules --json -- -entry.srp`.

Rust embedders call `ModuleProgram::write_inspection_json(&mut writer)`. It streams
borrowed metadata without a second signature table or JSON dependency. I/O errors
preserve their kind and may leave partial bytes, just like text inspection. The
CLI retains its established broken-pipe behavior. This is metadata serialization,
not a JSON parser/library for Scarp values or an LSP implementation.

## Explain diagnostic codes

`scarp explain E####` prints the code's meaning and actionable repair guidance
without opening any source file. Codes are exact canonical uppercase identifiers;
missing/extra arguments and unknown codes return status 2. All current E0001 through
E0045 codes have explanations. Existing diagnostic rendering/codes are unchanged.

Rust hosts use DiagnosticKind::ALL, from_code and explanation. One catalogue owns
the enum variants, stable codes and descriptions so new diagnostics cannot drift
between separate registries. Explanations are general guidance; a source-specific
diagnostic's primary span, related sites and help remain the most precise context.

## Verification and performance

Rust tests cover the exact snapshot, graph/reference ownership, escaped controls,
original signatures, concurrent reuse, writer failures and CLI usage. An independent
Python parser checks emitted JSON and slices real source bytes to validate names.
Native packages parse every shipped entry's JSON with PowerShell and exercise
diagnostic explanations. Compiler/JSON benchmarks use correctness oracles and
three CSV timing samples; see [performance](performance.md).

The separate documentation website needs synchronization after these source PRs
are accepted. This cycle modifies only in-repository documentation. See
[Proposal 0034](proposals/0034-developer-inspection.md) for the scoped contract.
