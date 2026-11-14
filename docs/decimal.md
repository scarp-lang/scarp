# Exact decimal values — development compiler

> **Status:** Experimental source-language feature in draft compiler PR #248.
> The published Scarp v0.0.6-alpha.9 does **not** support this syntax.
> This guide is for checking the feature branch, not a release announcement.

Scarp decimal values are **base-10 exact numbers**, distinct from its finite
binary64 `float`, signed `integer`, and arbitrary-size `bigint` values.
Source expressions do not silently convert or combine these numeric types.

## Creating exact values

Use an explicit textual constructor instead of writing a binary float literal:

```scarp
bind price: decimal = decimal("199.99");
bind tax: decimal = decimal("0.18");
bind count = decimal_from_integer(3);
print(price + price * tax); // 235.9882
print(count);               // 3
```

The constructor accepts only optional ASCII sign, ASCII integer digits and
an optional dot followed by fractional digits (including digits before the dot).
Whitespace, exponent notation, thousands separators, NaN and infinity are
rejected. Input is limited to 96 UTF-8 bytes. Leading zeros and trailing
fractional zeros canonicalize, so `decimal("1.20") == decimal("1.2")`.

The **maximum is 28 canonical coefficient digits and 18 fractional places**.
This is a conservative bounded format, not IEEE decimal128 and not an
unbounded BigInt decimal.

## Arithmetic, comparisons and conversion

The operators `+`, `-`, `*` and `/` perform exact arithmetic. Equality
and ordering work across different decimal scales. Division accepts only
quotients exactly representable within both limits; for example:

```scarp
print(decimal("0.1") + decimal("0.2")); // 0.3
print(decimal("1") / decimal("8"));     // 0.125
// decimal("1") / decimal("3") reports E0069, not 0.333...
```

The remainder operator `%` and bitwise operators reject `decimal` at
compile time. There are no implicit conversions to or from other numeric
types. Use `decimal_from_integer` explicitly to preserve exactness;
`decimal_to_integer` requires a whole value within signed 64-bit range.
`decimal_to_text` returns canonical decimal text, while printing a decimal
also uses its canonical representation.

## Rounding

Explicit rounding modes are spelled `"half_even"`, `"half_up"`,
`"toward_zero"`, and `"away_from_zero"`. The scale argument is an integer
from 0 to 18:

```scarp
bind third = decimal_div_rounded(
    decimal("1"), decimal("3"), 2, "half_even");
print(third); // 0.33

print(decimal_round(decimal("1.225"), 2, "half_even")); // 1.22
print(decimal_round(decimal("-1.225"), 2, "half_up")); // -1.23
print(decimal_format(decimal("1.2"), 2, "half_even")); // 1.20
```

`decimal_round(value, places, mode)` returns a canonical decimal with
**at most** those fractional places. `decimal_format` returns a `string`
with **exactly** those displayed places, so it is suitable for presenting
currency without altering the stored exact value. Rounding happens only
when explicitly requested.

## Type integration and limitations

Decimal is a primitive Scarp type. Bindings, `var` assignments, function
parameters/results, imported functions, tuples and immutable arrays can carry
its exact value:

```scarp
pure funcra add(x: decimal, y: decimal) -> decimal {
    return x + y;
}
bind values: array<decimal, 2> =
    [array: decimal("0.1"), decimal("0.2")];
print(add(values[0], values[1])); // 0.3
```

Module constant initializers currently forbid all function calls, including
the text-based `decimal(...)` constructor; use a function or entry script.
Native C11 and freestanding ELF64 scalar backends deliberately reject decimal
programs with E0049 rather than emit a lossy binary64 conversion.

Diagnostics distinguish invalid decimal text or mode (E0067), coefficient or
scale limits (E0068), and inexact division/conversion (E0069). Division by zero
uses the existing E0008. Runtime diagnostics prevent replay of ordinary
buffered program output. Interactive prompts retain their established
separate visibility rules.

## Verification

This feature is tested in `tests/decimal_language.rs`, with its underlying
bounded Rust core covered by `tests/decimal_core.rs` and
`tests/decimal_cross_scale_differential.rs`. The runnable development fixture
is `examples/decimal_exact.srp`. Cross-platform CI must pass before any merge.
An authorized release and ecosystem synchronizations are separate tasks.
