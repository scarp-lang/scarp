//! Persistent immutable values for the experimental data engine.
use std::{cmp::Ordering, sync::Arc};
use crate::types::DefinitionId;
use crate::value::Value;
use super::schema::{DataType, MapKeyType};

/// Unlike an unrestricted hashmap key, keys have explicit semantics and order.
/// Exact integer order and UTF-8 byte order are deterministic across platforms.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum DataKey {
    Integer(i64),
    Text(Arc<str>),
}
impl Ord for DataKey {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self, other) {
            (Self::Integer(a), Self::Integer(b)) => a.cmp(b),
            (Self::Text(a), Self::Text(b)) => a.cmp(b),
            (Self::Integer(_), Self::Text(_)) => Ordering::Less,
            (Self::Text(_), Self::Integer(_)) => Ordering::Greater,
        }
    }
}
impl PartialOrd for DataKey {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> { Some(self.cmp(other)) }
}
impl DataKey {
    pub fn kind(&self) -> MapKeyType {
        match self { Self::Integer(_) => MapKeyType::Integer, Self::Text(_) => MapKeyType::Text }
    }
    pub fn text(s: impl Into<Arc<str>>) -> Self { Self::Text(s.into()) }
}

/// This enum is intentionally not grafted into the current source-language
/// Value enum. An experimental host datum must not accidentally become an
/// accepted .srp literal or a native-compiler ABI promise.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Datum {
    Source(Value),
    Record(RecordValue),
    Variant(VariantValue),
    List(ListValue),
    Map(MapValue),
    Optional(Option<Arc<Datum>>),
    Outcome(OutcomeValue),
}
impl Datum {
    pub fn integer(value: i64) -> Self { Self::Source(Value::Integer(value)) }
    pub fn boolean(value: bool) -> Self { Self::Source(Value::Boolean(value)) }
    pub fn text(value: impl Into<Arc<str>>) -> Self {
        Self::Source(Value::String(value.into()))
    }
    pub fn some(value: Self) -> Self { Self::Optional(Some(Arc::new(value))) }
    pub fn none() -> Self { Self::Optional(None) }
    pub fn ok(value: Self) -> Self { Self::Outcome(OutcomeValue::Ok(Arc::new(value))) }
    pub fn err(value: Self) -> Self { Self::Outcome(OutcomeValue::Err(Arc::new(value))) }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordValue {
    pub(super) identity: DefinitionId,
    pub(super) values: Arc<[Datum]>,
}
impl RecordValue {
    pub fn identity(&self) -> DefinitionId { self.identity }
    pub fn values(&self) -> &[Datum] { &self.values }
    pub fn len(&self) -> usize { self.values.len() }
    pub fn is_empty(&self) -> bool { self.values.is_empty() }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VariantValue {
    pub(super) identity: DefinitionId,
    pub(super) case_index: usize,
    pub(super) payload: Option<Arc<Datum>>,
}
impl VariantValue {
    pub fn identity(&self) -> DefinitionId { self.identity }
    pub fn case_index(&self) -> usize { self.case_index }
    pub fn payload(&self) -> Option<&Datum> { self.payload.as_deref() }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListValue {
    pub(super) element: DataType,
    pub(super) values: Arc<[Datum]>,
}
impl ListValue {
    pub fn element_type(&self) -> &DataType { &self.element }
    pub fn values(&self) -> &[Datum] { &self.values }
    pub fn len(&self) -> usize { self.values.len() }
    pub fn is_empty(&self) -> bool { self.values.is_empty() }
    pub fn get(&self, index: usize) -> Option<&Datum> { self.values.get(index) }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MapValue {
    pub(super) keys: MapKeyType,
    pub(super) element: DataType,
    pub(super) pairs: Arc<[(DataKey, Datum)]>,
}
impl MapValue {
    pub fn key_type(&self) -> MapKeyType { self.keys }
    pub fn element_type(&self) -> &DataType { &self.element }
    pub fn entries(&self) -> &[(DataKey,Datum)] { &self.pairs }
    pub fn len(&self) -> usize { self.pairs.len() }
    pub fn is_empty(&self) -> bool { self.pairs.is_empty() }
    pub fn get(&self, key: &DataKey) -> Option<&Datum> {
        self.pairs.binary_search_by(|(entry,_)| entry.cmp(key))
            .ok().map(|index| &self.pairs[index].1)
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutcomeValue { Ok(Arc<Datum>), Err(Arc<Datum>) }
impl OutcomeValue {
    pub fn is_ok(&self) -> bool { matches!(self, Self::Ok(_)) }
    pub fn payload(&self) -> &Datum {
        match self { Self::Ok(v) | Self::Err(v) => v }
    }
}
