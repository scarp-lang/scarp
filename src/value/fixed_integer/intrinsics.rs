//! Width-preserving arithmetic modes and bit operations.

use super::{Repr, Value};
use crate::{
    ast::BinaryOperator,
    diagnostics::{Diagnostic, Span},
    limits::Budget,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OverflowMode {
    Wrap,
    Saturate,
    Report,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Bits {
    Ones,
    LeadingZeros,
    TrailingZeros,
    Reverse,
    SwapBytes,
    RotateLeft,
    RotateRight,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Intrinsic {
    Arithmetic(BinaryOperator, OverflowMode),
    Bits(Bits),
}

pub(crate) enum ResultValue {
    Scalar(Value),
    Overflow(Value, bool),
}

impl Intrinsic {
    pub(crate) fn named(name: &str) -> Option<Self> {
        use BinaryOperator::{Add, Multiply, Subtract};
        use OverflowMode::{Report, Saturate, Wrap};
        let intrinsic = match name {
            "wrapping_add" => Self::Arithmetic(Add, Wrap),
            "wrapping_sub" => Self::Arithmetic(Subtract, Wrap),
            "wrapping_mul" => Self::Arithmetic(Multiply, Wrap),
            "saturating_add" => Self::Arithmetic(Add, Saturate),
            "saturating_sub" => Self::Arithmetic(Subtract, Saturate),
            "saturating_mul" => Self::Arithmetic(Multiply, Saturate),
            "overflowing_add" => Self::Arithmetic(Add, Report),
            "overflowing_sub" => Self::Arithmetic(Subtract, Report),
            "overflowing_mul" => Self::Arithmetic(Multiply, Report),
            "bits_count_ones" => Self::Bits(Bits::Ones),
            "bits_leading_zeros" => Self::Bits(Bits::LeadingZeros),
            "bits_trailing_zeros" => Self::Bits(Bits::TrailingZeros),
            "bits_reverse" => Self::Bits(Bits::Reverse),
            "bits_swap_bytes" => Self::Bits(Bits::SwapBytes),
            "bits_rotate_left" => Self::Bits(Bits::RotateLeft),
            "bits_rotate_right" => Self::Bits(Bits::RotateRight),
            _ => return None,
        };
        Some(intrinsic)
    }

    pub(crate) fn is_bit_operation(self) -> bool {
        matches!(self, Self::Bits(_))
    }

    pub(crate) fn takes_count(self) -> bool {
        matches!(self, Self::Bits(Bits::RotateLeft | Bits::RotateRight))
    }

    pub(crate) fn returns_count(self) -> bool {
        matches!(
            self,
            Self::Bits(Bits::Ones | Bits::LeadingZeros | Bits::TrailingZeros)
        )
    }

    pub(crate) fn reports_overflow(self) -> bool {
        matches!(self, Self::Arithmetic(_, OverflowMode::Report))
    }

    pub(crate) fn arity(self) -> usize {
        if self.takes_count() || matches!(self, Self::Arithmetic(_, _)) {
            2
        } else {
            1
        }
    }

    pub(crate) fn evaluate(
        self,
        args: &[Value],
        budget: &mut Budget,
        span: Span,
    ) -> Result<ResultValue, Diagnostic> {
        budget.charge(1, span)?;
        let left = Repr::from_value(&args[0]).expect("validated fixed-width operand");
        Ok(match self {
            Self::Arithmetic(op, mode) => {
                let right = Repr::from_value(&args[1]).expect("validated fixed-width operand");
                arithmetic(left, op, right, mode)
            }
            Self::Bits(op) => ResultValue::Scalar(bits(left, op, args, span)?),
        })
    }
}

fn arithmetic(left: Repr, op: BinaryOperator, right: Repr, mode: OverflowMode) -> ResultValue {
    use BinaryOperator::{Add, Multiply, Subtract};
    let checked = left.checked_arithmetic(op, right);
    if let Some(value) = checked {
        return if mode == OverflowMode::Report {
            ResultValue::Overflow(value.value(), false)
        } else {
            ResultValue::Scalar(value.value())
        };
    }
    if mode == OverflowMode::Saturate {
        let saturated = if left.signed() {
            let left_negative = left.signed_value().is_negative();
            let negative = if op == Multiply {
                left_negative != right.signed_value().is_negative()
            } else {
                left_negative
            };
            let endpoint = if negative { i128::MIN } else { i128::MAX };
            Repr::from_signed(left.kind, endpoint >> (128 - left.width())).expect("type endpoint")
        } else {
            left.with_pattern(if op == Subtract { 0 } else { u128::MAX })
        };
        return ResultValue::Scalar(saturated.value());
    }
    let wrapped = left.with_pattern(match op {
        Add => left.bits.wrapping_add(right.bits),
        Subtract => left.bits.wrapping_sub(right.bits),
        Multiply => left.bits.wrapping_mul(right.bits),
        _ => unreachable!("arithmetic mode applies to add, subtract and multiply"),
    });
    if mode == OverflowMode::Report {
        ResultValue::Overflow(wrapped.value(), true)
    } else {
        ResultValue::Scalar(wrapped.value())
    }
}

fn bits(value: Repr, op: Bits, args: &[Value], span: Span) -> Result<Value, Diagnostic> {
    let width = value.width();
    let padding = 128 - width;
    let raw = match op {
        Bits::Ones => return Ok(Value::Integer(i64::from(value.bits.count_ones()))),
        Bits::LeadingZeros => {
            return Ok(Value::Integer(i64::from(
                value.bits.leading_zeros() - padding,
            )))
        }
        Bits::TrailingZeros => {
            return Ok(Value::Integer(i64::from(
                value.bits.trailing_zeros().min(width),
            )))
        }
        Bits::Reverse => value.bits.reverse_bits() >> padding,
        Bits::SwapBytes => value.bits.swap_bytes() >> padding,
        Bits::RotateLeft | Bits::RotateRight => {
            let count = value.shift_count(args[1].require_integer(span)?, span)?;
            if count == 0 {
                return Ok(value.value());
            }
            let left = if op == Bits::RotateLeft {
                count
            } else {
                width - count
            };
            (value.bits << left) | (value.bits >> (width - left))
        }
    };
    Ok(value.with_pattern(raw).value())
}
