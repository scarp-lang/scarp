# Experimental Scarp Data Engine — nominal host foundation

**Status:** Development-only host Rust API, not source-language syntax.
**Baseline:** protected main at 49726ad96fccc429fa16c02628f56ec0cf54e5d3.
**Release policy:** No tag, no package publication, no stable ABI promise.

## Why this exists

Scarp already has checked primitive types, interned structural descriptors,
immutable tuples and fixed-size arrays. Those foundations do not themselves
provide user-defined records, tagged variants, dynamically sized collections,
typed options/results, or pattern matching. Adding only lexer tokens for those
features would create semantic holes in modules, limits, the interpreter, and
the experimental native compiler.

This first implementation deliberately isolates an executable semantic model
that host Rust programs and tests can exercise without changing what current
Scarp source code accepts.

## Scope

- Nominal record and tagged-variant schemas using existing DefinitionId.
- Ordered record fields with exact types; explicit case names and payloads.
- Existing SourceType and Value are reused for scalar, tuple, and fixed-array
  leaves, without silently widening an integer into a float or BigInt.
- Typed, immutable, persistent dynamic lists.
- Typed maps with either signed integer or UTF-8 text keys.
- Deterministic key order and duplicate-key rejection.
- Explicit, typed optional and outcome values.
- Immutable record/list/map snapshots; functional update operations.
- Recursive graph verification with node, depth, entry and text limits.
- Structural patterns: wildcard, binder, literal, records, variants, lists,
  optional and outcome matching.
- Conservative coverage analysis for tagged-variant match arms.
- Source-agnostic Rust API tests; no new dependencies.

## Explicitly not implemented

- No `.srp` grammar for `record`, `variant`, `list`, `map`, `some`,
  `none`, `ok`, `err`, `match`, or destructuring.
- No new public Scarp primitive value kinds.
- No compiler-owned AST, source span or diagnostic code for these constructs.
- No source-level exhaustive checking or editor symbol-index integration.
- No mutable aliases, in-place collection mutation, recursive self-mutation,
  user-visible pointers, garbage collector configuration, or unsafe Rust.
- No generic functions, traits, higher-order closures, or field-method syntax.
- No binary ABI or native/bytecode code generation.
- No general JSON codec, network or persistence integration.
- No claim that Rust host limits are interchangeable with interpreter budgets.
- No release request or automatic publication.

## API model

The engine lives in `src/data_engine/` and exports its public types through
`scarp::data_engine`. The value and schema subsystems are separated.

A `DefinitionId` uses the same module/declaration pair as the existing
compiler type registry. It is **not** a cross-process persistent identifier
and must not be serialized as a durable type ID.

`DataType::Source(SourceType)` is the only bridge to current Scarp types.
Other arms describe nominal records, nominal variants, dynamic lists, maps,
optional values, and typed outcomes.

`SchemaRegistry` owns a bounded set of declarations. A registration either
passes validation entirely or has no effect. An equivalent member list with
a distinct `DefinitionId` remains a distinct nominal type.

Names use the current source identifier convention: ASCII letter/underscore
at the start and ASCII letters, digits or underscore subsequently. The host
can register equal display names under different nominal IDs: the compiler
must eventually enforce module lexical lookup and visibility separately.

Referenced non-self nominal types must already be registered. A record or
variant may reference its own identity because the representation stores that
identity as a leaf rather than recursively expanding the type graph. Recursive
runtime data is possible through optional, list or variant indirection, but
the immutable values are created bottom-up rather than constructing cyclic
Rust references.

Registration costs are bounded by maximum definitions, maximum members,
type-expression depth and cumulative type nodes. A failed registration does
not consume the cumulative type-node budget.

## Value semantics

Every `Datum` is a shared or copied immutable value. Record, variant, list
and map fields cannot be set directly because their backing payload is
private. The only mutation-like APIs produce a *new* verified snapshot.

A `RecordValue` stores its nominal declaration identity and field values
in the order of the schema. `record_named` validates exact membership and
canonicalizes order. Duplicate and missing fields fail.

