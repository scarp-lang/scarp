//! Experimental bounded exact base-10 arithmetic for future Scarp decimal.
//!
//! This is a host-side building block, NOT a Scarp source-language type yet.
//! Decimal = coefficient / 10^scale. No binary float fallback.

use std::{fmt, str::FromStr};

/// Initial conservative precision, independent of binary64 floats.
pub const MAX_DECIMAL_DIGITS: u32 = 28;
/// Maximum fractional digits.
pub const MAX_DECIMAL_SCALE: u8 = 18;
const MAX_COEFFICIENT: i128 = 10_i128.pow(MAX_DECIMAL_DIGITS) - 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecimalError {
    InvalidSyntax,
    ScaleExceeded,
    PrecisionExceeded,
    DivisionByZero,
    Inexact,
}
impl fmt::Display for DecimalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::InvalidSyntax => "invalid decimal text",
            Self::ScaleExceeded => "decimal scale exceeds 18 fractional digits",
            Self::PrecisionExceeded => "decimal coefficient exceeds 28 digits",
            Self::DivisionByZero => "decimal division by zero",
            Self::Inexact => "decimal operation requires explicit rounding",
        };
        f.write_str(message)
    }
}
impl std::error::Error for DecimalError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoundingMode {
    /// Nearest result, exact ties to even.
    HalfEven,
    /// Nearest result, exact ties away from zero.
    HalfUp,
    TowardZero,
    AwayFromZero,
}

/// Immutable canonical decimal. Trailing fractional zeros and negative zero
/// are removed; display uses shortest exact decimal spelling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Decimal {
    coefficient: i128,
    scale: u8,
}
impl Decimal {
    pub fn new(mut coefficient: i128, mut scale: u8) -> Result<Self, DecimalError> {
        if scale > MAX_DECIMAL_SCALE { return Err(DecimalError::ScaleExceeded); }
        if coefficient.unsigned_abs() > MAX_COEFFICIENT as u128 {
            return Err(DecimalError::PrecisionExceeded);
        }
        if coefficient == 0 { return Ok(Self { coefficient: 0, scale: 0 }); }
        while scale > 0 && coefficient % 10 == 0 {
            coefficient /= 10;
            scale -= 1;
        }
        Ok(Self { coefficient, scale })
    }

    pub fn from_integer(value: i64) -> Self {
        Self { coefficient: i128::from(value), scale: 0 }
    }
    pub fn coefficient(self) -> i128 { self.coefficient }
    pub fn scale(self) -> u8 { self.scale }

    /// Checked exact addition; alignment is bounded, with no float fallback.
    pub fn checked_add(self, other: Self) -> Result<Self, DecimalError> {
        let (a, b) = self.align(other)?;
        let coefficient = a.checked_add(b).ok_or(DecimalError::PrecisionExceeded)?;
        Self::new(coefficient, self.scale.max(other.scale))
    }

    pub fn checked_sub(self, other: Self) -> Result<Self, DecimalError> {
        self.checked_add(Self { coefficient: -other.coefficient, scale: other.scale })
    }

    /// Exact multiplication only. Loss of fractional precision is an error.
    pub fn checked_mul(self, other: Self) -> Result<Self, DecimalError> {
        let mut coefficient = self.coefficient.checked_mul(other.coefficient)
            .ok_or(DecimalError::PrecisionExceeded)?;
        let mut scale = self.scale + other.scale;
        while scale > MAX_DECIMAL_SCALE {
            if coefficient % 10 != 0 { return Err(DecimalError::ScaleExceeded); }
            coefficient /= 10;
            scale -= 1;
        }
        Self::new(coefficient, scale)
    }

    /// Round to at most the requested fractional scale.
    pub fn round_to(self, target_scale: u8, mode: RoundingMode) -> Result<Self, DecimalError> {
        if target_scale > MAX_DECIMAL_SCALE { return Err(DecimalError::ScaleExceeded); }
        if target_scale >= self.scale { return Ok(self); }
        let divisor = power_of_ten(self.scale - target_scale);
        let quotient = self.coefficient / divisor;
        let remainder = self.coefficient % divisor;
        let increment = should_increment(
            quotient.unsigned_abs(), remainder.unsigned_abs(), divisor as u128, mode
        );
        let rounded = if increment {
            quotient.checked_add(if self.coefficient < 0 { -1 } else { 1 })
                .ok_or(DecimalError::PrecisionExceeded)?
        } else { quotient };
        Self::new(rounded, target_scale)
    }

