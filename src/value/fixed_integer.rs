//! Width-qualified integer values with explicit, checked conversions.

use super::{Value, ValueType};
use crate::{
    ast::BinaryOperator,
    diagnostics::{Diagnostic, DiagnosticKind, Span},
    limits::Budget,
    types::SourceType,
};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FixedInteger {
    bits: u64,
    kind: ValueType,
}

impl FixedInteger {
    /// Construct a mathematical value if it fits exactly in its declared type.
    pub fn new(kind: ValueType, value: i128) -> Option<Self> {
        let (width, signed) = kind.fixed_integer_width()?;
        let (min, max) = if signed {
            let sign = 1_i128 << (width - 1);
            (-sign, sign - 1)
        } else {
            (0, (1_i128 << width) - 1)
        };
        (min..=max)
            .contains(&value)
            .then(|| Self::from_pattern(kind, value as u64))
    }

    fn from_pattern(kind: ValueType, bits: u64) -> Self {
        let (width, _) = kind.fixed_integer_width().expect("fixed integer type");
        let mask = u64::MAX >> (64 - width);
        Self {
            kind,
            bits: bits & mask,
        }
    }

    pub fn kind(self) -> ValueType {
        self.kind
    }

    /// Mathematical value, including unsigned 64-bit values above i64::MAX.
    pub fn value(self) -> i128 {
        let (width, signed) = self.kind.fixed_integer_width().expect("fixed integer type");
        if signed {
            let shift = 64 - width;
            ((self.bits << shift) as i64 >> shift) as i128
        } else {
            i128::from(self.bits)
        }
    }

    pub(crate) fn negate(self, span: Span) -> Result<Value, Diagnostic> {
        self.checked(-self.value(), span)
    }

    pub(crate) fn invert(self) -> Value {
        Value::FixedInteger(Self::from_pattern(self.kind, !self.bits))
    }

    fn checked(self, value: i128, span: Span) -> Result<Value, Diagnostic> {
        Self::new(self.kind, value)
            .map(Value::FixedInteger)
            .ok_or_else(|| self.overflow(span))
    }

    fn overflow(self, span: Span) -> Diagnostic {
        Diagnostic::new(
            DiagnosticKind::FixedIntegerOverflow,
            span,
            format!("integer arithmetic result is outside {}", self.kind),
        )
    }

    pub(crate) fn binary(
        self,
        op: BinaryOperator,
        right: &Value,
        span: Span,
    ) -> Result<Value, Diagnostic> {
        use BinaryOperator::*;
        let a = self.value();
        if is_shift(op) {
            let (width, _) = self.kind.fixed_integer_width().expect("fixed integer type");
            let amount = right.require_integer(span)?;
            let n = u32::try_from(amount)
                .ok()
                .filter(|&n| n < width)
                .ok_or_else(|| {
                    Diagnostic::new(
                        DiagnosticKind::InvalidShift,
                        span,
                        format!(
                            "shift count must be between 0 and {} for {}",
                            width - 1,
                            self.kind
                        ),
                    )
                })?;
            let bits = match op {
                ShiftLeft => self.bits << n,
                ShiftRight => (a >> n) as u64,
                UnsignedRight => self.bits >> n,
                _ => unreachable!("checked shift operation"),
            };
            return Ok(Value::FixedInteger(Self::from_pattern(self.kind, bits)));
        }
        let other = right
            .as_fixed_integer()
            .filter(|r| r.kind == self.kind)
            .ok_or_else(|| super::type_mismatch(span, self.kind, right.kind()))?;
        let b = other.value();
        let value = match op {
            Equal => Value::Boolean(a == b),
            NotEqual => Value::Boolean(a != b),
            Less => Value::Boolean(a < b),
            LessEqual => Value::Boolean(a <= b),
            Greater => Value::Boolean(a > b),
            GreaterEqual => Value::Boolean(a >= b),
            BitAnd => Value::FixedInteger(Self::from_pattern(self.kind, self.bits & other.bits)),
            BitOr => Value::FixedInteger(Self::from_pattern(self.kind, self.bits | other.bits)),
            BitXor => Value::FixedInteger(Self::from_pattern(self.kind, self.bits ^ other.bits)),
            Add => self.checked(a + b, span)?,
            Subtract => self.checked(a - b, span)?,
            Multiply => self.checked(a.checked_mul(b).ok_or_else(|| self.overflow(span))?, span)?,
            Divide | Remainder => {
                if b == 0 {
                    return Err(Diagnostic::new(
                        DiagnosticKind::DivisionByZero,
                        span,
                        "fixed-width integer divisor is zero",
                    ));
                }
                // Check the quotient for both operations, including MIN % -1.
                let quotient = a / b;
                let checked = self.checked(quotient, span)?;
                if op == Divide {
                    checked
                } else {
                    self.checked(a % b, span)?
                }
            }
            ShiftLeft | ShiftRight | UnsignedRight => unreachable!("shift handled separately"),
        };
        Ok(value)
    }
}

impl fmt::Display for FixedInteger {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.value().fmt(f)
    }
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
    let value = match input {
        Value::FixedInteger(number) => number.value(),
        Value::Integer(number) => i128::from(*number),
        Value::BigInt(number) => {
            // A u64 magnitude requires at most three of the BigInt's base-10^9 limbs.
            budget.charge(3, span)?;
            let magnitude = i128::from(number.magnitude_u64().ok_or_else(invalid)?);
            if number.is_negative() {
                -magnitude
            } else {
                magnitude
            }
        }
        _ => unreachable!("fixed-width conversion argument was statically checked"),
    };
    if target == ValueType::Integer {
        i64::try_from(value)
            .map(Value::Integer)
            .map_err(|_| invalid())
    } else {
        FixedInteger::new(target, value)
            .map(Value::FixedInteger)
            .ok_or_else(invalid)
    }
}
