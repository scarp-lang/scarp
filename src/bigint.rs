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

mod advanced;
pub(crate) mod bitwise;
mod modular;
mod number_theory;
mod radix;
mod sequences;
mod utilities;

const BASE: u64 = 1_000_000_000;
/// Opt-in implementation ceiling; hosts may select a smaller per-value limit.
/// Keep smaller address-space targets (including wasm32) at ten million.
#[cfg(target_pointer_width = "64")]
pub const MAX_BIGINT_DIGITS: usize = 1_000_000_000;
#[cfg(not(target_pointer_width = "64"))]
pub const MAX_BIGINT_DIGITS: usize = 10_000_000;
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
        // Batch both dense and sparse decimal limbs into bounded chunks.
        // Do not allocate an intermediate decimal string or make one writer
        // call per limb. Every non-leading limb has exactly nine ASCII digits.
        const CHUNK_BYTES: usize = 9 * 1024;
        let mut chunk = [b'0'; CHUNK_BYTES];
        let mut used = 0;
        for &limb in limbs {
            let slot = &mut chunk[used..used + 9];
            if limb == 0 {
                slot.fill(b'0');
            } else {
                let mut value = limb;
                for digit in slot.iter_mut().rev() {
                    *digit = b'0' + (value % 10) as u8;
                    value /= 10;
                }
            }
            used += 9;
            if used == CHUNK_BYTES {
                f.write_str(std::str::from_utf8(&chunk).map_err(|_| fmt::Error)?)?;
                used = 0;
            }
        }
        if used != 0 {
            f.write_str(std::str::from_utf8(&chunk[..used]).map_err(|_| fmt::Error)?)?;
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
    if !left.is_zero() && !right.is_zero() {
        // The exact result has either this many digits or one more.
        budget.check_bigint_digits(left.decimal_digits() + right.decimal_digits() - 1, span)?;
    }
    let result = multiply_limbs(&left.limbs, &right.limbs, budget, span)?;
    finish(result, left.negative != right.negative, budget, span)
}

// Balanced large products use Karatsuba (three half-size products instead of
// four). Small or very unbalanced products retain the schoolbook kernel.
const KARATSUBA_THRESHOLD: usize = 64;

fn multiply_limbs(
    left: &[u32],
    right: &[u32],
    budget: &mut Budget,
    span: Span,
) -> Result<Vec<u32>, Diagnostic> {
    let (n, m) = (left.len(), right.len());
    let capacity = n.checked_add(m).ok_or_else(|| allocation(span))?;
    if n == 0 || m == 0 {
        budget.charge(capacity as u64 + 1, span)?;
        return Ok(Vec::new());
    }

    if n.max(m) >= KARATSUBA_THRESHOLD {
        // Charge before scanning, even when no zero limbs are found. Factoring
        // whole base-10^9 limbs makes decimal powers and shifted values cheap.
        budget.charge(capacity as u64, span)?;
        let lz = left.iter().take_while(|&&x| x == 0).count();
        let rz = right.iter().take_while(|&&x| x == 0).count();
        if lz + rz != 0 {
            let mut core = multiply_limbs(&left[lz..], &right[rz..], budget, span)?;
            if core.is_empty() {
                return Ok(core);
            }
            let shift = lz + rz;
            let length = shift
                .checked_add(core.len())
                .ok_or_else(|| allocation(span))?;
            budget.charge(length as u64, span)?;
            let mut result = buffer(length, budget, span)?;
            result.resize(shift, 0);
            result.append(&mut core);
            return Ok(result);
        }
    }

    if n.min(m) >= KARATSUBA_THRESHOLD && n.max(m) <= n.min(m) * 2 {
        // Prepay linear splitting, sums, differences, and recombination.
        // Each recursive kernel additionally prepays its own work and buffers.
        budget.charge((capacity as u64).saturating_mul(16), span)?;
        let split = n.max(m) / 2;
        let (a0, a1) = left.split_at(split);
        let (b0, b1) = right.split_at(split);
        let a0 = trimmed(a0);
        let b0 = trimmed(b0);
        let z0 = multiply_limbs(a0, b0, budget, span)?;
        let z2 = multiply_limbs(a1, b1, budget, span)?;
        let a_sum = sum_limbs(a0, a1, budget, span)?;
        let b_sum = sum_limbs(b0, b1, budget, span)?;
        let mut middle = multiply_limbs(&a_sum, &b_sum, budget, span)?;
        // (a0+a1)(b0+b1) - a0*b0 - a1*b1 is nonnegative.
        subtract_magnitude(&mut middle, &z0);
        subtract_magnitude(&mut middle, &z2);
        let mut result = buffer(capacity, budget, span)?;
        result.resize(capacity, 0);
        add_shifted(&mut result, &z0, 0);
        add_shifted(&mut result, &middle, split);
        add_shifted(&mut result, &z2, split * 2);
        normalize(&mut result);
        return Ok(result);
    }

    budget.charge(
        (n as u64)
            .saturating_mul(m as u64)
            .saturating_add(capacity as u64)
            .saturating_add(1),
        span,
    )?;
    let mut result = buffer(capacity, budget, span)?;
    result.resize(capacity, 0);
    for (i, &a) in left.iter().enumerate() {
        let mut carry = 0_u64;
        for (j, &b) in right.iter().enumerate() {
            // (BASE-1)^2 + 2*(BASE-1) == BASE^2-1 fits u64.
            let product = u64::from(a) * u64::from(b) + u64::from(result[i + j]) + carry;
            result[i + j] = (product % BASE) as u32;
            carry = product / BASE;
        }
        result[i + m] = carry as u32;
    }
    normalize(&mut result);
    Ok(result)
}

