# Source symbol navigation (v0.0.6-alpha.1 prerelease)

The navigation index is available in the published
[v0.0.6-alpha.1 prerelease](https://github.com/scarp-lang/scarp/releases/tag/v0.0.6-alpha.1).
It is a reusable compiler API and static CLI inventory, not an LSP server,
rename engine or incremental cache. Older v0.0.5-alpha.5 binaries lack
the symbol/reference commands.

## Commands and failure behavior

```sh
scarp inspect symbols --json examples/symbol_navigation.srp
scarp inspect references --json examples/symbol_navigation.srp
scarp inspect references --json --diagnostic-format=json path/to/project
cargo run --locked --example symbol_index
```

An explicit project directory reuses `Scarp.project` entrypoint and trusted-loader
rules. Without `--json` the commands print an escaped, readable inventory. Budget
flags for interpreter execution are rejected. Filenames beginning with a dash
follow `--`. Help is available with `guide inspect symbols` or `references`.

Both commands load, link and type-check the entire graph. Compile-time scalar
constants are checked/folded normally; runtime statements, defaults and contracts
are never executed. Invalid/incomplete source, duplicate declarations, inaccessible
members, cyclic imports and invalid manifests fail with the existing diagnostic
and exit status 1 (usage errors use 2). No inventory is written on these failures.
`--diagnostic-format=json` retains the existing diagnostic schema on stderr and
is independent from `--json` on stdout. I/O failure can leave a partial stream;
writer errors propagate, and CLI broken-pipe behavior is preserved.

## Definition and reference contract

Definitions include modules, import aliases, functions, scalar module constants,
ordinary bind/var bindings, parameters and repeat indices. Lexical shadowing uses
separate identities. An initializer resolves before its new binding is inserted;
parameter defaults see earlier parameters/module constants. Function scopes do
not capture entry-script variables. Function identities stay distinct even when
their interned signatures are equal.

References include reads, writes, compound read/writes, calls, namespace qualifiers,
import paths and named argument labels. Qualified `lib . function` sites preserve
separate alias/member tokens even with whitespace around the dot. Labels navigate
to the callee's parameter. Folded constant reads retain original reference sites,
including initializer dependencies and operands skipped by scalar short-circuiting.
Only declarations and references validated by the compiler are returned.

A module definition has a zero-width file-start anchor; clicking an import path
navigates to that file. Used builtins and implicit contract `result` values have
no invented source declaration (`location: null`). Each implicit result belongs
to its function; all of that function's postconditions refer to the same result.
Primitive type keywords and operators are not value-symbol reference sites.

## Rust embedding

Use `modules::compile_indexed_with_loader`, `compile_indexed_file`, or
`resolve_indexed` for a loaded graph. `resolve_indexed_with_compiler_limits`
retains custom constant/type budgets. These additive APIs return `IndexedProgram`:
`program()` exposes the existing `ModuleProgram`, and `index()` the snapshot.
`examples/symbol_index.rs` demonstrates a virtual loader and navigation queries.

`SymbolIndex::goto_definition(source_id, byte_offset)` selects a definition or
reference token using a **file-local UTF-8 byte offset**. Whitespace, punctuation,
EOF, invalid source IDs and non-boundary byte offsets return `None`.
`find_references(symbol_id)` returns source-ordered use sites, excluding the
declaration itself. Both queries operate on immutable metadata without I/O or
recompilation. Clones share immutable storage and original source text.

IDs are dense and local to a snapshot. Rebuilding after an edit/removal produces
a new snapshot; IDs must not be persisted or mixed across compilations. There is
no process-global cache or dependency invalidation protocol. IDE/Playground
clients can translate editor coordinates using the byte spans and original text,
then discard old snapshots when documents change. No integration in those
separate products is implied.

## UTF-16 editor adapter (development API)

IDE/LSP clients use zero-based line numbers and **UTF-16 code-unit** columns,
which are different from the index's file-local UTF-8 byte offsets and the
one-based Unicode-scalar columns emitted in JSON. The additive dependency-free
Rust adapter in `modules::symbols::editor` provides:

- `Utf16Position::new(line, character)` and half-open `Utf16Range`.
- `utf16_to_byte(source, position)` and `byte_to_utf16(source, byte)` for
  strict conversions against an original immutable source string.
- `SymbolIndex::goto_definition_utf16(source_id, position)`,
  `utf16_position(source_id, byte)`, and `utf16_range(source_location)`.

The adapter treats LF, CRLF and lone CR as line separators; each CRLF is one
line break. It accepts valid end-of-line and EOF positions, including a final
empty line, and rejects missing lines, beyond-line columns, offsets inside
UTF-8 characters, positions splitting UTF-16 surrogate pairs, and the byte
boundary between CR and LF. It never silently clamps a bad position.

These conversions read existing source text, with no hidden process-wide state
or cache; a query may scan the source and thus costs O(source length) worst
case. Editors needing repeated low-latency conversion can maintain their own
versioned line index. The adapter does not implement LSP transport, change
notifications, incremental recompilation or cancellation; consumers must
discard stale symbol snapshots after edits. Existing schema-v1 JSON remains
unchanged: it still uses UTF-8 byte spans and one-based scalar columns.

## Limits and measured quantities

Ordinary compilation does not construct an index or enforce index limits. The
opt-in `IndexLimits` defaults are 65,536 symbols, 1,048,576 references, 8 MiB of
retained UTF-8 definition-name payload and 4,194,304 logical work units. Work
counts visited syntax and emitted metadata; `IndexUsage` reports these logical
construction quantities, not runtime steps or operating-system memory. Source
loading, constant evaluation, semantic type and execution limits remain separate.

Index construction stops retaining metadata when its cap is reached, but normal
semantic validation completes so a compiler error takes precedence over the index
limit. A valid program exceeding an index cap reports **E0050**. No partial index
is returned. Immutable reverse-reference and token-site tables support binary
search queries; metadata locations use incremental source scans, not a full
source-prefix scan per occurrence. Flat expression nodes avoid recursive visitors.

## JSON schema version 1

Each output is one UTF-8 JSON object followed by LF. Both views contain:

- `schema_version: 1`, `kind: "scarp.symbols"` or `"scarp.references"`;
- `span_encoding: "utf-8-bytes"`, `column_encoding: "unicode-scalar"`,
  `position_base: 1`;
- `files`: discovery-ordered `{id, path}` records; paths come from the existing
  loader display-name policy and can be absolute/platform-specific;
- `symbols`: `{id, name, kind, owner, public, mutable, target_module, location}`.
  `owner` is a symbol ID or null; `target_module` is a **file ID**, used by
  module/alias definitions; `location` is null for builtin/implicit definitions.

The reference view additionally contains `references`, each with `target` (symbol
ID), `kind` and `location`. A location is `{source, span: [start,end], start:
{line,column}, end: {line,column}}`. Byte spans are half-open; line/column values
are one-based Unicode scalar positions in the original file, including BOM/CRLF.
Source bodies, constant values and default expressions are not serialized.

Source definitions sort by file ID and token start, with kind/name/owner ties.
Implicit results follow token definitions in their file; builtins follow files
in name order. References sort by file ID/start/end; IDs refer to this resulting
symbol table. Existing module/diagnostic schema-v1 outputs are unchanged.
`examples/symbol_navigation.symbols.json` and `.references.json` are exact
memory-loader fixtures; their relative display paths are independent of the host.
The independent checker also validates actual CLI file ownership and positions:

```sh
python scripts/test_symbol_index_json.py target/debug/scarp
```
