//! Bounded signed integers with little-endian base-10^9 limbs.
//!
//! Decimal limbs make parsing/formatting linear without a second conversion
//! buffer. Arithmetic is iterative safe Rust. All new limb buffers (including
//! division scratch) are charged before fallible reservation; work is prepaid.
use std::{cmp::Ordering, fmt, sync::Arc};

use crate::{
    ast::BinaryOperator,
    diagnostics::{Diagnostic, DiagnosticKind, Span},
    limits::Budget,
    value::{Value, ValueType},
};

const BASE: u64 = 1_000_000_000;
/// Initial implementation ceiling; hosts may select a smaller per-value limit.
pub const MAX_BIGINT_DIGITS: usize = 1_000_000;
pub const DEFAULT_BIGINT_DIGITS: usize = 10_000;
pub const DEFAULT_BIGINT_BYTES: usize = 8 * 1024 * 1024;

/// Immutable exact signed integer. Zero has no limbs and is never negative.
/// Clones share their immutable magnitude; arithmetic creates a new value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigInt {
    negative: bool,
    limbs: Arc<Vec<u32>>,
}

impl BigInt {
    pub fn is_zero(&self) -> bool {
        self.limbs.is_empty()
    }
    pub fn is_negative(&self) -> bool {
        self.negative
    }
    pub fn decimal_digits(&self) -> usize {
        digits(&self.limbs)
    }
    /// Logical magnitude payload, excluding allocator/Arc/Vec overhead.
    pub fn payload_bytes(&self) -> usize {
        self.limbs.len() * std::mem::size_of::<u32>()
    }
    /// Exact conversion. Out-of-range values never saturate or round.
    pub fn to_integer(&self) -> Option<i64> {
        let mut magnitude = 0_u64;
        for &limb in self.limbs.iter().rev() {
            magnitude = magnitude.checked_mul(BASE)?.checked_add(u64::from(limb))?;
        }
        if self.negative {
            if magnitude == 1_u64 << 63 {
                Some(i64::MIN)
            } else {
                i64::try_from(magnitude).ok()?.checked_neg()
            }
        } else {
            i64::try_from(magnitude).ok()
        }
    }
    pub(crate) fn negated(&self) -> Self {
        Self {
            negative: !self.negative && !self.is_zero(),
            limbs: Arc::clone(&self.limbs),
        }
    }
    pub(crate) fn absolute(&self) -> Self {
        Self {
            negative: false,
            limbs: Arc::clone(&self.limbs),
        }
    }
    pub(crate) fn from_integer(
        value: i64,
        budget: &mut Budget,
        span: Span,
    ) -> Result<Self, Diagnostic> {
        budget.charge(3, span)?;
        let mut magnitude = value.unsigned_abs();
        let count = if magnitude == 0 {
            0
        } else if magnitude < BASE {
            1
        } else if magnitude < BASE * BASE {
            2
        } else {
            3
        };
        let mut limbs = buffer(count, budget, span)?;
        while magnitude != 0 {
            limbs.push((magnitude % BASE) as u32);
            magnitude /= BASE;
        }
        finish(limbs, value < 0, budget, span)
    }
    pub(crate) fn parse_decimal(
        text: &str,
        budget: &mut Budget,
        span: Span,
    ) -> Result<Self, Diagnostic> {
        let (negative, number) = match text.as_bytes().first() {
            Some(b'-') => (true, &text[1..]),
            Some(b'+') => (false, &text[1..]),
            _ => (false, text),
        };
        // Count raw digits, including leading zeros, before scanning input.
        budget.check_bigint_digits(number.len(), span)?;
        budget.charge(number.len().max(1) as u64, span)?;
        if number.is_empty() || !number.bytes().all(|b| b.is_ascii_digit()) {
            return Err(Diagnostic::new(DiagnosticKind::InvalidBigInt, span,
                "bigint requires an optional sign followed by ASCII decimal digits")
                .with_help("use bigint(\"-12345678901234567890\"); spaces, separators, radix prefixes and decimal points are not accepted"));
        }
        let number = number.trim_start_matches('0');
        let mut limbs = buffer(number.len().div_ceil(9), budget, span)?;
        for chunk in number.as_bytes().rchunks(9) {
            let mut limb = 0;
            for digit in chunk {
                limb = limb * 10 + u32::from(digit - b'0');
            }
            limbs.push(limb);
        }
        finish(limbs, negative, budget, span)
    }
}

