//! Opt-in caller-owned cache of successful whole-graph semantic compilation.
//!
//! The default cache entry point still loads, lexes, parses and validates
//! the entire graph through the trusted SourceLoader. Optional front-end
//! caches can skip lexing/parsing on exact per-source hits, but the loader
//! and graph validation always run. [`CompilationCache`] reuses a program
//! when every source byte, display label, resolved per-import target and
//! caller-owned revision matches exactly. [`EditorCompilationCache`] can
//! bind new revisions to a verified, byte-identical graph and editor index.
//!
//! Opt-in caches also cover tokens, syntax trees, constant evaluation and editor snapshots.

use super::invalidation::{EditImpact, InvalidationError, InvalidationSnapshot};
use super::{
    load, load_with_frontend_caches, load_with_syntax_cache, load_with_token_cache,
    resolve_with_compiler_limits, ConstantLimits, ModuleDiagnostic, ModuleGraph, ModuleLimits,
    ModuleProgram, SourceLoader,
};
use crate::types::TypeLimits;
use std::{collections::VecDeque, error::Error, fmt, sync::Arc};

mod constants;
mod editor;
mod syntax;
mod tokens;
pub use constants::{ConstantCache, ConstantCacheLimits, ConstantCacheStats};
pub use editor::{
    CachedEditorProgram, EditorCacheLimits, EditorCacheStats, EditorCompilationCache,
    EditorCompileError, EditorCompileLimits,
};
pub use syntax::{SyntaxCache, SyntaxCacheLimits, SyntaxCacheStats};
pub use tokens::{TokenCache, TokenCacheLimits, TokenCacheStats};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CachedCompileOptions<'a> {
    pub module_limits: ModuleLimits,
    pub constant_limits: ConstantLimits,
    pub type_limits: TypeLimits,
    /// Revisions in freshly discovered SourceId order.
    pub revisions: &'a [u64],
}
impl<'a> CachedCompileOptions<'a> {
    pub fn new(revisions: &'a [u64]) -> Self {
        Self {
            module_limits: ModuleLimits::default(),
            constant_limits: ConstantLimits::default(),
            type_limits: TypeLimits::default(),
            revisions,
        }
    }
}

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
    /// The current graph was validated and a prior compilation result reused.
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

        same_source_graph(&self.program, graph)
    }
}

