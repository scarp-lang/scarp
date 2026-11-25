//! Bounded integer predicates, decimal inspection and number theory utilities.
use super::*;

impl BigInt {
    pub(crate) fn is_even(&self) -> bool {
        self.limbs.first().map_or(true, |x| x % 2 == 0)
    }

    pub(crate) fn digit_sum(&self, budget: &mut Budget, span: Span) -> Result<i64, Diagnostic> {
        budget.check_bigint_digits(self.decimal_digits(), span)?;
        budget.charge(self.decimal_digits() as u64, span)?;
        let mut sum = 0_i64;
        for &limb in self.limbs.iter() {
            let mut digit = limb;
            while digit != 0 {
                sum += i64::from(digit % 10);
                digit /= 10;
            }
        }
        Ok(sum)
    }

    /// Decimal trailing zero count; zero itself has no finite valuation.
    pub(crate) fn trailing_zeros(&self, budget: &mut Budget, span: Span) -> Result<i64, Diagnostic> {
        budget.check_bigint_digits(self.decimal_digits(), span)?;
        if self.is_zero() {
            return Err(domain(span, "bigint_trailing_zeros requires a nonzero bigint"));
        }
        let mut count = 0_i64;
        for &limb in self.limbs.iter() {
            budget.charge(9, span)?;
            if limb == 0 {
                count += 9;
                continue;
            }
            let mut value = limb;
            while value % 10 == 0 {
                count += 1;
                value /= 10;
            }
            break;
        }
        Ok(count)
    }

    /// Balanced product tree; each node and every limb operation is charged.
    pub(crate) fn factorial(n: i64, budget: &mut Budget, span: Span) -> Result<Self, Diagnostic> {
        if n < 0 {
            return Err(domain(span, "bigint_factorial requires a nonnegative integer"));
        }
        if n < 2 {
            return Self::from_integer(1, budget, span);
        }
        // For n >= 20 the upper half of factors are >= 10.
        if n >= 20 {
            budget.check_bigint_digits(
                usize::try_from(n / 2 + 1).unwrap_or(usize::MAX),
                span,
            )?;
        }
        product_range(2, n, budget, span)
    }

    /// Floor(log2(|n|)); unlike bit_length, zero is outside the domain.
    pub(crate) fn log2(&self, budget: &mut Budget, span: Span) -> Result<i64, Diagnostic> {
        if self.is_zero() {
            return Err(domain(span, "bigint_log2 requires a nonzero bigint"));
        }
        self.bit_length(budget, span).map(|bits| bits - 1)
    }

    /// Floor of the real nonnegative nth root, using a bounded binary search.
    pub(crate) fn integer_root(
        &self,
        degree: i64,
        budget: &mut Budget,
        span: Span,
    ) -> Result<Self, Diagnostic> {
        if degree <= 0 || self.is_negative() {
            return Err(domain(
                span,
                "bigint_root requires a nonnegative bigint and positive integer degree",
            ));
        }
        budget.check_bigint_digits(self.decimal_digits(), span)?;
        budget.charge(1, span)?;
        if degree == 1 || self.is_zero() {
            return Ok(self.clone());
        }
        if degree == 2 {
            return self.integer_sqrt(budget, span);
        }
        let one = Self::from_integer(1, budget, span)?;
        if self == &one || degree as u64 >= (self.decimal_digits() as u64) * 4 {
            return Ok(one);
        }
        let two = Self::from_integer(2, budget, span)?;
        let mut low = one;
        let mut high = self.clone();
        // A decimal upper bound much smaller than n for higher roots.
        if self.decimal_digits() > 1 {
            let d = (self.decimal_digits() as u64).div_ceil(degree as u64);
            let ten = Self::from_integer(10, budget, span)?;
            high = ten.pow(d as i64, budget, span)?;
        }
        loop {
            let delta = add(&high, &low.negated(), budget, span)?;
            if delta.is_zero() || (delta.limbs.len() == 1 && delta.limbs[0] == 1) {
                return Ok(low);
            }
            let half = quotient(&delta, &two, budget, span)?;
            let mid = add(&low, &half, budget, span)?;
            if power_at_most(&mid, degree as u64, self, budget, span)? {
                low = mid;
            } else {
                high = mid;
            }
        }
    }

