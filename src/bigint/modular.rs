//! Bounded modular exponentiation over signed, base-10^9 BigInts.
//! Variable-time algorithm: NOT constant-time and NOT suitable as a
//! side-channel-resistant cryptographic primitive.
use super::*;

enum ModularExponent<'a> {
    Small(u128),
    Limbs(&'a [u32]),
}

impl BigInt {
    /// Return the nonnegative residue of a signed integer modulo a positive
    /// modulus, with truncating division internally.
    fn modular_remainder(
        value: &Self,
        modulus: &Self,
        budget: &mut Budget,
        span: Span,
    ) -> Result<Self, Diagnostic> {
        let remainder = remainder(value, modulus, budget, span)?;
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
        let exponent =
            u128::try_from(exponent).map_err(|_| modular_exponent_error(exponent_span))?;
        self.pow_mod_checked(
            ModularExponent::Small(exponent),
            modulus,
            budget,
            exponent_span,
            modulus_span,
        )
    }

    pub(crate) fn pow_mod_big(
        &self,
        exponent: &Self,
        modulus: &Self,
        budget: &mut Budget,
        exponent_span: Span,
        modulus_span: Span,
    ) -> Result<Self, Diagnostic> {
        if exponent.is_negative() {
            return Err(modular_exponent_error(exponent_span));
        }
        budget.check_bigint_digits(exponent.decimal_digits(), exponent_span)?;
        let exponent = exponent.magnitude_as_u128().map_or(
            ModularExponent::Limbs(&exponent.limbs),
            ModularExponent::Small,
        );
        self.pow_mod_checked(exponent, modulus, budget, exponent_span, modulus_span)
    }

    fn pow_mod_checked(
        &self,
        exponent: ModularExponent<'_>,
        modulus: &Self,
        budget: &mut Budget,
        exponent_span: Span,
        modulus_span: Span,
    ) -> Result<Self, Diagnostic> {
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

        if matches!(exponent, ModularExponent::Small(0)) {
            return Self::from_integer(1, budget, exponent_span);
        }
        let factor = Self::modular_remainder(self, modulus, budget, modulus_span)?;
        if factor.is_zero() || factor.limbs.as_slice() == [1] {
            return Ok(factor);
        }
        match exponent {
            ModularExponent::Small(exponent) => {
                Self::pow_mod_reduced(factor, exponent, modulus, budget, exponent_span)
            }
            ModularExponent::Limbs(limbs) => {
                // For each exponent digit, update the power from e to
                // e*BASE + digit, without converting the exponent to binary.
                budget.charge(limbs.len() as u64, exponent_span)?;
                let (top, lower) = limbs.split_last().expect("wide exponent has limbs");
                let mut result = Self::pow_mod_reduced(
                    factor.clone(),
                    u128::from(*top),
                    modulus,
                    budget,
                    exponent_span,
                )?;
                for &limb in lower.iter().rev() {
                    result = Self::pow_mod_reduced(
                        result,
                        u128::from(BASE),
                        modulus,
                        budget,
                        exponent_span,
                    )?;
                    if limb != 0 {
                        let digit_power = Self::pow_mod_reduced(
                            factor.clone(),
                            u128::from(limb),
                            modulus,
                            budget,
                            exponent_span,
                        )?;
                        result = Self::modular_product(
                            &result,
                            &digit_power,
                            modulus,
                            budget,
                            exponent_span,
                        )?;
                    }
                }
                Ok(result)
            }
        }
    }

    fn pow_mod_reduced(
        mut factor: Self,
        mut exponent: u128,
        modulus: &Self,
        budget: &mut Budget,
        span: Span,
    ) -> Result<Self, Diagnostic> {
        debug_assert_ne!(exponent, 0);
        let mut result = None;
        while exponent != 0 {
            if exponent & 1 != 0 {
                result = Some(match result {
                    Some(previous) => {
                        Self::modular_product(&previous, &factor, modulus, budget, span)?
                    }
                    None => factor.clone(),
                });
            }
            exponent >>= 1;
            if exponent != 0 {
                factor = Self::modular_product(&factor, &factor, modulus, budget, span)?;
            }
        }
        Ok(result.expect("nonzero exponent has a set bit"))
    }

    fn modular_product(
        left: &Self,
        right: &Self,
        modulus: &Self,
        budget: &mut Budget,
        span: Span,
    ) -> Result<Self, Diagnostic> {
        let product = multiply(left, right, budget, span)?;
        remainder(&product, modulus, budget, span)
    }
}

fn modular_exponent_error(span: Span) -> Diagnostic {
    Diagnostic::new(
        DiagnosticKind::NumericDomain,
        span,
        "bigint modular exponent must be nonnegative",
    )
    .with_help("provide a nonnegative exponent")
}