fn same_source_graph(program: &ModuleProgram, graph: &ModuleGraph) -> bool {
    let original_sources = program.sources();
    if original_sources.entry != graph.sources.entry
        || original_sources.files().len() != graph.sources.files().len()
        || program.modules().len() != graph.modules.len()
    {
        return false;
    }

    for (id, module) in graph.modules.iter().enumerate() {
        let old = &program.modules()[id];
        if old.identity != module.identity
            || old.source != id
            || original_sources.files()[id].name != graph.sources.files()[id].name
            || original_sources.source(id) != graph.sources.source(id)
        {
            return false;
        }

        // Per-import targets matter even when the unique dependency sets agree.
        let Some(old_imports) = program.imports_in(id) else {
            return false;
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

    /// Evict every cached *whole-graph* success containing this resolved
    /// loader identity, including when it is an imported dependency.
    ///
    /// Only cache-owned references are removed. Already returned
    /// `Arc<ModuleProgram>` values remain valid and may keep memory alive.
    /// This is neither a filesystem operation nor secure memory erasure.
    /// A miss has no effect on FIFO order or lifetime hit/miss counters.
    pub fn evict_identity(&mut self, identity: &str) -> usize {
        let mut removed = 0usize;
        let mut freed_bytes = 0usize;
        self.entries.retain(|entry| {
            let affected = entry
                .program
                .modules()
                .iter()
                .any(|module| module.identity == identity);
            if affected {
                removed += 1;
                freed_bytes += entry.source_bytes;
            }
            !affected
        });
        self.retained_source_bytes -= freed_bytes;
        self.evictions = self.evictions.saturating_add(removed as u64);
        removed
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
        self.compile_graph(
            load(entry, loader, module_limits)?,
            module_limits,
            constant_limits,
            type_limits,
            revisions,
        )
    }

    /// Compile with independently owned lexical token caching and the existing
    /// whole-graph semantic cache. Both caches are explicit caller-owned state.
    /// Exact source bytes are loaded first; reused token spans are source-local
    /// and are shifted by the fresh graph's source-map base.
    ///
    /// This changes no language behavior and still reparses each source.
    pub fn compile_with_token_cache(
        &mut self,
        entry: &str,
        loader: &dyn SourceLoader,
        options: CachedCompileOptions<'_>,
        tokens: &mut TokenCache,
    ) -> Result<CachedProgram, CachedCompileError> {
        self.compile_graph(
            load_with_token_cache(entry, loader, options.module_limits, tokens)?,
            options.module_limits,
            options.constant_limits,
            options.type_limits,
            options.revisions,
        )
    }

    /// Compile using source-relative parsed AST reuse and regular whole-graph
    /// semantic caching. The source loader, import graph checks, and linker
    /// validity gates still run on every request.
    pub fn compile_with_syntax_cache(
        &mut self,
        entry: &str,
        loader: &dyn SourceLoader,
        options: CachedCompileOptions<'_>,
        syntax: &mut SyntaxCache,
    ) -> Result<CachedProgram, CachedCompileError> {
        self.compile_graph(
            load_with_syntax_cache(entry, loader, options.module_limits, syntax)?,
            options.module_limits,
            options.constant_limits,
            options.type_limits,
            options.revisions,
        )
    }

    /// Combine independently bounded source-relative token, syntax, and
    /// whole-graph semantic caches in the standard compiler path.
    pub fn compile_with_frontend_caches(
        &mut self,
        entry: &str,
        loader: &dyn SourceLoader,
        options: CachedCompileOptions<'_>,
        tokens: &mut TokenCache,
        syntax: &mut SyntaxCache,
    ) -> Result<CachedProgram, CachedCompileError> {
        self.compile_graph(
            load_with_frontend_caches(entry, loader, options.module_limits, tokens, syntax)?,
            options.module_limits,
            options.constant_limits,
            options.type_limits,
            options.revisions,
        )
    }

    fn compile_graph(
        &mut self,
        graph: ModuleGraph,
        module_limits: ModuleLimits,
        constant_limits: ConstantLimits,
        type_limits: TypeLimits,
        revisions: &[u64],
    ) -> Result<CachedProgram, CachedCompileError> {
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

/// Number of cache-owned entries evicted in each independent cache tier.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SourceCacheEvictions {
    pub semantic_entries: usize,
    pub token_entries: usize,
    pub syntax_entries: usize,
}

/// Explicitly evict one trusted, resolved loader identity across all three
/// cache tiers. Matching semantic entries are conservatively dropped when
/// *any* module in their compiled graph has this identity; unrelated graphs
/// and source-relative front-end results remain retained in FIFO order.
///
/// This is optional retention management, not an incremental compiler
/// shortcut: every later compile still reloads sources and validates the
/// current import graph. An absent identity is a no-op; human-facing display
/// paths and unresolved import spellings are not cache identity keys.
///
/// Eviction does not revoke or erase already returned `Arc<ModuleProgram>`
/// values, and it does not guarantee process memory is wiped or freed.
pub fn purge_source_identity(
    identity: &str,
    semantic: &mut CompilationCache,
    tokens: &mut TokenCache,
    syntax: &mut SyntaxCache,
) -> SourceCacheEvictions {
    SourceCacheEvictions {
        semantic_entries: semantic.evict_identity(identity),
        token_entries: tokens.evict_identity(identity),
        syntax_entries: syntax.evict_identity(identity),
    }
}

/// The dependency planner's complete preflight result and resulting cache
/// retention changes. SourceIds in `impact` refer only to the supplied
/// original snapshot, never to a later compiler graph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedCachePurge {
    pub impact: EditImpact,
    pub removed: SourceCacheEvictions,
}

/// Validate an entire batch of reported edits against a previously compiled
/// dependency graph **before modifying any cache**.
///
/// The caller supplies trusted, resolved loader identities, not display
/// names or unresolved import literals. Known edits remove semantic results
/// containing any old-graph affected importer, while token and syntax cache
/// entries are removed only for the exact reported identities. Unrelated
/// front-end sources and whole-graph programs remain cached.
///
/// An unknown identity can introduce a new import or change a loader's
/// canonical mapping, so *all three* caches are conservatively cleared.
/// Failed edit preflight (including budgets) leaves every cache untouched.
/// An empty input is a no-op and never certifies that source files are
/// unchanged. Any nonempty edit plan still needs a fresh trusted loader,
/// graph discovery, parse and semantic validation; this is retention
/// management, **not** incremental semantic linking or stale-result recovery.
///
/// The returned counts describe cache-owned entries only. Existing shared
/// `Arc<ModuleProgram>` results remain live and are not securely erased.
pub fn purge_reported_edits(
    snapshot: &InvalidationSnapshot,
    changed: &[&str],
    semantic: &mut CompilationCache,
    tokens: &mut TokenCache,
    syntax: &mut SyntaxCache,
) -> Result<PlannedCachePurge, InvalidationError> {
    // This call enforces the COMPLETE edit-count and aggregate identity-byte
    // quotas before the first mutation, even when the first name is unknown.
    let impact = snapshot.plan_edits(changed)?;
    let mut removed = SourceCacheEvictions::default();

    if impact.unknown_identity {
        removed = SourceCacheEvictions {
            semantic_entries: semantic.stats().entries,
            token_entries: tokens.stats().entries,
            syntax_entries: syntax.stats().entries,
        };
        semantic.clear();
        tokens.clear();
        syntax.clear();
    } else if !changed.is_empty() {
        // Old SourceIds are scoped to this exact snapshot; do not compare
        // them to IDs in cache entries created for other discovery orders.
        // Instead, use the loader identities owned by the old snapshot.
        for &id in &impact.affected {
            if let Some(identity) = snapshot.identity(id) {
                removed.semantic_entries += semantic.evict_identity(identity);
            }
        }
        // A changed source does not make its unchanged importers' original
        // token/AST bytes invalid. Preserve those independently keyed caches.
        for (index, identity) in changed.iter().enumerate() {
            if !changed[..index].contains(identity) {
                removed.token_entries += tokens.evict_identity(identity);
                removed.syntax_entries += syntax.evict_identity(identity);
            }
        }
    }

    Ok(PlannedCachePurge { impact, removed })
}
