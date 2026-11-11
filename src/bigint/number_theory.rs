//! Exact GCD/LCM and integer square root, sharing the bounded limb engine.
//! These algorithms never depend on floating-point estimates.
use super::*;

impl BigInt {
    /// Euclid's algorithm on nonnegative magnitudes. gcd(0,0) = 0.
    pub(crate) fn gcd(
        &self,
        other: &Self,
        budget: &mut Budget,
        span: Span,
    ) -> Result<Self, Diagnostic> {
        budget.check_bigint_digits(self.decimal_digits(), span)?;
        budget.check_bigint_digits(other.decimal_digits(), span)?;
        budget.charge(1, span)?;
        let mut a = self.absolute();
        let mut b = other.absolute();
        while !b.is_zero() {
            // The division engine returns both quotient and remainder in one pass.
            let (_, remainder) = divide(&a, &b, budget, span)?;
            a = b;
            b = remainder;
        }
        Ok(a)
    }

    /// Reduced multiplication avoids multiplying the two full inputs first.
    pub(crate) fn lcm(
        &self,
        other: &Self,
        budget: &mut Budget,
        span: Span,
    ) -> Result<Self, Diagnostic> {
        budget.check_bigint_digits(self.decimal_digits(), span)?;
        budget.check_bigint_digits(other.decimal_digits(), span)?;
        budget.charge(1, span)?;
        if self.is_zero() || other.is_zero() {
            return Self::from_integer(0, budget, span);
        }
        let a = self.absolute();
        let b = other.absolute();
        let divisor = a.gcd(&b, budget, span)?;
        let (quotient, remainder) = divide(&a, &divisor, budget, span)?;
        debug_assert!(remainder.is_zero(), "gcd must divide its inputs");
        multiply(&quotient, &b, budget, span)
    }

    /// Return floor(sqrt(n)) using descending integer Newton iterations.
    /// Initial guess 10^ceil(d/2) is strictly above sqrt(n) for d decimal
    /// digits. The single-digit case avoids an oversized intermediate when
    /// the configured BigInt limit is itself only one digit.
    pub(crate) fn integer_sqrt(&self, budget: &mut Budget, span: Span) -> Result<Self, Diagnostic> {
        if self.is_negative() {
            return Err(Diagnostic::new(
                DiagnosticKind::NumericDomain,
                span,
                "bigint_sqrt requires a nonnegative bigint",
            )
            .with_help("use bigint_abs first if the magnitude is intended"));
        }
        budget.check_bigint_digits(self.decimal_digits(), span)?;
        budget.charge(1, span)?;
        if self.is_zero() {
            return Ok(self.clone());
        }
        if self.decimal_digits() == 1 {
            let digit = self.limbs[0];
            let root = if digit >= 9 {
                3
            } else if digit >= 4 {
                2
            } else {
                1
            };
            return Self::from_integer(root, budget, span);
        }

        let half = self.decimal_digits().div_ceil(2);
        let shift = half / 9;
        let capacity = shift.checked_add(1).ok_or_else(|| allocation(span))?;
        budget.charge(capacity as u64, span)?;
        let mut limbs = buffer(capacity, budget, span)?;
        limbs.resize(capacity, 0);
        limbs[shift] = 10_u32.pow((half % 9) as u32);
        let mut current = finish(limbs, false, budget, span)?;
        let two = Self::from_integer(2, budget, span)?;

        loop {
            let (quotient, _) = divide(self, &current, budget, span)?;
            let sum = add(&current, &quotient, budget, span)?;
            let (next, _) = divide(&sum, &two, budget, span)?;
            if next >= current {
                return Ok(current);
            }
            current = next;
        }
    }
}
