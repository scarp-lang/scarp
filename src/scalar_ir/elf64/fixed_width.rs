//! Checked arithmetic over sign- or zero-extended private stack lanes.

use super::{invalid, BinaryOperator, Code, Condition, Diagnostic, IntegerWidth, Span, Unary};
use crate::native_object::{
    CHECKED_KERNEL_DIVISION_BY_ZERO, CHECKED_KERNEL_FIXED_INTEGER_CONVERSION,
    CHECKED_KERNEL_INTEGER_OVERFLOW,
};

impl Code {
    /// All signed values are sign-extended and unsigned values zero-extended.
    fn normalize_width(&mut self, width: IntegerWidth) -> Result<(), Diagnostic> {
        let shift = 64 - width.bits;
        if shift != 0 {
            self.push(&[0x48, 0xc1, 0xe0, shift])?; // shl rax, shift
            let mod_rm = if width.signed { 0xf8 } else { 0xe8 };
            self.push(&[0x48, 0xc1, mod_rm, shift])?;
        }
        Ok(())
    }

    fn check_width(
        &mut self,
        width: IntegerWidth,
        status: u32,
        span: Span,
    ) -> Result<(), Diagnostic> {
        if width.bits != 64 {
            let span_id = self.record_failure_span(span)?;
            self.move_register(1, 0)?;
            self.normalize_width(width)?;
            self.push(&[0x48, 0x39, 0xc8])?; // cmp rax, rcx
            self.branch_to_failure(Condition::NotEqual, status, span_id)?;
        }
        Ok(())
    }

    pub(super) fn fixed_cast(
        &mut self,
        from: IntegerWidth,
        to: IntegerWidth,
        span: Span,
    ) -> Result<(), Diagnostic> {
        if from.signed != to.signed {
            // The high sign bit denotes a negative signed operand or an
            // unsigned 64-bit value above the largest signed 64-bit value.
            let span_id = self.record_failure_span(span)?;
            self.push(&[0x48, 0x85, 0xc0])?;
            self.branch_to_failure(
                Condition::Sign,
                CHECKED_KERNEL_FIXED_INTEGER_CONVERSION,
                span_id,
            )?;
        }
        self.check_width(to, CHECKED_KERNEL_FIXED_INTEGER_CONVERSION, span)
    }

    pub(super) fn fixed_unary(
        &mut self,
        operation: Unary,
        width: IntegerWidth,
        span: Span,
    ) -> Result<(), Diagnostic> {
        match operation {
            Unary::BitNot => {
                self.push(&[0x48, 0xf7, 0xd0])?;
                self.normalize_width(width)
            }
            Unary::Negate if width.signed => {
                self.push(&[0x48, 0xf7, 0xd8])?;
                self.guard_overflow(span)?;
                self.check_width(width, CHECKED_KERNEL_INTEGER_OVERFLOW, span)
            }
            Unary::Negate => {
                let span_id = self.record_failure_span(span)?;
                self.push(&[0x48, 0x85, 0xc0])?;
                self.branch_to_failure(
                    Condition::NotEqual,
                    CHECKED_KERNEL_INTEGER_OVERFLOW,
                    span_id,
                )
            }
            Unary::Not => Err(invalid(span)),
        }
    }

    pub(super) fn fixed_binary(
        &mut self,
        op: BinaryOperator,
        width: IntegerWidth,
        span: Span,
    ) -> Result<(), Diagnostic> {
        use BinaryOperator::*;
        if op.is_comparison() {
            return self.compare(op, width.signed, span);
        }
        match op {
            BitAnd | BitOr | BitXor => self.binary(op, true, span),
            ShiftLeft | ShiftRight | UnsignedRight => {
                if op == UnsignedRight {
                    self.normalize_width(IntegerWidth {
                        signed: false,
                        ..width
                    })?;
                }
                let mod_rm = match op {
                    ShiftLeft => 0xe0,
                    ShiftRight if width.signed => 0xf8,
                    _ => 0xe8,
                };
                self.shift(mod_rm, width.bits - 1, span)?;
                self.normalize_width(width)
            }
            Divide | Remainder if width.signed => {
                let minimum = i64::MIN >> (64 - width.bits);
                self.signed_division(op == Remainder, minimum, span)
            }
            Divide | Remainder => {
                let span_id = self.record_failure_span(span)?;
                self.push(&[0x48, 0x85, 0xc9])?; // test rcx, rcx
                self.branch_to_failure(Condition::Equal, CHECKED_KERNEL_DIVISION_BY_ZERO, span_id)?;
                self.push(&[0x31, 0xd2, 0x48, 0xf7, 0xf1])?; // xor edx, edx; div rcx
                if op == Remainder {
                    self.move_register(0, 2)?;
                }
                Ok(())
            }
            Add | Subtract | Multiply => {
                if width.signed {
                    self.binary(op, true, span)?;
                } else {
                    let instruction = match op {
                        Add => [0x48, 0x01, 0xc8],
                        Subtract => [0x48, 0x29, 0xc8],
                        Multiply => [0x48, 0xf7, 0xe1], // mul rcx, high bits set CF
                        _ => unreachable!("checked arithmetic"),
                    };
                    let span_id = self.record_failure_span(span)?;
                    self.push(&instruction)?;
                    self.branch_to_failure(
                        Condition::Below,
                        CHECKED_KERNEL_INTEGER_OVERFLOW,
                        span_id,
                    )?;
                }
                self.check_width(width, CHECKED_KERNEL_INTEGER_OVERFLOW, span)
            }
            _ => Err(invalid(span)),
        }
    }
}