A `VariantValue` stores its declaration identity, case index and optional
payload. Case index is internal: `case_name` is the stable host-facing
inspection path. A unit case cannot carry a value; a payload case must carry
exactly the declared type.

A `ListValue` preserves one element type even when empty. Creation, append,
replacement and removal validate the resulting value. Previously returned
lists remain unchanged.

A `MapValue` preserves the key and value types even when empty. Entries are
sorted by signed integer order or UTF-8 byte order. Duplicate entries fail,
and inserting an existing key creates a new map with the new value. There is
no undefined hash iteration order, locale-dependent comparison, or silent
duplicate key overwrite on initial construction.

Optional and Outcome values have distinct cases and are verified against
their declared type, not inferred from a bare null sentinel.

## Validation and resource accounting

`verify` traverses a bounded stack of type/value pairs instead of recursively
calling Rust functions for each nested datum. It checks:

- Exact type identity (nominal or structural).
- Correct record field count and exact type per field.
- Correct variant case index and payload presence.
- Homogeneous collection contents with exact declared element types.
- Sorted, unique and correctly typed map keys.
- Optional/result payload type and case consistency.
- Maximum traversal depth.
- Total logical visited nodes.
- Maximum entries per collection.
- Cumulative visible UTF-8 string and text-key bytes.

Checks are deterministic for a fixed schema and input. The text quota counts
visible UTF-8 bytes, not Unicode scalar count or grapheme clusters. Separate
Scarp numerical/bigint payloads continue to belong to their existing type
engines. The logical quotas are deliberately **not** stated as exact operating
system memory quotas.

The current implementation uses ordinary fallible `Result` paths for
validation. It does not rewrite the existing interpreter's resource ledger.
That integration must preserve the current atomic-output guarantee.

## Pattern semantics

`match_pattern` accepts an immutable host pattern and a datum. It returns
either `Some(bindings)`, `None` for an ordinary mismatch, or a typed error.

- Bindings are cloned immutable snapshots, never mutable references into
  a live writer.
- Duplicate binder names are rejected, not treated as implicit equality.
- Record patterns can select a subset of named fields.
- Variant patterns select the exact nominal type/case.
- List patterns require the exact pattern length.
- Optional/outcome patterns select the case first, then match payloads.
- A matched pattern never partially publishes bindings on a failed match.
- Pattern traversal has an independent work/depth budget.

`variant_coverage` conservatively determines whether all cases have
unconditional top-level arms, identifies missing cases in declaration order,
and marks arms unreachable after a catch-all or fully covering earlier arm.
A payload pattern with literal or nested field conditions cannot claim to
cover its entire case. This prevents false exhaustive-match guarantees.

The future resolver should lower source pattern branches only after resolving
names, exhaustiveness, conditional guards, and source spans. This host API
does not replace compiler analysis.

## Example host use

```rust
use scarp::data_engine::{
    DataType as T, Datum as V, Definition, Field, SchemaRegistry,
};
use scarp::types::DefinitionId;

let id = DefinitionId { module: 4, declaration: 1 };
let mut schemas = SchemaRegistry::default();
schemas.register(Definition::record(
    id,
    "Student",
    vec![
        Field::new("name", T::string()),
        Field::new("age", T::integer()),
    ],
))?;

let original = schemas.record_named(id, vec![
    ("age".into(), V::integer(20)),
    ("name".into(), V::text("Ada")),
])?;
let newer = schemas.replace_field(&original, "age", V::integer(21))?;
assert_eq!(schemas.field(&original, "age")?, &V::integer(20));
assert_eq!(schemas.field(&newer, "age")?, &V::integer(21));
# Ok::<(), scarp::data_engine::DataError>(())
```

## Host compatibility and testing

This change adds one opt-in public module, leaving existing
`scarp::compile`, `scarp::run`, `scarp check`, `scarp exec`, module linking,
numeric kernels and existing Value/SourceType representations unchanged.

