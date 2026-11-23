# Alpha.19 — named callable references, indirect dispatch and Lifetime gates

Status: **design and executable regression groundwork only**. Published
alpha.18 remains named-call only. No runtime function values, closure syntax,
anonymous functions, captures, native function-value ABI, tags or releases
are introduced by this change.

## Identities and provenance

A semantic TypeId describes the canonical callable signature, **not** the
address of an executable declaration. Two named functions may share one
TypeId but must remain distinct execution targets. A composite TypeId is valid
only in the immutable TypeRegistry arena from which it came. Recompilations
create independent arenas, even when their types have the same shapes.

The current compiled program resolves direct calls to internal declaration
indices. Linked imports require their own declaration identity and visibility
rules. A future function reference must never use a naked array index,
serialized TypeId or raw host pointer as a dispatch target.

## Future noncapturing named-reference representation

A proposed handle contains: (1) strong ownership of the immutable compiled
program or linked-program snapshot, (2) an index validated against that exact
snapshot, and (3) a signature ID validated in that snapshot's registry.
Construction must reject missing functions, foreign IDs, wrong signatures
and unsupported backends. Namespaces and same-signature declarations must
not be confused. Stable cross-rebuild identity is not promised.

The first possible executable slice should be restricted to file-level
named functions with no captured environment. Source spelling and visibility
must be reviewed before a parser feature is introduced. Pure candidates can
fill impure callable slots, not the reverse. Parameters and result types are
invariant for now. Defaults are a declaration/call-adapter contract, not part
of the canonical callable type identity.

## Runtime and Lifetime invariants

Indirect dispatch must reuse the *existing* call machine: evaluate supplied
arguments once in source order, then defaults, check needs/ensures, preserve
the unit-return rule and call traces, enforce call count/depth/live-slot/work
budgets, and publish neither output stream following runtime failure.

An immutable compiled-snapshot reference can survive normal function-frame
retirement without borrowing frame-local memory. Safe Rust shared retention
is acceptable only with an acyclic ownership graph: an immutable program
must not hold a runtime reference back to itself. Host retention of old
compiled snapshots must be explicit and bounded. This design does not justify
claiming that a closure has escaped safely or that peak process memory is
measured by cumulative allocation budgets.

Lexical closures, returned nested functions and mutable captures require a
separate explicit capture mode, environment ownership policy, cycle analysis,
cleanup tests and retained-bytes quota. Do not introduce unsafe Rust or an
unreviewed collector. The current Lifetime implementation is scope
retirement plus immutable Rust-managed shared storage, not tracing GC.

## Acceptance gates before enabling syntax or native lowering

1. Checked construction and rejection of stale or foreign handles, plus
   same-signature different-declaration tests.
2. Differential direct/indirect calls with ordered named/default arguments,
   purity, contracts, recursion, unit results, atomic streams and limits.
3. Cross-module visibility and exact Unicode/CRLF diagnostic spans.
4. At-boundary and over-boundary tests for type arity, depth, component
   quotas, call depth/count, live slots, work and retained storage.
5. Drop/cleanup tests for noncapturing references after frame exit, separate
   cycle/capture tests before allowing any closure.
6. Green Rust 1.74, debug/release, Linux/Windows/macOS and extracted-package
   checks, and explicit native rejection for unsupported callable forms.

The associated tests only exercise presently supported compilation,
registry compatibility, ownership snapshots, module linking and *named*
execution. They must not be presented as first-class-function support.
