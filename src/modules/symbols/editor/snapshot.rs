//! An opt-in immutable, revision-checked editor view of a symbol snapshot.
//!
//! The caller owns document revisions and must pass the *current revisions for
//! every file in the loaded graph* on each query. Any mismatch invalidates the
//! complete graph, not just the source containing the cursor.

use super::{Utf16Position, Utf16Range};
use crate::modules::{
    symbols::{SymbolDefinition, SymbolId, SymbolIndex, SymbolReference},
    SourceId, SourceLocation,
};
use std::{error::Error, fmt, sync::Arc};

/// Maximum retained line records across the graph. Independent of IndexLimits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EditorLimits {
    pub max_lines: usize,
}
impl Default for EditorLimits {
    fn default() -> Self {
        Self { max_lines: 262_144 }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorBuildError {
    VersionCount { expected: usize, actual: usize },
    LineLimit,
    LineNumberLimit,
    Allocation,
}
impl fmt::Display for EditorBuildError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::VersionCount { expected, actual } => {
                write!(
                    f,
                    "expected {expected} document revisions; received {actual}"
                )
            }
            Self::LineLimit => f.write_str("editor line index limit exceeded"),
            Self::LineNumberLimit => {
                f.write_str("editor line number exceeds UTF-16 protocol range")
            }
            Self::Allocation => f.write_str("could not reserve editor line index"),
        }
    }
}
impl Error for EditorBuildError {}

/// A stale graph is distinct from invalid coordinates and a valid non-symbol.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorQueryError {
    VersionCount {
        expected: usize,
        actual: usize,
    },
    StaleVersion {
        source: SourceId,
        indexed: u64,
        current: u64,
    },
    UnknownSource,
    InvalidCoordinate,
    UnknownSymbol,
}
impl fmt::Display for EditorQueryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::VersionCount { expected, actual } => {
                write!(
                    f,
                    "expected {expected} document revisions; received {actual}"
                )
            }
            Self::StaleVersion {
                source,
                indexed,
                current,
            } => write!(
                f,
                "stale source {source}: indexed revision {indexed}, current revision {current}"
            ),
            Self::UnknownSource => f.write_str("unknown source file"),
            Self::InvalidCoordinate => f.write_str("invalid UTF-16 or UTF-8 source coordinate"),
            Self::UnknownSymbol => f.write_str("symbol belongs to a different index"),
        }
    }
}
impl Error for EditorQueryError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Line {
    start: usize,
    end: usize, // Content end; CR/LF bytes are excluded.
}
#[derive(Debug)]
struct EditorData {
    lines: Vec<Vec<Line>>,
    total_lines: usize,
}

/// Read-only editor coordinates bound to one successful compilation and its
/// caller-supplied revisions. Clones share both original sources and line tables.
#[derive(Debug, Clone)]
pub struct EditorSnapshot {
    index: SymbolIndex,
    versions: Arc<Vec<u64>>,
    data: Arc<EditorData>,
}

fn append_line(
    lines: &mut Vec<Line>,
    total: &mut usize,
    limits: EditorLimits,
    line: Line,
) -> Result<(), EditorBuildError> {
    if *total >= limits.max_lines {
        return Err(EditorBuildError::LineLimit);
    }
    if u32::try_from(lines.len()).is_err() {
        return Err(EditorBuildError::LineNumberLimit);
    }
    lines
        .try_reserve(1)
        .map_err(|_| EditorBuildError::Allocation)?;
    lines.push(line);
    *total += 1;
    Ok(())
}

fn index_lines(
    source: &str,
    total: &mut usize,
    limits: EditorLimits,
) -> Result<Vec<Line>, EditorBuildError> {
    let bytes = source.as_bytes();
    let mut lines = Vec::new();
    let mut start = 0;
    let mut offset = 0;
    while offset < bytes.len() {
        let byte = bytes[offset];
        if byte == b'\n' || byte == b'\r' {
            append_line(&mut lines, total, limits, Line { start, end: offset })?;
            if byte == b'\r' && bytes.get(offset + 1) == Some(&b'\n') {
                offset += 1;
            }
            start = offset + 1;
        }
        offset += 1;
    }
    append_line(
        &mut lines,
        total,
        limits,
        Line {
            start,
            end: bytes.len(),
        },
    )?;
    Ok(lines)
}

fn utf16_to_offset(source: &str, lines: &[Line], at: Utf16Position) -> Option<usize> {
    let line = *lines.get(at.line as usize)?;
    let contents = source.get(line.start..line.end)?;
    let mut units = 0u32;
    for (offset, scalar) in contents.char_indices() {
        if units == at.character {
            return Some(line.start + offset);
        }
        units = units.checked_add(scalar.len_utf16() as u32)?;
        if units > at.character {
            return None;
        }
    }
    (units == at.character).then_some(line.end)
}
fn offset_to_utf16(source: &str, lines: &[Line], offset: usize) -> Option<Utf16Position> {
    if !source.is_char_boundary(offset) {
        return None;
    }
    let id = lines
        .partition_point(|line| line.start <= offset)
        .checked_sub(1)?;
    let line = lines[id];
    if offset > line.end {
        return None; // Within CRLF or after a line's content boundary.
    }
    let contents = source.get(line.start..offset)?;
    let mut units = 0u32;
    for scalar in contents.chars() {
        units = units.checked_add(scalar.len_utf16() as u32)?;
    }
    Some(Utf16Position::new(u32::try_from(id).ok()?, units))
}

