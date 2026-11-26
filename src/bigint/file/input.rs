use super::{checkpoint, io_error, FileError, FilePhase, FileProgress, LIMB_DIGITS};
use std::{
    cmp::Ordering,
    fs::{self, File, Metadata},
    io::{Read, Seek, SeekFrom},
    ops::ControlFlow,
    path::Path,
    time::SystemTime,
};

pub(super) struct DecimalInput {
    file: File,
    operand: &'static str,
    pub(super) len: u64,
    modified: Option<SystemTime>,
    pub(super) digits: u64,
    pub(super) negative: bool,
}

impl DecimalInput {
    pub(super) fn open(path: &Path, operand: &'static str) -> Result<Self, FileError> {
        // The caller supplies stable paths. Check before opening to avoid
        // blocking on an existing FIFO, then check the opened handle as well.
        let context = io_error(operand);
        require_regular(&fs::metadata(path).map_err(context)?, operand)?;
        let file = File::open(path).map_err(context)?;
        let metadata = file.metadata().map_err(context)?;
        require_regular(&metadata, operand)?;
        Ok(Self {
            file,
            operand,
            len: metadata.len(),
            modified: metadata.modified().ok(),
            digits: 0,
            negative: false,
        })
    }

    pub(super) fn scan(
        &mut self,
        bytes: &mut [u8],
        already_read: u64,
        total_input: u64,
        progress: &mut impl FnMut(FileProgress) -> ControlFlow<()>,
    ) -> Result<(), FileError> {
        let mut position = 0;
        let mut first_nonzero = None;
        let mut has_sign = false;
        checkpoint(progress, FilePhase::ReadInputs, already_read, total_input)?;
        while position < self.len {
            let count = (self.len - position).min(bytes.len() as u64) as usize;
            self.file
                .read_exact(&mut bytes[..count])
                .map_err(io_error(self.operand))?;
            let start = if position == 0 && matches!(bytes[0], b'+' | b'-') {
                has_sign = true;
                self.negative = bytes[0] == b'-';
                1
            } else {
                0
            };
            self.validate_at(&bytes[start..count], position + start as u64)?;
            if first_nonzero.is_none() {
                first_nonzero = bytes[start..count]
                    .iter()
                    .position(|digit| *digit != b'0')
                    .map(|index| position + start as u64 + index as u64);
            }
            position += count as u64;
            checkpoint(
                progress,
                FilePhase::ReadInputs,
                already_read + position,
                total_input,
            )?;
        }
        if self.len == u64::from(has_sign) {
            return Err(self.invalid(self.len));
        }
        self.digits = first_nonzero.map_or(0, |start| self.len - start);
        self.negative &= self.digits != 0;
        Ok(())
    }

    pub(super) fn check_unchanged(&self) -> Result<(), FileError> {
        let metadata = self.file.metadata().map_err(io_error(self.operand))?;
        if self.len != metadata.len() || self.modified != metadata.modified().ok() {
            return Err(self.changed());
        }
        Ok(())
    }

    pub(super) fn changed(&self) -> FileError {
        FileError::InputChanged {
            operand: self.operand,
        }
    }

    pub(super) fn compare(
        &mut self,
        other: &mut Self,
        left_buffer: &mut [u8],
        right_buffer: &mut [u8],
        progress: &mut impl FnMut(FileProgress) -> ControlFlow<()>,
    ) -> Result<Ordering, FileError> {
        let order = self.digits.cmp(&other.digits);
        if order != Ordering::Equal {
            return Ok(order);
        }
        let mut position = 0;
        checkpoint(
            progress,
            FilePhase::CompareMagnitudes,
            position,
            self.digits,
        )?;
        while position < self.digits {
            let count = (self.digits - position).min(left_buffer.len() as u64) as usize;
            let left = &mut left_buffer[..count];
            let right = &mut right_buffer[..count];
            self.read_magnitude(position, left)?;
            other.read_magnitude(position, right)?;
            position += count as u64;
            checkpoint(
                progress,
                FilePhase::CompareMagnitudes,
                position,
                self.digits,
            )?;
            let order = (*left).cmp(right);
            if order != Ordering::Equal {
                return Ok(order);
            }
        }
        Ok(Ordering::Equal)
    }

    pub(super) fn read_magnitude(
        &mut self,
        offset: u64,
        bytes: &mut [u8],
    ) -> Result<(), FileError> {
        debug_assert!(offset <= self.digits && bytes.len() as u64 <= self.digits - offset);
        self.read_digits(self.len - self.digits + offset, bytes)
    }

    pub(super) fn read_low<'a>(
        &mut self,
        limb_offset: u64,
        buffer: &'a mut [u8],
    ) -> Result<std::slice::RChunks<'a, u8>, FileError> {
        if limb_offset >= self.digits.div_ceil(LIMB_DIGITS as u64) {
            return Ok(buffer[..0].rchunks(LIMB_DIGITS));
        }
        let skip = limb_offset * LIMB_DIGITS as u64;
        let count = (self.digits - skip).min(buffer.len() as u64) as usize;
        let chunk = &mut buffer[..count];
        self.read_digits(self.len - skip - count as u64, chunk)?;
        Ok(chunk.rchunks(LIMB_DIGITS))
    }

    fn read_digits(&mut self, at: u64, bytes: &mut [u8]) -> Result<(), FileError> {
        let context = io_error(self.operand);
        self.file.seek(SeekFrom::Start(at)).map_err(context)?;
        self.file.read_exact(bytes).map_err(context)?;
        self.validate_at(bytes, at)
    }

    fn validate_at(&self, bytes: &[u8], at: u64) -> Result<(), FileError> {
        if let Some(index) = bytes.iter().position(|digit| !digit.is_ascii_digit()) {
            return Err(self.invalid(at + index as u64));
        }
        Ok(())
    }

    fn invalid(&self, offset: u64) -> FileError {
        FileError::InvalidDecimal {
            operand: self.operand,
            offset,
        }
    }
}

fn require_regular(metadata: &Metadata, operand: &'static str) -> Result<(), FileError> {
    if metadata.is_file() {
        Ok(())
    } else {
        Err(FileError::NotRegularFile { operand })
    }
}

pub(super) fn parse_limb(bytes: &[u8]) -> u32 {
    bytes
        .iter()
        .fold(0, |limb, byte| limb * 10 + u32::from(byte - b'0'))
}
