//! Fixed-width integer values with explicit, checked conversions.

pub(crate) mod intrinsics;

use super::{Value, ValueType};
use crate::{
    ast::BinaryOperator,
    diagnostics::{Diagnostic, DiagnosticKind, Span},
    limits::Budget,
    types::SourceType,
};
use std::{cmp::Ordering, fmt};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FixedInteger {
    bits: u64,
    kind: ValueType,
}

impl FixedInteger {
    /// Construct a representable value of a type with at most 64 bits.
    pub fn new(kind: ValueType, value: i128) -> Option<Self> {
        let (width, _) = kind.fixed_integer_width()?;
        if width > 64 {
            return None;
        }
        let repr = Repr::from_signed(kind, value)?;
        Some(Self {
            bits: repr.bits as u64,
            kind,
        })
    }

    pub fn kind(self) -> ValueType {
        self.kind
    }

    /// Return the mathematical value, including the full u64 range.
    pub fn value(self) -> i128 {
        let repr = Repr {
            bits: u128::from(self.bits),
            kind: self.kind,
        };
        if repr.signed() {
            repr.signed_value()
        } else {
            i128::from(self.bits)
        }
    }
}

impl fmt::Display for FixedInteger {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.value().fmt(f)
    }
}

/// A full-range i128 or u128 stored inline as two 64-bit words.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WideFixedInteger {
    words: [u64; 2],
    kind: ValueType,
}

impl WideFixedInteger {
    pub const fn from_i128(value: i128) -> Self {
        Self::from_bits(ValueType::I128, value as u128)
    }

    pub const fn from_u128(value: u128) -> Self {
        Self::from_bits(ValueType::U128, value)
    }

    const fn from_bits(kind: ValueType, bits: u128) -> Self {
        Self {
            words: [bits as u64, (bits >> 64) as u64],
            kind,
        }
    }

    pub const fn kind(self) -> ValueType {
        self.kind
    }

    fn bits(self) -> u128 {
        u128::from(self.words[0]) | (u128::from(self.words[1]) << 64)
    }

    /// Convert to a signed host integer when the mathematical value fits.
    pub fn to_i128(self) -> Option<i128> {
        if self.kind == ValueType::I128 {
            Some(self.bits() as i128)
        } else {
            self.bits().try_into().ok()
        }
    }

    /// Convert to an unsigned host integer when the value is nonnegative.
    pub fn to_u128(self) -> Option<u128> {
        if self.kind == ValueType::I128 {
            (self.bits() as i128).try_into().ok()
        } else {
            Some(self.bits())
        }
    }
}

impl fmt::Display for WideFixedInteger {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.kind == ValueType::I128 {
            (self.bits() as i128).fmt(f)
        } else {
            self.bits().fmt(f)
        }
    }
}

#[derive(Clone, Copy)]
struct Repr {
    bits: u128,
    kind: ValueType,
}

impl Repr {
    fn from_value(value: &Value) -> Option<Self> {
        match value {
            Value::FixedInteger(value) => Some(Self {
                bits: value.bits.into(),
                kind: value.kind,
            }),
            Value::WideFixedInteger(value) => Some(Self {
                bits: value.bits(),
                kind: value.kind,
            }),
            _ => None,
        }
    }

    fn width(self) -> u32 {
        self.kind.fixed_integer_width().expect("fixed integer").0
    }

    fn signed(self) -> bool {
        self.kind.fixed_integer_width().expect("fixed integer").1
    }

    fn signed_value(self) -> i128 {
        let shift = 128 - self.width();
        ((self.bits << shift) as i128) >> shift
    }

    fn with_pattern(self, bits: u128) -> Self {
        Self {
            bits: bits & (u128::MAX >> (128 - self.width())),
            kind: self.kind,
        }
    }

    fn signed_magnitude(self) -> (bool, u128) {
        if self.signed() {
            let value = self.signed_value();
            (value.is_negative(), value.unsigned_abs())
        } else {
            (false, self.bits)
        }
    }

