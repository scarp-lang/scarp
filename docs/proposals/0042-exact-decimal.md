# Proposal 0042: bounded exact decimal scalars

Status: **Phase 1 host-side Rust core merged**; **Phase 2 typed Scarp source-language prototype is draft PR #248 and not in a published release**.

Scarp's existing float remains finite IEEE-754 binary64. This proposal adds a separate exact base-10 numeric representation and does not rename, reinterpret, or silently coerce existing float, integer, or bigint values.

## Rationale

Many decimal fractions cannot be represented exactly by binary64; for instance, binary float arithmetic prints 0.30000000000000004 for 0.1 + 0.2. Financial and accounting applications benefit from exact decimal inputs and explicit rounding. AI and scientific workloads still need fast binary floats.

## Implemented Phase 1 Rust contract

- A Decimal value represents coefficient / 10^scale, with private signed i128 coefficient and u8 scale.
- Maximum 28 coefficient digits and 18 fractional digits. These are conservative limits, not decimal128 or IEEE-754 decimal128 compliance.
- Canonicalization removes negative zero and trailing fractional zeros. Therefore text 1.20 equals 1.2, and canonical Display prints 1.2.
- Strict text parsing allows optional sign, digits and an optional dot with digits on both sides. No scientific exponents, separators, whitespace, infinity or NaN. Text is capped at 96 UTF-8 bytes before scanning.
- Canonicalized precision checks remove redundant fractional zeros before testing the coefficient range. `Decimal::new` still rejects an incoming scale above 18.
- Exact numeric comparisons use a total mathematical order, including across different scales and negative values, without binary floating-point rounding or overflowing a full coefficient alignment.
- Checked exact addition, subtraction and multiplication report an error instead of silently rounding or falling back to binary64.
- Multiplication first cancels paired factors of 2 and 5 across the coefficients to avoid overflow of intermediates when the canonical exact product is representable; the final 28-digit precision ceiling remains enforced.
- Cross-scale addition/subtraction checks aligned intermediates using i128 and applies the 28-digit coefficient limit **after** exact addition and canonicalization. A representable cancellation such as `10^27 - 0.1` is accepted; an intermediate exceeding i128 or a final nonrepresentable result is rejected.
- Decimal division uses bounded digit-by-digit arithmetic without expanding both operands to the maximum scale; it requires a target fractional scale and explicit rounding mode. Modes are HalfEven, HalfUp (ties away), TowardZero and AwayFromZero. Rounding and trailing-zero normalization precede the final 28-digit coefficient limit: representable results are not rejected merely because an intermediate includes an extra fractional zero. Division intermediates remain checked u128 values.
- Exact-only division (`checked_div_exact`) rejects repeating or out-of-scale quotients with `Inexact`; it never silently rounds. Rounded division continues to require an explicit scale and rounding mode.
- `format_fixed(places, rounding_mode)` produces a fixed number of fractional places (for presentation such as currency), while stored decimals remain canonical and strip trailing zeros.
- Conversion to i64 requires an exact whole number and in-range value.
- Errors distinguish invalid syntax, scale excess, precision excess, division by zero and inexact conversion.
- No external dependency, unsafe code or unbounded coefficient storage.
- Cross-scale division is checked with independent scaled-integer rational oracle tests covering signs, rounding modes, and target scales; the initial core remains intentionally limited to 28 coefficient digits and 18 fractional digits.

### Host Rust API example

~~~rust
use scarp::decimal::{Decimal, RoundingMode};

let a: Decimal = "0.1".parse()?;
let b: Decimal = "0.2".parse()?;
assert_eq!(a.checked_add(b)?.to_string(), "0.3");

let third = Decimal::from_integer(1)
    .div_rounded(Decimal::from_integer(3), 2, RoundingMode::HalfEven)?;
assert_eq!(third.to_string(), "0.33");
# Ok::<(), scarp::decimal::DecimalError>(())
~~~

This is Rust API code, **not executable Scarp source**.

## Phase 2 experimental Scarp syntax — draft PR #248 only

~~~scarp
bind price: decimal = decimal("199.99");
bind tax: decimal = decimal("0.18");
bind total = price + price * tax;
print(decimal_format(total, 2, "half_even")); // 235.99
print(decimal_div_rounded(decimal("1"), decimal("3"), 2, "half_even")); // 0.33
~~~

This is executable **only on the reviewed source integration branch** once its CI gates pass. The published compiler remains v0.0.6-alpha.9, which does not support a decimal source type. For the draft syntax, see [the development guide](../decimal.md).

## Remaining gates before publishing source-level Decimal

1. **In prototype PR #248:** the parser recognizes the explicit `decimal` type name and constructor call without reinterpreting float literals or introducing a new literal token.
2. **In prototype PR #248:** resolver and runtime enforce a distinct Decimal type across bindings, functions, arrays, tuples and imported function signatures; no implicit numeric mixing.
3. **In prototype PR #248:** exact arithmetic and comparisons, typed E0067–E0069 errors and transactional output are exercised by end-to-end tests. The text constructor is not a valid module constant initializer because module constant calls are still forbidden.
4. Audit fixed-width precision/scale work charges, source span accuracy, format/rounding boundaries and output snapshots under reduced host limits.
5. Native backends must continue rejecting Decimal with E0049; do not lower to binary64. Implement exact native/WASM lowering in a separate design/review stage.
6. Require full Linux, macOS, Windows and minimum Rust CI on the final branch head, plus differential numeric tests and compiler compatibility checks.
7. Only after merge, review and explicit release approval may public docs, packages, playground and IDE ecosystems advertise `.srp` Decimal support.

## Design questions requiring review

- Maintain 28 coefficient digits/18 fractional digits, or expand to 34 significant digits with larger exponent range?
- Review the prototype decision that `/` is exact-only and rejects repeating quotients; rounded division requires a named rounding mode.
- Which fixed-width formatting API will be appropriate for currency-specific applications?
- How should decimal contracts and native/WASM serialization be implemented?

## Release discipline

The Phase 1 host library is merged without publishing any new tag. Phase 2 in draft PR #248 introduces source-language support but is **not** merged or released. Do not advertise `.srp` Decimal until that PR passes final CI and review, merges through the protected branch, and an explicitly authorized release includes the final code.
