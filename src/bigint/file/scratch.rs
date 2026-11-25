use super::{io_error, FileError, BASE, CHECKSUM_BYTES, LIMB_BYTES};
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom, Write},
};

const MAGIC: &[u8; 8] = b"SCLIMB01";
const HEADER_BYTES: usize = 24;

/// Portable scratch format: magic, two LE u64 geometry fields, then ascending
/// blocks of LE u32 decimal limbs with an LE u64 checksum at each block's end.
pub(super) struct ScratchLayout {
    pub(super) limbs: u64,
    chunk_limbs: u64,
    pub(super) blocks: u64,
    pub(super) bytes: u64,
}

impl ScratchLayout {
    pub(super) fn new(limbs: u64, chunk_limbs: usize) -> Result<Self, FileError> {
        let chunk_limbs = chunk_limbs as u64;
        let blocks = limbs.div_ceil(chunk_limbs);
        let bytes = limbs
            .checked_mul(LIMB_BYTES as u64)
            .and_then(|payload| payload.checked_add(blocks.checked_mul(CHECKSUM_BYTES as u64)?))
            .and_then(|records| records.checked_add(HEADER_BYTES as u64))
            .ok_or(FileError::SizeOverflow)?;
        Ok(Self {
            limbs,
            chunk_limbs,
            blocks,
            bytes,
        })
    }

    fn header(&self) -> [u8; HEADER_BYTES] {
        let mut header = [0; HEADER_BYTES];
        header[..8].copy_from_slice(MAGIC);
        header[8..16].copy_from_slice(&self.chunk_limbs.to_le_bytes());
        header[16..].copy_from_slice(&self.limbs.to_le_bytes());
        header
    }

    pub(super) fn write_header(&self, file: &mut File) -> Result<(), FileError> {
        file.write_all(&self.header()).map_err(io_error("scratch"))
    }

    pub(super) fn verify_header(&self, file: &mut File) -> Result<(), FileError> {
        let context = io_error("scratch");
        if file.metadata().map_err(context)?.len() != self.bytes {
            return Err(FileError::ScratchIntegrity);
        }
        file.rewind().map_err(context)?;
        let mut header = [0; HEADER_BYTES];
        file.read_exact(&mut header).map_err(context)?;
        if header != self.header() {
            return Err(FileError::ScratchIntegrity);
        }
        Ok(())
    }

    pub(super) fn start(&self, index: u64) -> u64 {
        // A valid block index has a start strictly below the planned limb count.
        index * self.chunk_limbs
    }

    pub(super) fn count(&self, index: u64) -> usize {
        (self.limbs - self.start(index)).min(self.chunk_limbs) as usize
    }

    pub(super) fn write_block(
        &self,
        file: &mut File,
        index: u64,
        buffer: &mut [u8],
    ) -> Result<(), FileError> {
        let payload_bytes = self.count(index) * LIMB_BYTES;
        let record = &mut buffer[..payload_bytes + CHECKSUM_BYTES];
        let checksum = checksum(index, &record[..payload_bytes]);
        record[payload_bytes..].copy_from_slice(&checksum.to_le_bytes());
        file.write_all(record).map_err(io_error("scratch"))
    }

    pub(super) fn read_block<'a>(
        &self,
        file: &mut File,
        index: u64,
        buffer: &'a mut [u8],
    ) -> Result<impl DoubleEndedIterator<Item = u32> + 'a, FileError> {
        let payload_bytes = self.count(index) * LIMB_BYTES;
        let record = &mut buffer[..payload_bytes + CHECKSUM_BYTES];
        // Earlier records are full, so this offset is bounded by the checked file size.
        let offset = HEADER_BYTES as u64
            + self.start(index) * LIMB_BYTES as u64
            + index * CHECKSUM_BYTES as u64;
        let context = io_error("scratch");
        file.seek(SeekFrom::Start(offset)).map_err(context)?;
        file.read_exact(record).map_err(context)?;
        let (limbs, expected) = record.split_at(payload_bytes);
        if checksum(index, limbs).to_le_bytes() != expected {
            return Err(FileError::ScratchIntegrity);
        }
        let limbs = limbs.chunks_exact(LIMB_BYTES).map(|bytes| {
            u32::from_le_bytes(bytes.try_into().expect("four bytes per scratch limb"))
        });
        if limbs.clone().any(|limb| u64::from(limb) >= BASE) {
            return Err(FileError::ScratchIntegrity);
        }
        Ok(limbs)
    }
}

// FNV-1a detects accidental corruption. It does not authenticate the workspace.
fn checksum(index: u64, bytes: &[u8]) -> u64 {
    index
        .to_le_bytes()
        .into_iter()
        .chain(bytes.iter().copied())
        .fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x100_0000_01b3)
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checks_exact_record_sizes_and_overflow() {
        let layout = ScratchLayout::new(5, 2).unwrap();
        assert_eq!(layout.blocks, 3);
        assert_eq!(layout.bytes, 24 + 20 + 24);
        assert_eq!(layout.count(2), 1);
        for (limbs, chunk_limbs) in [(u64::MAX, 1), (u64::MAX / 4, 2)] {
            assert!(matches!(
                ScratchLayout::new(limbs, chunk_limbs),
                Err(FileError::SizeOverflow)
            ));
        }
    }
}
