# First-class functions — alpha.19 foundation (development only)

Status: **experimental type-system milestone**, not a source-language feature.
Baseline: published `v0.0.6-alpha.18`. This change must not modify the
published alpha.18 source, tag, release requests or release assets.

## What exists today

Named `funcra` and `pure funcra` declarations are statically resolved,
with typed arguments, defaults, `needs`/`ensures`, recursion and bounded
runtime call frames. Functions are not runtime `Value`s. Source-language
function types, anonymous functions, nested declarations, captures, passing
functions as arguments and returning functions are **not yet implemented**.

The semantic registry already interns `TypeKind::Function` descriptors for
named functions. This milestone adds a checked, read-only
`TypeRegistry::accepts_callable(expected, candidate)` query over these
descriptors, including strict identity and effect compatibility.

## Initial callable compatibility rule

* Both `TypeId` handles must exist in the **same registry**; unknown/foreign
  handles return `UnknownType` rather than being confused with local IDs.
* A non-function type is never callable-compatible.
* Parameter count, parameter order, exact parameter types and result type must
  match. This deliberately starts with *invariant* types; variance, coercions
  and generic instantiation are deferred.
* Effects are ordered: a pure callable is safe where an impure callable is
  accepted, but an impure callable cannot be used where purity is required.
* Calls and callable-value construction remain unchanged. The helper does not
  permit assignment, indirect dispatch, default-argument conversion, or
  effect inference in `.srp`.

This conservative rule can be used by future lowering and verifier passes
without giving source programs access to partially implemented function values.

## Proposed staged implementation

1. **Semantic signatures (this milestone):** share the existing canonical
   function type registry, reject foreign IDs, define purity compatibility,
   and test real resolved function signatures and resource limits.
2. **Named function references:** design exact source spelling, typed storage,
   global-vs-local lookup, exported symbol identity, argument defaults and
   `unit` return handling. Add explicit parser diagnostics before enabling
   value construction.
3. **Indirect dispatch:** add a callable runtime value with immutable identity;
   validate the signature before invocation, preserve argument evaluation
   order, the current call-count/depth/live-slot quotas, contract behavior,
   short-circuiting and atomic output.
4. **Anonymous functions:** specify syntax and static return/effect checking
   before supporting nested forms.
5. **Lexical captures / Lifetime:** freeze capture semantics (value snapshots
   versus shared mutable cells), prohibit accidental reference cycles and
   dangling environments, preserve frame retirement, and measure cumulative
   and live-memory budgets. The currently published Lifetime system does not
   solve arbitrary captured environments or cycle collection.
6. **Modules, tooling and native backends:** define stable declaration
   identities, structured inspection metadata and honest unsupported-target
   diagnostics. Do not advertise native closure support until independently
   validated.

## Required acceptance tests for later milestones

* Zero/one/many parameters, `unit` result, param and result mismatch,
  nested function type signatures, both purity directions.
* Forward references, exports/imports, shadowing, recursive calls and
  default arguments under indirect dispatch.
* Closures returned after their defining frame exits; copied immutable
  captures; mutation policy; nested captures; cleanup and cycle tests.
* Exact Unicode/CRLF spans and related locations on failed indirect calls.
* Limits for AST/type complexity, active frames, calls, live slots, work
  steps and retained capture bytes, including exact-limit and over-limit.
* Interpreter/module/CLI parity, and explicit native rejection until native
  lowering exists. Keep Rust 1.74 and all OS/package jobs green.

## Verification of this milestone

```sh
cargo fmt --all -- --check
cargo test --locked --test callable_types
cargo test --locked --test type_registry
cargo test --locked --test functions
cargo test --locked
```

Do not tag or publish `v0.0.6-alpha.19` through this PR. Keep it in draft
until the tests and maintainers' semantic review are complete.