    fn from_signed(kind: ValueType, value: i128) -> Option<Self> {
        Self::from_magnitude(kind, value.is_negative(), value.unsigned_abs())
    }

    fn from_magnitude(kind: ValueType, negative: bool, magnitude: u128) -> Option<Self> {
        let (width, signed) = kind.fixed_integer_width()?;
        let bits = if negative {
            if !signed || magnitude > 1_u128 << (width - 1) {
                return None;
            }
            0_u128.wrapping_sub(magnitude) & (u128::MAX >> (128 - width))
        } else {
            let max = u128::MAX >> (128 - width + u32::from(signed));
            if magnitude > max {
                return None;
            }
            magnitude
        };
        Some(Self { bits, kind })
    }

    fn value(self) -> Value {
        if self.width() == 128 {
            Value::WideFixedInteger(WideFixedInteger::from_bits(self.kind, self.bits))
        } else {
            Value::FixedInteger(FixedInteger {
                bits: self.bits as u64,
                kind: self.kind,
            })
        }
    }

    fn compare(self, other: Self) -> Ordering {
        if self.signed() {
            self.signed_value().cmp(&other.signed_value())
        } else {
            self.bits.cmp(&other.bits)
        }
    }

    fn checked_arithmetic(self, op: BinaryOperator, other: Self) -> Option<Self> {
        use BinaryOperator::*;
        if self.signed() {
            let a = self.signed_value();
            let b = other.signed_value();
            let value = match op {
                Add => a.checked_add(b),
                Subtract => a.checked_sub(b),
                Multiply => a.checked_mul(b),
                Divide => a.checked_div(b),
                Remainder => a.checked_rem(b),
                _ => unreachable!("fixed-width arithmetic"),
            }?;
            // Preserve the checked quotient rule for narrower MIN % -1.
            if op == Remainder {
                Self::from_signed(self.kind, a.checked_div(b)?)?;
            }
            Self::from_signed(self.kind, value)
        } else {
            let a = self.bits;
            let b = other.bits;
            let value = match op {
                Add => a.checked_add(b),
                Subtract => a.checked_sub(b),
                Multiply => a.checked_mul(b),
                Divide => a.checked_div(b),
                Remainder => a.checked_rem(b),
                _ => unreachable!("fixed-width arithmetic"),
            }?;
            Self::from_magnitude(self.kind, false, value)
        }
    }

    fn overflow(self, span: Span) -> Diagnostic {
        Diagnostic::new(
            DiagnosticKind::FixedIntegerOverflow,
            span,
            format!("integer arithmetic result is outside {}", self.kind),
        )
    }

    fn shift_count(self, amount: i64, span: Span) -> Result<u32, Diagnostic> {
        u32::try_from(amount)
            .ok()
            .filter(|&n| n < self.width())
            .ok_or_else(|| {
                Diagnostic::new(
                    DiagnosticKind::InvalidShift,
                    span,
                    format!(
                        "shift or rotation count must be between 0 and {} for {}",
                        self.width() - 1,
                        self.kind,
                    ),
                )
            })
    }

    fn binary(self, op: BinaryOperator, right: &Value, span: Span) -> Result<Value, Diagnostic> {
        use BinaryOperator::*;
        if is_shift(op) {
            let n = self.shift_count(right.require_integer(span)?, span)?;
            let bits = match op {
                ShiftLeft => self.bits << n,
                ShiftRight if self.signed() => (self.signed_value() >> n) as u128,
                ShiftRight | UnsignedRight => self.bits >> n,
                _ => unreachable!("checked shift operation"),
            };
            return Ok(self.with_pattern(bits).value());
        }
        let other = Self::from_value(right)
            .filter(|right| right.kind == self.kind)
            .ok_or_else(|| super::type_mismatch(span, self.kind, right.kind()))?;
        Ok(match op {
            Equal => Value::Boolean(self.bits == other.bits),
            NotEqual => Value::Boolean(self.bits != other.bits),
            Less => Value::Boolean(self.compare(other).is_lt()),
            LessEqual => Value::Boolean(self.compare(other).is_le()),
            Greater => Value::Boolean(self.compare(other).is_gt()),
            GreaterEqual => Value::Boolean(self.compare(other).is_ge()),
            BitAnd => self.with_pattern(self.bits & other.bits).value(),
            BitOr => self.with_pattern(self.bits | other.bits).value(),
            BitXor => self.with_pattern(self.bits ^ other.bits).value(),
            Divide | Remainder if other.bits == 0 => {
                return Err(Diagnostic::new(
                    DiagnosticKind::DivisionByZero,
                    span,
                    "fixed-width integer divisor is zero",
                ));
            }
            Add | Subtract | Multiply | Divide | Remainder => self
                .checked_arithmetic(op, other)
                .ok_or_else(|| self.overflow(span))?
                .value(),
            ShiftLeft | ShiftRight | UnsignedRight => unreachable!("handled shift"),
        })
    }
}

