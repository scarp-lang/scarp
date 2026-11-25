//! Bounded exact integer utilities built on the existing decimal-limb engine.
//! No float conversions, unchecked intermediate arithmetic, or global state.
use super::*;
use std::fmt::Write as _;

impl BigInt {
    /// Exact nonnegative integer exponentiation, using repeated squaring.
    pub(crate) fn pow(
        &self,
        exponent: i64,
        budget: &mut Budget,
        span: Span,
    ) -> Result<Self, Diagnostic> {
        if exponent < 0 {
            return Err(Diagnostic::new(
                DiagnosticKind::NumericDomain,
                span,
                "bigint_pow exponent must be nonnegative",
            )
            .with_help("use a nonnegative integer exponent; negative powers are not integers"));
        }
        budget.check_bigint_digits(self.decimal_digits(), span)?;
        budget.charge(1, span)?;
        // As for integer exponentiation, 0^0 is defined to be 1.
        if exponent == 0 {
            return Self::from_integer(1, budget, span);
        }
        if self.is_zero() {
            return Self::from_integer(0, budget, span);
        }
        if self.limbs.len() == 1 && self.limbs[0] == 1 {
            return if self.negative && exponent % 2 == 1 {
                Ok(self.clone())
            } else {
                Self::from_integer(1, budget, span)
            };
        }

        // |base| >= 2, so digits(base^e) >= floor(3e/10) + 1.
        // This conservative integer bound cheaply rejects immense exponents.
        let exponent = exponent as u64;
        let lower_digits = (exponent / 10)
            .saturating_mul(3)
            .saturating_add((exponent % 10) * 3 / 10)
            .saturating_add(1);
        budget.check_bigint_digits(usize::try_from(lower_digits).unwrap_or(usize::MAX), span)?;

        // Powers of ten need no multiplication: directly construct the
        // canonical decimal limbs after prepaying exact size and work.
        budget.charge(self.limbs.len() as u64, span)?;
        if self.limbs[..self.limbs.len() - 1].iter().all(|&x| x == 0) {
            let top = *self.limbs.last().expect("nonzero magnitude");
            if (0_u32..9).any(|i| 10_u32.pow(i) == top) {
                let zeros = self.decimal_digits() - 1;
                let output_zeros = (zeros as u64).saturating_mul(exponent);
                let output_digits = output_zeros.saturating_add(1);
                budget.check_bigint_digits(
                    usize::try_from(output_digits).unwrap_or(usize::MAX),
                    span,
                )?;
                let zeros = usize::try_from(output_zeros).map_err(|_| allocation(span))?;
                let count = zeros / 9 + 1;
                budget.charge(count as u64, span)?;
                let mut limbs = buffer(count, budget, span)?;
                limbs.resize(count - 1, 0);
                limbs.push(10_u32.pow((zeros % 9) as u32));
                return finish(limbs, self.negative && exponent % 2 == 1, budget, span);
            }
        }

        // For a d-digit base, |base| >= 10^(d-1). Reject impossible dense
        // powers before allocating intermediate products.
        let lower_digits = ((self.decimal_digits() - 1) as u64)
            .saturating_mul(exponent)
            .saturating_add(1);
        budget.check_bigint_digits(usize::try_from(lower_digits).unwrap_or(usize::MAX), span)?;

        let mut power = Self::from_integer(1, budget, span)?;
        let mut factor = self.clone();
        let mut remaining = exponent;
        while remaining > 0 {
            if remaining & 1 != 0 {
                power = multiply(&power, &factor, budget, span)?;
            }
            remaining >>= 1;
            if remaining != 0 {
                factor = multiply(&factor, &factor, budget, span)?;
            }
        }
        Ok(power)
    }

    /// Magnitude bit width, including negative values; zero has width zero.
    /// Divide decimal limbs by 2^32, not by 2 bit by bit. Work and scratch
    /// capacity are prepaid using an integer upper bound (4 bits per digit).
    pub(crate) fn bit_length(&self, budget: &mut Budget, span: Span) -> Result<i64, Diagnostic> {
        budget.check_bigint_digits(self.decimal_digits(), span)?;
        let n = self.limbs.len();
        if n <= 2 {
            budget.charge(n as u64 + 1, span)?;
            let mut magnitude = 0_u64;
            for &limb in self.limbs.iter().rev() {
                magnitude = magnitude * BASE + u64::from(limb);
            }
            return Ok(i64::from(64 - magnitude.leading_zeros()));
        }

        let upper_rounds = self.decimal_digits().saturating_mul(4).div_ceil(32);
        budget.charge(
            (n as u64)
                .saturating_mul(upper_rounds as u64)
                .saturating_add(1),
            span,
        )?;
        let mut scratch = buffer(n, budget, span)?;
        scratch.extend_from_slice(&self.limbs);
        let mut bits = 0_i64;
        loop {
            let mut rem = 0_u64;
            for limb in scratch.iter_mut().rev() {
                // rem < 2^32 and BASE < 2^30: fits safely in u64.
                let current = rem * BASE + u64::from(*limb);
                *limb = (current >> 32) as u32;
                rem = current & 0xffff_ffff;
            }
            normalize(&mut scratch);
            if scratch.is_empty() {
                return Ok(bits + i64::from(64 - rem.leading_zeros()));
            }
            bits += 32;
        }
    }

    /// Canonical decimal string with resource charging before construction.
    pub(crate) fn to_text(&self, budget: &mut Budget, span: Span) -> Result<Arc<str>, Diagnostic> {
        budget.check_bigint_digits(self.decimal_digits(), span)?;
        let bytes = self
            .decimal_digits()
            .checked_add(usize::from(self.negative))
            .ok_or_else(|| crate::limits::text_construction(span, "bigint text size overflow"))?;
        budget.charge_text_allocation(bytes, span)?;
        budget.charge(u64::try_from(bytes).unwrap_or(u64::MAX), span)?;
        let mut result = String::new();
        result
            .try_reserve_exact(bytes)
            .map_err(|_| crate::limits::text_construction(span, "could not reserve bigint text"))?;
        if self.negative {
            result.push('-');
        }
        let mut parts = self.limbs.iter().rev();
        if let Some(&top) = parts.next() {
            write!(&mut result, "{top}").expect("writing into String cannot fail");
            for &limb in parts {
                write!(&mut result, "{limb:09}").expect("writing into String cannot fail");
            }
        } else {
            result.push('0');
        }
        debug_assert_eq!(result.len(), bytes);
        Ok(Arc::<str>::from(result))
    }
}
