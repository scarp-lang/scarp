# Alpha.19 — host named-argument binding

Status: experimental Rust embedding API on a draft PR. This follows
alpha.19 PR #341's executable host-only function handles. The last published
prerelease is alpha.18. No source-language function values, capture syntax,
closure semantics, native callable ABI, tags or protected-main modifications.

## Public additive API

resolver::HostArguments contains positional Vec<Value> and named
Vec<(String, Value)> and offers new and positional constructors.
NamedCallableRef::invoke_bound_with_io_and_options and
invoke_bound_with_input_and_options accept this descriptor. Existing
positional-only invoke APIs retain their signatures and delegate through
the same binder.

All labels match the exact resolved declaration's FunctionInfo.parameters,
not a canonical callable type, which contains only types/effects. Distinct
declarations with the same signature can have different valid labels.
Bindings are mapped to declaration-indexed slots. Omitted slots pass to
the existing interpreter Stage::Defaults machinery. The host binder
does not evaluate any default.

## Validation and deterministic errors

Preflight rejects an excessive argument count before allocating the bounded
declaration-indexed slot vector. The binder then validates positional
slots, exact name lookup, unknown/duplicate/overlapping bindings, each
supplied scalar's type/UTF-8 byte limit and missing required parameters.

Errors are Scarp diagnostics with CallArguments, TypeMismatch,
FunctionUnsupported and TextConstruction codes. No Scarp execution,
input read or program output occurs on preflight failure. Untrusted unknown
label text is not embedded in error messages to prevent unbounded
diagnostic size.

Supplied host values were constructed before invocation, so the caller owns
their evaluation order and memory. There is no promise that host expressions
behave like source-level Scarp argument evaluation. The frame itself evaluates
defaults in formal declaration order, then needs/body/ensures, and enforces
normal call depth/count/live-slot, work and output-capture quotas. A supplied
named value suppresses its default.

Unsupported host composites and other non-scalar values remain explicitly
rejected until provenance and allocation policies are reviewed. The existing
Arc<ResolvedProgram> retains the callable target after the original catalog
is dropped.

## Test coverage

Mixed and reverse-ordered labels, sparse defaults referencing earlier args,
default suppression, needs/ensures, unknown, duplicate, overlapping, missing,
excess and Unicode labels, same-signature functions with distinct parameter
names, type/UTF-8 limits, atomic input/output on failure, linked metadata and
resource limits.

## Explicit limitations

Scarp source still cannot assign, pass, return or indirectly call a function
as a runtime value. Linked-module visibility and host authorization remain
separate concerns: arbitrary internal function indices are not a safe
untrusted-client access-control interface. No new GC, native callable ABI,
native compile support or release.