    /// Exact primality on all successful returns. The large-number proof
    /// path can exhaust the work quota; it must never return a guess.
    pub(crate) fn is_prime(&self, budget: &mut Budget, span: Span) -> Result<bool, Diagnostic> {
        budget.check_bigint_digits(self.decimal_digits(), span)?;
        budget.charge(1, span)?;
        if self.negative || self.is_zero() {
            return Ok(false);
        }
        if let Some(n) = self.as_u64() {
            return prime_u64(n, budget, span);
        }
        if !self.is_probable_prime(budget, span)? {
            return Ok(false);
        }
        let mut divisor = Self::from_integer(3, budget, span)?;
        let two = Self::from_integer(2, budget, span)?;
        loop {
            budget.charge(1, span)?;
            let (q, r) = divmod(self, &divisor, budget, span)?;
            budget.charge(divisor.limbs.len().max(q.limbs.len()) as u64, span)?;
            if divisor > q {
                return Ok(true);
            }
            if r.is_zero() {
                return Ok(false);
            }
            divisor = add(&divisor, &two, budget, span)?;
        }
    }

    /// Fixed-base strong probable-prime screen. Above u64, a true result
    /// is not a proof, and the bases are not randomized.
    pub(crate) fn is_probable_prime(
        &self,
        budget: &mut Budget,
        span: Span,
    ) -> Result<bool, Diagnostic> {
        budget.check_bigint_digits(self.decimal_digits(), span)?;
        budget.charge(1, span)?;
        if self.negative || self.is_zero() {
            return Ok(false);
        }
        if let Some(n) = self.as_u64() {
            return prime_u64(n, budget, span);
        }
        if self.is_even() {
            return Ok(false);
        }
        let one = Self::from_integer(1, budget, span)?;
        let two = Self::from_integer(2, budget, span)?;
        let minus_one = add(self, &one.negated(), budget, span)?;
        let mut d = minus_one.clone();
        let mut s = 0_u64;
        while d.is_even() {
            d = quotient(&d, &two, budget, span)?;
            s += 1;
        }
        for base in [2, 3, 5, 7, 11, 13, 17, 19, 23, 29, 31, 37] {
            let a = Self::from_integer(base, budget, span)?;
            let mut x = pow_mod_big_exponent(&a, &d, self, &one, &two, budget, span)?;
            budget.charge(self.limbs.len() as u64 + 1, span)?;
            if x == one || x == minus_one {
                continue;
            }
            let mut passed = false;
            for _ in 1..s {
                x = remainder(&multiply(&x, &x, budget, span)?, self, budget, span)?;
                budget.charge(self.limbs.len() as u64 + 1, span)?;
                if x == minus_one {
                    passed = true;
                    break;
                }
                if x == one {
                    return Ok(false);
                }
            }
            if !passed {
                return Ok(false);
            }
        }
        Ok(true)
    }

    fn as_u64(&self) -> Option<u64> {
        if self.negative || self.limbs.len() > 3 {
            return None;
        }
        self.limbs.iter().rev().try_fold(0_u64, |acc, &limb| {
            acc.checked_mul(BASE)?.checked_add(u64::from(limb))
        })
    }
}

fn domain(span: Span, message: &str) -> Diagnostic {
    Diagnostic::new(DiagnosticKind::NumericDomain, span, message)
}

fn product_range(lo: i64, hi: i64, budget: &mut Budget, span: Span) -> Result<BigInt, Diagnostic> {
    budget.charge(1, span)?;
    if lo == hi {
        return BigInt::from_integer(lo, budget, span);
    }
    let mid = lo + (hi - lo) / 2;
    let left = product_range(lo, mid, budget, span)?;
    let right = product_range(mid + 1, hi, budget, span)?;
    multiply(&left, &right, budget, span)
}

