//! Reuse complete editor compilation results after fresh graph validation.

use super::{same_source_graph, CacheLimits, CacheStatus, ConstantCache};
use crate::{
    modules::{
        self,
        symbols::{
            editor::{EditorBuildError, EditorLimits, EditorSnapshot},
            IndexLimits,
        },
        ConstantLimits, ModuleDiagnostic, ModuleGraph, ModuleLimits, ModuleProgram, SourceId,
    },
    types::TypeLimits,
};
use std::{collections::VecDeque, error::Error, fmt, sync::Arc};

/// Compilation and editor work limits for each request.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EditorCompileLimits {
    pub constant_limits: ConstantLimits,
    pub type_limits: TypeLimits,
    pub index_limits: IndexLimits,
    pub editor_limits: EditorLimits,
}

/// Cumulative cache retention bounds across successful editor compilations.
///
/// Index items count symbol definitions and references. Source and name bytes
/// count logical UTF-8 payload. Line records count each retained source line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EditorCacheLimits {
    pub graphs: CacheLimits,
    pub max_retained_index_items: usize,
    pub max_retained_name_bytes: usize,
    pub max_retained_lines: usize,
}

impl Default for EditorCacheLimits {
    fn default() -> Self {
        Self {
            graphs: CacheLimits::default(),
            max_retained_index_items: 262_144,
            max_retained_name_bytes: 4 * 1024 * 1024,
            max_retained_lines: EditorLimits::default().max_lines,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EditorCacheStats {
    pub entries: usize,
    pub retained_source_bytes: usize,
    pub retained_index_items: usize,
    pub retained_name_bytes: usize,
    pub retained_lines: usize,
    pub hits: u64,
    pub misses: u64,
    pub bypasses: u64,
    pub evictions: u64,
}

#[derive(Debug)]
pub enum EditorCompileError {
    Module(ModuleDiagnostic),
    MissingRevision { source: SourceId, identity: String },
    RevisionAllocation,
    Editor(EditorBuildError),
}

impl fmt::Display for EditorCompileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Module(error) => error.fmt(f),
            Self::MissingRevision { source, identity } => {
                write!(
                    f,
                    "missing document revision for source {source} ({identity:?})"
                )
            }
            Self::RevisionAllocation => f.write_str("could not reserve document revisions"),
            Self::Editor(error) => error.fmt(f),
        }
    }
}

impl Error for EditorCompileError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Module(error) => Some(error),
            Self::Editor(error) => Some(error),
            Self::MissingRevision { .. } | Self::RevisionAllocation => None,
        }
    }
}

impl From<ModuleDiagnostic> for EditorCompileError {
    fn from(error: ModuleDiagnostic) -> Self {
        Self::Module(error)
    }
}

impl From<EditorBuildError> for EditorCompileError {
    fn from(error: EditorBuildError) -> Self {
        Self::Editor(error)
    }
}

/// The compiled program and editor snapshot share one source coordinate space.
#[derive(Debug, Clone)]
pub struct CachedEditorProgram {
    pub program: Arc<ModuleProgram>,
    pub snapshot: EditorSnapshot,
    pub status: CacheStatus,
}

#[derive(Debug, Clone, Copy, Default)]
struct Retained {
    source_bytes: usize,
    index_items: usize,
    name_bytes: usize,
    lines: usize,
}

impl Retained {
    fn including(self, extra: Self, limits: EditorCacheLimits) -> Option<Self> {
        let total = Self {
            source_bytes: self.source_bytes.checked_add(extra.source_bytes)?,
            index_items: self.index_items.checked_add(extra.index_items)?,
            name_bytes: self.name_bytes.checked_add(extra.name_bytes)?,
            lines: self.lines.checked_add(extra.lines)?,
        };
        (total.source_bytes <= limits.graphs.max_retained_source_bytes
            && total.index_items <= limits.max_retained_index_items
            && total.name_bytes <= limits.max_retained_name_bytes
            && total.lines <= limits.max_retained_lines)
            .then_some(total)
    }

    fn remove(&mut self, entry: Self) {
        self.source_bytes -= entry.source_bytes;
        self.index_items -= entry.index_items;
        self.name_bytes -= entry.name_bytes;
        self.lines -= entry.lines;
    }
}

#[derive(Debug)]
struct Entry {
    program: Arc<ModuleProgram>,
    snapshot: EditorSnapshot,
    module_limits: ModuleLimits,
    compile_limits: EditorCompileLimits,
    retained: Retained,
}

/// Caller-owned FIFO of successful editor compilations.
///
/// Each request starts from a freshly loaded graph. A hit requires exact
/// source bytes, loader identities, display names, ordered import targets
/// and compiler limits. Hits can bind new revisions to the same verified
/// program, index and line table. Previously returned snapshots keep their
/// original revisions.
#[derive(Debug)]
pub struct EditorCompilationCache {
    limits: EditorCacheLimits,
    entries: VecDeque<Entry>,
    retained: Retained,
    hits: u64,
    misses: u64,
    bypasses: u64,
    evictions: u64,
}

