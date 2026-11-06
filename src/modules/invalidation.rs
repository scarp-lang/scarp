//! Conservative dependency invalidation over previously validated module graphs.
//!
//! This is an opt-in planning API, not an incremental compiler or a cache.
//! Source identities come from the compiler's trusted loader. No filesystem or
//! loader access occurs here; only callers can establish whether a file changed.

use super::{ModuleProgram, SourceId, SourceMap};
use std::{collections::HashSet, error::Error, fmt, sync::Arc};

/// Independent limits for retained invalidation metadata and preflight edits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidationLimits {
    pub max_files: usize,
    pub max_edges: usize,
    pub max_identity_bytes: usize,
    pub max_changed_files: usize,
}
impl Default for InvalidationLimits {
    fn default() -> Self {
        Self {
            max_files: 128,
            max_edges: 1024,
            max_identity_bytes: 1024 * 1024,
            max_changed_files: 128,
        }
    }
}

/// Structural limits are checked before exposing any partially built snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InvalidationError {
    FileLimit,
    EdgeLimit,
    IdentityLimit,
    EditLimit,
    DuplicateIdentity,
    InvalidGraph,
    Allocation,
}
impl fmt::Display for InvalidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FileLimit => f.write_str("invalidation file count limit exceeded"),
            Self::EdgeLimit => f.write_str("invalidation import edge limit exceeded"),
            Self::IdentityLimit => f.write_str("invalidation identity byte limit exceeded"),
            Self::EditLimit => f.write_str("invalidation changed file limit exceeded"),
            Self::DuplicateIdentity => f.write_str("duplicate loader identity in module graph"),
            Self::InvalidGraph => f.write_str("invalid compiler module graph metadata"),
            Self::Allocation => f.write_str("could not reserve invalidation metadata"),
        }
    }
}
impl Error for InvalidationError {}

/// A preflight result, expressed in SourceIds of the ORIGINAL graph.
///
/// For every nonempty edit list the loader must still rediscover and revalidate
/// the graph; newly introduced imports, deletions or invalid code cannot be
/// inferred from the previous source graph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditImpact {
    pub affected: Vec<SourceId>,
    pub unknown_identity: bool,
    pub must_rediscover: bool,
}

/// Compare two successful, independently compiled graphs using exact contents
/// and resolved loader identities, never mtimes or unstable SourceIds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransitionImpact {
    /// Removed files in old-graph discovery order.
    pub removed_old: Vec<SourceId>,
    /// Newly reachable files in new-graph discovery order.
    pub added_new: Vec<SourceId>,
    /// Same-identity files whose source or import target changed.
    pub modified_old: Vec<SourceId>,
    pub modified_new: Vec<SourceId>,
    /// Changed roots and all their transitive importers in each graph.
    pub affected_old: Vec<SourceId>,
    pub affected_new: Vec<SourceId>,
}

/// Immutable dependency metadata referring to exactly one compiled source graph.
///
/// SourceId and SymbolId are not stable across graph rebuilds. Keep this
/// snapshot scoped to its original compilation and compare canonical loader
/// identities to bridge versions. Clones share the original validated text.
#[derive(Debug, Clone)]
pub struct InvalidationSnapshot {
    identities: Vec<String>,
    sources: Arc<SourceMap>,
    forward: Vec<Vec<SourceId>>,
    reverse: Vec<Vec<SourceId>>,
    limits: InvalidationLimits,
}

impl InvalidationSnapshot {
    /// Extract the compiler's own dependency edges. No secondary parser,
    /// import resolver, path normalization, or external I/O is performed.
    pub fn new(
        program: &ModuleProgram,
        limits: InvalidationLimits,
    ) -> Result<Self, InvalidationError> {
        let files = program.modules().len();
        if files > limits.max_files {
            return Err(InvalidationError::FileLimit);
        }
        if program.sources().files().len() != files {
            return Err(InvalidationError::InvalidGraph);
        }
        let mut identities = Vec::new();
        let mut forward = Vec::new();
        let mut reverse = Vec::new();
        identities
            .try_reserve(files)
            .map_err(|_| InvalidationError::Allocation)?;
        forward
            .try_reserve(files)
            .map_err(|_| InvalidationError::Allocation)?;
        reverse
            .try_reserve(files)
            .map_err(|_| InvalidationError::Allocation)?;

        let mut seen = HashSet::new();
        let mut identity_bytes = 0usize;
        let mut total_edges = 0usize;
        for (id, module) in program.modules().iter().enumerate() {
            if module.source != id || !seen.insert(module.identity.as_str()) {
                return Err(InvalidationError::DuplicateIdentity);
            }
            identity_bytes = identity_bytes
                .checked_add(module.identity.len())
                .ok_or(InvalidationError::IdentityLimit)?;
            if identity_bytes > limits.max_identity_bytes {
                return Err(InvalidationError::IdentityLimit);
            }
            let targets = program
                .dependencies(id)
                .ok_or(InvalidationError::InvalidGraph)?;
            let importers = program
                .dependents(id)
                .ok_or(InvalidationError::InvalidGraph)?;
            total_edges = total_edges
                .checked_add(targets.len())
                .ok_or(InvalidationError::EdgeLimit)?;
            if total_edges > limits.max_edges {
                return Err(InvalidationError::EdgeLimit);
            }
            if targets.iter().any(|&target| target >= files)
                || importers.iter().any(|&source| source >= files)
            {
                return Err(InvalidationError::InvalidGraph);
            }
            identities.push(module.identity.clone());
            forward.push(targets.to_vec());
            reverse.push(importers.to_vec());
        }

        Ok(Self {
            identities,
            sources: program.sources().clone(),
            forward,
            reverse,
            limits,
        })
    }

