use super::{
    check_output_sizes, checkpoint, division::QuotientPlan, io_error, open_operands, parse_limb,
    publish_decimal, require, reserve_buffer, DecimalInput, FileError, FileLimits, FilePhase,
    FileProgress, FileResource, FileUsage, OutputStage, BASE, LIMB_BYTES, LIMB_DIGITS,
};
use std::{fmt, ops::ControlFlow, path::Path, time::Instant};

const IO_BLOCK_BYTES: usize = 64 * 1024;
const STEPS_PER_CHECKPOINT: u64 = 1_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WideDivisionLimits {
    pub file: FileLimits,
    /// Upper bound on arithmetic limb visits, including preparation.
    pub max_limb_steps: u64,
}

impl Default for WideDivisionLimits {
    fn default() -> Self {
        Self {
            file: FileLimits::default(),
            max_limb_steps: 100_000_000,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct WideDivisionUsage {
    pub quotient: FileUsage,
    pub remainder: WideRemainder,
    /// The conservative bound checked before staging the quotient.
    pub limb_steps: u64,
}

/// Exact signed remainder. Decimal formatting streams the retained limbs.
#[derive(Debug, PartialEq, Eq)]
pub struct WideRemainder {
    negative: bool,
    limbs: Vec<u32>,
}

impl fmt::Display for WideRemainder {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Some((first, rest)) = self.limbs.split_first() else {
            return output.write_str("0");
        };
        if self.negative {
            output.write_str("-")?;
        }
        write!(output, "{first}")?;
        for limb in rest {
            write!(output, "{limb:09}")?;
        }
        Ok(())
    }
}

/// Divide by a nonzero signed divisor whose limbs fit the configured workspace.
///
/// The dividend and quotient are streamed. The quotient truncates toward zero
/// and the nonzero remainder has the dividend's sign. Input stability and
/// publication requirements match [`super::calculate`].
pub fn divide_wide(
    dividend: impl AsRef<Path>,
    divisor: impl AsRef<Path>,
    output: impl AsRef<Path>,
    limits: WideDivisionLimits,
) -> Result<WideDivisionUsage, FileError> {
    divide_wide_with_progress(dividend, divisor, output, limits, |_| {
        ControlFlow::Continue(())
    })
}

/// As [`divide_wide`], with cooperative cancellation during input scanning,
/// prefix loading, quotient streaming, and before publication.
pub fn divide_wide_with_progress(
    dividend: impl AsRef<Path>,
    divisor: impl AsRef<Path>,
    output: impl AsRef<Path>,
    limits: WideDivisionLimits,
    mut progress: impl FnMut(FileProgress) -> ControlFlow<()>,
) -> Result<WideDivisionUsage, FileError> {
    let start = Instant::now();
    let output = output.as_ref();
    let (mut dividend, mut divisor, input_bytes) =
        open_operands(dividend.as_ref(), divisor.as_ref(), output, limits.file)?;
    let budget = limits.file.max_buffer_bytes;
    require(FileResource::BufferBytes, LIMB_DIGITS as u64, budget as u64)?;
    let useful = dividend
        .len
        .max(divisor.len)
        .div_ceil(LIMB_DIGITS as u64)
        .max(1);
    let io_groups = (budget.min(IO_BLOCK_BYTES) / LIMB_DIGITS) as u64;
    let scan_len = useful.min(io_groups) as usize * LIMB_DIGITS;
    let mut buffer = reserve_buffer(scan_len)?;
    dividend.scan(&mut buffer, 0, input_bytes, &mut progress)?;
    divisor.scan(&mut buffer, dividend.len, input_bytes, &mut progress)?;
    if divisor.digits == 0 {
        return Err(FileError::DivisionByZero);
    }

    let limbs = divisor.digits.div_ceil(LIMB_DIGITS as u64);
    let workspace = (limbs * 2 + 1)
        .checked_mul(LIMB_BYTES as u64)
        .ok_or(FileError::SizeOverflow)?;
    require(
        FileResource::BufferBytes,
        workspace + LIMB_DIGITS as u64,
        budget as u64,
    )?;
    let preparation = limbs * 9 + 4;
    check_work(preparation, limits.max_limb_steps)?;
    let workspace = workspace as usize;
    let arithmetic_buffer_len = scan_len.min((budget - workspace) / LIMB_DIGITS * LIMB_DIGITS);
    if arithmetic_buffer_len != scan_len {
        drop(buffer);
        buffer = reserve_buffer(arithmetic_buffer_len)?;
    }
    let buffer_bytes = scan_len.max(workspace + buffer.len());
    let mut divisor_limbs = limb_buffer(limbs as usize)?;
    let mut window = limb_buffer(limbs as usize + 1)?;

    let prefix = dividend.digits.min(divisor.digits);
    let load_total = divisor.digits + prefix;
    checkpoint(&mut progress, FilePhase::CompareMagnitudes, 0, load_total)?;
    let divisor_digits = divisor.digits;
    read_limbs(
        &mut divisor,
        divisor_digits,
        &mut divisor_limbs,
        &mut buffer,
        (0, load_total),
        &mut progress,
    )?;
    read_limbs(
        &mut dividend,
        prefix,
        &mut window[1..],
        &mut buffer,
        (divisor_digits, load_total),
        &mut progress,
    )?;
    // The prefix has at most the divisor's width. When it reaches the
    // divisor, retain one fewer decimal digit as the initial remainder.
    let mut offset = prefix;
    if window[1..] >= divisor_limbs[..] {
        divide_small(&mut window[1..], 10);
        offset -= 1;
    }

    let plan = QuotientPlan::new(&dividend, &divisor, offset);
    let output_bytes = plan.output_bytes()?;
    let peak_temp_bytes = check_output_sizes(output_bytes, 0, limits.file)?;
    let per_group = limbs * 3 + 4;
    let limb_steps = plan
        .groups()
        .checked_mul(per_group)
        .and_then(|work| work.checked_add(preparation))
        .ok_or(FileError::SizeOverflow)?;
    check_work(limb_steps, limits.max_limb_steps)?;
    let checkpoint_groups = (STEPS_PER_CHECKPOINT / per_group).max(1) as usize;
    let chunk_len = buffer.len().min(checkpoint_groups * LIMB_DIGITS);
    let mut kernel = DivisionKernel::new(divisor_limbs, window);
    let input_time = start.elapsed();

    let arithmetic_start = Instant::now();
    let stage =
        OutputStage::create(output, "bigint").map_err(io_error("create staging directory"))?;
    let mut staged = stage
        .open_output()
        .map_err(io_error("create staged decimal output"))?;
    plan.write(
        &mut dividend,
        &mut buffer[..chunk_len],
        &mut staged,
        &mut progress,
        |group, radix| kernel.divide_group(group, radix),
    )?;
    let remainder = kernel.remainder(dividend.negative);
    let arithmetic_time = arithmetic_start.elapsed();

    let output_start = Instant::now();
    publish_decimal(&stage, staged, (&dividend, &divisor), &mut progress)?;
    Ok(WideDivisionUsage {
        quotient: FileUsage {
            input_bytes,
            result_digits: plan.result_digits(),
            output_bytes,
            scratch_bytes: 0,
            peak_temp_bytes,
            buffer_bytes,
            input_time,
            arithmetic_time,
            output_time: output_start.elapsed(),
        },
        remainder,
        limb_steps,
    })
}

fn check_work(required: u64, limit: u64) -> Result<(), FileError> {
    if required > limit {
        return Err(FileError::LimbWorkLimit { required, limit });
    }
    Ok(())
}

fn limb_buffer(count: usize) -> Result<Vec<u32>, FileError> {
    let mut buffer = Vec::new();
    buffer
        .try_reserve_exact(count)
        .map_err(|_| FileError::Allocation)?;
    buffer.resize(count, 0);
    Ok(buffer)
}

fn read_limbs(
    input: &mut DecimalInput,
    digits: u64,
    out: &mut [u32],
    buffer: &mut [u8],
    (already_read, total): (u64, u64),
    progress: &mut impl FnMut(FileProgress) -> ControlFlow<()>,
) -> Result<(), FileError> {
    if digits == 0 {
        return Ok(());
    }
    let out_len = out.len();
    let count = digits.div_ceil(LIMB_DIGITS as u64) as usize;
    let out = &mut out[out_len - count..];
    let head = ((digits - 1) % LIMB_DIGITS as u64 + 1) as usize;
    input.read_magnitude(0, &mut buffer[..head])?;
    if buffer[0] == b'0' {
        return Err(input.changed());
    }
    out[0] = parse_limb(&buffer[..head]);
    let mut position = head as u64;
    checkpoint(
        progress,
        FilePhase::CompareMagnitudes,
        already_read + position,
        total,
    )?;
    for block in out[1..].chunks_mut(buffer.len() / LIMB_DIGITS) {
        let bytes = &mut buffer[..block.len() * LIMB_DIGITS];
        input.read_magnitude(position, bytes)?;
        for (limb, part) in block.iter_mut().zip(bytes.chunks_exact(LIMB_DIGITS)) {
            *limb = parse_limb(part);
        }
        position += bytes.len() as u64;
        checkpoint(
            progress,
            FilePhase::CompareMagnitudes,
            already_read + position,
            total,
        )?;
    }
    Ok(())
}

// Limbs are big endian here, with one extra high limb in the rolling window.
struct DivisionKernel {
    divisor: Vec<u32>,
    window: Vec<u32>,
    factor: u64,
}

impl DivisionKernel {
    fn new(mut divisor: Vec<u32>, mut window: Vec<u32>) -> Self {
        // Normalize the divisor's leading limb to at least BASE / 2.
        let factor = BASE / (u64::from(divisor[0]) + 1);
        let divisor_carry = multiply_small(&mut divisor, factor, 0);
        let remainder_carry = multiply_small(&mut window[1..], factor, 0);
        debug_assert_eq!((divisor_carry, remainder_carry), (0, 0));
        Self {
            divisor,
            window,
            factor,
        }
    }

    fn divide_group(&mut self, group: u32, radix: u64) -> u32 {
        let carry = u64::from(group) * self.factor;
        self.window[0] = multiply_small(&mut self.window[1..], radix, carry);
        let mut quotient = estimate(&self.window, &self.divisor);
        if quotient == 0 {
            return 0;
        }

        // After the two-limb estimate, the trial digit is exact or one high.
        if subtract_trial(&mut self.window, &self.divisor, quotient) {
            quotient -= 1;
            add_back(&mut self.window[1..], &self.divisor);
        }
        self.window[0] = 0;
        quotient as u32
    }

    fn remainder(mut self, negative: bool) -> WideRemainder {
        let fraction = divide_small(&mut self.window[1..], self.factor);
        debug_assert_eq!(fraction, 0);
        let first = self
            .window
            .iter()
            .position(|limb| *limb != 0)
            .unwrap_or(self.window.len());
        self.window.drain(..first);
        WideRemainder {
            negative: negative && !self.window.is_empty(),
            limbs: self.window,
        }
    }
}

fn multiply_small(limbs: &mut [u32], factor: u64, mut carry: u64) -> u32 {
    for limb in limbs.iter_mut().rev() {
        let value = u64::from(*limb) * factor + carry;
        *limb = (value % BASE) as u32;
        carry = value / BASE;
    }
    carry as u32
}

fn divide_small(limbs: &mut [u32], divisor: u64) -> u64 {
    let mut remainder = 0;
    for limb in limbs {
        let value = remainder * BASE + u64::from(*limb);
        *limb = (value / divisor) as u32;
        remainder = value % divisor;
    }
    remainder
}

fn estimate(window: &[u32], divisor: &[u32]) -> u64 {
    let high = u64::from(divisor[0]);
    let top = u64::from(window[0]) * BASE + u64::from(window[1]);
    let mut quotient = (top / high).min(BASE - 1);
    if let Some(&second) = divisor.get(1) {
        let mut excess = top - quotient * high;
        while excess < BASE && quotient * u64::from(second) > excess * BASE + u64::from(window[2]) {
            quotient -= 1;
            excess += high;
        }
    }
    quotient
}

fn subtract_trial(window: &mut [u32], divisor: &[u32], quotient: u64) -> bool {
    let (high, low) = window.split_first_mut().expect("one extra limb");
    let mut carry = 0;
    let mut borrow = 0;
    for (numerator, &limb) in low.iter_mut().zip(divisor).rev() {
        let product = quotient * u64::from(limb) + carry;
        carry = product / BASE;
        let difference = u64::from(*numerator) + BASE - (product % BASE + borrow);
        borrow = u64::from(difference < BASE);
        *numerator = (difference - BASE * (1 - borrow)) as u32;
    }
    u64::from(*high) < carry + borrow
}

fn add_back(window: &mut [u32], divisor: &[u32]) {
    let mut carry = 0;
    for (numerator, &limb) in window.iter_mut().zip(divisor).rev() {
        let sum = u64::from(*numerator) + u64::from(limb) + carry;
        carry = u64::from(sum >= BASE);
        *numerator = (sum - carry * BASE) as u32;
    }
    debug_assert_eq!(carry, 1);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quotient_estimate_can_need_two_decrements() {
        let mut window = [499_999_986, 249_999_969, 500_000_029];
        let divisor = [500_000_000, 999_999_999];
        let quotient = estimate(&window, &divisor);
        assert_eq!(quotient, 999_999_970);
        assert!(!subtract_trial(&mut window, &divisor, quotient));
        assert_eq!(window[1..], [250_000_000, 499_999_999]);
    }

    #[test]
    fn lower_divisor_limb_can_force_add_back() {
        let mut kernel = DivisionKernel::new(vec![500_000_000, 0, 1], vec![0, 0, 500_000_000, 0]);
        assert_eq!(estimate(&[0, 500_000_000, 0, 0], &kernel.divisor), 1);
        assert_eq!(kernel.divide_group(0, BASE), 0);
        assert_eq!(
            kernel.remainder(false).to_string(),
            "500000000000000000000000000"
        );
    }
}