impl EditorCompilationCache {
    pub fn new(limits: EditorCacheLimits) -> Self {
        Self {
            limits,
            entries: VecDeque::new(),
            retained: Retained::default(),
            hits: 0,
            misses: 0,
            bypasses: 0,
            evictions: 0,
        }
    }

    pub fn stats(&self) -> EditorCacheStats {
        EditorCacheStats {
            entries: self.entries.len(),
            retained_source_bytes: self.retained.source_bytes,
            retained_index_items: self.retained.index_items,
            retained_name_bytes: self.retained.name_bytes,
            retained_lines: self.retained.lines,
            hits: self.hits,
            misses: self.misses,
            bypasses: self.bypasses,
            evictions: self.evictions,
        }
    }

    /// Drop cache-owned results while retaining cumulative counters.
    pub fn clear(&mut self) {
        self.evictions = self.evictions.saturating_add(self.entries.len() as u64);
        self.entries.clear();
        self.retained = Retained::default();
    }

    /// Drop every cached graph containing this exact resolved loader identity.
    /// Previously returned programs and snapshots remain usable.
    pub fn evict_identity(&mut self, identity: &str) -> usize {
        let old_len = self.entries.len();
        self.entries.retain(|entry| {
            let affected = entry
                .program
                .modules()
                .iter()
                .any(|module| module.identity == identity);
            if affected {
                self.retained.remove(entry.retained);
            }
            !affected
        });
        let removed = old_len - self.entries.len();
        self.evictions = self.evictions.saturating_add(removed as u64);
        removed
    }

    /// Resolve a fresh graph and bind its editor snapshot to the current revisions.
    ///
    /// The callback supplies a revision for each canonical loader identity from
    /// the same workspace snapshot used to load the graph. Editor queries still
    /// take the current revisions in the returned program's SourceId order.
    /// An optional constant cache can reuse individual folds after source edits.
    pub fn resolve(
        &mut self,
        graph: ModuleGraph,
        limits: EditorCompileLimits,
        mut revision_of: impl FnMut(&str) -> Option<u64>,
        constants: Option<&mut ConstantCache>,
    ) -> Result<CachedEditorProgram, EditorCompileError> {
        let mut revisions = Vec::new();
        revisions
            .try_reserve_exact(graph.file_count())
            .map_err(|_| EditorCompileError::RevisionAllocation)?;
        for (source, module) in graph.modules.iter().enumerate() {
            let revision = revision_of(&module.identity).ok_or_else(|| {
                EditorCompileError::MissingRevision {
                    source,
                    identity: module.identity.clone(),
                }
            })?;
            revisions.push(revision);
        }

        let module_limits = graph.limits;
        for entry in &mut self.entries {
            if entry.module_limits == module_limits
                && entry.compile_limits == limits
                && same_source_graph(&entry.program, &graph)
            {
                entry.snapshot.set_revisions(revisions);
                self.hits = self.hits.saturating_add(1);
                return Ok(CachedEditorProgram {
                    program: entry.program.clone(),
                    snapshot: entry.snapshot.clone(),
                    status: CacheStatus::Hit,
                });
            }
        }

        let source_bytes = graph.sources.text.len();
        let indexed = modules::linker::resolve_indexed(
            graph,
            limits.constant_limits,
            limits.type_limits,
            limits.index_limits,
            constants,
        )?;
        let (program, index) = indexed.into_parts();
        let program = Arc::new(program);
        let snapshot = EditorSnapshot::new(&index, &revisions, limits.editor_limits)?;
        let usage = index.usage();
        let status = self.store(Entry {
            program: program.clone(),
            snapshot: snapshot.clone(),
            module_limits,
            compile_limits: limits,
            retained: Retained {
                source_bytes,
                index_items: usage.symbols + usage.references,
                name_bytes: usage.name_bytes,
                lines: snapshot.total_lines(),
            },
        });
        Ok(CachedEditorProgram {
            program,
            snapshot,
            status,
        })
    }

    fn store(&mut self, entry: Entry) -> CacheStatus {
        self.misses = self.misses.saturating_add(1);
        if self.limits.graphs.max_entries == 0
            || Retained::default()
                .including(entry.retained, self.limits)
                .is_none()
            || self.entries.try_reserve(1).is_err()
        {
            self.bypasses = self.bypasses.saturating_add(1);
            return CacheStatus::Uncached;
        }

        loop {
            if let Some(combined) = self
                .retained
                .including(entry.retained, self.limits)
                .filter(|_| self.entries.len() < self.limits.graphs.max_entries)
            {
                self.retained = combined;
                self.entries.push_back(entry);
                return CacheStatus::Stored;
            }
            let old = self
                .entries
                .pop_front()
                .expect("the entry fits an empty cache");
            self.retained.remove(old.retained);
            self.evictions = self.evictions.saturating_add(1);
        }
    }
}
