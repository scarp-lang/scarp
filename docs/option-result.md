# Concrete `option<T>` and `result<T, E>` — draft compiler slice

> **Development branch only.** This feature is not included in published
> Scarp v0.0.6-alpha.11. Its implementation, CI, and integration are still under
> review in the stacked draft PR. Do not describe it as a stable language API.

This slice introduces explicit, checked concrete specializations by lowering
them into Scarp's existing immutable nominal variant implementation. It does
**not** add user-defined type parameters or unchecked payload extraction.

## Types and construction

```srp
bind missing: option<integer> = new option<integer>::None();
bind present: option<integer> = new option<integer>::Some(42);

bind success: result<integer, string> =
    new result<integer, string>::Ok(42);
bind failure: result<integer, string> =
    new result<integer, string>::Err("invalid");
```

The constructors always carry explicit concrete type arguments. There is no
implicit unwrapping or contextual construction inference in this slice.
Spelling is lowercase `option`/`result` and title-case `None`, `Some`,
`Ok`, `Err`. The types can be nested with existing finite concrete source
types up to the existing compiler limits, but `unit` is not a payload type.

`option<integer>` and `option<string>` are distinct. Both parameters of
`result<T, E>` contribute to its identity. Every specialization is lowered
to a checked two-case nominal variant with the existing compiler type registry
and storage limits. Ordinary source `variant` declarations are unchanged.

## Safe extraction

```srp
match on present {
    case None { print("No value"); }
    case Some(value) { print(value); }
}

match on failure {
    case Ok(value) { print(value); }
    case Err(message) { print(message); }
}
```

The existing match checker enforces exhaustive coverage, case-local payload
bindings, and payload shape. A wrong payload or missing arm is diagnosed before
execution. Matching evaluates the subject once and retires scoped bindings
through the existing compiler plan.

## Limitations and next development steps

- **No explicit propagation yet.** Early returns must be spelled as ordinary
  `return` statements inside matching branches. A future slice must define
  and verify once-only expression evaluation, compatible enclosing function
  return types, postconditions, purity, retirement, and transactional output.
- This draft uses private synthetic nominal declarations for each concrete
  specialization. Cross-file/module equivalence is **not yet guaranteed**;
  canonical global specialization identity and linking require separate
  validation. Do not rely on exporting these types across independent modules.
- Constructor parameter inference, general generics, refutable nested tag
  patterns, and native aggregate lowering are not included. Native backends
  must retain their existing explicit composite-operation rejection.
- The compiler resource, syntax-cache, editor, platform CI, and minimum-Rust
  checks remain release gates. A green test on one runner alone is insufficient.

See `examples/typed_submission.srp` and
`tests/source_option_result.rs` for the executable contract and diagnostics.
