//! Host-side exact decimal API. Source-language decimal is NOT available yet.
use scarp::decimal::{Decimal, DecimalError, RoundingMode};

fn d(text: &str) -> Decimal {
    text.parse().unwrap()
}

#[test]
fn exact_decimal_addition_does_not_change_existing_float_results() {
    let exact = d("0.1").checked_add(d("0.2")).unwrap();
    assert_eq!(exact.to_string(), "0.3");
    let mut output = Vec::new();
    scarp::run("print(0.1 + 0.2);", &mut output).unwrap();
    assert_eq!(String::from_utf8(output).unwrap(), "0.30000000000000004\n");
}

#[test]
fn exact_accounting_example_and_explicit_rounding() {
    let subtotal = d("199.99");
    let tax = subtotal.checked_mul(d("0.18")).unwrap();
    assert_eq!(tax.to_string(), "35.9982");
    let total = subtotal.checked_add(tax).unwrap();
    assert_eq!(total.to_string(), "235.9882");
    assert_eq!(
        total
            .round_to(2, RoundingMode::HalfEven)
            .unwrap()
            .to_string(),
        "235.99"
    );
    assert_eq!(
        d("1")
            .div_rounded(d("3"), 2, RoundingMode::HalfEven)
            .unwrap()
            .to_string(),
        "0.33"
    );
    assert_eq!(
        d("1").div_rounded(d("0"), 2, RoundingMode::HalfEven),
        Err(DecimalError::DivisionByZero)
    );
}

#[test]
fn source_level_type_does_not_exist_until_separate_integration() {
    assert!(scarp::compile("bind amount: decimal = 10;").is_err());
}

#[test]
fn canonical_decimal_sorting_is_mathematically_ordered() {
    let mut values = [
        d("0.3"),
        d("-1.1"),
        d("1.000000000000000001"),
        d("0"),
        d("0.03"),
        d("-1.01"),
        d("1"),
        d("0.30"),
    ];
    values.sort();
    assert_eq!(
        values.iter().map(ToString::to_string).collect::<Vec<_>>(),
        [
            "-1.1",
            "-1.01",
            "0",
            "0.03",
            "0.3",
            "0.3",
            "1",
            "1.000000000000000001"
        ]
    );
}

#[test]
fn exact_arithmetic_matches_independent_small_scaled_integer_oracle() {
    // Distinct coefficients and scales prevent relying on the implementation's
    // own arithmetic to construct expected results.
    for a in -73i128..=73 {
        for b in -73i128..=73 {
            let x = Decimal::new(a, 2).unwrap();
            let y = Decimal::new(b, 2).unwrap();
            assert_eq!(x.checked_add(y).unwrap(), Decimal::new(a + b, 2).unwrap());
            assert_eq!(x.checked_sub(y).unwrap(), Decimal::new(a - b, 2).unwrap());
            assert_eq!(x.checked_mul(y).unwrap(), Decimal::new(a * b, 4).unwrap());
            assert_eq!(x.cmp(&y), a.cmp(&b));
        }
    }
}

#[test]
fn parsing_is_length_bounded_and_precision_checked() {
    use scarp::decimal::MAX_DECIMAL_TEXT_BYTES;
    let long_text = "0".repeat(MAX_DECIMAL_TEXT_BYTES + 1);
    assert_eq!(
        long_text.parse::<Decimal>(),
        Err(DecimalError::PrecisionExceeded)
    );
    assert_eq!(d("+00012.3400").to_string(), "12.34");
    assert_eq!(
        Decimal::new(10000000000000000000000000000, 1)
            .unwrap()
            .to_string(),
        "1000000000000000000000000000"
    );
}

#[test]
fn rounded_division_handles_large_mixed_scales_without_alignment() {
    let numerator = d("9999999999999999999999999999");
    let denominator = d("99999999999999999999999999.99");
    assert_eq!(
        numerator
            .div_rounded(denominator, 18, RoundingMode::HalfEven)
            .unwrap()
            .to_string(),
        "100"
    );
    assert_eq!(
        d("0.000000000000000001")
            .div_rounded(d("2"), 18, RoundingMode::HalfEven)
            .unwrap()
            .to_string(),
        "0"
    );
    assert_eq!(
        d("0.000000000000000001")
            .div_rounded(d("2"), 18, RoundingMode::HalfUp)
            .unwrap()
            .to_string(),
        "0.000000000000000001"
    );
    assert_eq!(
        d("-0.5")
            .div_rounded(d("1"), 0, RoundingMode::HalfEven)
            .unwrap()
            .to_string(),
        "0"
    );
    assert_eq!(
        d("-0.5")
            .div_rounded(d("1"), 0, RoundingMode::HalfUp)
            .unwrap()
            .to_string(),
        "-1"
    );
}

#[test]
fn decimal_division_matches_rational_integer_oracle() {
    for a in -9i128..=9 {
        for b in -9i128..=9 {
            if b == 0 {
                continue;
            }
            for sa in 0..=2u8 {
                for sb in 0..=2u8 {
                    for scale in 0..=4u8 {
                        for mode in [
                            RoundingMode::HalfEven,
                            RoundingMode::HalfUp,
                            RoundingMode::TowardZero,
                            RoundingMode::AwayFromZero,
                        ] {
                            let lhs = Decimal::new(a, sa).unwrap();
                            let rhs = Decimal::new(b, sb).unwrap();
                            let num = a * 10_i128.pow(u32::from(sb + scale));
                            let den = b * 10_i128.pow(u32::from(sa));
                            let q = num.abs() / den.abs();
                            let r = num.abs() % den.abs();
                            let increment = match mode {
                                RoundingMode::TowardZero => false,
                                RoundingMode::AwayFromZero => r != 0,
                                RoundingMode::HalfUp => r * 2 >= den.abs(),
                                RoundingMode::HalfEven => {
                                    r * 2 > den.abs() || (r * 2 == den.abs() && q % 2 != 0)
                                }
                            };
                            let magnitude = q + i128::from(increment);
                            let signed = if (num < 0) != (den < 0) {
                                -magnitude
                            } else {
                                magnitude
                            };
                            let expected = Decimal::new(signed, scale).unwrap();
                            let actual = lhs.div_rounded(rhs, scale, mode).unwrap();
                            assert_eq!(
                                actual, expected,
                                "divide {a}/10^{sa} by {b}/10^{sb}, target {scale}, mode {mode:?}"
                            );
                        }
                    }
                }
            }
        }
    }
}
