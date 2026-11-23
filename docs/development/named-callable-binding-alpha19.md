# Alpha.19: checked named callable identities (host-only)

Status: experimental Rust embedding API, not an executable source-language
feature. Stacked on alpha.19 type-system and Lifetime regression PRs.

## Implemented identity and compatibility contract

CallableCatalog shares an immutable Arc<ResolvedProgram>, with constructors
for an already shared or owned resolved program. bind_exact(index) verifies a
named declaration exists. bind(index, required) also validates the required
TypeId within the same program registry, applying the existing invariant
parameter/result rule and pure-to-impure compatibility.

NamedCallableRef retains the entire source-program snapshot alongside its
declaration index and canonical signature. Its metadata() retrieves static
FunctionInfo. same_target() requires identical snapshot and declaration, not
merely identical signatures; belongs_to() verifies snapshot provenance.
Recompiling the same source creates a different target and type arena.

Unknown declaration indices fail before expected-TypeId validation. Foreign
or forged composite type IDs produce UnknownType through the existing registry,
while a valid noncallable required type is incompatible. Successful handle
construction leaves the registry unchanged. Callers cannot construct the
opaque handle fields directly.

## Lifetime

The handles and catalogs retain a single immutable program using safe Rust
Arc ownership, without borrowing retired function frames. A Weak reference
test shows the snapshot surviving an original owner and catalog drop, and
being released after the final handle is dropped. The compiled program has
no back-reference to these handles, avoiding a newly introduced ownership
cycle. This is not a proof of process resident-memory reclamation.

Rust embedding hosts must still cap snapshots they retain. This milestone
does not add source-language callable allocation or a peak heap budget.

## Explicit non-goals

There is no invoke method, no runtime function Value, no Scarp syntax for a
function reference, no anonymous function, capture, closure, higher-order
execution, native function-value lowering or new CLI command. Binding by
declaration index is an inspection API for a resolved program, not a
module visibility or authorization boundary. Existing direct calls are
unchanged. Do not advertise first-class functions or publish a release.

The next milestone must introduce an independently verified indirect
dispatch adapter reusing argument evaluation order, defaults, needs/ensures,
recursion and work limits, call diagnostics, and atomic output without a
second evaluator. Public-module visibility and explicit rejection by native
backends remain separate review gates.