    /// Long division with explicit target scale and rounding mode.
    /// No decimal operation silently converts to binary floating point.
    pub fn div_rounded(self, other: Self, target_scale: u8, mode: RoundingMode) -> Result<Self, DecimalError> {
        if other.coefficient == 0 { return Err(DecimalError::DivisionByZero); }
        if target_scale > MAX_DECIMAL_SCALE { return Err(DecimalError::ScaleExceeded); }
        let (a, b) = self.align(other)?;
        let negative = (a < 0) != (b < 0);
        let numerator = a.abs();
        let denominator = b.abs();
        let mut digits = numerator / denominator;
        let mut remainder = numerator % denominator;
        let mut output_scale = 0u8;
        while remainder != 0 && output_scale < target_scale {
            remainder = remainder.checked_mul(10).ok_or(DecimalError::PrecisionExceeded)?;
            let digit = remainder / denominator;
            remainder %= denominator;
            digits = digits.checked_mul(10).and_then(|v| v.checked_add(digit))
                .ok_or(DecimalError::PrecisionExceeded)?;
            if digits > MAX_COEFFICIENT { return Err(DecimalError::PrecisionExceeded); }
            output_scale += 1;
        }
        if should_increment(digits as u128, remainder as u128, denominator as u128, mode) {
            digits = digits.checked_add(1).ok_or(DecimalError::PrecisionExceeded)?;
        }
        Self::new(if negative { -digits } else { digits }, output_scale)
    }

    /// Never truncate a fraction during integer conversion.
    pub fn to_integer_exact(self) -> Result<i64, DecimalError> {
        if self.scale != 0 { return Err(DecimalError::Inexact); }
        i64::try_from(self.coefficient).map_err(|_| DecimalError::PrecisionExceeded)
    }

    fn align(self, other: Self) -> Result<(i128, i128), DecimalError> {
        let scale = self.scale.max(other.scale);
        let a = shift(self.coefficient, scale - self.scale)?;
        let b = shift(other.coefficient, scale - other.scale)?;
        Ok((a, b))
    }
}

fn power_of_ten(exponent: u8) -> i128 {
    10_i128.pow(u32::from(exponent))
}
fn shift(coefficient: i128, exponent: u8) -> Result<i128, DecimalError> {
    let shifted = coefficient.checked_mul(power_of_ten(exponent))
        .ok_or(DecimalError::PrecisionExceeded)?;
    if shifted.unsigned_abs() > MAX_COEFFICIENT as u128 {
        return Err(DecimalError::PrecisionExceeded);
    }
    Ok(shifted)
}
fn should_increment(quotient: u128, remainder: u128, divisor: u128, mode: RoundingMode) -> bool {
    if remainder == 0 { return false; }
    match mode {
        RoundingMode::TowardZero => false,
        RoundingMode::AwayFromZero => true,
        RoundingMode::HalfEven => {
            let doubled = remainder * 2;
            doubled > divisor || (doubled == divisor && quotient % 2 != 0)
        }
        RoundingMode::HalfUp => remainder * 2 >= divisor,
    }
}

impl FromStr for Decimal {
    type Err = DecimalError;
    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let (negative, body) = if let Some(rest) = text.strip_prefix('-') {
            (true, rest)
        } else if let Some(rest) = text.strip_prefix('+') {
            (false, rest)
        } else { (false, text) };
        let (whole, fraction) = match body.split_once('.') {
            Some((whole, fraction)) if !whole.is_empty() && !fraction.is_empty() => (whole, fraction),
            Some(_) => return Err(DecimalError::InvalidSyntax),
            None => (body, ""),
        };
        if whole.is_empty() { return Err(DecimalError::InvalidSyntax); }
        if fraction.len() > usize::from(MAX_DECIMAL_SCALE) {
            return Err(DecimalError::ScaleExceeded);
        }
        let mut coefficient = 0_i128;
        for part in [whole, fraction] {
            for byte in part.bytes() {
                if !byte.is_ascii_digit() { return Err(DecimalError::InvalidSyntax); }
                coefficient = coefficient.checked_mul(10)
                    .and_then(|v| v.checked_add(i128::from(byte - b'0')))
                    .ok_or(DecimalError::PrecisionExceeded)?;
                if coefficient > MAX_COEFFICIENT {
                    return Err(DecimalError::PrecisionExceeded);
                }
            }
        }
        Self::new(if negative { -coefficient } else { coefficient }, fraction.len() as u8)
    }
}

