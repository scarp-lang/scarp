//! Source-relative, bounded lexical token reuse.
//!
//! Never use cached absolute source-map spans. A fresh module discovery assigns
//! new byte bases, so all retained tokens use offsets local to their own file.
//! Parser and semantic validation still run even on a successful token hit.

use crate::{
    diagnostics::Diagnostic,
    lexer::{self, Token},
};
use std::collections::VecDeque;

/// Logical resource limits; neither source bytes nor token counts measure
/// allocator overhead or decoded token-string capacity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TokenCacheLimits {
    pub max_entries: usize,
    pub max_source_bytes: usize,
    pub max_tokens: usize,
}
impl Default for TokenCacheLimits {
    fn default() -> Self {
        Self {
            max_entries: 128,
            max_source_bytes: 8 * 1024 * 1024,
            max_tokens: 1_000_000,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TokenCacheStats {
    pub entries: usize,
    pub retained_source_bytes: usize,
    pub retained_tokens: usize,
    pub hits: u64,
    pub misses: u64,
    pub bypasses: u64,
    pub evictions: u64,
}

#[derive(Debug)]
struct TokenEntry {
    identity: String,
    source: String,
    tokens: Vec<Token>,
}

/// Explicit owner of successful lexing results, independently bounded by
/// source bytes, token count and file entries. Replacement is deterministic
/// FIFO; a hit never changes eviction order. No external I/O or global state.
///
/// An entry can be reused by a different trusted loader only when its source
/// identity AND exact source bytes match. Lexing is independent of canonical
/// root, file label, module graph depth, importer aliases and syntax budgets.
/// Reusing tokens does not skip any validation performed by later phases.
#[derive(Debug)]
pub struct TokenCache {
    limits: TokenCacheLimits,
    entries: VecDeque<TokenEntry>,
    bytes: usize,
    tokens: usize,
    hits: u64,
    misses: u64,
    bypasses: u64,
    evictions: u64,
}
impl TokenCache {
    pub fn new(limits: TokenCacheLimits) -> Self {
        Self {
            limits,
            entries: VecDeque::new(),
            bytes: 0,
            tokens: 0,
            hits: 0,
            misses: 0,
            bypasses: 0,
            evictions: 0,
        }
    }

    pub fn stats(&self) -> TokenCacheStats {
        TokenCacheStats {
            entries: self.entries.len(),
            retained_source_bytes: self.bytes,
            retained_tokens: self.tokens,
            hits: self.hits,
            misses: self.misses,
            bypasses: self.bypasses,
            evictions: self.evictions,
        }
    }

    pub fn clear(&mut self) {
        self.evictions = self.evictions.saturating_add(self.entries.len() as u64);
        self.entries.clear();
        self.bytes = 0;
        self.tokens = 0;
    }

    /// Return freshly owned, source-relative tokens. Failure diagnostics are
    /// also relative to this source and must be shifted by the caller.
    ///
    /// Lexical errors are NEVER cached. A syntactically invalid but lexically
    /// valid source may retain tokens, but still must run the parser anew.
    pub(super) fn lex(
        &mut self,
        identity: &str,
        source: &str,
    ) -> Result<Vec<Token>, Diagnostic> {
        if let Some(entry) = self
            .entries
            .iter()
            .find(|entry| entry.identity == identity && entry.source == source)
        {
            self.hits = self.hits.saturating_add(1);
            return Ok(entry.tokens.clone());
        }

        // No result is cached on lex errors; every invocation gets its own
        // compiler-provided failure location.
        let tokens = lexer::lex(source)?;
        self.misses = self.misses.saturating_add(1);

        if self.limits.max_entries == 0
            || source.len() > self.limits.max_source_bytes
            || tokens.len() > self.limits.max_tokens
        {
            self.bypasses = self.bypasses.saturating_add(1);
            return Ok(tokens);
        }

        // Failure to reserve a cache entry must not invalidate good source.
        if self.entries.try_reserve(1).is_err() {
            self.bypasses = self.bypasses.saturating_add(1);
            return Ok(tokens);
        }

        while self.entries.len() >= self.limits.max_entries
            || self.bytes > self.limits.max_source_bytes - source.len()
            || self.tokens > self.limits.max_tokens - tokens.len()
        {
            let old = self
                .entries
                .pop_front()
                .expect("nonzero limits and fitting source imply a removable entry");
            self.bytes -= old.source.len();
            self.tokens -= old.tokens.len();
            self.evictions = self.evictions.saturating_add(1);
        }

        self.bytes += source.len();
        self.tokens += tokens.len();
        self.entries.push_back(TokenEntry {
            identity: identity.to_owned(),
            source: source.to_owned(),
            tokens: tokens.clone(),
        });
        Ok(tokens)
    }
}
