# Explicit `?` propagation

Scarp alpha.12 introduced explicit propagation for concrete Option and Result
in the interpreter.

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
interpreter returns a variant of the enclosing function's result type through
normal completion. An `ensures` contract still runs before control returns to
the caller. On current development source, forwarding shares an existing
immutable error payload, and forwarding `None` needs no payload allocation.
The alpha.12 binary debits the variant quota on both early-return paths. Storage retirement is audited on the success continuation; failure
exits without executing subsequent statements.

No extra native lowering is provided. C11 and object backends must reject
functions and aggregates as before; they must never quietly ignore `?`.

## Tests and boundaries

- Golden fixture: `examples/typed_propagation.srp`.
- Regression file: `tests/source_propagation.rs`.
- Linux and PowerShell compiler checks and all three package smoke suites
  include the golden example.
- `tests/source_option_modules.rs` checks canonical concrete type identity
  across imports and propagation with an exact cross-module error type.
