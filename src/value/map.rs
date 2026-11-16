//! Ordered immutable typed source maps. Keys are integer, boolean or string.
//! Ordering is canonical by key value, independent of insertion order.
use super::{
    tuple::{display_nested, value_capture, value_work},
    Value,
};
use crate::types::{SourceType, MAX_MAP_ENTRIES};
use std::{cmp::Ordering, fmt, mem::size_of, sync::Arc};

#[derive(Debug, Clone)]
pub struct MapValue(Arc<MapStorage>);

#[derive(Debug)]
struct MapStorage {
    kind: SourceType,
    entries: Vec<(Value, Value)>,
    work: u64,
    capture: usize,
}

/// Only call after the resolver has validated that both operands share one of
/// the three supported scalar key types. String ordering is UTF-8 lexical.
pub(crate) fn key_compare(left: &Value, right: &Value) -> Ordering {
    match (left, right) {
        (Value::Integer(a), Value::Integer(b)) => a.cmp(b),
        (Value::Boolean(a), Value::Boolean(b)) => a.cmp(b),
        (Value::String(a), Value::String(b)) => a.cmp(b),
        _ => unreachable!("map key type checked by resolver"),
    }
}

/// Charge comparison work in units of 64 UTF-8 bytes. All values are already
/// bounded by the existing one-megabyte string limit.
pub(crate) fn key_work(left: &Value, right: &Value) -> u64 {
    let bytes = match (left, right) {
        (Value::String(a), Value::String(b)) => a.len().saturating_add(b.len()),
        _ => 0,
    };
    1u64.saturating_add((bytes as u64).saturating_add(63) / 64)
}

impl MapValue {
    pub fn kind(&self) -> &SourceType {
        &self.0.kind
    }

    pub fn entries(&self) -> &[(Value, Value)] {
        &self.0.entries
    }

    pub fn len(&self) -> usize {
        self.0.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.entries.is_empty()
    }

    pub fn key_type(&self) -> &SourceType {
        self.kind().as_map().expect("typed map").key()
    }

    pub fn value_type(&self) -> &SourceType {
        self.kind().as_map().expect("typed map").value()
    }

    pub(crate) fn from_checked(kind: SourceType, entries: Vec<(Value, Value)>) -> Self {
        debug_assert!(kind.as_map().is_some());
        debug_assert!(entries.len() <= MAX_MAP_ENTRIES);
        debug_assert!(entries.windows(2).all(|w| key_compare(&w[0].0, &w[1].0).is_lt()));
        let mut work = kind.node_count() as u64 + 8;
        let mut capture = kind.display_bytes() + 8;
        for (key, value) in &entries {
            work = work
                .saturating_add(value_work(key))
                .saturating_add(value_work(value));
            capture = capture
                .saturating_add(value_capture(key))
                .saturating_add(value_capture(value))
                .saturating_add(size_of::<(Value, Value)>());
        }
        Self(Arc::new(MapStorage {
            kind,
            entries,
            work,
            capture,
        }))
    }

    pub(crate) fn allocation_bytes(
        key: &SourceType,
        value: &SourceType,
        length: usize,
    ) -> Option<usize> {
        if length > MAX_MAP_ENTRIES {
            return None;
        }
        key.node_count()
            .checked_add(value.node_count())?
            .checked_add(1)?
            .checked_mul(size_of::<SourceType>())?
            .checked_add(length.checked_mul(size_of::<(Value, Value)>())?)?
            .checked_add(size_of::<MapStorage>())
    }

    pub(crate) fn work(&self) -> u64 {
        self.0.work
    }

    pub(crate) fn capture_bytes(&self) -> usize {
        self.0.capture
    }
}

impl PartialEq for MapValue {
    fn eq(&self, other: &Self) -> bool {
        self.kind() == other.kind() && self.entries() == other.entries()
    }
}
impl Eq for MapValue {}

impl fmt::Display for MapValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("[map: ")?;
        if self.is_empty() {
            write!(f, "<{}, {}>", self.key_type(), self.value_type())?;
        } else {
            for (index, (key, value)) in self.entries().iter().enumerate() {
                if index > 0 {
                    f.write_str(", ")?;
                }
                display_nested(key, f)?;
                f.write_str(": ")?;
                display_nested(value, f)?;
            }
        }
        f.write_str("]")
    }
}
