# Generic functions

A file-level `funcra` can declare up to 64 type parameters in angle brackets.
Each parameter names one exact Scarp value type for an invocation. Type
parameters are visible in the signature, defaults, contracts, and body. Function
result types are still declared with `->`.

```scarp
pure funcra identity<T>(value: T) -> T {
    return value;
}

pure funcra pair<A, B>(first: A, second: B) -> tuple<A, B> {
    return (first, second);
}

pure funcra square<T: Numeric>(value: T) -> T {
    return value * value;
}

print(identity(100), identity("Scarp"), identity(true), identity(3.14));
print(pair::<integer, string>(100, "Scarp"));
print(square(0.1d));
```

Use the existing type names, such as `integer` and `string`, and Scarp's
`tuple<A, B>` annotation. Explicit type arguments use `::<...>` after the
function name. This keeps a call distinct from comparisons such as
`low < high` and uses the same angle-delimiter handling as nested collection
types. Imported functions use `alias.name::<...>(...)`.

## How inference works

Scarp matches explicit value arguments against the declared parameter types.
It can infer through tuples, fixed arrays, lists and the values of maps with a
concrete key type. Array lengths, tuple arity and map keys must agree. Repeated
uses of a type parameter must infer the same type. Numerical types preserve
their own precision and overflow behavior.

A declared binding type, return type or parameter default can supply any
remaining type arguments from the expected result. That expected type is also
available inside grouping, conditional result branches, and corresponding
collection elements when the surrounding type is known. Omitted value
arguments are evaluated as usual after the concrete type is selected.

```scarp
pure funcra empty<T>() -> list<T> {
    return [list: <T>];
}

bind names: list<string> = empty();
bind counts = empty::<integer>();
```

Use an explicit type argument for a call such as `print(empty());`, where
neither a value argument nor an expected result determines `T`. Supply either
all type arguments or none. Scarp resolves input expressions from the inside
out. A surrounding call's parameter type currently cannot infer a missing type
argument in another call, so use `outer(empty::<integer>())` in this case.

## Constraints

| Constraint | Accepted types | Operations |
| --- | --- | --- |
| Omitted | Any non-unit value type | Forwarding, equality, storage, and composition with compatible types |
| `Numeric` | `integer`, `float`, `decimal`, `bigint` | Addition, subtraction, multiplication, division, unary negation, and ordering |
| `Comparable` | Numeric types and tuples whose members are recursively comparable | Ordering, including the existing lexicographic tuple ordering |

The body is checked against its declared constraints, including when it is
unused. A `Comparable` parameter can be ordered but cannot be multiplied.
Integer-only operations still require a concrete `integer` where the ordinary
operator would require one. `Numeric` satisfies `Comparable`. Purity,
argument order, defaults, contracts and lexical scopes follow ordinary
[`funcra` rules](functions.md).

## Compilation and execution

The resolver validates each generic body with distinct type parameter identities.
It then lowers the concrete instances requested by entry code and ordinary
functions. One instance is retained per declaration and canonical list of
type arguments. Direct and mutual recursion reuse a previously requested
instance. Type-changing recursion is allowed within the same specialization
and type-shape budgets. The compiler reports a diagnostic when expansion
exceeds 256 distinct instances, 4 MiB of expanded source, 250,000 expanded
plan nodes, or existing type and shape limits.

Each instance uses normal interpreter frames and
[Lifetime](memory-lifetimes.md). Returned strings, lists and other immutable
values keep their existing shared backing storage. The concrete plans use the
same runtime budgets and diagnostics as ordinary functions. Tooling can inspect
source signatures and count retained instances with
`ResolvedProgram::generic_specialization_count()`.

Generic source calls currently run in the interpreter. Rust embedding can
invoke a concrete Scarp wrapper. User-defined generic records, variants and
aliases, parameterized `option<T>` or `result<T, E>` payloads, callable
values, user-defined constraints and native generic code generation remain
future work. Concrete records and variants, including already concrete
`option<integer>` and `result<integer, string>`, can be passed as type arguments.
