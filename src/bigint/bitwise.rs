//! Infinite signed two's-complement bitwise operations on bounded BigInts.
//! Bits are computed with an extra 32-bit guard word, so signed behavior is
//! independent of any fixed-width i64 representation. All buffers and scans
//! charge the existing deterministic work and logical allocation budgets.
use super::*;

#[derive(Clone, Copy)]
pub(crate) enum BitwiseOperation {
    And,
    Or,
    Xor,
}

impl BigInt {
    /// Convert the nonnegative magnitude to little-endian base-2^32 words.
    fn magnitude_words(&self, budget: &mut Budget, span: Span) -> Result<Vec<u32>, Diagnostic> {
        budget.check_bigint_digits(self.decimal_digits(), span)?;
        let mut decimal = buffer(self.limbs.len(), budget, span)?;
        decimal.extend_from_slice(&self.limbs);
        let mut words = Vec::new();
        while !decimal.is_empty() {
            budget.charge(decimal.len() as u64 + 1, span)?;
            let mut remainder = 0_u64;
            for digit in decimal.iter_mut().rev() {
                let current = (remainder * BASE) + u64::from(*digit);
                *digit = (current >> 32) as u32;
                remainder = current & 0xffff_ffff;
            }
            normalize(&mut decimal);
            budget.charge_bigint_allocation(std::mem::size_of::<u32>(), span)?;
            words.try_reserve_exact(1).map_err(|_| allocation(span))?;
            words.push(remainder as u32);
        }
        Ok(words)
    }

    /// Expand a little-endian positive magnitude to signed two's complement.
    fn padded_twos(
        &self,
        mut words: Vec<u32>,
        width: usize,
        budget: &mut Budget,
        span: Span,
    ) -> Result<Vec<u32>, Diagnostic> {
        let more = width
            .checked_sub(words.len())
            .ok_or_else(|| allocation(span))?;
        budget.charge(width as u64, span)?;
        budget.charge_bigint_allocation(
            more.checked_mul(std::mem::size_of::<u32>())
                .ok_or_else(|| allocation(span))?,
            span,
        )?;
        words
            .try_reserve_exact(more)
            .map_err(|_| allocation(span))?;
        words.resize(width, 0);
        if self.negative {
            let mut carry = true;
            for word in &mut words {
                let (digit, next_carry) = (!*word).overflowing_add(u32::from(carry));
                *word = digit;
                carry = next_carry;
            }
        }
        Ok(words)
    }

    /// Decode signed two's-complement words to canonical decimal limbs.
    fn from_twos(mut words: Vec<u32>, budget: &mut Budget, span: Span) -> Result<Self, Diagnostic> {
        budget.charge(words.len() as u64 + 1, span)?;
        let negative = words.last().is_some_and(|word| word & 0x8000_0000 != 0);
        if negative {
            let mut carry = true;
            for word in &mut words {
                let (digit, next_carry) = (!*word).overflowing_add(u32::from(carry));
                *word = digit;
                carry = next_carry;
            }
        }
        while words.last() == Some(&0) {
            words.pop();
        }
        let mut limbs = Vec::new();
        for &word in words.iter().rev() {
            budget.charge(limbs.len() as u64 + 1, span)?;
            let mut carry = u64::from(word);
            for limb in &mut limbs {
                // (BASE - 1) * 2^32 + (2^32 - 1) < 2^64.
                let current = u64::from(*limb) * (1_u64 << 32) + carry;
                *limb = (current % BASE) as u32;
                carry = current / BASE;
            }
            while carry != 0 {
                budget.charge_bigint_allocation(std::mem::size_of::<u32>(), span)?;
                limbs.try_reserve_exact(1).map_err(|_| allocation(span))?;
                limbs.push((carry % BASE) as u32);
                carry /= BASE;
            }
        }
        finish(limbs, negative, budget, span)
    }

    pub(crate) fn bitwise(
        &self,
        other: &Self,
        operation: BitwiseOperation,
        budget: &mut Budget,
        span: Span,
    ) -> Result<Self, Diagnostic> {
        let left = self.magnitude_words(budget, span)?;
        let right = other.magnitude_words(budget, span)?;
        let width = left
            .len()
            .max(right.len())
            .checked_add(1)
            .ok_or_else(|| allocation(span))?;
        let mut left = self.padded_twos(left, width, budget, span)?;
        let right = other.padded_twos(right, width, budget, span)?;
        budget.charge(width as u64, span)?;
        for (lhs, rhs) in left.iter_mut().zip(right) {
            *lhs = match operation {
                BitwiseOperation::And => *lhs & rhs,
                BitwiseOperation::Or => *lhs | rhs,
                BitwiseOperation::Xor => *lhs ^ rhs,
            };
        }
        Self::from_twos(left, budget, span)
    }

    /// Infinite signed bitwise NOT: mathematically -x-1.
    pub(crate) fn bitwise_not(&self, budget: &mut Budget, span: Span) -> Result<Self, Diagnostic> {
        budget.check_bigint_digits(self.decimal_digits(), span)?;
        let minus_one = Self::from_integer(-1, budget, span)?;
        add(&self.negated(), &minus_one, budget, span)
    }

    /// Signed left shift: exact multiplication by 2^count.
    pub(crate) fn shift_left(
        &self,
        count: i64,
        budget: &mut Budget,
        span: Span,
    ) -> Result<Self, Diagnostic> {
        if count < 0 {
            return Err(Diagnostic::new(
                DiagnosticKind::NumericDomain,
                span,
                "bigint shift count must be nonnegative",
            ));
        }
        budget.check_bigint_digits(self.decimal_digits(), span)?;
        budget.charge(1, span)?;
        if count == 0 || self.is_zero() {
            return Ok(self.clone());
        }
        let two = Self::from_integer(2, budget, span)?;
        let multiplier = two.pow(count, budget, span)?;
        multiply(self, &multiplier, budget, span)
    }

    /// Arithmetic right shift: floor(x / 2^count), including negative x.
    pub(crate) fn shift_right(
        &self,
        count: i64,
        budget: &mut Budget,
        span: Span,
    ) -> Result<Self, Diagnostic> {
        if count < 0 {
            return Err(Diagnostic::new(
                DiagnosticKind::NumericDomain,
                span,
                "bigint shift count must be nonnegative",
            ));
        }
        budget.check_bigint_digits(self.decimal_digits(), span)?;
        budget.charge(1, span)?;
        if count == 0 || self.is_zero() {
            return Ok(self.clone());
        }
        // Overshifting a signed value is defined by sign extension, without
        // constructing an enormous power of two.
        let width = self.bit_length(budget, span)?;
        if count >= width {
            return Self::from_integer(if self.negative { -1 } else { 0 }, budget, span);
        }
        let two = Self::from_integer(2, budget, span)?;
        let divisor = two.pow(count, budget, span)?;
        let (quotient, remainder) = divmod(self, &divisor, budget, span)?;
        if self.negative && !remainder.is_zero() {
            let minus_one = Self::from_integer(-1, budget, span)?;
            add(&quotient, &minus_one, budget, span)
        } else {
            Ok(quotient)
        }
    }
}
