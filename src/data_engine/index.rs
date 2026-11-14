//! Caller-owned immutable typed indexes and deterministic equijoins.
//!
//! Indexes are built from verified immutable ListValue snapshots, never from
//! mutable external references. Query results retain the exact source datum
//! type and index row order. A new list requires a new index; silent stale
//! updates are not possible through this API.
use super::limits::{bounded, DataError, DataResource};
use super::paths::PathSegment;
use super::schema::{DataType, MapKeyType, SchemaRegistry};
use super::value::{DataKey, Datum, ListValue};
use crate::types::{PrimitiveType, SourceType};
use crate::value::Value;
use std::collections::BTreeMap;
use std::ops::Bound::{Included, Unbounded};
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IndexLimits {
    pub max_rows: usize,
    pub max_distinct_keys: usize,
    pub max_key_bytes: usize,
    pub max_join_pairs: usize,
}
impl Default for IndexLimits {
    fn default() -> Self {
        Self {
            max_rows: 4096,
            max_distinct_keys: 4096,
            max_key_bytes: 8 * 1024 * 1024,
            max_join_pairs: 65_536,
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexStats {
    pub indexed_rows: usize,
    pub distinct_keys: usize,
    pub key_text_bytes: usize,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexHit {
    pub key: DataKey,
    pub position: usize,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataIndex {
    source: ListValue,
    path: Arc<[PathSegment]>,
    key_type: MapKeyType,
    entries: BTreeMap<DataKey, Vec<usize>>,
    stats: IndexStats,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JoinRow {
    pub key: DataKey,
    pub left_position: usize,
    pub right_position: usize,
    pub left: Datum,
    pub right: Datum,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JoinResult {
    pub rows: Vec<JoinRow>,
    pub compared_keys: usize,
}
impl DataIndex {
    pub fn build(
        registry: &SchemaRegistry,
        source: &ListValue,
        path: Vec<PathSegment>,
        limits: IndexLimits,
    ) -> Result<Self, DataError> {
        bounded(source.len(), limits.max_rows, DataResource::IndexRows)?;
        let kind = registry.path_type(source.element_type(), &path)?;
        let key_type = match kind {
            DataType::Source(SourceType::Primitive(PrimitiveType::Integer)) => MapKeyType::Integer,
            DataType::Source(SourceType::Primitive(PrimitiveType::String)) => MapKeyType::Text,
            _ => return Err(DataError::InvalidKey),
        };
        registry.verify(
            &DataType::list(source.element_type().clone()),
            &Datum::List(source.clone()),
        )?;
        let mut entries: BTreeMap<DataKey, Vec<usize>> = BTreeMap::new();
        let mut text_bytes = 0usize;
        for (index, row) in source.values().iter().enumerate() {
            let selected = registry.select_path(source.element_type(), row, &path)?;
            let key = match selected {
                Datum::Source(Value::Integer(value)) => DataKey::Integer(*value),
                Datum::Source(Value::String(value)) => DataKey::Text(value.clone()),
                _ => return Err(DataError::InvalidKey),
            };
            // This bound counts each visible index entry's key bytes, even
            // if the same Arc is reused by the source or another index.
            if let DataKey::Text(value) = &key {
                text_bytes = text_bytes
                    .checked_add(value.len())
                    .ok_or(DataError::Limit {
                        resource: DataResource::IndexKeyBytes,
                        limit: limits.max_key_bytes,
                    })?;
                bounded(
                    text_bytes,
                    limits.max_key_bytes,
                    DataResource::IndexKeyBytes,
                )?;
            }
            if !entries.contains_key(&key) {
                bounded(
                    entries.len().saturating_add(1),
                    limits.max_distinct_keys,
                    DataResource::IndexKeys,
                )?;
            }
            entries.entry(key).or_default().push(index);
        }
        let stats = IndexStats {
            indexed_rows: source.len(),
            distinct_keys: entries.len(),
            key_text_bytes: text_bytes,
        };
        Ok(Self {
            source: source.clone(),
            path: path.into(),
            key_type,
            entries,
            stats,
        })
    }
    pub fn source(&self) -> &ListValue {
        &self.source
    }
    pub fn element_type(&self) -> &DataType {
        self.source.element_type()
    }
    pub fn key_type(&self) -> MapKeyType {
        self.key_type
    }
    pub fn path(&self) -> &[PathSegment] {
        &self.path
    }
    pub fn stats(&self) -> &IndexStats {
        &self.stats
    }
    pub fn unique_keys(&self) -> impl Iterator<Item = &DataKey> {
        self.entries.keys()
    }
    pub fn occurrences(&self, key: &DataKey) -> Result<&[usize], DataError> {
        if self.key_type != key.kind() {
            return Err(DataError::InvalidKey);
        }
        Ok(self.entries.get(key).map(Vec::as_slice).unwrap_or(&[]))
    }
    fn fetch(&self, key: &DataKey, index: usize) -> Result<IndexHit, DataError> {
        if index >= self.source.len() {
            return Err(DataError::InvalidShape);
        }
        Ok(IndexHit {
            key: key.clone(),
            position: index,
        })
    }
    /// Return indexed hits in the original source row order.
    pub fn find(&self, key: &DataKey) -> Result<Vec<IndexHit>, DataError> {
        let positions = self.occurrences(key)?;
        positions.iter().map(|&i| self.fetch(key, i)).collect()
    }
    /// Inclusive integer bounds. Unbounded ends are expressed as None.
    pub fn integer_range(
        &self,
        min: Option<i64>,
        max: Option<i64>,
    ) -> Result<Vec<IndexHit>, DataError> {
        if self.key_type != MapKeyType::Integer {
            return Err(DataError::InvalidKey);
        }
        if let (Some(a), Some(b)) = (min, max) {
            if a > b {
                return Err(DataError::InvalidShape);
            }
        }
        let lower = min
            .map(|value| Included(DataKey::Integer(value)))
            .unwrap_or(Unbounded);
        let upper = max
            .map(|value| Included(DataKey::Integer(value)))
            .unwrap_or(Unbounded);
        let mut result = Vec::new();
        for (key, indices) in self.entries.range((lower, upper)) {
            for &position in indices {
                result.push(self.fetch(key, position)?);
            }
        }
        Ok(result)
    }
    /// Prefix matching is UTF-8 byte-prefix matching and does not silently
    /// apply case folding, locale collation, grapheme logic or normalization.
    pub fn text_prefix(&self, prefix: &str) -> Result<Vec<IndexHit>, DataError> {
        if self.key_type != MapKeyType::Text {
            return Err(DataError::InvalidKey);
        }
        let mut result = Vec::new();
        for (key, indices) in self.entries.range(DataKey::text(prefix)..) {
            match key {
                DataKey::Text(text) if text.starts_with(prefix) => {
                    for &position in indices {
                        result.push(self.fetch(key, position)?);
                    }
                }
                _ => break,
            }
        }
        Ok(result)
    }
    /// Project index hits back into the immutable original row type.
    /// Duplicate requested hits are retained, respecting caller order.
    pub fn materialize(
        &self,
        registry: &SchemaRegistry,
        hits: &[IndexHit],
    ) -> Result<ListValue, DataError> {
        let mut values = Vec::new();
        bounded(
            hits.len(),
            registry.limits().max_collection_entries,
            DataResource::CollectionEntries,
        )?;
        values
            .try_reserve_exact(hits.len())
            .map_err(|_| DataError::InvalidShape)?;
        for hit in hits {
            let record = self
                .source
                .get(hit.position)
                .ok_or(DataError::InvalidShape)?;
            let actual = registry.select_path(self.source.element_type(), record, &self.path)?;
            let expected = match actual {
                Datum::Source(Value::Integer(i)) => DataKey::Integer(*i),
                Datum::Source(Value::String(s)) => DataKey::Text(s.clone()),
                _ => return Err(DataError::InvalidKey),
            };
            if hit.key != expected {
                return Err(DataError::InvalidKey);
            }
            values.push(record.clone());
        }
        registry.new_list(self.source.element_type().clone(), values)
    }
    /// Deterministic multi-match join. Results are grouped by ascending key,
    /// then by left original row, then right original row. No hash ordering
    /// and no hidden query optimizer or live mutating data source.
    pub fn equijoin(&self, other: &Self, limits: IndexLimits) -> Result<JoinResult, DataError> {
        if self.key_type != other.key_type {
            return Err(DataError::TypeMismatch);
        }
        let mut output = Vec::new();
        let mut compared = 0usize;
        for (key, left_rows) in &self.entries {
            compared = compared.checked_add(1).ok_or(DataError::Limit {
                resource: DataResource::IndexKeys,
                limit: limits.max_distinct_keys,
            })?;
            bounded(compared, limits.max_distinct_keys, DataResource::IndexKeys)?;
            let Some(right_rows) = other.entries.get(key) else {
                continue;
            };
            let extra = left_rows
                .len()
                .checked_mul(right_rows.len())
                .ok_or(DataError::Limit {
                    resource: DataResource::IndexPairs,
                    limit: limits.max_join_pairs,
                })?;
            bounded(
                output.len().saturating_add(extra),
                limits.max_join_pairs,
                DataResource::IndexPairs,
            )?;
            output.try_reserve(extra).map_err(|_| DataError::Limit {
                resource: DataResource::IndexPairs,
                limit: limits.max_join_pairs,
            })?;
            for &left in left_rows {
                for &right in right_rows {
                    output.push(JoinRow {
                        key: key.clone(),
                        left_position: left,
                        right_position: right,
                        left: self
                            .source
                            .get(left)
                            .ok_or(DataError::InvalidShape)?
                            .clone(),
                        right: other
                            .source
                            .get(right)
                            .ok_or(DataError::InvalidShape)?
                            .clone(),
                    });
                }
            }
        }
        Ok(JoinResult {
            rows: output,
            compared_keys: compared,
        })
    }
}
