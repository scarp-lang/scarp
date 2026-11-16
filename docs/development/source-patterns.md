# Structured binding patterns

Use braces to select named record fields. The order is independent of the record
declaration, and fields you leave out are ignored. The part after each colon is
a binding, a discard, or another pattern.

```scarp
record Button { label: string, position: tuple<integer, integer>, active: boolean }
bind button = new Button { label: "Save", position: (4, 9), active: true };
bind {position: (x, y), label: name} = button;
print(name, x, y);
```

`var` makes the resulting bindings mutable. Assignment to a binding replaces
its value. With either declaration form, the initializer evaluates once before
the new names are visible.

The same patterns work inside a tagged case.

```scarp
variant Event { Click(record Button), Resize(tuple<integer, integer>), Idle }
match on new Event::Click(button) {
    case Click({position: (x, _), label: title}) { print(title, x); }
    case Resize((width, height)) { print(width, height); }
    case Idle {}
}
```

An array pattern such as `[first, _]` must have the length of the fixed-array
type. A tuple pattern needs the tuple's full arity. Use `(value,)` for a
one-element tuple pattern. Every group needs at least one entry. Use `_` to
ignore an entire value. Duplicate fields and duplicate binding names are
errors. Match bindings are immutable and visible only in their arm.

The compiler checks each shape statically, including shapes in unselected
arms. Matching still dispatches by tag. A well-typed payload pattern succeeds
for every value of the declared payload type. Nested tag tests, literal
patterns, and guards have separate coverage semantics and remain future work.

The interpreter evaluates the subject once and charges each nested pattern
visit only when that arm runs. The C11 and ELF backends report E0049 for these
operations.

Run the panel event example with `scarp exec examples/ui_event_patterns.srp`.