pub(crate) fn binary(
    op: BinaryOperator,
    left: &Value,
    right: &Value,
    span: Span,
) -> Option<Result<Value, Diagnostic>> {
    Repr::from_value(left).map(|value| value.binary(op, right, span))
}

pub(crate) fn invert(value: &Value) -> Option<Value> {
    let value = Repr::from_value(value)?;
    Some(value.with_pattern(!value.bits).value())
}

pub(crate) fn negate(value: &Value, span: Span) -> Result<Value, Diagnostic> {
    let value = Repr::from_value(value).expect("validated fixed integer");
    let (negative, magnitude) = value.signed_magnitude();
    Repr::from_magnitude(value.kind, !negative && magnitude != 0, magnitude)
        .map(Repr::value)
        .ok_or_else(|| value.overflow(span))
}

pub(crate) fn compare(left: &Value, right: &Value) -> Option<Ordering> {
    let a = Repr::from_value(left)?;
    let b = Repr::from_value(right)?;
    (a.kind == b.kind).then(|| a.compare(b))
}

pub(crate) fn is_shift(op: BinaryOperator) -> bool {
    matches!(
        op,
        BinaryOperator::ShiftLeft | BinaryOperator::ShiftRight | BinaryOperator::UnsignedRight
    )
}

pub(crate) fn named_cast(name: &str) -> Option<ValueType> {
    ValueType::from_name(name.strip_prefix("to_")?)
        .filter(|kind| kind.fixed_integer_width().is_some())
}

pub(crate) fn can_cast(target: ValueType, source: &SourceType) -> bool {
    source.as_primitive().is_some_and(|kind| {
        kind.fixed_integer_width().is_some()
            || (target != ValueType::Integer
                && matches!(kind, ValueType::Integer | ValueType::BigInt))
    })
}

pub(crate) fn cast(
    target: ValueType,
    input: &Value,
    budget: &mut Budget,
    span: Span,
) -> Result<Value, Diagnostic> {
    let invalid = || {
        Diagnostic::new(
            DiagnosticKind::NumericConversion,
            span,
            format!("value cannot be represented exactly as {target}"),
        )
    };
    let (negative, magnitude) = match input {
        Value::Integer(number) => (number.is_negative(), u128::from(number.unsigned_abs())),
        Value::BigInt(number) => {
            let wide = target
                .fixed_integer_width()
                .is_some_and(|(width, _)| width == 128);
            budget.charge(if wide { 5 } else { 3 }, span)?;
            let magnitude = if wide {
                number.magnitude_u128()
            } else {
                number.magnitude_u64().map(u128::from)
            };
            (number.is_negative(), magnitude.ok_or_else(invalid)?)
        }
        _ => Repr::from_value(input)
            .expect("validated integer cast input")
            .signed_magnitude(),
    };
    if target == ValueType::Integer {
        let bits = Repr::from_magnitude(ValueType::I64, negative, magnitude).ok_or_else(invalid)?;
        Ok(Value::Integer(bits.signed_value() as i64))
    } else {
        Repr::from_magnitude(target, negative, magnitude)
            .map(Repr::value)
            .ok_or_else(invalid)
    }
}
