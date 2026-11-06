//! Opt-in caller-owned cache of successful whole-graph semantic compilation.
//!
//! Every request still loads, lexes, parses, and validates the entire graph
//! through the ordinary trusted SourceLoader. The expensive link/type/constant
//! resolution result can be reused ONLY if every original byte, source label,
//! resolved per-import target and caller-owned revision matches exactly.
//!
//! This is deliberately not a parser cache or partial incremental compiler.

use super::{
    load, resolve_with_compiler_limits, ConstantLimits, ModuleDiagnostic, ModuleGraph,
    ModuleLimits, ModuleProgram, SourceLoader,
};
use crate::types::TypeLimits;
use std::{collections::VecDeque, error::Error, fmt, sync::Arc};

/// A count bound and a cumulative bound on retained source text.
///
/// The source-byte count is logical retained source payload including the
/// SourceMap's internal separators. It is NOT a heap-size estimate for AST,
/// resolved functions, allocator overhead, or transient compiler memory.
/// Ordinary ModuleLimits and TypeLimits remain in force for every compilation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CacheLimits {
    pub max_entries: usize,
    pub max_retained_source_bytes: usize,
}
impl Default for CacheLimits {
    fn default() -> Self {
        Self {
            max_entries: 4,
            max_retained_source_bytes: 16 * 1024 * 1024,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheStatus {
    /// Full graph was loaded and parsed, but semantic resolution was reused.
    Hit,
    /// Semantic resolution ran and the successful output was retained.
    Stored,
    /// Semantic resolution ran but storage is disabled or over quota.
    Uncached,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CacheStats {
    pub entries: usize,
    pub retained_source_bytes: usize,
    pub hits: u64,
    pub misses: u64,
    pub bypasses: u64,
    pub evictions: u64,
}

#[derive(Debug)]
pub enum CachedCompileError {
    /// Loader, syntax, linker, type, constant or resource diagnostic.
    Module(ModuleDiagnostic),
    /// Exactly one caller-owned document revision is required per discovered
    /// SourceId. A stale version value remains the caller's responsibility.
    RevisionCount { expected: usize, actual: usize },
}
impl fmt::Display for CachedCompileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Module(error) => error.fmt(f),
            Self::RevisionCount { expected, actual } => {
                write!(f, "expected {expected} source revisions; received {actual}")
            }
        }
    }
}
impl Error for CachedCompileError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Module(error) => Some(error),
            Self::RevisionCount { .. } => None,
        }
    }
}
impl From<ModuleDiagnostic> for CachedCompileError {
    fn from(error: ModuleDiagnostic) -> Self {
        Self::Module(error)
    }
}

/// The immutable published output of one successful compilation.
///
/// Cloning an Arc from a cache hit does not duplicate the compiled program.
/// Document revisions remain external; consumers making EditorSnapshots must
/// still perform their own revision-gated navigation queries.
#[derive(Debug, Clone)]
pub struct CachedProgram {
    pub program: Arc<ModuleProgram>,
    pub status: CacheStatus,
}

#[derive(Debug)]
struct Entry {
    program: Arc<ModuleProgram>,
    revisions: Vec<u64>,
    modules: ModuleLimits,
    constants: ConstantLimits,
    types: TypeLimits,
    source_bytes: usize,
}
impl Entry {
    fn matches(
        &self,
        graph: &ModuleGraph,
        revisions: &[u64],
        modules: ModuleLimits,
        constants: ConstantLimits,
        types: TypeLimits,
    ) -> bool {
        if self.modules != modules
            || self.constants != constants
            || self.types != types
            || self.revisions != revisions
        {
            return false;
        }

        let original_sources = self.program.sources();
        if original_sources.entry != graph.sources.entry
            || original_sources.files().len() != graph.sources.files().len()
            || self.program.modules().len() != graph.modules.len()
        {
            return false;
        }

        for (id, module) in graph.modules.iter().enumerate() {
            let old = &self.program.modules()[id];
            if old.identity != module.identity
                || old.source != id
                || original_sources.files()[id].name != graph.sources.files()[id].name
                || original_sources.source(id) != graph.sources.source(id)
            {
                return false;
            }

            // Import declarations are compared in original order, not just
            // deduplicated dependency sets. A -> [A,A,B] and A -> [A,B,B]
            // have identical unique targets but different semantics.
            let old_imports = match self.program.imports_in(id) {
                Some(imports) => imports,
                None => return false,
            };
            if old_imports.len() != module.dependencies.len() {
                return false;
            }
            for (old_import, (_, new_target)) in old_imports.iter().zip(&module.dependencies) {
                if old_import.target != *new_target {
                    return false;
                }
            }
        }
        true
    }
}

