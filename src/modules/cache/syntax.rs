//! Caller-owned caching of successfully parsed source-relative ASTs.
//!
//! Import validation, cycle detection and semantic compilation are NOT cached.
//! Original source bytes and trusted loader identity are compared exactly.

use crate::{ast::{self, Program}, diagnostics::Span};
use std::collections::VecDeque;

/// Logical quotas, not a process-heap bound (AST strings, allocations, and
/// Vec capacities are not measured). A zero limit disables retention.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SyntaxCacheLimits {
    pub max_entries: usize,
    pub max_source_bytes: usize,
    pub max_identity_bytes: usize,
    pub max_ast_nodes: usize,
}
impl Default for SyntaxCacheLimits {
    fn default() -> Self {
        Self {
            max_entries: 128,
            max_source_bytes: 8 * 1024 * 1024,
            max_identity_bytes: 1024 * 1024,
            max_ast_nodes: 1_000_000,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SyntaxCacheStats {
    pub entries: usize,
    pub retained_source_bytes: usize,
    pub retained_identity_bytes: usize,
    pub retained_ast_nodes: usize,
    pub hits: u64,
    pub misses: u64,
    pub bypasses: u64,
    pub evictions: u64,
}
#[derive(Debug)]
struct SyntaxEntry {
    identity: String,
    source: String,
    program: Program,
    nodes: usize,
}

/// Bounded FIFO of successfully parsed module syntax trees.
/// On each hit, the tree is cloned and *all* source-relative syntax spans are
/// rebased to the newly assigned SourceMap offset. The original immutable
/// entry is not modified. This cache owns no loader and does no external I/O.
#[derive(Debug)]
pub struct SyntaxCache {
    limits: SyntaxCacheLimits,
    entries: VecDeque<SyntaxEntry>,
    bytes: usize,
    identities: usize,
    nodes: usize,
    hits: u64,
    misses: u64,
    bypasses: u64,
    evictions: u64,
}
impl SyntaxCache {
    pub fn new(limits: SyntaxCacheLimits) -> Self {
        Self {
            limits,
            entries: VecDeque::new(),
            bytes: 0,
            identities: 0,
            nodes: 0,
            hits: 0,
            misses: 0,
            bypasses: 0,
            evictions: 0,
        }
    }
    pub fn stats(&self) -> SyntaxCacheStats {
        SyntaxCacheStats {
            entries: self.entries.len(),
            retained_source_bytes: self.bytes,
            retained_identity_bytes: self.identities,
            retained_ast_nodes: self.nodes,
            hits: self.hits,
            misses: self.misses,
            bypasses: self.bypasses,
            evictions: self.evictions,
        }
    }
    /// Reset retained successful parses. Counters remain lifetime cumulative.
    pub fn clear(&mut self) {
        self.evictions = self.evictions.saturating_add(self.entries.len() as u64);
        self.entries.clear();
        self.bytes = 0;
        self.identities = 0;
        self.nodes = 0;
    }
    /// Return a cloned AST with all spans rebased, or record a miss.
    pub(crate) fn try_reuse(
        &mut self,
        identity: &str,
        source: &str,
        source_base: usize,
    ) -> Option<Program> {
        if let Some(old) = self
            .entries
            .iter()
            .find(|entry| entry.identity == identity && entry.source == source)
        {
            let mut result = old.program.clone();
            ast::relocate(&mut result, source_base, false);
            self.hits = self.hits.saturating_add(1);
            return Some(result);
        }
        self.misses = self.misses.saturating_add(1);
        None
    }
    /// Retain a successful *full parser result*; never called for lexer or
    /// parser failures. Current import validation always runs separately.
    pub(crate) fn store(
        &mut self,
        identity: &str,
        source: &str,
        source_base: usize,
        parsed_absolute: &Program,
    ) {
        if self.limits.max_entries == 0
            || source.len() > self.limits.max_source_bytes
            || identity.len() > self.limits.max_identity_bytes
        {
            self.bypasses = self.bypasses.saturating_add(1);
            return;
        }
        let mut local = parsed_absolute.clone();
        let node_count = ast::relocate(&mut local, source_base, true);
        if node_count > self.limits.max_ast_nodes {
            self.bypasses = self.bypasses.saturating_add(1);
            return;
        }
        if self.entries.try_reserve(1).is_err() {
            self.bypasses = self.bypasses.saturating_add(1);
            return;
        }
        while self.entries.len() >= self.limits.max_entries
            || self.bytes > self.limits.max_source_bytes - source.len()
            || self.identities > self.limits.max_identity_bytes - identity.len()
            || self.nodes > self.limits.max_ast_nodes - node_count
        {
            let old = self
                .entries
                .pop_front()
                .expect("positive entry capacity with fitting payload has an evictable item");
            self.bytes -= old.source.len();
            self.identities -= old.identity.len();
            self.nodes -= old.nodes;
            self.evictions = self.evictions.saturating_add(1);
        }
        self.bytes += source.len();
        self.identities += identity.len();
        self.nodes += node_count;
        self.entries.push_back(SyntaxEntry {
            identity: identity.to_owned(),
            source: source.to_owned(),
            program: local,
            nodes: node_count,
        });
    }
}
