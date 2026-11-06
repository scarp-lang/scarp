//! Strict conversion between Scarp's UTF-8 source offsets and editor UTF-16 positions.
//!
//! Unlike diagnostic locations (one-based Unicode scalar columns), editor
//! positions are zero-based UTF-16 code-unit offsets. CRLF and lone CR/LF
//! delimit lines, and no position may split a surrogate pair or CRLF terminator.

use super::{SourceId, SourceLocation, SymbolDefinition, SymbolIndex};

/// Zero-based line and UTF-16 code-unit column, as used by LSP editors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Utf16Position {
    pub line: u32,
    pub character: u32,
}
impl Utf16Position {
    pub const fn new(line: u32, character: u32) -> Self {
        Self { line, character }
    }
}

/// Half-open editor range. Source identity is carried by SourceLocation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Utf16Range {
    pub start: Utf16Position,
    pub end: Utf16Position,
}

/// Translate a valid UTF-8 byte boundary to a zero-based UTF-16 position.
///
/// Returns None for an out-of-range offset, an offset within a multi-byte
/// scalar, or the internal boundary between CR and LF in a CRLF terminator.
pub fn byte_to_utf16(source: &str, offset: usize) -> Option<Utf16Position> {
    if !source.is_char_boundary(offset) {
        return None;
    }
    let mut line = 0u32;
    let mut character = 0u32;
    let mut chars = source.char_indices();
    while let Some((at, ch)) = chars.next() {
        if at == offset {
            return Some(Utf16Position::new(line, character));
        }
        match ch {
            '\r' => {
                if source.as_bytes().get(at + 1) == Some(&b'\n') {
                    // This boundary is not a valid editor position: one CRLF
                    // sequence is one line break, not two character positions.
                    let _ = chars.next();
                }
                line = line.checked_add(1)?;
                character = 0;
            }
            '\n' => {
                line = line.checked_add(1)?;
                character = 0;
            }
            _ => character = character.checked_add(ch.len_utf16() as u32)?,
        }
    }
    (offset == source.len()).then_some(Utf16Position::new(line, character))
}

/// Translate an editor position to a file-local UTF-8 byte boundary.
///
/// Returns None for missing lines, columns beyond line end, and a column
/// inside the two UTF-16 units of a supplementary Unicode character. A column
/// at the end of a line maps to the byte before its line terminator.
pub fn utf16_to_byte(source: &str, position: Utf16Position) -> Option<usize> {
    let mut line = 0u32;
    let mut start = 0usize;
    loop {
        let delimiter = source.as_bytes()[start..]
            .iter()
            .position(|&b| b == b'\r' || b == b'\n');
        let end = delimiter.map_or(source.len(), |n| start + n);
        if line == position.line {
            let mut character = 0u32;
            for (relative, ch) in source[start..end].char_indices() {
                if character == position.character {
                    return Some(start + relative);
                }
                character = character.checked_add(ch.len_utf16() as u32)?;
                if character > position.character {
                    return None;
                }
            }
            return (character == position.character).then_some(end);
        }
        let delimiter = delimiter?;
        let at = start + delimiter;
        start = if source.as_bytes()[at] == b'\r' && source.as_bytes().get(at + 1) == Some(&b'\n') {
            at + 2
        } else {
            at + 1
        };
        line = line.checked_add(1)?;
    }
}

impl SymbolIndex {
    /// Resolve an editor position to the same symbol as goto_definition.
    /// No recompilation, source mutation, or runtime execution is performed.
    pub fn goto_definition_utf16(
        &self,
        source: SourceId,
        position: Utf16Position,
    ) -> Option<&SymbolDefinition> {
        let byte = utf16_to_byte(self.sources().source(source)?, position)?;
        self.goto_definition(source, byte)
    }

    /// Convert a file-local byte offset using the original immutable source.
    pub fn utf16_position(&self, source: SourceId, byte: usize) -> Option<Utf16Position> {
        byte_to_utf16(self.sources().source(source)?, byte)
    }

    /// Convert an original source location to an editor half-open range.
    pub fn utf16_range(&self, location: SourceLocation) -> Option<Utf16Range> {
        if location.span.start > location.span.end {
            return None;
        }
        let source = self.sources().source(location.source)?;
        Some(Utf16Range {
            start: byte_to_utf16(source, location.span.start)?,
            end: byte_to_utf16(source, location.span.end)?,
        })
    }
}
