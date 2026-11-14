//! Exact base-2 through base-36 conversion for bounded signed integers.
//! ASCII digits only: 0-9, a-z (case-insensitive on input); canonical lowercase
//! output. This module never routes integer digits through floating-point values.
use super::*;

fn valid_radix(radix: i64, span: Span) -> Result<u64, Diagnostic> {
    if !(2..=36).contains(&radix) {
        return Err(Diagnostic::new(
            DiagnosticKind::NumericDomain,
            span,
            "bigint radix must be an integer from 2 through 36",
        )
        .with_help("use radix 2 for binary, 8 for octal, 10 for decimal or 16 for hexadecimal"));
    }
    Ok(radix as u64)
}

// Carry and remainder both stay below BASE, so multiplying a decimal limb
// by the largest usable radix power takes at most a u64 intermediate.
fn radix_chunk(base: u64) -> (usize, u64) {
    let mut width = 1;
    let mut power = base;
    while power <= BASE / base {
        power *= base;
        width += 1;
    }
    (width, power)
}

fn append_text_byte(
    output: &mut Vec<u8>,
    byte: u8,
    budget: &mut Budget,
    span: Span,
) -> Result<(), Diagnostic> {
    if output.len() >= crate::value::MAX_TEXT_BYTES {
        return Err(crate::limits::text_construction(
            span,
            "radix text result exceeds the per-value size limit",
        ));
    }
    budget.charge(1, span)?;
    budget.charge_text_allocation(1, span)?;
    if output.len() == output.capacity() {
        let capacity = output
            .capacity()
            .saturating_mul(2)
            .clamp(1, crate::value::MAX_TEXT_BYTES);
        budget.charge(output.len() as u64, span)?;
        output
            .try_reserve_exact(capacity - output.len())
            .map_err(|_| {
                crate::limits::text_construction(span, "unable to reserve radix text output")
            })?;
    }
    output.push(byte);
    Ok(())
}

impl BigInt {
    /// Exact signed conversion from a positional ASCII numeral.
    /// The raw input digit count, including leading zeroes, must obey the
    /// configured decimal-digit cap; computed magnitude is rechecked too.
    pub(crate) fn from_radix(
        text: &str,
        radix: i64,
        budget: &mut Budget,
        text_span: Span,
        radix_span: Span,
    ) -> Result<Self, Diagnostic> {
        let base = valid_radix(radix, radix_span)?;
        let (negative, numeral) = match text.as_bytes().first() {
            Some(b'-') => (true, &text[1..]),
            Some(b'+') => (false, &text[1..]),
            _ => (false, text),
        };
        budget.check_bigint_digits(numeral.len(), text_span)?;
        if numeral.is_empty() {
            return Err(Diagnostic::new(
                DiagnosticKind::InvalidBigInt,
                text_span,
                "radix bigint requires at least one ASCII digit",
            ));
        }

        let (width, power) = radix_chunk(base);
        let first_width = (numeral.len() - 1) % width + 1;
        let (first, rest) = numeral.as_bytes().split_at(first_width);
        let mut limbs = Vec::new();
        for chunk in std::iter::once(first).chain(rest.chunks_exact(width)) {
            budget.charge((limbs.len() + chunk.len()) as u64, text_span)?;
            let mut carry = 0;
            for &byte in chunk {
                let digit = match byte {
                    b'0'..=b'9' => u64::from(byte - b'0'),
                    b'a'..=b'z' => u64::from(byte - b'a') + 10,
                    b'A'..=b'Z' => u64::from(byte - b'A') + 10,
                    _ => base,
                };
                if digit >= base {
                    return Err(Diagnostic::new(
                        DiagnosticKind::InvalidBigInt,
                        text_span,
                        format!("invalid digit for base-{base} bigint"),
                    )
                    .with_help(
                        "use only ASCII digits 0-9 and letters a-z within the selected radix",
                    ));
                }
                carry = carry * base + digit;
            }
            let multiplier = if chunk.len() == width {
                power
            } else {
                base.pow(chunk.len() as u32)
            };
            for limb in &mut limbs {
                let value = u64::from(*limb) * multiplier + carry;
                *limb = (value % BASE) as u32;
                carry = value / BASE;
            }
            if carry != 0 {
                // Account for one newly materialized magnitude limb before
                // any fallible capacity reservation, regardless of Vec growth.
                budget.charge_bigint_allocation(std::mem::size_of::<u32>(), text_span)?;
                limbs
                    .try_reserve_exact(1)
                    .map_err(|_| allocation(text_span))?;
                limbs.push(carry as u32);
            }
            budget.check_bigint_digits(digits(&limbs), text_span)?;
        }
        finish(limbs, negative, budget, text_span)
    }

    /// Format the exact magnitude by repeated division with a radix power.
    /// Each limb pass produces a group of radix digits in reverse order.
    pub(crate) fn to_radix(
        &self,
        radix: i64,
        budget: &mut Budget,
        value_span: Span,
        radix_span: Span,
    ) -> Result<Arc<str>, Diagnostic> {
        let base = valid_radix(radix, radix_span)?;
        let (width, power) = radix_chunk(base);
        budget.check_bigint_digits(self.decimal_digits(), value_span)?;
        budget.charge(1, value_span)?;
        let mut scratch = buffer(self.limbs.len(), budget, value_span)?;
        scratch.extend_from_slice(&self.limbs);

        const ALPHABET: &[u8; 36] = b"0123456789abcdefghijklmnopqrstuvwxyz";
        let mut reversed = Vec::new();
        if scratch.is_empty() {
            append_text_byte(&mut reversed, b'0', budget, value_span)?;
        }
        while !scratch.is_empty() {
            budget.charge(scratch.len() as u64 + 1, value_span)?;
            let mut remainder = 0_u64;
            for limb in scratch.iter_mut().rev() {
                // remainder < power <= BASE, so current is below BASE squared.
                let current = remainder * BASE + u64::from(*limb);
                *limb = (current / power) as u32;
                remainder = current % power;
            }
            normalize(&mut scratch);
            for _ in 0..width {
                let digit = ALPHABET[(remainder % base) as usize];
                append_text_byte(&mut reversed, digit, budget, value_span)?;
                remainder /= base;
                if scratch.is_empty() && remainder == 0 {
                    break;
                }
            }
        }
        if self.negative {
            append_text_byte(&mut reversed, b'-', budget, value_span)?;
        }
        reversed.reverse();
        let text = String::from_utf8(reversed).expect("ASCII radix alphabet");
        Ok(Arc::<str>::from(text))
    }
}
