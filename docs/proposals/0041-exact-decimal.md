# Proposal 0041: bounded exact decimal scalars

Status: **Phase 1 host-side Rust arithmetic core; Scarp source-language support NOT implemented**.

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
- Cross-scale addition/subtraction checks aligned intermediates using i128 and applies the 28-digit coefficient limit **after** exact addition and canonicalization. A representable cancellation such as `10^27 - 0.1` is accepted; an intermediate exceeding i128 or a final nonrepresentable result is rejected.
- Decimal division uses bounded digit-by-digit arithmetic without expanding both operands to the maximum scale; it requires a target fractional scale and explicit rounding mode. Modes are HalfEven, HalfUp (ties away), TowardZero and AwayFromZero. Rounding and trailing-zero normalization precede the final 28-digit coefficient limit: representable results are not rejected merely because an intermediate includes an extra fractional zero. Division intermediates remain checked u128 values.
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

## Proposed Scarp syntax — design only

~~~scarp
bind price: decimal = decimal("199.99");
bind tax: decimal = decimal("0.18");
// Decimal arithmetic, explicit rounding and formatting: future work.
~~~

Do not publish this as an executable Scarp example: lexer/parser/resolver/value/runtime integration is still missing. The constructor spelling is a proposal, not a currently supported built-in.

## Implementation gates before source-language use

1. Lexer/parser/AST: decide contextual decimal keyword and explicit text constructor without changing float literal meaning.
2. Type checker/resolver: add a separate decimal type, reject implicit mixing with integer/float/bigint, support typed functions, constants, tuples and modules.
3. Runtime: add decimal values and checked arithmetic, comparison, division and diagnostic spans. Preserve atomic output on failure.
4. Resource budgets: cap literal length, conversions, memory, work and snapshots. Avoid host-dependent rounding.
5. Native backends: either implement correct decimal lowering or return an explicit unsupported-type diagnostic; never silently lower to C double or Rust f64.
6. End-to-end verification: successful and negative examples, scalar/variable/function/module tests, rounding boundaries, overflows, signed zero and differential oracle tests.
7. Docs and ecosystem: update Scarp language docs and by-example only after compiler-verified runnable support.

## Design questions requiring review

- Maintain 28 coefficient digits/18 fractional digits, or expand to 34 significant digits with larger exponent range?
- Should exact division error on repeating results by default, or require a rounding context for every division?
- Which fixed-width formatting API will be appropriate for currency-specific applications?
- How should decimal contracts and native/WASM serialization be implemented?

## Release discipline

Phase 1 introduces a host library and tests only. It does not make decimal available in .srp code, does not change a release tag, and must not be merged as a complete decimal language feature. Require passing cross-platform CI, code review, and a separate end-to-end integration PR before advertising source support.