// Return a product only if it fits the mathematical bound. Division is
// used only for ambiguous digit sizes, preventing oversized intermediates.
fn bounded_product(
    a: &BigInt,
    b: &BigInt,
    limit: &BigInt,
    budget: &mut Budget,
    span: Span,
) -> Result<Option<BigInt>, Diagnostic> {
    budget.charge(1, span)?;
    if a.is_zero() || b.is_zero() {
        return Ok(Some(BigInt::from_integer(0, budget, span)?));
    }
    let digits = a.decimal_digits() + b.decimal_digits();
    let max_digits = limit.decimal_digits();
    if digits - 1 > max_digits {
        return Ok(None);
    }
    if digits >= max_digits {
        let bound = quotient(limit, b, budget, span)?;
        budget.charge(a.limbs.len().max(bound.limbs.len()) as u64, span)?;
        if a > &bound {
            return Ok(None);
        }
    }
    multiply(a, b, budget, span).map(Some)
}

fn power_at_most(
    base: &BigInt,
    mut exponent: u64,
    limit: &BigInt,
    budget: &mut Budget,
    span: Span,
) -> Result<bool, Diagnostic> {
    let mut result = BigInt::from_integer(1, budget, span)?;
    let mut factor = base.clone();
    while exponent != 0 {
        if exponent & 1 != 0 {
            let Some(next) = bounded_product(&result, &factor, limit, budget, span)? else {
                return Ok(false);
            };
            result = next;
        }
        exponent >>= 1;
        if exponent != 0 {
            let Some(next) = bounded_product(&factor, &factor, limit, budget, span)? else {
                return Ok(false);
            };
            factor = next;
        }
    }
    Ok(true)
}

fn pow_mod_big_exponent(
    base: &BigInt,
    exponent: &BigInt,
    modulus: &BigInt,
    one: &BigInt,
    two: &BigInt,
    budget: &mut Budget,
    span: Span,
) -> Result<BigInt, Diagnostic> {
    let mut result = one.clone();
    let mut factor = remainder(base, modulus, budget, span)?;
    let mut e = exponent.clone();
    while !e.is_zero() {
        if !e.is_even() {
            result = remainder(&multiply(&result, &factor, budget, span)?, modulus, budget, span)?;
        }
        e = quotient(&e, two, budget, span)?;
        if !e.is_zero() {
            factor = remainder(&multiply(&factor, &factor, budget, span)?, modulus, budget, span)?;
        }
    }
    Ok(result)
}

fn mul_mod_u64(a: u64, b: u64, modulus: u64) -> u64 {
    ((u128::from(a) * u128::from(b)) % u128::from(modulus)) as u64
}

fn pow_mod_u64(mut base: u64, mut exponent: u64, modulus: u64) -> u64 {
    let mut result = 1;
    base %= modulus;
    while exponent != 0 {
        if exponent & 1 != 0 {
            result = mul_mod_u64(result, base, modulus);
        }
        exponent >>= 1;
        if exponent != 0 {
            base = mul_mod_u64(base, base, modulus);
        }
    }
    result
}

// Deterministic Miller-Rabin for every u64, using the proven seven-base set.
// Prepay a conservative bound for all modular squarings and products.
fn prime_u64(n: u64, budget: &mut Budget, span: Span) -> Result<bool, Diagnostic> {
    budget.charge(12, span)?;
    if n < 2 {
        return Ok(false);
    }
    for p in [2, 3, 5, 7, 11, 13, 17, 19, 23, 29, 31, 37] {
        if n % p == 0 {
            return Ok(n == p);
        }
    }
    let s = (n - 1).trailing_zeros();
    let d = (n - 1) >> s;
    budget.charge(7 * 192, span)?;
    'bases: for a in [2, 325, 9375, 28178, 450775, 9780504, 1795265022] {
        if a % n == 0 {
            continue;
        }
        let mut x = pow_mod_u64(a, d, n);
        if x == 1 || x == n - 1 {
            continue;
        }
        for _ in 1..s {
            x = mul_mod_u64(x, x, n);
            if x == n - 1 {
                continue 'bases;
            }
        }
        return Ok(false);
    }
    Ok(true)
}
