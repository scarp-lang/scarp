# Alpha.19 — export-constrained dynamic host callable slots

Status: **draft-only Rust embedding feature**. Builds on alpha.19 PR #344's
same-program rebindable CallableSlot, PR #343's ModuleCallableCatalog
visibility-aware binding, and PR #342's named host arguments.

## API and verified selection boundary

ModuleCallableCatalog::slot_exported(source_id, name, required_signature)
checks that the declaration comes from a real linked module, is a pub
function symbol, and has a compatible canonical function signature in
that compiled snapshot. It then creates a CallableSlot whose internal
Arc<ResolvedProgram> is shared with the catalog.

ExportedCallableSlot::select_exported(source_id, name) repeats those
checks before replacing the existing target. The slot never exposes
select(index) or set(raw NamedCallableRef) and never allows clients to
turn an arbitrary internal function index into a new selection through
this API. Both the function target and selected (source_id, original name)
metadata change together only after all checks pass; rejection is atomic.

The fixed required type uses the existing pure/impure effect subtyping
rule and invariant arguments/result. Selected targets provide their own
declaration-specific parameter labels and default values, which are not
part of the canonical function type. A rebinding can therefore change
valid named argument labels and defaults without changing its type.

## Ownership, diagnostics and quotas

The exported slot retains a clone of ModuleCallableCatalog: its linked
ModuleProgram Arc owns original module symbols and source map, and its
resolver CallableCatalog shares the compiled program with CallableSlot.
Cloning a slot shares compiled immutable data but not its mutable
selection. Clearing a slot releases its chosen NamedCallableRef, but the
slot/catalog still retains the compiled snapshot to permit future
selection.

The slot's invoke_with_io_and_options and invoke_with_input_and_options
delegate to the existing checked CallableSlot interpreter entry. An
unbound call is rejected without Scarp code, input reads or output.
Bound failures are translated into ModuleRunError with the original
SourceMap, so runtime errors identify the original .srp file and Unicode
spans. Ordinary output remains buffered until successful evaluation;
reads and explicitly flushed interactive output are not reversible.

ExecutionOptions budgets are per invocation, and repeated host
invocations require their own cumulative host quota if needed. The API
does not increase the Rust process memory cap or impose authorization.
The caller is responsible for who can call exported modules and which
source IDs and names are allowed. Public visibility alone is not an
authentication or OS sandbox boundary.

## Non-goals

This is NOT Scarp-source first-class callable syntax, captured closures,
escape analysis, a new memory collector, a native callable ABI, a
protected-main change or an alpha.19 release. Raw Rust resolver APIs
still exist for trusted compiler tooling, and a host must not expose
them blindly to untrusted callers.

Regression coverage checks public-only rebinding, private/constant
rejection, two modules' same-shaped functions, pure/impure compatibility,
actual target parameter labels, default argument semantics, output
rollback, input non-consumption for empty slots, imported-source
diagnostics, cloned-selection independence and Lifetime retention.