impl Ord for BigInt {
    fn cmp(&self, other: &Self) -> Ordering {
        match self.negative.cmp(&other.negative) {
            Ordering::Equal => {
                let order = magnitude_cmp(&self.limbs, &other.limbs);
                if self.negative {
                    order.reverse()
                } else {
                    order
                }
            }
            Ordering::Less => Ordering::Greater,
            Ordering::Greater => Ordering::Less,
        }
    }
}
impl PartialOrd for BigInt {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl fmt::Display for BigInt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_zero() {
            return f.write_str("0");
        }
        if self.negative {
            f.write_str("-")?;
        }
        let mut limbs = self.limbs.iter().rev();
        write!(f, "{}", limbs.next().expect("nonzero magnitude"))?;
        for limb in limbs {
            write!(f, "{limb:09}")?;
        }
        Ok(())
    }
}

fn digits(limbs: &[u32]) -> usize {
    let Some(&last) = limbs.last() else {
        return 1;
    };
    let mut top = last;
    let mut count = 1;
    while top >= 10 {
        top /= 10;
        count += 1;
    }
    (limbs.len() - 1) * 9 + count
}
fn normalize(limbs: &mut Vec<u32>) {
    while limbs.last() == Some(&0) {
        limbs.pop();
    }
}
fn magnitude_cmp(left: &[u32], right: &[u32]) -> Ordering {
    left.len()
        .cmp(&right.len())
        .then_with(|| left.iter().rev().cmp(right.iter().rev()))
}
fn buffer(count: usize, budget: &mut Budget, span: Span) -> Result<Vec<u32>, Diagnostic> {
    let bytes = count
        .checked_mul(std::mem::size_of::<u32>())
        .ok_or_else(|| allocation(span))?;
    budget.charge_bigint_allocation(bytes, span)?;
    let mut values = Vec::new();
    values
        .try_reserve_exact(count)
        .map_err(|_| allocation(span))?;
    Ok(values)
}
fn allocation(span: Span) -> Diagnostic {
    Diagnostic::new(
        DiagnosticKind::BigIntAllocation,
        span,
        "unable to reserve bigint limb storage",
    )
    .with_help("reduce operand sizes or host memory pressure")
}
fn finish(
    mut limbs: Vec<u32>,
    negative: bool,
    budget: &Budget,
    span: Span,
) -> Result<BigInt, Diagnostic> {
    normalize(&mut limbs);
    budget.check_bigint_digits(digits(&limbs), span)?;
    Ok(BigInt {
        negative: negative && !limbs.is_empty(),
        limbs: Arc::new(limbs),
    })
}

