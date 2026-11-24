//! Checked file and stack layouts for the Linux x86-64 SysV target.

use super::{failure, Diagnostic};

pub(super) const INTEGER_ARGUMENT_REGISTERS: [u8; 6] = [7, 6, 2, 1, 8, 9];

const SLOT_BYTES: usize = 8;
const STACK_ALIGNMENT: usize = 16;
// The IR permits 256 bindings and 1024 temporaries, plus frame padding.
const MAX_FRAME_BYTES: usize = 16 * 1024;

pub(super) fn align_up(value: usize, alignment: usize) -> Result<usize, Diagnostic> {
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
}
