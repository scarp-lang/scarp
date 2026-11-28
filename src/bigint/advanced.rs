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
        let exponent = u128::try_from(exponent).map_err(|_| negative_exponent(span))?;
        self.pow_checked(Some(exponent), exponent % 2 != 0, budget, span)
    }

    pub(crate) fn pow_big(
        &self,
        exponent: &Self,
        budget: &mut Budget,
        span: Span,
    ) -> Result<Self, Diagnostic> {
        if exponent.is_negative() {
            return Err(negative_exponent(span));
        }
        budget.check_bigint_digits(exponent.decimal_digits(), span)?;
        let odd = exponent.limbs.first().is_some_and(|limb| limb % 2 != 0);
        self.pow_checked(exponent.magnitude_as_u128(), odd, budget, span)
    }

    pub(super) fn magnitude_as_u128(&self) -> Option<u128> {
        self.limbs.iter().rev().try_fold(0_u128, |magnitude, limb| {
            magnitude
                .checked_mul(u128::from(BASE))?
                .checked_add(u128::from(*limb))
        })
    }

    fn pow_checked(
        &self,
        exponent: Option<u128>,
        odd: bool,
        budget: &mut Budget,
        span: Span,
    ) -> Result<Self, Diagnostic> {
        let base_digits = self.decimal_digits();
        budget.check_bigint_digits(base_digits, span)?;
        budget.charge(1, span)?;
        // As for integer exponentiation, 0^0 is defined to be 1.
        if exponent == Some(0) {
            return Self::from_integer(1, budget, span);
        }
        if self.is_zero() || exponent == Some(1) {
            return Ok(self.clone());
        }
        if self.limbs.as_slice() == [1] {
            return Ok(if odd { self.clone() } else { self.absolute() });
        }

        // Every remaining base is at least 2 in magnitude. An exponent above
        // u128 would imply at least floor(0.3 * 2^128) digits, above usize.
        let exponent = exponent.ok_or_else(|| expansion_limit(span))?;
        let top = *self.limbs.last().expect("nonzero magnitude");
        let decade = 10_u32.pow(top.ilog10());
        let leading = u64::from(top / decade);
        // |base| >= leading * 10^(base_digits-1). Taking the decimal logarithm
        // of leading^10 with ilog10 gives a proved lower bound in tenths.
        let tenths = (base_digits as u128 - 1) * 10 + u128::from(leading.pow(10).ilog10());
        let lower_digits = exponent
            .checked_mul(tenths)
            .ok_or_else(|| expansion_limit(span))?
            / 10
            + 1;
        let lower_digits = usize::try_from(lower_digits).map_err(|_| expansion_limit(span))?;
        budget.check_bigint_digits(lower_digits, span)?;
        let minimum_payload = lower_digits
            .div_ceil(9)
            .checked_mul(std::mem::size_of::<u32>())
            .ok_or_else(|| expansion_limit(span))?;
        budget.check_bigint_allocation(minimum_payload, span)?;

        // Powers of ten need no multiplication: directly construct the
        // canonical decimal limbs. For this case the lower bound is exact.
        if top == decade {
            budget.charge(self.limbs.len() as u64, span)?;
            if self.limbs[..self.limbs.len() - 1].iter().all(|&x| x == 0) {
                let zeros = lower_digits - 1;
                let count = zeros / 9 + 1;
                budget.charge(count as u64, span)?;
                let mut limbs = buffer(count, budget, span)?;
                limbs.resize(count - 1, 0);
                limbs.push(10_u32.pow((zeros % 9) as u32));
                return finish(limbs, self.negative && odd, budget, span);
            }
        }

        let mut power = None;
        let mut factor = self.clone();
        let mut remaining = exponent;
        while remaining > 0 {
            if remaining & 1 != 0 {
                power = Some(match power {
                    Some(previous) => multiply(&previous, &factor, budget, span)?,
                    None => factor.clone(),
                });
            }
            remaining >>= 1;
            if remaining != 0 {
                factor = multiply(&factor, &factor, budget, span)?;
            }
        }
        Ok(power.expect("nonzero exponent has a set bit"))
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

fn negative_exponent(span: Span) -> Diagnostic {
    Diagnostic::new(
        DiagnosticKind::NumericDomain,
        span,
        "bigint power exponent must be nonnegative",
    )
    .with_help("use a nonnegative integer exponent for exact integer powers")
}

fn expansion_limit(span: Span) -> Diagnostic {
    Diagnostic::new(
        DiagnosticKind::BigIntLimit,
        span,
        "bigint power exceeds the addressable result size",
    )
    .with_help("reduce the exponent or use bigint_pow_mod_big to compute only a modular residue")
}
