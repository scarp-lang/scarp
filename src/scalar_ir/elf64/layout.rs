//! Checked file and stack layouts for the Linux x86-64 SysV target.

use super::{failure, Diagnostic};

pub(super) const INTEGER_ARGUMENT_REGISTERS: [u8; 6] = [7, 6, 2, 1, 8, 9];

const SLOT_BYTES: usize = 8;
const STACK_ALIGNMENT: usize = 16;
// The IR permits 256 bindings and 1024 temporaries, plus frame padding.
const MAX_FRAME_BYTES: usize = crate::native_object::FREESTANDING_X86_64_MAX_FRAME_BYTES;

pub(super) const MAX_TEXT_BYTES: usize = 200_000;
pub(super) const ELF_HEADER_BYTES: usize = 64;
pub(super) const SECTION_HEADER_BYTES: usize = 64;
pub(super) const SECTION_COUNT: usize = 6;
pub(super) const SYMBOL_BYTES: usize = 24;
const MAX_OBJECT_BYTES: usize = 262_144;

#[derive(Debug, Clone, Copy)]
pub(super) struct FileRegion {
    offset: usize,
    size: usize,
    alignment: usize,
}

impl FileRegion {
    fn place(after: usize, size: usize, alignment: usize) -> Result<Self, Diagnostic> {
        let region = Self {
            offset: align_up(after, alignment)?,
            size,
            alignment,
        };
        region.end()?;
        Ok(region)
    }

    pub(super) fn end(self) -> Result<usize, Diagnostic> {
        self.offset
            .checked_add(self.size)
            .filter(|end| *end <= MAX_OBJECT_BYTES)
            .ok_or_else(failure)
    }

    pub(super) fn offset(self) -> usize {
        self.offset
    }

    pub(super) fn size(self) -> usize {
        self.size
    }

    pub(super) fn alignment(self) -> usize {
        self.alignment
    }
}

/// File ranges shared by the payload writer and the headers. Each placement
/// checks padding and end offsets before any output allocation.
pub(super) struct ObjectLayout {
    pub(super) text: FileRegion,
    pub(super) symbols: FileRegion,
    pub(super) strings: FileRegion,
    pub(super) section_names: FileRegion,
    pub(super) section_headers: FileRegion,
}

impl ObjectLayout {
    pub(super) fn plan(
        text_bytes: usize,
        string_bytes: usize,
        section_name_bytes: usize,
    ) -> Result<Self, Diagnostic> {
        if !(1..=MAX_TEXT_BYTES).contains(&text_bytes) || string_bytes < 3 || section_name_bytes < 2
        {
            return Err(failure());
        }
        let text = FileRegion::place(ELF_HEADER_BYTES, text_bytes, 16)?;
        let symbols = FileRegion::place(text.end()?, 2 * SYMBOL_BYTES, 8)?;
        let strings = FileRegion::place(symbols.end()?, string_bytes, 1)?;
        let section_names = FileRegion::place(strings.end()?, section_name_bytes, 1)?;
        let section_headers = FileRegion::place(
            section_names.end()?,
            SECTION_COUNT * SECTION_HEADER_BYTES,
            8,
        )?;
        Ok(Self {
            text,
            symbols,
            strings,
            section_names,
            section_headers,
        })
    }
}

fn align_up(value: usize, alignment: usize) -> Result<usize, Diagnostic> {
    if !alignment.is_power_of_two() {
        return Err(failure());
    }
    let mask = alignment - 1;
    value
        .checked_add(mask)
        .map(|padded| padded & !mask)
        .ok_or_else(failure)
}

pub(super) fn slot_offset(index: usize) -> Result<[u8; 4], Diagnostic> {
    index
        .checked_mul(SLOT_BYTES)
        .and_then(|offset| i32::try_from(offset).ok())
        .map(i32::to_le_bytes)
        .ok_or_else(failure)
}

