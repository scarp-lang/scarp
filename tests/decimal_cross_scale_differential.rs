//! Deterministic, dependency-free differential tests for experimental host
//! Decimal. Source-language decimal is not implemented.
//!
//! An independent rational oracle expands input coefficients and powers of ten;
//! it does not use Decimal division or arithmetic to calculate expectations.

use scarp::decimal::{Decimal, DecimalError, RoundingMode};

fn next(state: &mut u64) -> u64 {
    *state = state
        .wrapping_mul(6_364_136_223_846_793_005)
        .wrapping_add(1_442_695_040_888_963_407);
    *state
}

fn coefficient(state: &mut u64) -> i128 {
    i128::from(next(state) % 2_000_000_001) - 1_000_000_000
}

fn pow10(scale: u8) -> i128 {
    10_i128.pow(u32::from(scale))
}

fn rational_oracle(
    numerator: i128,
    numerator_scale: u8,
    denominator: i128,
    denominator_scale: u8,
    target_scale: u8,
    mode: RoundingMode,
) -> Result<Decimal, DecimalError> {
    if denominator == 0 {
        return Err(DecimalError::DivisionByZero);
    }
    // Input coefficients <= 10^9; scales <= 9; target <= 18.
    // The independently expanded numerator is <= 10^36, within i128.
    let num = numerator * pow10(denominator_scale + target_scale);
    let den = denominator * pow10(numerator_scale);
    let unsigned_num = num.unsigned_abs();
    let unsigned_den = den.unsigned_abs();
    let quotient = unsigned_num / unsigned_den;
    let remainder = unsigned_num % unsigned_den;
    let increment = if remainder == 0 {
        false
    } else {
        match mode {
            RoundingMode::TowardZero => false,
            RoundingMode::AwayFromZero => true,
            RoundingMode::HalfUp => remainder * 2 >= unsigned_den,
            RoundingMode::HalfEven => {
                remainder * 2 > unsigned_den
                    || (remainder * 2 == unsigned_den && quotient % 2 != 0)
            }
        }
    };
    let magnitude = i128::try_from(quotient + u128::from(increment)).unwrap();
    let signed = if (num < 0) != (den < 0) {
        -magnitude
    } else {
        magnitude
    };
    Decimal::new(signed, target_scale)
}

#[test]
fn signed_mixed_scale_arithmetic_order_and_exact_division() {
    let mut seed = 0x0ddc_0ffe_e5ca_1e55_u64;

    for case in 0..10_000 {
        let a = coefficient(&mut seed);
        let mut b = coefficient(&mut seed);
        if b == 0 {
            b = 1;
        }
        let sa = (next(&mut seed) % 10) as u8;
        let sb = (next(&mut seed) % 10) as u8;

        let left = Decimal::new(a, sa).unwrap();
        let right = Decimal::new(b, sb).unwrap();
        let common_scale = sa.max(sb);
        let aligned_a = a * pow10(common_scale - sa);
        let aligned_b = b * pow10(common_scale - sb);

        assert_eq!(
            left.checked_add(right),
            Decimal::new(aligned_a + aligned_b, common_scale),
            "addition case {case}"
        );
        assert_eq!(
            left.checked_sub(right),
            Decimal::new(aligned_a - aligned_b, common_scale),
            "subtraction case {case}"
        );
        assert_eq!(
            left.checked_mul(right),
            Decimal::new(a * b, sa + sb),
            "multiplication case {case}"
        );
        assert_eq!(
            left.cmp(&right),
            aligned_a.cmp(&aligned_b),
            "comparison case {case}"
        );

        let product = left.checked_mul(right).unwrap();
        assert_eq!(
            product.checked_div_exact(right),
            Ok(left),
            "exact-division inverse case {case}"
        );
        assert_eq!(
            left.to_string().parse::<Decimal>(),
            Ok(left),
            "display/parse round trip case {case}"
        );
    }
}

#[test]
fn independently_rounded_rational_oracle_covers_all_four_modes() {
    let mut seed = 0xdec1_a1c0_ffee_2026_u64;
    let modes = [
        RoundingMode::HalfEven,
        RoundingMode::HalfUp,
        RoundingMode::TowardZero,
        RoundingMode::AwayFromZero,
    ];
    let scales = [0, 1, 2, 9, 18];

    for case in 0..10_000 {
        let a = coefficient(&mut seed);
        let mut b = coefficient(&mut seed);
        if b == 0 {
            b = 1;
        }
        let sa = (next(&mut seed) % 10) as u8;
        let sb = (next(&mut seed) % 10) as u8;
        let target = scales[(next(&mut seed) % scales.len() as u64) as usize];
        let numerator = Decimal::new(a, sa).unwrap();
        let denominator = Decimal::new(b, sb).unwrap();

        for mode in modes {
            assert_eq!(
                numerator.div_rounded(denominator, target, mode),
                rational_oracle(a, sa, b, sb, target, mode),
                "division case {case}: {a}/10^{sa} / ({b}/10^{sb}), target {target}, mode {mode:?}"
            );
            assert_eq!(
                numerator.round_to(target, mode),
                rational_oracle(a, sa, 1, 0, target, mode),
                "round-to case {case}: {a}/10^{sa}, target {target}, mode {mode:?}"
            );
        }
    }
}

#[test]
fn decimal_precision_edges_remain_explicit_and_exact() {
    let max = "9999999999999999999999999999".parse::<Decimal>().unwrap();
    let min_unit = "0.000000000000000001".parse::<Decimal>().unwrap();
    let max_negative = "-9999999999999999999999999999"
        .parse::<Decimal>()
        .unwrap();

    assert_eq!(max.checked_add(min_unit), Err(DecimalError::PrecisionExceeded));
    assert_eq!(
        max_negative.checked_sub(min_unit),
        Err(DecimalError::PrecisionExceeded)
    );
    assert_eq!(
        "1000000000000000000000000000"
            .parse::<Decimal>()
            .unwrap()
            .checked_sub("0.1".parse().unwrap())
            .unwrap()
            .to_string(),
        "999999999999999999999999999.9"
    );

    for mode in [
        RoundingMode::HalfEven,
        RoundingMode::HalfUp,
        RoundingMode::TowardZero,
        RoundingMode::AwayFromZero,
    ] {
        assert_eq!(
            min_unit.div_rounded(Decimal::from_integer(1), 18, mode),
            Ok(min_unit)
        );
    }

    assert_eq!(
        min_unit.checked_div_exact(Decimal::from_integer(10)),
        Err(DecimalError::Inexact)
    );
}