fn trimmed(limbs: &[u32]) -> &[u32] {
    let length = limbs.iter().rposition(|&x| x != 0).map_or(0, |i| i + 1);
    &limbs[..length]
}

fn sum_limbs(
    left: &[u32],
    right: &[u32],
    budget: &mut Budget,
    span: Span,
) -> Result<Vec<u32>, Diagnostic> {
    let length = left.len().max(right.len());
    let mut result = buffer(
        length.checked_add(1).ok_or_else(|| allocation(span))?,
        budget,
        span,
    )?;
    let mut carry = 0_u64;
    for i in 0..length {
        let value =
            u64::from(*left.get(i).unwrap_or(&0)) + u64::from(*right.get(i).unwrap_or(&0)) + carry;
        result.push((value % BASE) as u32);
        carry = value / BASE;
    }
    if carry != 0 {
        result.push(carry as u32);
    }
    Ok(result)
}

fn add_shifted(result: &mut [u32], value: &[u32], shift: usize) {
    let mut carry = 0_u64;
    for (i, &limb) in value.iter().enumerate() {
        let slot = &mut result[shift + i];
        let sum = u64::from(*slot) + u64::from(limb) + carry;
        *slot = (sum % BASE) as u32;
        carry = sum / BASE;
    }
    let mut i = shift + value.len();
    while carry != 0 {
        let sum = u64::from(result[i]) + carry;
        result[i] = (sum % BASE) as u32;
        carry = sum / BASE;
        i += 1;
    }
}

enum DivisionOutputs {
    Quotient,
    Remainder,
    Both,
}

pub(crate) fn divmod(
    left: &BigInt,
    right: &BigInt,
    budget: &mut Budget,
    span: Span,
) -> Result<(BigInt, BigInt), Diagnostic> {
    budget.check_bigint_digits(left.decimal_digits().max(right.decimal_digits()), span)?;
    let (quotient, remainder) = divide(left, right, DivisionOutputs::Both, budget, span)?;
    Ok((
        quotient.expect("requested quotient"),
        remainder.expect("requested remainder"),
    ))
}

fn quotient(
    left: &BigInt,
    right: &BigInt,
    budget: &mut Budget,
    span: Span,
) -> Result<BigInt, Diagnostic> {
    divide(left, right, DivisionOutputs::Quotient, budget, span)
        .map(|(quotient, _)| quotient.expect("requested quotient"))
}

fn remainder(
    left: &BigInt,
    right: &BigInt,
    budget: &mut Budget,
    span: Span,
) -> Result<BigInt, Diagnostic> {
    divide(left, right, DivisionOutputs::Remainder, budget, span)
        .map(|(_, remainder)| remainder.expect("requested remainder"))
}

/// Long division finds each base-10^9 quotient digit by binary search.
/// Every mode pays the same work and materializes only its requested results.
fn divide(
    left: &BigInt,
    right: &BigInt,
    outputs: DivisionOutputs,
    budget: &mut Budget,
    span: Span,
) -> Result<(Option<BigInt>, Option<BigInt>), Diagnostic> {
    let keep_quotient = !matches!(outputs, DivisionOutputs::Remainder);
    let keep_remainder = !matches!(outputs, DivisionOutputs::Quotient);
    if right.is_zero() {
        return Err(
            Diagnostic::new(DiagnosticKind::DivisionByZero, span, "division by zero")
                .with_help("the bigint divisor must be nonzero"),
        );
    }
    let (n, m) = (left.limbs.len(), right.limbs.len());
    budget.charge(n.max(m).max(1) as u64, span)?;
    if magnitude_cmp(&left.limbs, &right.limbs) == Ordering::Less {
        let quotient = keep_quotient
            .then(|| finish(Vec::new(), false, budget, span))
            .transpose()?;
        return Ok((quotient, keep_remainder.then(|| left.clone())));
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
        let quotient = quotient
            .map(|limbs| finish(limbs, left.negative != right.negative, budget, span))
            .transpose()?;
        let remainder = keep_remainder
            .then(|| {
                let mut limbs = buffer(usize::from(remainder != 0), budget, span)?;
                if remainder != 0 {
                    limbs.push(remainder as u32);
                }
                finish(limbs, left.negative, budget, span)
            })
            .transpose()?;
        return Ok((quotient, remainder));
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
    let quotient = quotient
        .map(|limbs| finish(limbs, left.negative != right.negative, budget, span))
        .transpose()?;
    let remainder = keep_remainder
        .then(|| finish(remainder, left.negative, budget, span))
        .transpose()?;
    Ok((quotient, remainder))
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
        Divide => quotient(left, right, budget, span).map(Value::BigInt),
        Remainder => remainder(left, right, budget, span).map(Value::BigInt),
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