/// Offset of a caller-owned stack argument from RSP after frame allocation.
/// Entry RSP points at the return address, then argument seven, eight, etc.
pub(super) fn stack_argument_offset(frame: u32, stack_index: usize) -> Result<[u8; 4], Diagnostic> {
    (frame as usize)
        .checked_add(SLOT_BYTES)
        .and_then(|base| stack_index.checked_mul(SLOT_BYTES)?.checked_add(base))
        .and_then(|offset| i32::try_from(offset).ok())
        .map(i32::to_le_bytes)
        .ok_or_else(failure)
}

pub(super) fn frame_bytes(slots: usize, temporaries: usize) -> Result<u32, Diagnostic> {
    let bytes = slots
        .checked_add(temporaries)
        .and_then(|count| count.checked_mul(SLOT_BYTES))
        .ok_or_else(failure)?;
    // The return address occupies one slot. Align the complete frame so
    // RSP is 16-byte aligned after the prologue, as required at a call site.
    let with_return_address = bytes.checked_add(SLOT_BYTES).ok_or_else(failure)?;
    let aligned = align_up(with_return_address, STACK_ALIGNMENT)? - SLOT_BYTES;
    if aligned > MAX_FRAME_BYTES {
        return Err(failure());
    }
    u32::try_from(aligned).map_err(|_| failure())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn incoming_arguments_follow_the_return_address() {
        for frame in [8, 24, 4088, 8200] {
            for index in 0..10 {
                assert_eq!(
                    i32::from_le_bytes(stack_argument_offset(frame, index).unwrap()),
                    frame as i32 + 8 + index as i32 * 8
                );
            }
        }
        assert!(stack_argument_offset(u32::MAX, 0).is_err());
        assert!(stack_argument_offset(8, usize::MAX).is_err());
    }

    #[test]
    fn alignment_and_frame_boundaries_are_checked() {
        assert_eq!(align_up(17, 16).unwrap(), 32);
        for alignment in [0, 3, 6] {
            assert!(align_up(16, alignment).is_err());
        }
        assert!(align_up(usize::MAX, 8).is_err());
        assert!(slot_offset(usize::MAX).is_err());
        assert!(frame_bytes(usize::MAX, 1).is_err());
        assert!(frame_bytes(MAX_FRAME_BYTES / SLOT_BYTES, 1).is_err());
        for slots in 0..=256 {
            for temporaries in [0, 1, 512, 1024] {
                let frame = frame_bytes(slots, temporaries).unwrap() as usize;
                let used = (slots + temporaries) * SLOT_BYTES;
                assert_eq!((frame + SLOT_BYTES) % STACK_ALIGNMENT, 0);
                assert!((used..used + STACK_ALIGNMENT).contains(&frame));
            }
        }
    }

    #[test]
    fn elf_payloads_and_section_headers_are_aligned_bounded_and_disjoint() {
        for text_size in [1, 7, 8, 9, MAX_TEXT_BYTES] {
            let layout = ObjectLayout::plan(text_size, 19, 44).unwrap();
            let sections = [
                layout.text,
                layout.symbols,
                layout.strings,
                layout.section_names,
                layout.section_headers,
            ];
            assert_eq!(layout.text.offset, ELF_HEADER_BYTES);
            for (index, section) in sections.iter().enumerate() {
                assert_eq!(section.offset % section.alignment, 0);
                assert!(section.end().unwrap() <= MAX_OBJECT_BYTES);
                if let Some(previous) = index.checked_sub(1) {
                    let end = sections[previous].end().unwrap();
                    assert!(section.offset >= end);
                    assert!(section.offset - end < section.alignment);
                }
            }
        }
        for (text, strings, names) in [
            (0, 19, 44),
            (MAX_TEXT_BYTES + 1, 19, 44),
            (1, 2, 44),
            (1, 19, 1),
            (1, MAX_OBJECT_BYTES, 44),
            (1, 19, usize::MAX),
        ] {
            assert!(ObjectLayout::plan(text, strings, names).is_err());
        }
        assert!(FileRegion::place(usize::MAX, 1, 8).is_err());
    }
}
