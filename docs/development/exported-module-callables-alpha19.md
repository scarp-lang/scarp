# Alpha.19 — exported linked-module callable access

Status: draft, experimental Rust embedding API. This builds on PR #342,
which adds checked named host arguments to PR #341's executable named-call
machine, and on the underlying alpha.19 canonical type and Lifetime work.

## Module visibility and target identity

A raw resolver CallableCatalog accepts any named declaration index in a
resolved plan, including private functions. That is useful for compiler
inspection but should not be presented as an export-aware host interface.

ModuleCallableCatalog wraps a linked ModuleProgram and builds one shared
resolver CallableCatalog for the program. bind_exported(source_id, name)
locates the original SymbolInfo in that module, requires a pub function
symbol, and maps its source-ordered function ordinal to the resolved plan's
global declaration index. Non-function symbols, private declarations,
unknown source IDs and names fail before any runtime effect. Diagnostic
messages avoid echoing arbitrary unknown names from the embedding caller.

bind_exported_as additionally validates a caller-required TypeId against
the exact linked registry, preserving the existing purity subeffect rule and
foreign-TypeId rejection. ExportedNamedCallable exposes its original source
ID, symbol name, canonical TypeId and immutable FunctionInfo.

## Execution and source maps

The exported wrapper invokes the exact checked named host call API. It
does not create a new evaluator, permit forbidden private targets, or
execute the entry script. Named/default arguments, contracts, effects,
input, call-depth/count/live-slot/work/capture budgets, recursion and
atomic output retain the same runtime behavior.

Unlike bare NamedCallableRef, an exported handle retains the linked
Arc<SourceMap>. Runtime and host preflight diagnostics are returned as
ModuleRunError::Diagnostic(ModuleDiagnostic) so byte spans resolve to the
original UTF-8/CRLF module file and source-aware error rendering stays
available after the owning catalog is dropped. Writer failures retain
ModuleRunError::Output.

## Ownership and limitations

ModuleCallableCatalog creates a separate immutable clone of the resolved
program once per catalog, shared across all exported handles. The original
linked module snapshot remains available for symbol visibility, while
individual exported handles retain only the compiled plan and source map.
Hosts must cap aggregate retained program/source snapshots; this is not a
live heap or RSS memory bound.

The API enforces the language-level pub boundary for *name-based lookup*.
It is not an OS sandbox or authentication layer. A Rust caller owning the
original ModuleProgram can intentionally use the raw resolver inspection
API; it remains responsible for not exposing private declarations to
untrusted clients. Linking of imported modules is already checked by the
loader/compiler; this API does not dynamically load new files or change
dependency access.

No first-class Scarp function Value, closure capture, source-level indirect
calls, native callable ABI, new collector, protected-main merge or alpha.19
publication are part of this milestone.
