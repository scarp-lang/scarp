//! Fixed-buffer arithmetic over seekable signed decimal files.
//!
//! Each operand contains one integer in `[+-]?[0-9]+` form. Leading zeros
//! are accepted. The result is canonical decimal with no newline. Workspace
//! lives beside the output and is dropped on success, error or cancellation.

mod division;
mod input;
mod scratch;
mod wide_division;

pub use division::{divide, divide_with_progress, DivisionUsage};
pub use wide_division::{
    divide_wide, divide_wide_with_progress, WideDivisionLimits, WideDivisionUsage, WideRemainder,
};

use super::BASE;
use crate::output_stage::OutputStage;
use input::{parse_limb, DecimalInput};
use scratch::ScratchLayout;
use std::{
    cmp::Ordering,
    error, fmt,
    fs::{self, File},
    io::{self, Write},
    ops::ControlFlow,
    path::Path,
    time::{Duration, Instant},
};

const LIMB_DIGITS: usize = 9;
const LIMB_BYTES: usize = 4;
const CHECKSUM_BYTES: usize = 8;
const BUFFER_BYTES_PER_LIMB: usize = 2 * LIMB_DIGITS + LIMB_BYTES;
const MAX_BLOCK_BUFFER_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileOperation {
    Add,
    Subtract,
    /// Multiply when either magnitude fits in one base-10^9 limb, or is zero.
    Multiply,
}

/// Per-operation logical limits. Filesystem metadata, OS caching and allocator
/// overhead are outside these limits. Limits never reserve disk space in advance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileLimits {
    /// Combined byte lengths of both source files, including signs and zeros.
    pub max_input_bytes: u64,
    /// Final decimal byte length including its sign, if present.
    pub max_output_bytes: u64,
    /// Peak combined byte length of the limb scratch and staged decimal files.
    pub max_temp_bytes: u64,
    /// Total payload of reusable processing buffers. Each I/O block is at most
    /// 1 MiB. Wide division also accounts for its in-memory limb workspace.
    pub max_buffer_bytes: usize,
}