The conformance suite covers nominal identity, schema registration rollback,
duplicate field/case rejection, field ordering, correct and incorrect payload
types, optional/outcome propagation, collection ordering, immutable updates,
nested value verification, pattern bindings, false exhaustiveness prevention,
exact limit boundaries, UTF-8 byte accounting, deterministic permutations,
and safe sharing across Rust threads.

Before accepting this branch as an integration foundation, require:

1. `cargo fmt --all -- --check`
2. `cargo clippy --locked --all-targets -- -D warnings`
3. `cargo test --locked`
4. `cargo test --locked --release`
5. `cargo build --locked`
6. Existing editor/module/native-object integration tests
7. Linux, Windows, macOS and Rust 1.74 matrix at the exact PR head
8. Independent API review for public type stability and limit semantics

## Integration beyond this milestone

These are explicit further implementation phases, not already completed work.

**Language and AST:** Define original `.srp` grammar and source spans for
record declarations, tagged variants, typed list/map constructors, field
projection, optional/outcome values, patterns and `match`.

**Resolver:** Adopt nominal definition identities, introduce field visibility,
recursive schema lookup and exhaustive branch analysis. Resolve against
current SourceType/TypeRegistry boundaries, not a second type checker.

**Runtime:** Lower compiler-resolved record/list/map operations to verified
values, share the existing interpreter's work/allocation/print accounting, and
avoid silently charging a skipped match branch.

**Modules:** Export/import nominal type declarations with visibility and
module identity; ensure imported declarations cannot alias or spoof local ones.

**Tooling:** Symbol references for fields/cases; diagnostics with original
UTF-8 byte spans; versioned JSON inspection and editor API updates.

**Code generation:** Reject unsupported composite values explicitly from the
existing restricted scalar C and ELF object backends. Only later add verified
IR lowering and native data layout.

**Additional collections:** Deterministic sets, generic algorithms, richer
iterators, and sized bounds are separate reviewable extensions.

**Release gate:** This branch must stay development-only until reviewed and
merged through normal repository protections. No prerelease or stable release
authorization is implied.

## Extended host semantic layers on this branch

These parts are subsequent development work within this draft PR, not part of
a released Scarp source-language feature:

- `paths.rs`: exact typed nested selection, immutable leaf replacement, and
  transactional ordered patch execution for records, variants, typed lists,
  maps, optionals and outcomes.
- `generics.rs`: first-order type templates and exact substitution with
  constraints and a bounded cache keyed by complete schema contents.
- `schema_graph.rs`: forward-linked atomic nominal declarations, dependency
  graph analysis, cycle detection and deterministic invalidation queries.
- `query.rs`: pure typed predicate preflight, projection, integer sum, key
  grouping, and explicit scan/work/output quotas.
- `index.rs`: persistent typed secondary indexes, deterministic text-prefix
  and integer-range lookup, original-row materialization and bounded joins.
- `diff.rs`: deterministic structural patches with maximum graph-visit,
  operation, and recursion-depth budgets; roundtrip reconciliation checks.
- `wire.rs`: experimental type-checked canonical value codec v1, with
  strict decoding and no stable ABI guarantee.
- `wire_schema.rs`: opt-in v2 exact schema descriptor envelope that checks
  declared member names, order, variant cases, roots, nominal identities
  and the entire registered schema before decoding the v1 value payload.

### Wire schema boundaries

The v1 raw data codec requires both parties to already agree upon the exact
schema and does **not** embed that schema in the payload. It is inappropriate
to assume that equal-layout nominal types can be distinguished from v1 bytes
alone. For safer interchange, prefer the exact-schema v2 envelope.

The v2 envelope is intentionally strict: the complete ordered schema content
must match, after canonicalizing declaration registration order by nominal
identity. Additional unrelated registered definitions cause the decoder to
reject the envelope. This is a developmental format with no migration plan
or long-term persistence promise yet.

The v2 descriptor is neither cryptographically signed nor a substitute for
authenticating data. Type IDs remain build-local logical coordinates; callers
must independently determine that they trust the schema source. Encoded
metadata and value bytes are bounded separately and by the total envelope
quota. Malformed sizes and trailing payloads are rejected.