fn add(
    left: &BigInt,
    right: &BigInt,
    budget: &mut Budget,
    span: Span,
) -> Result<BigInt, Diagnostic> {
    let length = left.limbs.len().max(right.limbs.len());
    budget.charge((length as u64).saturating_mul(3).saturating_add(1), span)?;
    if left.negative == right.negative {
        let capacity = length.checked_add(1).ok_or_else(|| allocation(span))?;
        let mut result = buffer(capacity, budget, span)?;
        let mut carry = 0_u64;
        for i in 0..length {
            let sum = u64::from(*left.limbs.get(i).unwrap_or(&0))
                + u64::from(*right.limbs.get(i).unwrap_or(&0))
                + carry;
            result.push((sum % BASE) as u32);
            carry = sum / BASE;
        }
        if carry != 0 {
            result.push(carry as u32);
        }
        finish(result, left.negative, budget, span)
    } else {
        let (large, small) = if magnitude_cmp(&left.limbs, &right.limbs) == Ordering::Less {
            (right, left)
        } else {
            (left, right)
        };
        let mut result = buffer(large.limbs.len(), budget, span)?;
        result.extend_from_slice(&large.limbs);
        subtract_magnitude(&mut result, &small.limbs);
        finish(result, large.negative, budget, span)
    }
}
/// Subtract a smaller magnitude in place. Both inputs are normalized.
fn subtract_magnitude(left: &mut Vec<u32>, right: &[u32]) {
    let mut borrow = 0_i64;
    for (i, limb) in left.iter_mut().enumerate() {
        let difference = i64::from(*limb) - i64::from(*right.get(i).unwrap_or(&0)) - borrow;
        borrow = i64::from(difference < 0);
        *limb = (difference + borrow * BASE as i64) as u32;
    }
    debug_assert_eq!(borrow, 0);
    normalize(left);
}
fn multiply(
    left: &BigInt,
    right: &BigInt,
    budget: &mut Budget,
    span: Span,
) -> Result<BigInt, Diagnostic> {
    let (n, m) = (left.limbs.len(), right.limbs.len());
    let capacity = n.checked_add(m).ok_or_else(|| allocation(span))?;
    budget.charge(
        (n as u64)
            .saturating_mul(m as u64)
            .saturating_add(capacity as u64)
            .saturating_add(1),
        span,
    )?;
    if n == 0 || m == 0 {
        return finish(Vec::new(), false, budget, span);
    }
    // A nonzero product has at least d(left) + d(right) - 1 digits.
    budget.check_bigint_digits(left.decimal_digits() + right.decimal_digits() - 1, span)?;
    let mut result = buffer(capacity, budget, span)?;
    result.resize(capacity, 0);
    for (i, &a) in left.limbs.iter().enumerate() {
        let mut carry = 0_u64;
        for (j, &b) in right.limbs.iter().enumerate() {
            // (BASE-1)^2 + 2*(BASE-1) == BASE^2-1 fits u64.
            let product = u64::from(a) * u64::from(b) + u64::from(result[i + j]) + carry;
            result[i + j] = (product % BASE) as u32;
            carry = product / BASE;
        }
        result[i + m] = carry as u32;
    }
    finish(result, left.negative != right.negative, budget, span)
}

/// Long division, finding each base-10^9 quotient digit by binary search.
fn divide(
    left: &BigInt,
    right: &BigInt,
    budget: &mut Budget,
    span: Span,
) -> Result<(BigInt, BigInt), Diagnostic> {
    let (quotient, remainder) = divide_with_remainder(left, right, true, budget, span)?;
    Ok((quotient.expect("requested quotient"), remainder))
}

