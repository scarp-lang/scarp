//! Exact sequences, combinatorics and additional integer operations.
use super::*;

impl BigInt {
    pub(crate) fn fibonacci(n: i64, budget: &mut Budget, span: Span) -> Result<Self, Diagnostic> {
        if n < 0 {
            return Err(domain(span, "bigint_fibonacci requires a nonnegative index"));
        }
        if n == 0 {
            return Self::from_integer(0, budget, span);
        }
        if n == 1 {
            return Self::from_integer(1, budget, span);
        }
        // F(n) >= 2^floor((n-2)/2), and log10(2) > 0.3.
        let lower = ((n as u64 - 2) / 2).saturating_mul(3) / 10 + 1;
        budget.check_bigint_digits(usize::try_from(lower).unwrap_or(usize::MAX), span)?;
        let (a, b) = fib_pair(n as u64 / 2, budget, span)?;
        if n % 2 == 0 {
            let twice = add(&b, &b, budget, span)?;
            multiply(&a, &add(&twice, &a.negated(), budget, span)?, budget, span)
        } else {
            let aa = multiply(&a, &a, budget, span)?;
            let bb = multiply(&b, &b, budget, span)?;
            add(&aa, &bb, budget, span)
        }
    }

    pub(crate) fn binomial(
        n: i64,
        k: i64,
        budget: &mut Budget,
        span: Span,
    ) -> Result<Self, Diagnostic> {
        validate_selection(n, k, span)?;
        let k = k.min(n - k);
        let mut result = Self::from_integer(1, budget, span)?;
        for i in 1..=k {
            budget.charge(1, span)?;
            let mut numerator = Self::from_integer(n - k + i, budget, span)?;
            let mut divisor = Self::from_integer(i, budget, span)?;
            // Cancel before multiplying to avoid oversized intermediates.
            let common = numerator.gcd(&divisor, budget, span)?;
            numerator = quotient(&numerator, &common, budget, span)?;
            divisor = quotient(&divisor, &common, budget, span)?;
            result = quotient(&result, &divisor, budget, span)?;
            result = multiply(&result, &numerator, budget, span)?;
        }
        Ok(result)
    }

    pub(crate) fn permutation(
        n: i64,
        k: i64,
        budget: &mut Budget,
        span: Span,
    ) -> Result<Self, Diagnostic> {
        validate_selection(n, k, span)?;
        if k == 0 {
            return Self::from_integer(1, budget, span);
        }
        range_product(n - k + 1, n, budget, span)
    }

    pub(crate) fn next_prime(&self, budget: &mut Budget, span: Span) -> Result<Self, Diagnostic> {
        budget.check_bigint_digits(self.decimal_digits(), span)?;
        budget.charge(1, span)?;
        if self.is_negative() || self.is_zero() || self.limbs.as_slice() == [1] {
            return Self::from_integer(2, budget, span);
        }
        let one = Self::from_integer(1, budget, span)?;
        let two = Self::from_integer(2, budget, span)?;
        let mut candidate = add(self, &one, budget, span)?;
        if candidate.is_even() {
            candidate = add(&candidate, &one, budget, span)?;
        }
        loop {
            budget.charge(1, span)?;
            if candidate.is_prime(budget, span)? {
                return Ok(candidate);
            }
            candidate = add(&candidate, &two, budget, span)?;
        }
    }

    pub(crate) fn is_perfect_square(
        &self,
        budget: &mut Budget,
        span: Span,
    ) -> Result<bool, Diagnostic> {
        budget.check_bigint_digits(self.decimal_digits(), span)?;
        budget.charge(1, span)?;
        if self.is_negative() {
            return Ok(false);
        }
        let root = self.integer_sqrt(budget, span)?;
        let square = multiply(&root, &root, budget, span)?;
        budget.charge(self.limbs.len() as u64 + 1, span)?;
        Ok(&square == self)
    }

    pub(crate) fn is_power_of_two(
        &self,
        budget: &mut Budget,
        span: Span,
    ) -> Result<bool, Diagnostic> {
        budget.check_bigint_digits(self.decimal_digits(), span)?;
        budget.charge(1, span)?;
        if self.is_negative() || self.is_zero() || !self.is_even() {
            return Ok(!self.is_negative() && self.limbs.as_slice() == [1]);
        }
        let words = self.magnitude_words(budget, span)?;
        budget.charge(words.len() as u64, span)?;
        let mut seen = false;
        for word in words {
            if word != 0 {
                if seen || !word.is_power_of_two() {
                    return Ok(false);
                }
                seen = true;
            }
        }
        Ok(seen)
    }

