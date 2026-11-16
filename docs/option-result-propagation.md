# Explicit `?` propagation — experimental draft

> **Not published.** This is a stacked development change in draft PR #260,
> depending on #259. It is not part of the released v0.0.6-alpha.11 compiler.

## Language rule

Only an **immutable binding initializer inside a named `funcra`** may
currently use the trailing `?` marker:

```srp
pure funcra parse_title(input: option<string>) -> result<string, string> {
    match on input {
        case None { return new result<string, string>::Err("missing"); }
        case Some(value) { return new result<string, string>::Ok(value); }
    }
}

pure funcra make_title(input: option<string>) -> result<string, string> {
    bind title: string = parse_title(input)?;
    return new result<string, string>::Ok(title);
}
```

This is **not** an arbitrary expression-level operator. It does not support
`var`, destructuring, assignment, output arguments, or a bare return operand.
The initializer is evaluated exactly once. On success, its payload is bound to
the local name. On absence/failure, the function returns immediately through
the usual return completion and postcondition path. No panic or implicit
unchecked extraction is exposed.

## Static compatibility

- `option<T> ?` requires the enclosing function to return `option<U>`.
  `None` forwards as `None`; `Some(T)` binds `T`. `T` and `U` may
  differ.
- `result<T, E> ?` requires enclosing return type `result<U, E>` with the
  **identical concrete `E` type**; `Err(E)` forwards and `Ok(T)` binds `T`.
  There are no implicit error conversions.
- `bind name: T = expression?;` checks the declared type against **the
  extracted payload** `T`, not the source wrapper.
- If the source is neither Option nor Result, or there is no compatible
  enclosing function return type, compilation fails at the question mark.

```srp
pure funcra inspect(value: option<integer>) -> option<boolean> {
    bind count = value?;
    return new option<boolean>::Some(count > 0);
}
```

## Runtime contract

The original expression's existing checked, bounded execution plan runs to
completion before the propagation decision. On the successful path, its
payload is stored in the new immutable slot. On the failure path, the
interpreter constructs a checked outer variant and uses normal function
completion: failed `ensures` contracts still fail, caller frames receive a
return value when allowed, and implementation-specific resource limits remain
in effect. Storage retirement is audited on the success continuation; failure
exits without executing subsequent statements.

No extra native lowering is provided. C11 and object backends must reject
functions and aggregates as before; they must never quietly ignore `?`.

## Tests and boundaries

- Golden fixture: `examples/typed_propagation.srp`.
- Regression file: `tests/source_propagation.rs`.
- Linux and PowerShell compiler checks and all three package smoke suites
  include the golden example.
- CI and human review are required. This is not yet a stable language feature.
- **Follow-ups:** safe cross-module canonical identity for concrete generic
  wrappers; arbitrary expression propagation; error conversion contracts;
  full module/editor and 512 KiB small-stack audits; language specification
  review and wider platform qualification.
