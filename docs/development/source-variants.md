# Tagged variants and exhaustive matches

A variant names mutually exclusive cases. Each case has zero or one payload. A record or
tuple can carry several pieces of data in one payload.

```scarp
variant Lookup { Missing, Found(string) }

pure funcra describe(value: variant Lookup) -> string {
    match on value {
        case Missing { return "No result"; }
        case Found(text) { return text; }
    }
}

print(describe(new Lookup::Found("Scarp")));
```

The compiler checks the subject once and validates every arm. A tagged arm matches that
case. `case Found(_)` explicitly discards its payload. [Structured patterns](source-patterns.md) can unpack tuple, fixed-array, and record payloads. `case _ { ... }` handles remaining
cases and must be last. Duplicate arms, unreachable wildcards, and missing cases produce
diagnostics. A payload binding is immutable and local to its arm. When every arm returns,
the match can satisfy a function's return requirement.

Use `pub variant` to export a type, `variant module.Lookup` in annotations, and
`new module.Lookup::Found("Scarp")` to construct it. A public variant can use public
nominal payload types. Source records, variants, tuples, and fixed arrays share the
existing bounds for acyclic composite layouts. A variant has up to 32 cases. Values
compare by nominal type, case, and payload. A payload is shared when its variant is
copied. The `--max-tuple-bytes` allowance covers tuple, record, and variant allocations.

Run the typed submission example with `scarp exec examples/variant_validation.srp`.
The interpreter supports matching and construction. The scalar native backends report
E0049 for these operations. Module inventory JSON uses schema 5 when the graph contains
variant declarations.

This milestone supports tagged statement matches, case-local bindings, and wildcards.
Generic `Option` and `Result`, expression matches, nested variant tests, guards, and native
aggregate lowering are follow-up work.
