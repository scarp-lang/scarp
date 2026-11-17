# Module JSON inspection and diagnostic explanations

The module inventory reports imports, dependencies and declarations with
file-local byte spans. Alpha.12 adds record, variant, list, and map metadata.

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
| 2 | A tuple type in a serialized signature |
| 3 | A fixed array, typed list or typed map in a serialized signature |
| 4 | A record declaration in the loaded graph |
| 5 | A variant declaration in the loaded graph, including a generated Option or Result specialization |

The highest applicable version wins. Private symbols participate in version
selection. A local array, list or map binding alone does not change the schema.
Type strings follow the compiler's canonical display, for example
`tuple<array<string, 0>, integer>`, `map<string, list<integer>>` and
`variant option<integer>`. Clients should treat these strings as presentation
metadata. Symbol/reference and diagnostic JSON have their own schema versions.

| Object | Fields |
| --- | --- |
| Top level | schema_version, entry=0, modules, constant_dependencies |
| Module | id, path, imports, dependencies, dependents, symbols |
| Import | path, alias, target, span, path_span, alias_span |
| Constant symbol | name, public, span, kind="constant", type |
| Record symbol (v4–v5) | name, public, span, kind="record", fields |
| Record field | name, type |
| Variant symbol (v5) | name, public, span, kind="variant", cases |
| Variant case | name, payload (type string or null) |
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
and declaration belong to source; target_declaration belongs to target. Explicit
symbol and parameter spans identify their original names. A generated Option
or Result symbol uses the source range of its `option` or `result` token.
Field and case metadata follow declaration order. IDs belong to this compilation. The writer computes offsets from source starts,
without repeatedly scanning source prefixes for line/columns. Editors can map
local offsets to their own coordinate system; Rust hosts can use the existing
SourceMap API and retained source snapshots.

No source contents, constant values, function bodies, default expressions or
internal mangled function names are emitted. has_default reports presence only.
Ordinary entry-script local bindings are omitted. Generated concrete Option and
Result type symbols can be present when their first use is in a local binding.
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
missing or extra arguments and unknown codes return status 2. Every diagnostic
code in the compiler's catalogue has an explanation.

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

See [Proposal 0034](proposals/0034-developer-inspection.md) for the original
inspection design.
