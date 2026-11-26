use super::{
    check_output_sizes, checkpoint, io_error, open_operands, parse_limb, publish_decimal, require,
    reserve_buffer, write_limb, DecimalInput, FileError, FileLimits, FilePhase, FileProgress,
    FileResource, FileUsage, OutputStage, BASE, LIMB_DIGITS, MAX_BLOCK_BUFFER_BYTES,
};
use std::{io::Write, ops::ControlFlow, path::Path, time::Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DivisionUsage {
    pub quotient: FileUsage,
    /// Signed remainder, with the dividend's sign when nonzero.
    pub remainder: i32,
}

/// Divide by a nonzero signed decimal integer of at most nine significant digits.
///
/// Publish the quotient, truncated toward zero, to a new output path and return
/// the signed remainder. The staged quotient is the only temporary file.
/// Input stability and filesystem requirements match [`super::calculate`].
pub fn divide(
    dividend: impl AsRef<Path>,
    divisor: impl AsRef<Path>,
    output: impl AsRef<Path>,
    limits: FileLimits,
) -> Result<DivisionUsage, FileError> {
    divide_with_progress(dividend, divisor, output, limits, |_| {
        ControlFlow::Continue(())
    })
}

/// As [`divide`], with cooperative cancellation during input scanning and
/// quotient output and before publication. Arithmetic and decimal writes share
/// the [`FilePhase::WriteDecimal`] phase and use output bytes as the progress unit.
pub fn divide_with_progress(
    dividend: impl AsRef<Path>,
    divisor: impl AsRef<Path>,
    output: impl AsRef<Path>,
    limits: FileLimits,
    mut progress: impl FnMut(FileProgress) -> ControlFlow<()>,
) -> Result<DivisionUsage, FileError> {
    let start = Instant::now();
    let output = output.as_ref();
    let (mut dividend, mut divisor_file, input_bytes) =
        open_operands(dividend.as_ref(), divisor.as_ref(), output, limits)?;
    let max_buffer_bytes = limits.max_buffer_bytes.min(MAX_BLOCK_BUFFER_BYTES);
    require(
        FileResource::BufferBytes,
        LIMB_DIGITS as u64,
        max_buffer_bytes as u64,
    )?;
    let useful = dividend
        .len
        .max(divisor_file.len)
        .div_ceil(LIMB_DIGITS as u64)
        .max(1);
    let blocks = useful.min((max_buffer_bytes / LIMB_DIGITS) as u64) as usize;
    let mut buffer = reserve_buffer(blocks * LIMB_DIGITS)?;
    dividend.scan(&mut buffer, 0, input_bytes, &mut progress)?;
    divisor_file.scan(&mut buffer, dividend.len, input_bytes, &mut progress)?;
    if divisor_file.digits == 0 {
        return Err(FileError::DivisionByZero);
    }
    if divisor_file.digits > LIMB_DIGITS as u64 {
        return Err(FileError::DivisorTooLarge);
    }
    let divisor_bytes = &mut buffer[..divisor_file.digits as usize];
    divisor_file.read_magnitude(0, divisor_bytes)?;
    let divisor = u64::from(parse_limb(divisor_bytes));
    if divisor_bytes[0] == b'0' {
        return Err(divisor_file.changed());
    }

    let plan = QuotientPlan::new(&mut dividend, &divisor_file, divisor, &mut buffer)?;
    let result_digits = plan.digits.max(1);
    let output_bytes = result_digits
        .checked_add(u64::from(plan.negative))
        .ok_or(FileError::SizeOverflow)?;
    let peak_temp_bytes = check_output_sizes(output_bytes, 0, limits)?;
    let input_time = start.elapsed();

    let arithmetic_start = Instant::now();
    let stage =
        OutputStage::create(output, "bigint").map_err(io_error("create staging directory"))?;
    let mut staged = stage
        .open_output()
        .map_err(io_error("create staged decimal output"))?;
    let remainder = plan.write(
        &mut dividend,
        divisor,
        &mut buffer,
        &mut staged,
        &mut progress,
    )?;
    let arithmetic_time = arithmetic_start.elapsed();

    let output_start = Instant::now();
    publish_decimal(&stage, staged, (&dividend, &divisor_file), &mut progress)?;
    Ok(DivisionUsage {
        quotient: FileUsage {
            input_bytes,
            result_digits,
            output_bytes,
            scratch_bytes: 0,
            peak_temp_bytes,
            buffer_bytes: buffer.len(),
            input_time,
            arithmetic_time,
            output_time: output_start.elapsed(),
        },
        remainder: if dividend.negative {
            -remainder
        } else {
            remainder
        },
    })
}

struct QuotientPlan {
    offset: u64,
    digits: u64,
    negative: bool,
    remainder: u64,
}

impl QuotientPlan {
    fn new(
        dividend: &mut DecimalInput,
        divisor_file: &DecimalInput,
        divisor: u64,
        buffer: &mut [u8],
    ) -> Result<Self, FileError> {
        let prefix = &mut buffer[..dividend.digits.min(divisor_file.digits) as usize];
        dividend.read_magnitude(0, prefix)?;
        if prefix.first() == Some(&b'0') {
            return Err(dividend.changed());
        }
        let first = u64::from(parse_limb(prefix));
        // The first quotient digit appears when the dividend prefix reaches
        // the divisor. The digits before it form the initial remainder.
        let skipped = prefix.len() - usize::from(first >= divisor);
        let offset = skipped as u64;
        let digits = dividend.digits - offset;
        Ok(Self {
            offset,
            digits,
            negative: digits != 0 && dividend.negative != divisor_file.negative,
            remainder: u64::from(parse_limb(&prefix[..skipped])),
        })
    }

    fn write(
        self,
        dividend: &mut DecimalInput,
        divisor: u64,
        buffer: &mut [u8],
        output: &mut impl Write,
        progress: &mut impl FnMut(FileProgress) -> ControlFlow<()>,
    ) -> Result<i32, FileError> {
        let sign = u64::from(self.negative);
        let total = self.digits.max(1) + sign;
        let context = io_error("staged decimal output");
        checkpoint(progress, FilePhase::WriteDecimal, 0, total)?;
        if self.digits == 0 {
            output.write_all(b"0").map_err(context)?;
            checkpoint(progress, FilePhase::WriteDecimal, 1, total)?;
            return Ok(self.remainder as i32);
        }
        if self.negative {
            output.write_all(b"-").map_err(context)?;
        }
        let mut remainder = self.remainder;
        let mut position = 0;
        while position < self.digits {
            let count = (self.digits - position).min(buffer.len() as u64) as usize;
            let block = &mut buffer[..count];
            dividend.read_magnitude(self.offset + position, block)?;
            for digits in block.chunks_mut(LIMB_DIGITS) {
                let radix = if digits.len() == LIMB_DIGITS {
                    BASE
                } else {
                    10_u64.pow(digits.len() as u32)
                };
                // The remainder is below the divisor. Both the divisor and
                // radix are at most BASE, so this product fits in u64.
                let current = remainder * radix + u64::from(parse_limb(digits));
                let quotient = (current / divisor) as u32;
                remainder = current % divisor;
                write_limb(quotient, digits);
            }
            if position == 0 && block[0] == b'0' {
                return Err(dividend.changed());
            }
            output.write_all(block).map_err(context)?;
            position += count as u64;
            checkpoint(progress, FilePhase::WriteDecimal, position + sign, total)?;
        }
        Ok(remainder as i32)
    }
}
