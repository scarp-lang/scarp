# Native assertions (unreleased development feature)

**Development status:** The syntax described here is implemented in the
stacked assertion branch, not in a published Scarp compiler. It depends on
the project foundation and test-reporting development PRs. No release or
pre-release was created as part of this work.

## Syntax

Use the `assert` statement to check an ordinary Scarp boolean expression:

```scarp
assert(2 + 2 == 4);
assert(2 + 2 == 4, "addition should be correct");
```

A semicolon is required. The second argument, when present, **must be a
quoted string literal** of at most 256 UTF-8 bytes; it cannot be an arbitrary
expression. `assert` is a reserved statement word, not a first-class
function, and is unavailable as a variable or function name.

The predicate must type-check as `boolean`. Examples such as
`assert(1);`, `assert("yes");`, `assert();`, and
`assert(true, 123);` are rejected before program execution. This
includes conditions inside branches that happen not to run.

## Runtime behavior

A predicate is evaluated **once, only if execution reaches the statement**.
Boolean `and` and `or` preserve their existing short-circuit semantics.
The assertion operation itself does not emit output; any permitted calls
inside its predicate retain their existing effects, input rules and failure
semantics. In a pure function, normal purity restrictions still apply.
A false predicate terminates execution with diagnostic
**E0065 — AssertionFailure**, optionally including the quoted literal
message. Codes E0062–E0064 are reserved for the separate fixed-array
feature, to keep error codes unambiguous when the branches are integrated.
Diagnostics retain source spans, including imports and nested
function calls, rather than silently discarding the failed location.
Errors in the predicate itself (such as division by zero) keep their original
diagnostics instead of being rewritten as failed assertions.

```scarp
pure funcra square_nonnegative(value: integer) -> integer {
    assert(value >= 0, "expected a nonnegative value");
    return value * value;
}
assert(square_nonnegative(3) == 9);
print("checks passed");
```

The `assert` statement works at file scope, in loops and conditionals,
and inside normal and `pure funcra` functions. Pure functions still
enforce their existing restrictions on impure calls in expressions.
Each assertion consumes the interpreter's ordinary deterministic execution
work budget; it does not introduce privileged execution or free unlimited
checks.

### Failure and output safety

```scarp
print("prepared");
assert(false, "intentional failure");
```

No user stdout is replayed if the assertion fails during the program's
existing buffered evaluation pass. The diagnostic is reported to stderr
by the CLI; in `scarp test --json`, its bounded text is stored in the
failed test result's `error` field. The assertion message is escaped in
diagnostics, so embedded newlines/control characters cannot create
ambiguous fake diagnostic lines.

### Static verification

`scarp verify FILE.srp` type-checks assertion predicates, but **does not
evaluate them**. As a result, `assert(false);` is valid for static
verification and fails only if a runtime invocation reaches it.

## Compiler integration

This feature uses the real parser, typed resolver, source-map rebasing,
module linker, interpreter, existing deterministic resource budgeting,
call-graph accounting, and binding-lifetime audit. It does not implement
a separate test-only evaluator. The experimental freestanding scalar C
backend still rejects assertions as outside its supported subset, rather
than silently dropping an assertion.

The feature is intended to be reviewed with Windows, Linux, macOS and
Rust 1.74 CI. It does not include `assert_eq`, property-based testing,
an assertion library, stacktrace redesign, remote test execution,
package installation, or a release.