/// The remainder-only path uses the same work charge and quotient-digit
/// search. It keeps only the current digit and skips the quotient buffer.
fn divide_with_remainder(
    left: &BigInt,
    right: &BigInt,
    keep_quotient: bool,
    budget: &mut Budget,
    span: Span,
) -> Result<(Option<BigInt>, BigInt), Diagnostic> {
    if right.is_zero() {
        return Err(
            Diagnostic::new(DiagnosticKind::DivisionByZero, span, "division by zero")
                .with_help("the bigint divisor must be nonzero"),
        );
    }
    let (n, m) = (left.limbs.len(), right.limbs.len());
    budget.charge(n.max(m).max(1) as u64, span)?;
    if magnitude_cmp(&left.limbs, &right.limbs) == Ordering::Less {
        let quotient = if keep_quotient {
            Some(finish(Vec::new(), false, budget, span)?)
        } else {
            None
        };
        return Ok((quotient, left.clone()));
    }
    let count = n - m + 1;
    let work = if m == 1 {
        n as u64
    } else {
        (count as u64)
            .saturating_mul(m as u64)
            .saturating_mul(64)
            .saturating_add((n as u64).saturating_mul(4))
    };
    budget.charge(work, span)?;
    let mut quotient = if keep_quotient {
        let mut limbs = buffer(count, budget, span)?;
        limbs.resize(count, 0);
        Some(limbs)
    } else {
        None
    };

    let remainder = if m == 1 {
        let mut remainder = 0_u64;
        let divisor = u64::from(right.limbs[0]);
        for i in (0..n).rev() {
            let current = remainder * BASE + u64::from(left.limbs[i]);
            if let Some(quotient) = &mut quotient {
                quotient[i] = (current / divisor) as u32;
            }
            remainder = current % divisor;
        }
        let mut rest = buffer(usize::from(remainder != 0), budget, span)?;
        if remainder != 0 {
            rest.push(remainder as u32);
        }
        rest
    } else {
        let capacity = m.checked_add(1).ok_or_else(|| allocation(span))?;
        let mut remainder = buffer(capacity, budget, span)?;
        remainder.extend_from_slice(&left.limbs[count..]);
        normalize(&mut remainder);
        let mut trial = buffer(capacity, budget, span)?;
        for i in (0..count).rev() {
            remainder.insert(0, left.limbs[i]);
            normalize(&mut remainder);
            let (mut low, mut high) = (0_u32, (BASE - 1) as u32);
            while low < high {
                let digit = low + (high - low) / 2 + 1;
                multiply_small(&right.limbs, digit, &mut trial);
                if magnitude_cmp(&trial, &remainder) != Ordering::Greater {
                    low = digit;
                } else {
                    high = digit - 1;
                }
            }
            if let Some(quotient) = &mut quotient {
                quotient[i] = low;
            }
            multiply_small(&right.limbs, low, &mut trial);
            subtract_magnitude(&mut remainder, &trial);
        }
        remainder
    };
    Ok((
        quotient
            .map(|limbs| finish(limbs, left.negative != right.negative, budget, span))
            .transpose()?,
        finish(remainder, left.negative, budget, span)?,
    ))
}
fn multiply_small(limbs: &[u32], digit: u32, result: &mut Vec<u32>) {
    result.clear();
    let mut carry = 0_u64;
    for &limb in limbs {
        let current = u64::from(limb) * u64::from(digit) + carry;
        result.push((current % BASE) as u32);
        carry = current / BASE;
    }
    if carry != 0 {
        result.push(carry as u32);
    }
    normalize(result);
}

pub(crate) fn binary(
    operator: BinaryOperator,
    left: &BigInt,
    right: &BigInt,
    budget: &mut Budget,
    span: Span,
) -> Result<Value, Diagnostic> {
    use BinaryOperator::*;
    budget.check_bigint_digits(left.decimal_digits().max(right.decimal_digits()), span)?;
    match operator {
        Add => add(left, right, budget, span).map(Value::BigInt),
        Subtract => add(left, &right.negated(), budget, span).map(Value::BigInt),
        Multiply => multiply(left, right, budget, span).map(Value::BigInt),
        Divide => divide(left, right, budget, span).map(|(quotient, _)| Value::BigInt(quotient)),
        Remainder => divide_with_remainder(left, right, false, budget, span)
            .map(|(_, remainder)| Value::BigInt(remainder)),
        Equal | NotEqual | Less | LessEqual | Greater | GreaterEqual => {
            budget.charge(left.limbs.len().max(right.limbs.len()).max(1) as u64, span)?;
            let order = left.cmp(right);
            Ok(Value::Boolean(match operator {
                Equal => order == Ordering::Equal,
                NotEqual => order != Ordering::Equal,
                Less => order == Ordering::Less,
                LessEqual => order != Ordering::Greater,
                Greater => order == Ordering::Greater,
                _ => order != Ordering::Less,
            }))
        }
        _ => Err(crate::types::type_mismatch(
            span,
            ValueType::Integer,
            ValueType::BigInt,
        )),
    }
}