impl EditorSnapshot {
    /// Build a reusable line index. The version list is in SourceId order,
    /// exactly one entry per file returned by the compiled SourceMap.
    ///
    /// It is not safe to reuse this snapshot after an edit even when a cursor
    /// offset or SymbolId still happens to be valid. Callers must increment
    /// the changed file's revision and rebuild before querying again.
    pub fn new(
        index: &SymbolIndex,
        versions: &[u64],
        limits: EditorLimits,
    ) -> Result<Self, EditorBuildError> {
        let files = index.sources().files();
        if versions.len() != files.len() {
            return Err(EditorBuildError::VersionCount {
                expected: files.len(),
                actual: versions.len(),
            });
        }
        let mut lines = Vec::new();
        lines
            .try_reserve(files.len())
            .map_err(|_| EditorBuildError::Allocation)?;
        let mut total_lines = 0;
        for source in 0..files.len() {
            let text = index
                .sources()
                .source(source)
                .expect("source map file count is authoritative");
            lines.push(index_lines(text, &mut total_lines, limits)?);
        }
        let mut retained_versions = Vec::new();
        retained_versions
            .try_reserve_exact(versions.len())
            .map_err(|_| EditorBuildError::Allocation)?;
        retained_versions.extend_from_slice(versions);
        Ok(Self {
            index: index.clone(),
            versions: Arc::new(retained_versions),
            data: Arc::new(EditorData { lines, total_lines }),
        })
    }

    // The compilation cache calls this only after validating the exact source
    // graph and collecting one current revision for each of its sources.
    pub(in crate::modules) fn set_revisions(&mut self, versions: Vec<u64>) {
        debug_assert_eq!(versions.len(), self.versions.len());
        if self.versions.as_slice() != versions.as_slice() {
            self.versions = Arc::new(versions);
        }
    }

    pub fn symbols(&self) -> &SymbolIndex {
        &self.index
    }
    pub fn total_lines(&self) -> usize {
        self.data.total_lines
    }
    pub fn line_count(&self, source: SourceId) -> Option<usize> {
        self.data.lines.get(source).map(Vec::len)
    }

    /// Check *all* loaded documents before any symbol/range lookup. A changed
    /// import invalidates entry-file navigation, even at an unchanged offset.
    pub fn ensure_current(&self, versions: &[u64]) -> Result<(), EditorQueryError> {
        if self.versions.len() != versions.len() {
            return Err(EditorQueryError::VersionCount {
                expected: self.versions.len(),
                actual: versions.len(),
            });
        }
        for (source, (&indexed, &current)) in self.versions.iter().zip(versions).enumerate() {
            if indexed != current {
                return Err(EditorQueryError::StaleVersion {
                    source,
                    indexed,
                    current,
                });
            }
        }
        Ok(())
    }

    /// Return the original file-local byte boundary, or a typed error.
    pub fn utf16_to_byte(
        &self,
        versions: &[u64],
        source: SourceId,
        position: Utf16Position,
    ) -> Result<usize, EditorQueryError> {
        self.ensure_current(versions)?;
        let text = self
            .index
            .sources()
            .source(source)
            .ok_or(EditorQueryError::UnknownSource)?;
        let lines = self
            .data
            .lines
            .get(source)
            .ok_or(EditorQueryError::UnknownSource)?;
        utf16_to_offset(text, lines, position).ok_or(EditorQueryError::InvalidCoordinate)
    }

    /// Binary-search the source line before scanning just that line's scalars.
    pub fn byte_to_utf16(
        &self,
        versions: &[u64],
        source: SourceId,
        byte: usize,
    ) -> Result<Utf16Position, EditorQueryError> {
        self.ensure_current(versions)?;
        let text = self
            .index
            .sources()
            .source(source)
            .ok_or(EditorQueryError::UnknownSource)?;
        let lines = self
            .data
            .lines
            .get(source)
            .ok_or(EditorQueryError::UnknownSource)?;
        offset_to_utf16(text, lines, byte).ok_or(EditorQueryError::InvalidCoordinate)
    }

    /// Ok(None) means a valid coordinate without a symbol; errors mean the
    /// request is stale, the file is unknown, or the coordinate is malformed.
    pub fn goto_definition(
        &self,
        versions: &[u64],
        source: SourceId,
        position: Utf16Position,
    ) -> Result<Option<&SymbolDefinition>, EditorQueryError> {
        let byte = self.utf16_to_byte(versions, source, position)?;
        Ok(self.index.goto_definition(source, byte))
    }

    pub fn utf16_range(
        &self,
        versions: &[u64],
        location: SourceLocation,
    ) -> Result<Utf16Range, EditorQueryError> {
        self.ensure_current(versions)?;
        if location.span.start > location.span.end {
            return Err(EditorQueryError::InvalidCoordinate);
        }
        let start = self.byte_to_utf16(versions, location.source, location.span.start)?;
        let end = self.byte_to_utf16(versions, location.source, location.span.end)?;
        Ok(Utf16Range { start, end })
    }

    /// Definition sites are excluded. All imported-file versions are checked
    /// before exposing any reference. Symbol IDs are snapshot-local.
    pub fn find_references<'a>(
        &'a self,
        versions: &[u64],
        id: SymbolId,
    ) -> Result<impl Iterator<Item = &'a SymbolReference> + 'a, EditorQueryError> {
        self.ensure_current(versions)?;
        if self.index.symbol(id).is_none() {
            return Err(EditorQueryError::UnknownSymbol);
        }
        Ok(self.index.find_references(id))
    }
}