    /// Reverse the decimal magnitude without allocating a text value.
    pub(crate) fn reverse_digits(&self, budget: &mut Budget, span: Span) -> Result<Self, Diagnostic> {
        let digits = self.decimal_digits();
        budget.check_bigint_digits(digits, span)?;
        budget.charge((digits as u64).saturating_mul(3), span)?;
        if self.is_zero() {
            return Ok(self.clone());
        }
        let mut limbs = buffer(digits.div_ceil(9), budget, span)?;
        limbs.resize(digits.div_ceil(9), 0);
        let mut position = digits;
        for &limb in self.limbs.iter() {
            let mut value = limb;
            for _ in 0..9.min(position) {
                position -= 1;
                limbs[position / 9] += (value % 10) * 10_u32.pow((position % 9) as u32);
                value /= 10;
            }
        }
        finish(limbs, self.is_negative(), budget, span)
    }

    pub(crate) fn is_palindrome(
        &self,
        budget: &mut Budget,
        span: Span,
    ) -> Result<bool, Diagnostic> {
        budget.check_bigint_digits(self.decimal_digits(), span)?;
        budget.charge(1, span)?;
        if self.is_negative() {
            return Ok(false);
        }
        let reversed = self.reverse_digits(budget, span)?;
        budget.charge(self.limbs.len() as u64 + 1, span)?;
        Ok(&reversed == self)
    }

    /// Extended Euclidean algorithm. Return the least nonnegative inverse.
    pub(crate) fn mod_inverse(
        &self,
        modulus: &Self,
        budget: &mut Budget,
        span: Span,
    ) -> Result<Self, Diagnostic> {
        budget.check_bigint_digits(self.decimal_digits(), span)?;
        budget.check_bigint_digits(modulus.decimal_digits(), span)?;
        budget.charge(1, span)?;
        if modulus.is_negative() || modulus.is_zero() || modulus.limbs.as_slice() == [1] {
            return Err(domain(span, "bigint_mod_inverse requires a modulus greater than one"));
        }
        let zero = Self::from_integer(0, budget, span)?;
        let one = Self::from_integer(1, budget, span)?;
        let mut old_r = modulus.clone();
        let mut r = remainder(self, modulus, budget, span)?;
        if r.is_negative() {
            r = add(&r, modulus, budget, span)?;
        }
        let mut old_t = zero;
        let mut t = one.clone();
        while !r.is_zero() {
            budget.charge(1, span)?;
            let (q, next_r) = divmod(&old_r, &r, budget, span)?;
            let product = multiply(&q, &t, budget, span)?;
            let next_t = add(&old_t, &product.negated(), budget, span)?;
            old_r = r;
            r = next_r;
            old_t = t;
            t = next_t;
        }
        if old_r != one {
            return Err(domain(span, "bigint_mod_inverse requires coprime operands"));
        }
        let inverse = remainder(&old_t, modulus, budget, span)?;
        if inverse.is_negative() {
            add(&inverse, modulus, budget, span)
        } else {
            Ok(inverse)
        }
    }
}

fn domain(span: Span, message: &str) -> Diagnostic {
    Diagnostic::new(DiagnosticKind::NumericDomain, span, message)
}

fn validate_selection(n: i64, k: i64, span: Span) -> Result<(), Diagnostic> {
    if n < 0 || k < 0 || k > n {
        return Err(domain(span, "combinatorics requires 0 <= k <= n"));
    }
    Ok(())
}

fn range_product(lo: i64, hi: i64, budget: &mut Budget, span: Span) -> Result<BigInt, Diagnostic> {
    budget.charge(1, span)?;
    if lo == hi {
        return BigInt::from_integer(lo, budget, span);
    }
    let mid = lo + (hi - lo) / 2;
    let a = range_product(lo, mid, budget, span)?;
    let b = range_product(mid + 1, hi, budget, span)?;
    multiply(&a, &b, budget, span)
}

// Size-decreasing recursion: at most 63 frames for an i64 index.
fn fib_pair(n: u64, budget: &mut Budget, span: Span) -> Result<(BigInt, BigInt), Diagnostic> {
    budget.charge(1, span)?;
    if n == 0 {
        return Ok((
            BigInt::from_integer(0, budget, span)?,
            BigInt::from_integer(1, budget, span)?,
        ));
    }
    let (a, b) = fib_pair(n / 2, budget, span)?;
    let twice = add(&b, &b, budget, span)?;
    let c = multiply(&a, &add(&twice, &a.negated(), budget, span)?, budget, span)?;
    let aa = multiply(&a, &a, budget, span)?;
    let bb = multiply(&b, &b, budget, span)?;
    let d = add(&aa, &bb, budget, span)?;
    if n % 2 == 0 {
        Ok((c, d))
    } else {
        let e = add(&c, &d, budget, span)?;
        Ok((d, e))
    }
}