### Scope boundary

This is still **host API only**. No new `.srp` parser, type declarations,
destructuring, generic function calls, pattern clauses or `match` syntax
has been enabled. Do not interpret passing Rust host tests as compiler-native
feature completeness. The compiler's existing public language behavior and
released CLI remain unchanged.

## Experimental schema evolution and migration

The host data engine now also contains a semantic schema comparison API and an
explicit source-to-target schema migration API. The comparison identifies
nominal definition changes, field additions/removals/reorders, type changes,
variant case changes, and transitively affected definitions using the
schema dependency graph.

Comparison risk classifications are conservative and are **not** a guarantee
that code using an old schema will compile against a newer one. Adding a
variant case is considered breaking because exhaustive matches can become
incomplete. Registration-order differences alone do not imply a schema
change; member order and names do.

Migration preserves immutable values and refuses implicit numeric coercion,
renaming or data loss. Added fields require explicit, type-checked defaults.
Removing fields requires an explicit caller option; missing or renamed variant
cases are errors. The runtime validates both source and destination data,
and charges bounded migration visits and depth independently of the existing
Scarp interpreter.

Importantly, semantic compatibility and binary interchange are different:
the strict schema-bound v2 envelope still rejects different complete schema
registries, even when a semantic comparison classifies a change as compatible.
Future language integration needs explicit migration syntax, compiler
resolution and versioned persistent schema IDs before this can serve as a
stable serialized application data model.

## Typed value inspection

The host-side inspection walker returns deterministic root-relative paths,
type descriptors and value categories for verified immutable data. It visits
nominal fields in declaration order and map entries in key order, and can
inspect nested variants, option/result cases and collections without executing
user code. The reader enforces independent maximum depth and visit quotas,
and reports structural node counts plus visible string payload byte totals.
The generated paths are accepted by the experimental `select_path` API.

Source-language tuple/array internals remain opaque in this visitor because
they do not yet have a stable typed path-segment representation. No inspector
API is currently exposed from a Scarp `.srp` program, and the visitor's
snapshots are not part of a stable debugger protocol.

## Initial source-language integration: flat destructuring

The first compiler-native use of structural data is intentionally narrow and
builds upon **existing source tuples and fixed-size typed arrays**:

```srp
bind (age, name) = (18, "Ada");
var [first, second] = [array: 4, 7];
first += second;
print(age, name, first);

bind (_, final) = (true, 42);
print(final);
```

The compiler now represents a destructuring declaration as a first-class AST
statement with exact source-byte spans, validates tuple/array arity and element
types using its **existing** `SourceType` registry, assigns each bound element
a unique lexical storage slot, and evaluates the initializer exactly once in
the resumed interpreter. Mutable `var` bindings retain normal assignment
rules; immutable `bind` bindings remain immutable. `_` discards a value and
allocates no binding slot. Destructuring positions are charged to the existing
runtime work budget before any new slots are written.

Source pattern syntax is **flat only**, not recursively nested:
- `bind (left, right) = tuple;`
- `bind (single,) = singleton_tuple;`
- `var [first, second] = fixed_array;`
- `bind (_, retained) = tuple;`

The array pattern is bounded to 128 elements and a tuple pattern to the
existing maximum tuple arity (32). Empty patterns are rejected; a singleton
tuple pattern requires the trailing comma. All pattern names are validated
against reserved names, duplicate declarations and lexical scope.
Malformed/mismatched patterns fail before program execution or output.

The module source rewriter preserves one-time evaluation and indexes the new
bindings for editor symbol/navigation information. Existing C and native
object backends explicitly reject these composite operations until supported
by their type-safe lowerers; they must not silently emit scalar code.

**This is not nominal record/variant syntax.** The host `SchemaRegistry`
and `Datum` representation remain opt-in Rust APIs; the existing `.srp`
`SourceType` and `Value` representations remain authoritative for runtime
tuple and array values. A shared type identity, pattern AST and interpreter
bridge for nominal values is a larger follow-on compiler milestone.
