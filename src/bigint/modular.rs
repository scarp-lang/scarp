//! Bounded modular exponentiation over signed, base-10^9 BigInts.
//! Variable-time algorithm: NOT constant-time and NOT suitable as a
//! side-channel-resistant cryptographic primitive.
use super::*;

impl BigInt {
    /// Return the nonnegative residue of a signed integer modulo a positive
    /// modulus, with truncating division internally.
    fn modular_remainder(
        value: &Self,
        modulus: &Self,
        budget: &mut Budget,
        span: Span,
    ) -> Result<Self, Diagnostic> {
        let (_, remainder) = divide(value, modulus, budget, span)?;
        if remainder.is_negative() {
            add(&remainder, modulus, budget, span)
        } else {
            Ok(remainder)
        }
    }

    /// Exact base^exponent mod modulus with repeated squaring and reduction
    /// after *each* product; the full base^exponent is never materialized.
    ///
    /// Intermediate products may need roughly twice the modulus digits and
    /// are subject to the same configured digit/limb/work limits. No unsafe,
    /// floating-point, or external big-integer implementation is involved.
    pub(crate) fn pow_mod(
        &self,
        exponent: i64,
        modulus: &Self,
        budget: &mut Budget,
        exponent_span: Span,
        modulus_span: Span,
    ) -> Result<Self, Diagnostic> {
        if exponent < 0 {
            return Err(Diagnostic::new(
                DiagnosticKind::NumericDomain,
                exponent_span,
                "bigint_pow_mod exponent must be nonnegative",
            )
            .with_help("provide a nonnegative integer exponent"));
        }
        if modulus.is_negative() || modulus.is_zero() {
            return Err(Diagnostic::new(
                DiagnosticKind::NumericDomain,
                modulus_span,
                "bigint_pow_mod modulus must be positive",
            )
            .with_help("provide a strictly positive bigint modulus"));
        }
        budget.check_bigint_digits(self.decimal_digits(), exponent_span)?;
        budget.check_bigint_digits(modulus.decimal_digits(), modulus_span)?;
        budget.charge(1, exponent_span)?;

        // For a modulus of 1 every residue, including 0^0, is zero.
        if modulus.limbs.len() == 1 && modulus.limbs[0] == 1 {
            return Self::from_integer(0, budget, modulus_span);
        }

        let one = Self::from_integer(1, budget, exponent_span)?;
        if exponent == 0 {
            return Ok(one);
        }
        let mut result = one;
        let mut factor = Self::modular_remainder(self, modulus, budget, modulus_span)?;
        let mut remaining = exponent as u64;
        while remaining > 0 {
            if remaining & 1 != 0 {
                let product = multiply(&result, &factor, budget, exponent_span)?;
                result = Self::modular_remainder(&product, modulus, budget, modulus_span)?;
            }
            remaining >>= 1;
            if remaining != 0 {
                let square = multiply(&factor, &factor, budget, exponent_span)?;
                factor = Self::modular_remainder(&square, modulus, budget, modulus_span)?;
            }
        }
        Ok(result)
    }
}
