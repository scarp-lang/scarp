# Static compiler service for editors

Scarp alpha.12 exposes bounded source navigation and exact-graph reuse through
Rust embedding. The compiler pipeline is the authority for syntax, diagnostics,
types and symbol identity. A service API can combine the current loader,
compiler and editor snapshot before adding any protocol transport.

## Request identity and publication

Capture a workspace generation, trusted loader root and immutable source
bytes with a revision for each canonical source identity. Read every requested
file from this workspace view. Load through the existing bounded module loader
and re-resolve each import. A display path is for humans. Scope editor source
and symbol IDs to one compiled graph. A rename, deletion or loader-root change
creates a new workspace generation even when remaining revisions match.

Resolve through `EditorCompilationCache::resolve_with_loaded_revisions`
when the request provides a compiler-owned cache. An exact byte and graph
match can reuse compiled metadata. The frontend caches and constant-fold
cache retain their separate limits. A failed resolution returns current
diagnostics. A successful resolution returns an immutable symbol snapshot.
Source execution and project initialization stay outside this pipeline.
Restricted constant evaluation follows the compiler's existing static limits.

Before publishing either result, the host checks the captured generation and
every canonical identity/revision against its current workspace. Discard late
results from older generations. Convert byte spans to UTF-16 against the
exact retained source. A coordinate inside a UTF-8 scalar, a UTF-16 surrogate
pair or a CRLF line ending is invalid. Revision lists supplied to existing
queries follow the snapshot's own compile-time file order.

## Implementation phases

| Phase | Contract and acceptance |
| --- | --- |
| Static diagnostics | Keep canonical codes and related byte spans. Compare cold and warm diagnostics after invalid edits, deletion, reopen and import-target changes. |
| Document symbols and references | Reuse compiler indexes. Require exact cold/warm JSON and reject late generations after a Unicode/CRLF edit or discovery-order change. |
| Definition and hover | Resolve symbols in the current snapshot. Generate hover text from checked signatures and verify nominal type identity and visibility across imports. |
| Semantic tokens and completion | Use owned lexer/resolver facts. On incomplete source, use a documented conservative fallback with bounded results and deterministic ordering. |
| Rename and formatting | Compute a bounded edit set with exact source ranges. Return edits for host application only after versions match. Specify language formatting separately. |
| Cancellation and transport | Add bounded polling at compilation phase/work boundaries. Discard cancelled outputs, define request and snapshot ownership, then qualify LSP lifecycle and shutdown. |

Current cache counters describe retained source bytes and logical metadata
limits. Host memory also includes allocations and snapshots held by callers.
Measure resource use independently of correctness. The required edit matrix
includes diamond dependencies, alias rebinding, privacy changes and failed
constant evaluation. Existing coverage is in `tests/module_invalidation.rs`,
`tests/editor_source_revisions.rs`, `tests/editor_compilation_cache.rs` and
`tests/editor_snapshot.rs`.
