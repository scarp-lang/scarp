//! Bounded reuse of scalar module-constant evaluation.

use crate::{
    limits::{Budget, BudgetAmounts},
    value::Value,
};
use std::{
    borrow::Cow,
    collections::{HashMap, VecDeque},
    hash::{Hash, Hasher},
    mem::{discriminant, size_of_val},
    sync::Arc,
};

/// Bounds for successful constant evaluations retained by a caller-owned cache.
///
/// Retained bytes count identity and declaration UTF-8, reference value slots,
/// and referenced and result string payloads. This is a logical payload quota.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConstantCacheLimits {
    pub max_entries: usize,
    pub max_retained_bytes: usize,
}

impl Default for ConstantCacheLimits {
    fn default() -> Self {
        Self {
            max_entries: 1024,
            max_retained_bytes: 4 * 1024 * 1024,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ConstantCacheStats {
    pub entries: usize,
    pub retained_bytes: usize,
    /// Evaluations reused after checking the current compilation budgets.
    pub hits: u64,
    pub misses: u64,
    /// Successful evaluations whose entries could not be retained.
    pub bypasses: u64,
    pub evictions: u64,
}

#[derive(Debug)]
struct Key<'a> {
    // Lookups borrow current compiler data. Retained keys own their data and
    // share it between the index and the FIFO.
    identity: Cow<'a, str>,
    declaration: Cow<'a, str>,
    references: Cow<'a, [Value]>,
}

impl PartialEq for Key<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.identity == other.identity
            && self.declaration == other.declaration
            && self.references.len() == other.references.len()
            && self
                .references
                .iter()
                .zip(other.references.iter())
                .all(|(left, right)| match (left, right) {
                    (Value::Float(left), Value::Float(right)) => {
                        left.get().to_bits() == right.get().to_bits()
                    }
                    _ => left == right,
                })
    }
}
impl Eq for Key<'_> {}
impl Hash for Key<'_> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.identity.hash(state);
        self.declaration.hash(state);
        self.references.len().hash(state);
        for value in self.references.iter() {
            discriminant(value).hash(state);
            match value {
                Value::Integer(value) => value.hash(state),
                Value::Float(value) => value.get().to_bits().hash(state),
                Value::Boolean(value) => value.hash(state),
                Value::String(value) => value.hash(state),
                _ => unreachable!("validated scalar constant"),
            }
        }
    }
}

#[derive(Debug)]
struct Entry {
    result: Value,
    spent: BudgetAmounts,
    bytes: usize,
}

/// FIFO of successful scalar evaluations, keyed by exact declaration bytes,
/// resolved loader identity and ordered, freshly resolved reference values.
///
/// Float references compare by bits so that the sign of zero is preserved.
/// Entries contain no source coordinates, symbol IDs or editor revisions.
/// Successful constant evaluations may be retained when a later compilation
/// stage fails. Current imports, visibility and references are checked again
/// on each compile, and a hit still spends its original evaluation budget.
#[derive(Debug)]
pub struct ConstantCache {
    limits: ConstantCacheLimits,
    entries: HashMap<Arc<Key<'static>>, Entry>,
    fifo: VecDeque<Arc<Key<'static>>>,
    stats: ConstantCacheStats,
}

impl ConstantCache {
    pub fn new(limits: ConstantCacheLimits) -> Self {
        Self {
            limits,
            entries: HashMap::new(),
            fifo: VecDeque::new(),
            stats: ConstantCacheStats::default(),
        }
    }

    pub fn stats(&self) -> ConstantCacheStats {
        ConstantCacheStats {
            entries: self.entries.len(),
            ..self.stats
        }
    }

    /// Discard all retained entries. Counters are lifetime cumulative.
    pub fn clear(&mut self) {
        self.stats.evictions = self
            .stats
            .evictions
            .saturating_add(self.entries.len() as u64);
        self.entries.clear();
        self.fifo.clear();
        self.stats.retained_bytes = 0;
    }

    /// Discard entries owned by this exact resolved loader identity.
    ///
    /// Other declarations still resolve and check their current reference
    /// values before reuse, including references to constants in this module.
    pub fn evict_identity(&mut self, identity: &str) -> usize {
        let old_len = self.entries.len();
        self.fifo.retain(|key| {
            if key.identity == identity {
                let entry = self.entries.remove(key).expect("FIFO key is indexed");
                self.stats.retained_bytes -= entry.bytes;
                false
            } else {
                true
            }
        });
        let removed = old_len - self.entries.len();
        self.stats.evictions = self.stats.evictions.saturating_add(removed as u64);
        removed
    }

    pub(in crate::modules) fn try_reuse(
        &mut self,
        identity: &str,
        declaration: &str,
        references: &[Value],
        budget: &mut Budget,
    ) -> Option<Value> {
        let key = Key {
            identity: Cow::Borrowed(identity),
            declaration: Cow::Borrowed(declaration),
            references: Cow::Borrowed(references),
        };
        let entries: &HashMap<Arc<Key<'_>>, Entry> = &self.entries;
        if let Some(entry) = entries.get(&key) {
            if budget.try_replay(entry.spent) {
                self.stats.hits = self.stats.hits.saturating_add(1);
                return Some(entry.result.clone());
            }
        }
        self.stats.misses = self.stats.misses.saturating_add(1);
        None
    }

    pub(in crate::modules) fn store(
        &mut self,
        identity: &str,
        declaration: &str,
        references: Vec<Value>,
        result: Value,
        spent: BudgetAmounts,
    ) {
        let bytes = identity
            .len()
            .checked_add(declaration.len())
            .and_then(|bytes| bytes.checked_add(size_of_val(references.as_slice())))
            .and_then(|bytes| {
                references
                    .iter()
                    .chain(std::iter::once(&result))
                    .try_fold(bytes, |total, value| {
                        total.checked_add(value.as_str().map_or(0, str::len))
                    })
            });
        let Some(bytes) = bytes.filter(|&bytes| bytes <= self.limits.max_retained_bytes) else {
            self.stats.bypasses = self.stats.bypasses.saturating_add(1);
            return;
        };

        let mut owned_identity = String::new();
        let mut owned_declaration = String::new();
        if self.limits.max_entries == 0
            || owned_identity.try_reserve_exact(identity.len()).is_err()
            || owned_declaration
                .try_reserve_exact(declaration.len())
                .is_err()
            || self.entries.try_reserve(1).is_err()
            || self.fifo.try_reserve(1).is_err()
        {
            self.stats.bypasses = self.stats.bypasses.saturating_add(1);
            return;
        }
        owned_identity.push_str(identity);
        owned_declaration.push_str(declaration);

        while self.entries.len() >= self.limits.max_entries
            || self.stats.retained_bytes > self.limits.max_retained_bytes - bytes
        {
            let key = self.fifo.pop_front().expect("a fitting entry can evict");
            let old = self.entries.remove(&key).expect("FIFO key is indexed");
            self.stats.retained_bytes -= old.bytes;
            self.stats.evictions = self.stats.evictions.saturating_add(1);
        }
        self.stats.retained_bytes += bytes;
        let key = Arc::new(Key {
            identity: Cow::Owned(owned_identity),
            declaration: Cow::Owned(owned_declaration),
            references: Cow::Owned(references),
        });
        let entry = Entry {
            result,
            spent,
            bytes,
        };
        let previous = self.entries.insert(key.clone(), entry);
        debug_assert!(previous.is_none(), "only newly evaluated keys are stored");
        self.fifo.push_back(key);
    }
}