impl fmt::Display for Decimal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.coefficient < 0 { f.write_str("-")?; }
        let digits = self.coefficient.abs().to_string();
        let scale = usize::from(self.scale);
        if scale == 0 { return f.write_str(&digits); }
        if digits.len() <= scale {
            f.write_str("0.")?;
            for _ in 0..scale - digits.len() { f.write_str("0")?; }
            f.write_str(&digits)
        } else {
            let split = digits.len() - scale;
            write!(f, "{}.{}", &digits[..split], &digits[split..])
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn d(text: &str) -> Decimal { text.parse().unwrap() }

    #[test]
    fn parsing_canonicalizes_zero_and_trailing_zeros() {
        assert_eq!(d("-0.000"), d("0"));
        assert_eq!(d("+00012.3400").to_string(), "12.34");
        assert_eq!(d("0.000000000000000001").to_string(), "0.000000000000000001");
        assert_eq!(d("-0.125").coefficient(), -125);
        assert_eq!(d("-0.125").scale(), 3);
    }

    #[test]
    fn parsing_rejects_bad_syntax_and_bounds() {
        for text in ["", ".5", "1.", "1e2", "1_000", " 1", "1.2.3", "--1", "+", "NaN", "∞"] {
            assert_eq!(text.parse::<Decimal>(), Err(DecimalError::InvalidSyntax), "{text}");
        }
        assert_eq!("0.0000000000000000001".parse::<Decimal>(), Err(DecimalError::ScaleExceeded));
        assert_eq!("99999999999999999999999999999".parse::<Decimal>(), Err(DecimalError::PrecisionExceeded));
    }

    #[test]
    fn exact_addition_and_mixed_scales() {
        assert_eq!(d("0.1").checked_add(d("0.2")).unwrap().to_string(), "0.3");
        assert_eq!(d("2").checked_add(d("0.125")).unwrap().to_string(), "2.125");
        assert_eq!(d("1.2").checked_sub(d("2.25")).unwrap().to_string(), "-1.05");
        assert_eq!(d("-0.1").checked_add(d("0.1")).unwrap().to_string(), "0");
    }

    #[test]
    fn exact_multiplication_and_checked_bounds() {
        assert_eq!(d("-1.25").checked_mul(d("0.2")).unwrap().to_string(), "-0.25");
        assert_eq!(d("0.000000000000000001").checked_mul(d("0.1")), Err(DecimalError::ScaleExceeded));
        assert_eq!(d("9999999999999999999999999999").checked_mul(d("2")), Err(DecimalError::PrecisionExceeded));
        assert_eq!(Decimal::new(0, 19), Err(DecimalError::ScaleExceeded));
    }

    #[test]
    fn half_even_half_up_and_signed_rounding() {
        assert_eq!(d("1.225").round_to(2, RoundingMode::HalfEven).unwrap().to_string(), "1.22");
        assert_eq!(d("1.235").round_to(2, RoundingMode::HalfEven).unwrap().to_string(), "1.24");
        assert_eq!(d("-1.225").round_to(2, RoundingMode::HalfUp).unwrap().to_string(), "-1.23");
        assert_eq!(d("-1.239").round_to(2, RoundingMode::TowardZero).unwrap().to_string(), "-1.23");
        assert_eq!(d("-1.231").round_to(2, RoundingMode::AwayFromZero).unwrap().to_string(), "-1.24");
        assert_eq!(d("-0.005").round_to(2, RoundingMode::HalfEven).unwrap().to_string(), "0");
    }

    #[test]
    fn explicitly_rounded_long_division() {
        assert_eq!(d("1").div_rounded(d("8"), 3, RoundingMode::HalfEven).unwrap().to_string(), "0.125");
        assert_eq!(d("1").div_rounded(d("8"), 2, RoundingMode::HalfEven).unwrap().to_string(), "0.12");
        assert_eq!(d("1").div_rounded(d("8"), 2, RoundingMode::HalfUp).unwrap().to_string(), "0.13");
        assert_eq!(d("2").div_rounded(d("3"), 2, RoundingMode::HalfEven).unwrap().to_string(), "0.67");
        assert_eq!(d("-2").div_rounded(d("3"), 2, RoundingMode::HalfEven).unwrap().to_string(), "-0.67");
        assert_eq!(d("0.01").div_rounded(d("0.2"), 2, RoundingMode::HalfEven).unwrap().to_string(), "0.05");
        assert_eq!(d("1").div_rounded(d("0"), 2, RoundingMode::HalfEven), Err(DecimalError::DivisionByZero));
    }

    #[test]
    fn conversion_requires_integer_exactness() {
        assert_eq!(d("42.0").to_integer_exact(), Ok(42));
        assert_eq!(d("42.1").to_integer_exact(), Err(DecimalError::Inexact));
        assert_eq!(d("9999999999999999999999999999").to_integer_exact(), Err(DecimalError::PrecisionExceeded));
    }
}
