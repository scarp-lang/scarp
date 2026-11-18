# Concrete `option<T>` and `result<T, E>`

Scarp alpha.12 supports explicitly typed Option and Result as immutable
nominal variants. Use a checked match or [explicit propagation](option-result-propagation.md)
to extract a payload.

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

## Module interfaces and limits

The linker gives identical concrete specializations shared identity across
imports. Nominal payload types retain their declaring module's identity and
visibility. A public interface must expose only public nominal payload types.

Constructor type arguments remain explicit. Refutable nested tag patterns and
general generics are future work. The experimental native backends diagnose
unsupported aggregate programs before emission.

See `examples/typed_submission.srp` and
`tests/source_option_result.rs` for the executable contract and diagnostics.
