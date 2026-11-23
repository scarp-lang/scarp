# Alpha.19 — checked dynamic callable slots (Rust embedding)

Status: **experimental draft-only API**. This is a concrete indirect host
dispatch milestone, NOT Scarp source-language first-class function values.

## Motivation and threat model

Previous alpha.19 work added canonical function-signature TypeIds, retained
NamedCallableRefs, executable host entry, named/default host arguments, and
linked-module export-aware lookup. A Rust embedder can now select a different
function at runtime while preserving a single required call signature.

SourceType does not yet represent a function type. Value cannot carry an
interpreter callable; parser calls still require a named function in source,
and no escaping closure environment exists. Conflating this host slot with
Scarp source-level function values would make false safety promises.
This milestone stays host-only and intentionally does not change source
grammar or memory ownership rules for Scarp programmers.

## API

- CallableCatalog::slot(required_type) creates an initially empty typed slot.
- CallableCatalog::slot_bound(required_type, declaration_index) creates a
  typed slot with an existing declaration already selected.
- CallableSlot::set(&NamedCallableRef) assigns a checked retained target.
- CallableSlot::select(declaration_index) rebinds to a valid same-program
  declaration through CallableCatalog::bind, including signature checking.
- CallableSlot::clear() removes the current target; clones are independent.
- CallableSlot::invoke_with_io_and_options and
  invoke_with_input_and_options execute the selected function using the
  already-qualified host entry path, accepting HostArguments.

The slot itself retains Arc<ResolvedProgram>, so its required TypeId stays
valid after a compiler cache, catalog, or previous target is dropped. Clones
do not share mutable current selection and never change the referenced
function body. The target's declaration index is program-snapshot-local, not
a persisted identifier or a public export grant.

## Type and effect invariants

The required type must resolve to TypeKind::Function in the snapshot's
TypeRegistry. Unknown IDs (including foreign nonprimitive types) fail.
Assignments require identical Arc<ResolvedProgram> allocation, preventing
cross-rebuild alias confusion, even when signatures are structurally equal.
Then TypeRegistry::accepts_callable checks exact invariant argument/result
shapes and the established effect rule: pure may fill an impure slot,
impure may NOT fill a pure slot.

Rejected targets never replace the old selected target. Empty-slot
invocation returns CallableSlotInvokeError::Unbound before evaluating code,
reading input, or creating output. A bound invocation delegates through
NamedCallableRef, preserving named parameter identities of the selected
actual declaration, omitted defaults, needs/ensures, call-depth/count/work,
live slots, capture quotas, and stdout/stderr error atomicity.

## Security boundaries and limitations

This is a Rust host inspection/execution capability; it does not enforce
linked-module visibility by itself. Hosts that expose access to untrusted
clients must first use ModuleCallableCatalog for export-aware selection,
then separately enforce their own allowlist/policy. A raw CallableCatalog
can access internal named declarations by design.

Runtime ExecutionOptions are **per invocation**; a host looping repeatedly
must impose any additional application-level cumulative quota. External
argument values were already allocated by Rust and are not newly charged
to the Scarp runtime allocation counters. The slot is not serializable
across compiler revisions, does not accept foreign program snapshots,
and makes no RSS or asymptotic performance claims.

No source-level callable Value, indirect-call grammar, closure capture,
new garbage collector, native function-pointer ABI, main merge, release
tag or alpha.19 publication is part of this milestone.

## Next architecture work for first-class Scarp functions

1. Define a source function-type syntax, checked type model and compositional
   node/arity limits; avoid treating raw TypeId as a portable type token.
2. Add explicit AST nodes for named function references and indirect
   invocation, preserving argument labels and pre-existing compile diagnostics.
3. Add an immutable interpreter Value discriminant with per-program target
   provenance, no implicit capture of lexical frame storage, and precise
   retirement rules for escaping references.
4. Reuse the existing machine's call transition and effects/quotas rather than
   a second expression evaluator. Verify all module visibility boundaries.
5. Explicitly reject function values in native/C backends until a verified ABI
   and lowering scheme exists. Do not claim full closure support early.