/// Caller-owned FIFO of whole-graph successful compilation results.
///
/// Every call reloads the graph using the trusted loader. An exact cache hit
/// skips semantic resolution only; no failed parse/type-check can be hidden by
/// an old result. Replacement is FIFO (not LRU), stable across runs for the
/// same sequence of successful requests. There is no global state or I/O.
#[derive(Debug)]
pub struct CompilationCache {
    limits: CacheLimits,
    entries: VecDeque<Entry>,
    retained_source_bytes: usize,
    hits: u64,
    misses: u64,
    bypasses: u64,
    evictions: u64,
}

impl CompilationCache {
    pub fn new(limits: CacheLimits) -> Self {
        Self {
            limits,
            entries: VecDeque::new(),
            retained_source_bytes: 0,
            hits: 0,
            misses: 0,
            bypasses: 0,
            evictions: 0,
        }
    }

    pub fn stats(&self) -> CacheStats {
        CacheStats {
            entries: self.entries.len(),
            retained_source_bytes: self.retained_source_bytes,
            hits: self.hits,
            misses: self.misses,
            bypasses: self.bypasses,
            evictions: self.evictions,
        }
    }

    /// Drop all entries explicitly. Counters are lifetime cumulative.
    pub fn clear(&mut self) {
        self.evictions = self.evictions.saturating_add(self.entries.len() as u64);
        self.entries.clear();
        self.retained_source_bytes = 0;
    }

    /// Recompile with safe whole-graph reuse of a prior success.
    ///
    /// Revisions are supplied in the current graph's SourceId discovery order
    /// and must advance whenever the corresponding editor document changes.
    /// They are part of the exact cache key, not an alternative to byte-level
    /// comparison. The loader and parser run on EVERY call (including hits).
    ///
    /// No errors or incomplete graphs are cached. Compiler failures retain
    /// earlier successful entries, but NEVER return an older result for the
    /// failed request. The caller may later explicitly clear those entries.
    pub fn compile_with_loader(
        &mut self,
        entry: &str,
        loader: &dyn SourceLoader,
        module_limits: ModuleLimits,
        constant_limits: ConstantLimits,
        type_limits: TypeLimits,
        revisions: &[u64],
    ) -> Result<CachedProgram, CachedCompileError> {
        let graph = load(entry, loader, module_limits)?;
        if revisions.len() != graph.file_count() {
            return Err(CachedCompileError::RevisionCount {
                expected: graph.file_count(),
                actual: revisions.len(),
            });
        }

        for previous in &self.entries {
            if previous.matches(
                &graph,
                revisions,
                module_limits,
                constant_limits,
                type_limits,
            ) {
                self.hits = self.hits.saturating_add(1);
                return Ok(CachedProgram {
                    program: previous.program.clone(),
                    status: CacheStatus::Hit,
                });
            }
        }

        let source_bytes = graph.sources.text.len();
        // Normal compiler semantics run on ALL misses and bypasses. This call
        // can fail: do not overwrite any previous entry on errors.
        let compiled = Arc::new(resolve_with_compiler_limits(
            graph,
            constant_limits,
            type_limits,
        )?);
        self.misses = self.misses.saturating_add(1);

        if self.limits.max_entries == 0 || source_bytes > self.limits.max_retained_source_bytes {
            self.bypasses = self.bypasses.saturating_add(1);
            return Ok(CachedProgram {
                program: compiled,
                status: CacheStatus::Uncached,
            });
        }

        // Reserving only the tiny version vector can fail. A valid compile
        // must still be returned, just without storage.
        let mut retained_versions = Vec::new();
        if retained_versions
            .try_reserve_exact(revisions.len())
            .is_err()
            || self.entries.try_reserve(1).is_err()
        {
            self.bypasses = self.bypasses.saturating_add(1);
            return Ok(CachedProgram {
                program: compiled,
                status: CacheStatus::Uncached,
            });
        }
        retained_versions.extend_from_slice(revisions);

        while self.entries.len() >= self.limits.max_entries
            || self.retained_source_bytes > self.limits.max_retained_source_bytes - source_bytes
        {
            let previous = self
                .entries
                .pop_front()
                .expect("positive capacity and charge imply an existing entry");
            self.retained_source_bytes -= previous.source_bytes;
            self.evictions = self.evictions.saturating_add(1);
        }

        self.retained_source_bytes += source_bytes;
        self.entries.push_back(Entry {
            program: compiled.clone(),
            revisions: retained_versions,
            modules: module_limits,
            constants: constant_limits,
            types: type_limits,
            source_bytes,
        });
        Ok(CachedProgram {
            program: compiled,
            status: CacheStatus::Stored,
        })
    }
}