impl Default for FileLimits {
    fn default() -> Self {
        Self {
            max_input_bytes: 64 * 1024 * 1024,
            max_output_bytes: 32 * 1024 * 1024,
            max_temp_bytes: 64 * 1024 * 1024,
            max_buffer_bytes: 256 * 1024,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileResource {
    InputBytes,
    OutputBytes,
    TemporaryBytes,
    BufferBytes,
}

#[derive(Debug)]
#[non_exhaustive]
pub enum FileError {
    Io {
        context: &'static str,
        source: io::Error,
    },
    NotRegularFile {
        operand: &'static str,
    },
    InvalidDecimal {
        operand: &'static str,
        offset: u64,
    },
    InputChanged {
        operand: &'static str,
    },
    FactorTooLarge,
    DivisionByZero,
    DivisorTooLarge,
    LimbWorkLimit {
        required: u64,
        limit: u64,
    },
    Limit {
        resource: FileResource,
        required: u64,
        limit: u64,
    },
    SizeOverflow,
    Allocation,
    ScratchIntegrity,
    Cancelled,
}

impl fmt::Display for FileError {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { context, source } => write!(out, "{context} ({source})"),
            Self::NotRegularFile { operand } => write!(out, "{operand} must be a regular file"),
            Self::InvalidDecimal { operand, offset } => {
                write!(out, "invalid decimal in the {operand} at byte {offset}")
            }
            Self::InputChanged { operand } => write!(out, "{operand} changed during calculation"),
            Self::FactorTooLarge => {
                let maximum = BASE - 1;
                write!(
                    out,
                    "file multiplication requires one operand with magnitude at most {maximum}"
                )
            }
            Self::DivisionByZero => out.write_str("file division by zero"),
            Self::DivisorTooLarge => {
                let maximum = BASE - 1;
                write!(
                    out,
                    "file division requires a divisor with magnitude at most {maximum}"
                )
            }
            Self::LimbWorkLimit { required, limit } => {
                write!(
                    out,
                    "wide division needs a work budget of {required} limb steps (limit {limit})"
                )
            }
            Self::Limit {
                resource,
                required,
                limit,
            } => {
                let resource = match resource {
                    FileResource::InputBytes => "input",
                    FileResource::OutputBytes => "output",
                    FileResource::TemporaryBytes => "temporary storage",
                    FileResource::BufferBytes => "processing buffers",
                };
                write!(
                    out,
                    "{resource} requires at least {required} bytes (limit {limit})"
                )
            }
            Self::SizeOverflow => {
                out.write_str("file arithmetic size exceeds the supported offset range")
            }
            Self::Allocation => out.write_str("unable to reserve file arithmetic buffers"),
            Self::ScratchIntegrity => {
                out.write_str("file arithmetic scratch failed its integrity check")
            }
            Self::Cancelled => out.write_str("file arithmetic cancelled"),
        }
    }
}

impl error::Error for FileError {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilePhase {
    ReadInputs,
    CompareMagnitudes,
    Arithmetic,
    WriteDecimal,
    Publish,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileProgress {
    pub phase: FilePhase,
    /// Bytes, except for Arithmetic (limbs) and Publish (one commit point).
    pub completed: u64,
    pub total: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileUsage {
    pub input_bytes: u64,
    pub result_digits: u64,
    pub output_bytes: u64,
    pub scratch_bytes: u64,
    pub peak_temp_bytes: u64,
    pub buffer_bytes: usize,
    /// Opening, validating and comparing the operands.
    pub input_time: Duration,
    /// Computing result limbs and writing either scratch or a staged quotient.
    pub arithmetic_time: Duration,
    /// Formatting scratch-backed results, if any, then syncing and publishing.
    pub output_time: Duration,
}

/// Calculate a sum, difference or one-limb product with reusable buffers.
///
/// Use stable input files and a trusted output directory. The output filesystem
/// must support hard links. An existing destination is preserved, even if it
/// appears during calculation. The output filename becomes visible at commit.
pub fn calculate(
    operation: FileOperation,
    left: impl AsRef<Path>,
    right: impl AsRef<Path>,
    output: impl AsRef<Path>,
    limits: FileLimits,
) -> Result<FileUsage, FileError> {
    calculate_with_progress(operation, left, right, output, limits, |_| {
        ControlFlow::Continue(())
    })
}

/// As [`calculate`], with cooperative cancellation at block boundaries and
/// before publication. Return `ControlFlow::Break(())` to cancel. An individual
/// blocking filesystem call must return before cancellation can be observed.
pub fn calculate_with_progress(
    operation: FileOperation,
    left: impl AsRef<Path>,
    right: impl AsRef<Path>,
    output: impl AsRef<Path>,
    limits: FileLimits,
    mut progress: impl FnMut(FileProgress) -> ControlFlow<()>,
) -> Result<FileUsage, FileError> {
    let start = Instant::now();
    let output = output.as_ref();
    let (mut left, mut right, input_bytes) =
        open_operands(left.as_ref(), right.as_ref(), output, limits)?;
    let mut buffers = Buffers::new(limits.max_buffer_bytes, left.len.max(right.len))?;
    left.scan(&mut buffers.left, 0, input_bytes, &mut progress)?;
    right.scan(&mut buffers.right, left.len, input_bytes, &mut progress)?;

    let plan = match operation {
        FileOperation::Add | FileOperation::Subtract => ResultPlan::add_sub(
            operation,
            &mut left,
            &mut right,
            &mut buffers,
            &mut progress,
        )?,
        FileOperation::Multiply => ResultPlan::multiply(&mut left, &mut right, &mut buffers)?,
    };
    let negative = plan.negative;
    let scratch_bytes = plan.scratch.as_ref().map_or(0, |(layout, _)| layout.bytes);
    let min_output = plan
        .min_digits
        .checked_add(u64::from(negative))
        .ok_or(FileError::SizeOverflow)?;
    check_output_sizes(min_output, scratch_bytes, limits)?;
    let input_time = start.elapsed();

    let arithmetic_start = Instant::now();
    let stage =
        OutputStage::create(output, "bigint").map_err(io_error("create staging directory"))?;
    let file_result = if let Some((layout, kernel)) = plan.scratch {
        let (larger, smaller) = if plan.swap {
            (&mut right, &mut left)
        } else {
            (&mut left, &mut right)
        };
        let mut scratch = stage.open_scratch().map_err(io_error("create scratch"))?;
        let shape = compute(
            &layout,
            &mut scratch,
            (larger, smaller),
            kernel,
            &mut buffers,
            &mut progress,
        )?;
        Some((layout, shape, scratch))
    } else {
        None
    };
    let arithmetic_time = arithmetic_start.elapsed();

    let output_start = Instant::now();
    let result_digits = file_result
        .as_ref()
        .map_or(Ok(1), |(_, shape, _)| shape.digits())?;
    let output_bytes = result_digits
        .checked_add(u64::from(negative))
        .ok_or(FileError::SizeOverflow)?;
    let peak_temp_bytes = check_output_sizes(output_bytes, scratch_bytes, limits)?;
    let mut staged = stage
        .open_output()
        .map_err(io_error("create staged decimal output"))?;
    checkpoint(&mut progress, FilePhase::WriteDecimal, 0, output_bytes)?;
    if let Some((layout, shape, mut scratch)) = file_result {
        write_decimal(
            &layout,
            &mut scratch,
            &mut staged,
            (shape, negative),
            &mut buffers,
            output_bytes,
            &mut progress,
        )?;
    } else {
        staged
            .write_all(b"0")
            .map_err(io_error("staged decimal output"))?;
        checkpoint(&mut progress, FilePhase::WriteDecimal, 1, 1)?;
    }
    publish_decimal(&stage, staged, (&left, &right), &mut progress)?;
    Ok(FileUsage {
        input_bytes,
        result_digits,
        output_bytes,
        scratch_bytes,
        peak_temp_bytes,
        buffer_bytes: buffers.payload_bytes(),
        input_time,
        arithmetic_time,
        output_time: output_start.elapsed(),
    })
}

fn open_operands(
    left: &Path,
    right: &Path,
    output: &Path,
    limits: FileLimits,
) -> Result<(DecimalInput, DecimalInput, u64), FileError> {
    ensure_absent(output)?;
    let left = DecimalInput::open(left, "left operand")?;
    let right = DecimalInput::open(right, "right operand")?;
    let input_bytes = left
        .len
        .checked_add(right.len)
        .ok_or(FileError::SizeOverflow)?;
    require(
        FileResource::InputBytes,
        input_bytes,
        limits.max_input_bytes,
    )?;
    Ok((left, right, input_bytes))
}

fn publish_decimal(
    stage: &OutputStage,
    staged: File,
    operands: (&DecimalInput, &DecimalInput),
    progress: &mut impl FnMut(FileProgress) -> ControlFlow<()>,
) -> Result<(), FileError> {
    staged
        .sync_all()
        .map_err(io_error("sync staged decimal output"))?;
    drop(staged);
    checkpoint(progress, FilePhase::Publish, 0, 1)?;
    operands.0.check_unchanged()?;
    operands.1.check_unchanged()?;
    stage.publish().map_err(io_error("publish decimal output"))
}

#[derive(Clone, Copy)]
enum LimbOperation {
    Add,
    Subtract,
    Multiply(i64),
}

struct ResultPlan {
    scratch: Option<(ScratchLayout, LimbOperation)>,
    swap: bool,
    negative: bool,
    min_digits: u64,
}

impl ResultPlan {
    const ZERO: Self = Self {
        scratch: None,
        swap: false,
        negative: false,
        min_digits: 1,
    };

    fn add_sub(
        operation: FileOperation,
        left: &mut DecimalInput,
        right: &mut DecimalInput,
        buffers: &mut Buffers,
        progress: &mut impl FnMut(FileProgress) -> ControlFlow<()>,
    ) -> Result<Self, FileError> {
        let right_negative =
            right.digits != 0 && (right.negative ^ (operation == FileOperation::Subtract));
        let subtract = left.negative != right_negative;
        let order = if subtract {
            left.compare(right, &mut buffers.left, &mut buffers.right, progress)?
        } else {
            Ordering::Greater
        };
        let max_digits = left.digits.max(right.digits);
        if order == Ordering::Equal || max_digits == 0 {
            return Ok(Self::ZERO);
        }
        let swap = order == Ordering::Less;
        let both_nonzero = left.digits != 0 && right.digits != 0;
        let min_digits = if !subtract || !both_nonzero {
            max_digits
        } else if left.digits.abs_diff(right.digits) >= 2 {
            // The smaller magnitude is below 10^(max_digits-2), while the
            // larger is at least 10^(max_digits-1).
            max_digits - 1
        } else {
            1
        };
        let limb_count = max_digits.div_ceil(LIMB_DIGITS as u64);
        let carry_limb = u64::from(!subtract && both_nonzero);
        let layout = ScratchLayout::new(limb_count + carry_limb, buffers.chunk_limbs())?;
        let kernel = if subtract {
            LimbOperation::Subtract
        } else {
            LimbOperation::Add
        };
        Ok(Self {
            scratch: Some((layout, kernel)),
            swap,
            negative: if swap { right_negative } else { left.negative },
            min_digits,
        })
    }

    fn multiply(
        left: &mut DecimalInput,
        right: &mut DecimalInput,
        buffers: &mut Buffers,
    ) -> Result<Self, FileError> {
        if left.digits == 0 || right.digits == 0 {
            return Ok(Self::ZERO);
        }
        let swap = left.digits < right.digits;
        let (larger, smaller) = if swap { (right, left) } else { (left, right) };
        if smaller.digits > LIMB_DIGITS as u64 {
            return Err(FileError::FactorTooLarge);
        }
        let factor = smaller
            .read_low(0, &mut buffers.right)?
            .next()
            .map(parse_limb)
            .filter(|&limb| limb != 0)
            .ok_or_else(|| smaller.changed())?;
        // For positive operands with a and b digits, the product has at least
        // a+b-1 digits. Check the exact size again after computing the limbs.
        let min_digits = larger
            .digits
            .checked_add(smaller.digits - 1)
            .ok_or(FileError::SizeOverflow)?;
        let limb_count = larger.digits.div_ceil(LIMB_DIGITS as u64);
        let carry_limb = u64::from(factor > 1);
        let layout = ScratchLayout::new(limb_count + carry_limb, buffers.chunk_limbs())?;
        Ok(Self {
            scratch: Some((layout, LimbOperation::Multiply(i64::from(factor)))),
            swap,
            negative: larger.negative != smaller.negative,
            min_digits,
        })
    }
}

struct Buffers {
    left: Vec<u8>,
    right: Vec<u8>,
    scratch: Vec<u8>,
}

impl Buffers {
    fn new(limit: usize, max_input_len: u64) -> Result<Self, FileError> {
        require(
            FileResource::BufferBytes,
            (BUFFER_BYTES_PER_LIMB + CHECKSUM_BYTES) as u64,
            limit as u64,
        )?;
        let useful = max_input_len.div_ceil(LIMB_DIGITS as u64).max(1);
        let max_limbs =
            (limit.min(MAX_BLOCK_BUFFER_BYTES) - CHECKSUM_BYTES) / BUFFER_BYTES_PER_LIMB;
        let limbs = (max_limbs as u64).min(useful) as usize;
        Ok(Self {
            left: reserve_buffer(limbs * LIMB_DIGITS)?,
            right: reserve_buffer(limbs * LIMB_DIGITS)?,
            scratch: reserve_buffer(limbs * LIMB_BYTES + CHECKSUM_BYTES)?,
        })
    }

    fn chunk_limbs(&self) -> usize {
        self.left.len() / LIMB_DIGITS
    }

    fn payload_bytes(&self) -> usize {
        self.left.len() + self.right.len() + self.scratch.len()
    }
}

fn reserve_buffer(capacity: usize) -> Result<Vec<u8>, FileError> {
    let mut buffer = Vec::new();
    buffer
        .try_reserve_exact(capacity)
        .map_err(|_| FileError::Allocation)?;
    buffer.resize(capacity, 0);
    Ok(buffer)
}

#[derive(Default)]
struct ResultShape {
    limbs: u64,
    top: u32,
}

impl ResultShape {
    fn digits(&self) -> Result<u64, FileError> {
        if self.limbs == 0 {
            return Ok(1);
        }
        (self.limbs - 1)
            .checked_mul(LIMB_DIGITS as u64)
            .and_then(|lower| lower.checked_add(u64::from(self.top.ilog10()) + 1))
            .ok_or(FileError::SizeOverflow)
    }
}

fn compute(
    layout: &ScratchLayout,
    scratch: &mut File,
    operands: (&mut DecimalInput, &mut DecimalInput),
    operation: LimbOperation,
    buffers: &mut Buffers,
    progress: &mut impl FnMut(FileProgress) -> ControlFlow<()>,
) -> Result<ResultShape, FileError> {
    let (left, right) = operands;
    let mut carry = 0;
    let mut shape = ResultShape::default();
    checkpoint(progress, FilePhase::Arithmetic, 0, layout.limbs)?;
    layout.write_header(scratch)?;
    for block in 0..layout.blocks {
        let start = layout.start(block);
        let mut left = left.read_low(start, &mut buffers.left)?.map(parse_limb);
        let right_digits = match operation {
            LimbOperation::Multiply(_) => buffers.right[..0].rchunks(LIMB_DIGITS),
            _ => right.read_low(start, &mut buffers.right)?,
        };
        let mut right = right_digits.map(parse_limb);
        let count = layout.count(block);
        for (index, bytes) in buffers.scratch[..count * LIMB_BYTES]
            .chunks_exact_mut(LIMB_BYTES)
            .enumerate()
        {
            let left = i64::from(left.next().unwrap_or(0));
            let right = i64::from(right.next().unwrap_or(0));
            let limb = match operation {
                LimbOperation::Subtract => {
                    let difference = left - right - carry;
                    carry = i64::from(difference < 0);
                    difference + carry * BASE as i64
                }
                LimbOperation::Add => {
                    let sum = left + right + carry;
                    carry = i64::from(sum >= BASE as i64);
                    sum - carry * BASE as i64
                }
                LimbOperation::Multiply(factor) => {
                    // carry < factor, so the intermediate is below BASE^2.
                    let product = left * factor + carry;
                    carry = product / BASE as i64;
                    product % BASE as i64
                }
            } as u32;
            bytes.copy_from_slice(&limb.to_le_bytes());
            if limb != 0 {
                shape = ResultShape {
                    limbs: start + index as u64 + 1,
                    top: limb,
                };
            }
        }
        layout.write_block(scratch, block, &mut buffers.scratch)?;
        checkpoint(
            progress,
            FilePhase::Arithmetic,
            start + count as u64,
            layout.limbs,
        )?;
    }
    if carry != 0 || shape.limbs == 0 {
        // A stable, validated input and the computed sign/order rule out both.
        return Err(FileError::ScratchIntegrity);
    }
    Ok(shape)
}

fn write_decimal(
    layout: &ScratchLayout,
    scratch: &mut File,
    output: &mut File,
    result: (ResultShape, bool),
    buffers: &mut Buffers,
    output_bytes: u64,
    progress: &mut impl FnMut(FileProgress) -> ControlFlow<()>,
) -> Result<(), FileError> {
    let (shape, negative) = result;
    layout.verify_header(scratch)?;
    let context = io_error("staged decimal output");
    if negative {
        output.write_all(b"-").map_err(context)?;
    }
    let mut written = u64::from(negative);
    for block in (0..layout.blocks).rev() {
        let count = layout.count(block);
        let used = shape
            .limbs
            .saturating_sub(layout.start(block))
            .min(count as u64) as usize;
        let limbs = layout.read_block(scratch, block, &mut buffers.scratch)?;
        let mut length = 0;
        for limb in limbs.rev().skip(count - used) {
            let width = if written == u64::from(negative) && length == 0 {
                shape.top.ilog10() as usize + 1
            } else {
                LIMB_DIGITS
            };
            write_limb(limb, &mut buffers.left[length..length + width]);
            length += width;
        }
        output.write_all(&buffers.left[..length]).map_err(context)?;
        written += length as u64;
        checkpoint(progress, FilePhase::WriteDecimal, written, output_bytes)?;
    }
    debug_assert_eq!(written, output_bytes);
    Ok(())
}

fn write_limb(mut limb: u32, digits: &mut [u8]) {
    for digit in digits.iter_mut().rev() {
        *digit = b'0' + (limb % 10) as u8;
        limb /= 10;
    }
}

fn checkpoint(
    progress: &mut impl FnMut(FileProgress) -> ControlFlow<()>,
    phase: FilePhase,
    completed: u64,
    total: u64,
) -> Result<(), FileError> {
    match progress(FileProgress {
        phase,
        completed,
        total,
    }) {
        ControlFlow::Continue(()) => Ok(()),
        ControlFlow::Break(()) => Err(FileError::Cancelled),
    }
}

fn io_error(context: &'static str) -> impl Fn(io::Error) -> FileError + Copy {
    move |source| FileError::Io { context, source }
}

fn require(resource: FileResource, required: u64, limit: u64) -> Result<(), FileError> {
    if required > limit {
        return Err(FileError::Limit {
            resource,
            required,
            limit,
        });
    }
    Ok(())
}

fn check_output_sizes(output: u64, scratch: u64, limits: FileLimits) -> Result<u64, FileError> {
    require(FileResource::OutputBytes, output, limits.max_output_bytes)?;
    let temporary = scratch.checked_add(output).ok_or(FileError::SizeOverflow)?;
    require(
        FileResource::TemporaryBytes,
        temporary,
        limits.max_temp_bytes,
    )?;
    Ok(temporary)
}

fn ensure_absent(path: &Path) -> Result<(), FileError> {
    let context = io_error("output path");
    match fs::symlink_metadata(path) {
        Ok(_) => Err(context(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "destination already exists",
        ))),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(context(error)),
    }
}