    pub fn file_count(&self) -> usize {
        self.identities.len()
    }

    /// Compiler-resolved source identity (not a human-facing display name).
    pub fn identity(&self, source: SourceId) -> Option<&str> {
        self.identities.get(source).map(String::as_str)
    }

    fn id_for(&self, identity: &str) -> Option<SourceId> {
        self.identities.iter().position(|name| name == identity)
    }

    /// Preflight edits using the old graph's identity space. Unknown
    /// identities invalidate the entire graph: they may be new imports,
    /// deleted imports or paths whose loader identity has changed.
    ///
    /// Passing no changes is an assertion from the caller, not a check
    /// that files are unchanged. It never authorizes a compiler cache hit.
    pub fn plan_edits(&self, changed: &[&str]) -> Result<EditImpact, InvalidationError> {
        if changed.len() > self.limits.max_changed_files {
            return Err(InvalidationError::EditLimit);
        }
        let mut bytes = 0usize;
        let mut roots = Vec::new();
        for identity in changed {
            bytes = bytes
                .checked_add(identity.len())
                .ok_or(InvalidationError::IdentityLimit)?;
            if bytes > self.limits.max_identity_bytes {
                return Err(InvalidationError::IdentityLimit);
            }
            match self.id_for(identity) {
                Some(id) => roots.push(id),
                None => {
                    return Ok(EditImpact {
                        affected: (0..self.file_count()).collect(),
                        unknown_identity: true,
                        must_rediscover: true,
                    });
                }
            }
        }
        Ok(EditImpact {
            affected: self.closure(&roots),
            unknown_identity: false,
            must_rediscover: !changed.is_empty(),
        })
    }

    /// Exact source-content and resolved-target comparison between successful
    /// compilations. A changed importer is a root even when its original source
    /// bytes are equal but a loader now maps an import to another target.
    ///
    /// This validates dependency impact, not eligibility to reuse a compiled
    /// program: compiler options, source revisions, successful parse/type-check,
    /// SourceId renumbering and diagnostic locations still need their own gate.
    pub fn compare(&self, next: &Self) -> TransitionImpact {
        let mut removed_old = Vec::new();
        let mut added_new = Vec::new();
        let mut modified_old = Vec::new();
        let mut modified_new = Vec::new();

        for (old, identity) in self.identities.iter().enumerate() {
            let Some(new) = next.id_for(identity) else {
                removed_old.push(old);
                continue;
            };
            let changed_text = self.sources.source(old) != next.sources.source(new);
            let targets_changed = self.forward[old].len() != next.forward[new].len()
                || self.forward[old]
                    .iter()
                    .zip(&next.forward[new])
                    .any(|(&left, &right)| self.identities[left] != next.identities[right]);
            if changed_text || targets_changed {
                modified_old.push(old);
                modified_new.push(new);
            }
        }
        for (new, identity) in next.identities.iter().enumerate() {
            if self.id_for(identity).is_none() {
                added_new.push(new);
            }
        }
        let affected_old = self.closure(
            &removed_old
                .iter()
                .chain(&modified_old)
                .copied()
                .collect::<Vec<_>>(),
        );
        let affected_new = next.closure(
            &added_new
                .iter()
                .chain(&modified_new)
                .copied()
                .collect::<Vec<_>>(),
        );
        TransitionImpact {
            removed_old,
            added_new,
            modified_old,
            modified_new,
            affected_old,
            affected_new,
        }
    }

    /// Sorted, deduplicated transitive importer closure, including roots.
    fn closure(&self, roots: &[SourceId]) -> Vec<SourceId> {
        let mut dirty = vec![false; self.file_count()];
        let mut pending = roots.to_vec();
        while let Some(id) = pending.pop() {
            if dirty[id] {
                continue;
            }
            dirty[id] = true;
            pending.extend(
                self.reverse[id]
                    .iter()
                    .copied()
                    .filter(|&next| !dirty[next]),
            );
        }
        dirty
            .into_iter()
            .enumerate()
            .filter_map(|(id, affected)| affected.then_some(id))
            .collect()
    }
}
