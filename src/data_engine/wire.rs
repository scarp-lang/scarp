//! Versioned, schema-bound binary encoding for experimental host-side data.
//!
//! This is a deterministic *logical value* format, not a stable type ABI,
//! native object format or cross-process serialization of DefinitionId.
//! Both sides must provide the same ordered, versioned schema explicitly.
//! Malformed and oversized inputs are rejected before an external side effect.
use super::limits::{bounded, DataError, DataResource};
use super::schema::{DataType, MapKeyType, SchemaRegistry};
use super::value::{DataKey, Datum, OutcomeValue};
use crate::types::{PrimitiveType, SourceType};
use crate::value::{ArrayValue, FiniteFloat, TupleValue, Value};
use std::sync::Arc;

const MAGIC: &[u8; 5] = b"SCDE\x01";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WireLimits {
    pub max_bytes: usize,
    pub max_nodes: usize,
    pub max_depth: usize,
}
impl Default for WireLimits {
    fn default() -> Self {
        Self {
            max_bytes: 8 * 1024 * 1024,
            max_nodes: 65_536,
            max_depth: 64,
        }
    }
}

struct Writer {
    bytes: Vec<u8>,
    limits: WireLimits,
    nodes: usize,
}
impl Writer {
    fn new(limits: WireLimits) -> Self {
        Self {
            bytes: Vec::new(),
            limits,
            nodes: 0,
        }
    }
    fn node(&mut self, depth: usize) -> Result<(), DataError> {
        bounded(depth, self.limits.max_depth, DataResource::WireDepth)?;
        self.nodes = self.nodes.checked_add(1).ok_or(DataError::Limit {
            resource: DataResource::WireNodes,
            limit: self.limits.max_nodes,
        })?;
        bounded(self.nodes, self.limits.max_nodes, DataResource::WireNodes)
    }
    fn append(&mut self, data: &[u8]) -> Result<(), DataError> {
        let total = self
            .bytes
            .len()
            .checked_add(data.len())
            .ok_or(DataError::Limit {
                resource: DataResource::WireBytes,
                limit: self.limits.max_bytes,
            })?;
        bounded(total, self.limits.max_bytes, DataResource::WireBytes)?;
        self.bytes
            .try_reserve(data.len())
            .map_err(|_| DataError::Limit {
                resource: DataResource::WireBytes,
                limit: self.limits.max_bytes,
            })?;
        self.bytes.extend_from_slice(data);
        Ok(())
    }
    fn byte(&mut self, n: u8) -> Result<(), DataError> {
        self.append(&[n])
    }
    fn integer(&mut self, n: i64) -> Result<(), DataError> {
        self.append(&n.to_le_bytes())
    }
    fn u64(&mut self, n: u64) -> Result<(), DataError> {
        self.append(&n.to_le_bytes())
    }
    fn length(&mut self, n: usize) -> Result<(), DataError> {
        let len = u32::try_from(n).map_err(|_| DataError::InvalidWire)?;
        self.append(&len.to_le_bytes())
    }
    fn text(&mut self, s: &str) -> Result<(), DataError> {
        self.length(s.len())?;
        self.append(s.as_bytes())
    }
}
struct Reader<'a> {
    bytes: &'a [u8],
    offset: usize,
    limits: WireLimits,
    nodes: usize,
}
impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8], limits: WireLimits) -> Result<Self, DataError> {
        bounded(bytes.len(), limits.max_bytes, DataResource::WireBytes)?;
        Ok(Self {
            bytes,
            offset: 0,
            limits,
            nodes: 0,
        })
    }
    fn node(&mut self, depth: usize) -> Result<(), DataError> {
        bounded(depth, self.limits.max_depth, DataResource::WireDepth)?;
        self.nodes = self.nodes.checked_add(1).ok_or(DataError::Limit {
            resource: DataResource::WireNodes,
            limit: self.limits.max_nodes,
        })?;
        bounded(self.nodes, self.limits.max_nodes, DataResource::WireNodes)
    }
    fn take(&mut self, n: usize) -> Result<&'a [u8], DataError> {
        let end = self.offset.checked_add(n).ok_or(DataError::InvalidWire)?;
        let out = self
            .bytes
            .get(self.offset..end)
            .ok_or(DataError::InvalidWire)?;
        self.offset = end;
        Ok(out)
    }
    fn byte(&mut self) -> Result<u8, DataError> {
        Ok(self.take(1)?[0])
    }
    fn integer(&mut self) -> Result<i64, DataError> {
        let mut arr = [0; 8];
        arr.copy_from_slice(self.take(8)?);
        Ok(i64::from_le_bytes(arr))
    }
    fn u64(&mut self) -> Result<u64, DataError> {
        let mut arr = [0; 8];
        arr.copy_from_slice(self.take(8)?);
        Ok(u64::from_le_bytes(arr))
    }
    fn length(&mut self) -> Result<usize, DataError> {
        let mut arr = [0; 4];
        arr.copy_from_slice(self.take(4)?);
        Ok(u32::from_le_bytes(arr) as usize)
    }
    fn text(&mut self, max_text: usize) -> Result<Arc<str>, DataError> {
        let length = self.length()?;
        bounded(length, max_text, DataResource::TextBytes)?;
        let text = std::str::from_utf8(self.take(length)?).map_err(|_| DataError::InvalidWire)?;
        Ok(Arc::from(text))
    }
    fn ended(&self) -> bool {
        self.offset == self.bytes.len()
    }
}
fn encode_source(
    writer: &mut Writer,
    ty: &SourceType,
    val: &Value,
    depth: usize,
) -> Result<(), DataError> {
    writer.node(depth)?;
    match (ty, val) {
        (SourceType::Primitive(PrimitiveType::Integer), Value::Integer(i)) => writer.integer(*i),
        (SourceType::Primitive(PrimitiveType::Float), Value::Float(f)) => {
            writer.u64(f.get().to_bits())
        }
        (SourceType::Primitive(PrimitiveType::Boolean), Value::Boolean(b)) => {
            writer.byte(u8::from(*b))
        }
        (SourceType::Primitive(PrimitiveType::String), Value::String(s)) => writer.text(s),
        (SourceType::Tuple(ty), Value::Tuple(value)) => {
            let parts = ty.elements();
            if parts.len() != value.elements().len() {
                return Err(DataError::InvalidWire);
            }
            for (a, b) in parts.iter().zip(value.elements()) {
                encode_source(writer, a, b, depth + 1)?;
            }
            Ok(())
        }
        (SourceType::Array(ty), Value::Array(value)) => {
            if ty.len() != value.len() {
                return Err(DataError::InvalidWire);
            }
            for item in value.elements() {
                encode_source(writer, ty.element(), item, depth + 1)?;
            }
            Ok(())
        }
        _ => Err(DataError::UnsupportedWireType),
    }
}
fn decode_source(
    reader: &mut Reader<'_>,
    ty: &SourceType,
    depth: usize,
    max_text: usize,
) -> Result<Value, DataError> {
    reader.node(depth)?;
    match ty {
        SourceType::Primitive(PrimitiveType::Integer) => Ok(Value::Integer(reader.integer()?)),
        SourceType::Primitive(PrimitiveType::Float) => {
            let raw = f64::from_bits(reader.u64()?);
            let value = FiniteFloat::new(raw).ok_or(DataError::InvalidWire)?;
            Ok(Value::Float(value))
        }
        SourceType::Primitive(PrimitiveType::Boolean) => match reader.byte()? {
            0 => Ok(Value::Boolean(false)),
            1 => Ok(Value::Boolean(true)),
            _ => Err(DataError::InvalidWire),
        },
        SourceType::Primitive(PrimitiveType::String) => Ok(Value::String(reader.text(max_text)?)),
        SourceType::Tuple(ty) => {
            let mut values = Vec::new();
            values
                .try_reserve_exact(ty.elements().len())
                .map_err(|_| DataError::InvalidWire)?;
            for child in ty.elements() {
                values.push(decode_source(reader, child, depth + 1, max_text)?);
            }
            TupleValue::new(values)
                .map(Value::Tuple)
                .ok_or(DataError::InvalidWire)
        }
        SourceType::Array(ty) => {
            let mut values = Vec::new();
            values
                .try_reserve_exact(ty.len())
                .map_err(|_| DataError::InvalidWire)?;
            for _ in 0..ty.len() {
                values.push(decode_source(reader, ty.element(), depth + 1, max_text)?);
            }
            ArrayValue::new(ty.element().clone(), values)
                .map(Value::Array)
                .ok_or(DataError::InvalidWire)
        }
        _ => Err(DataError::UnsupportedWireType),
    }
}
fn encode(
    registry: &SchemaRegistry,
    writer: &mut Writer,
    ty: &DataType,
    val: &Datum,
    depth: usize,
) -> Result<(), DataError> {
    writer.node(depth)?;
    match (ty, val) {
        (DataType::Source(kind), Datum::Source(value)) => {
            encode_source(writer, kind, value, depth + 1)
        }
        (DataType::Record(id), Datum::Record(record)) => {
            if *id != record.identity() {
                return Err(DataError::TypeMismatch);
            }
            let fields = registry.record(*id)?;
            for (field, value) in fields.iter().zip(record.values()) {
                encode(registry, writer, &field.ty, value, depth + 1)?;
            }
            Ok(())
        }
        (DataType::Variant(id), Datum::Variant(variant)) => {
            if *id != variant.identity() {
                return Err(DataError::TypeMismatch);
            }
            let case = registry
                .variants(*id)?
                .get(variant.case_index())
                .ok_or(DataError::InvalidWire)?;
            writer.length(variant.case_index())?;
            if let Some(payload_ty) = &case.payload {
                let payload = variant.payload().ok_or(DataError::InvalidWire)?;
                encode(registry, writer, payload_ty, payload, depth + 1)?;
            }
            Ok(())
        }
        (DataType::List(element), Datum::List(list)) => {
            writer.length(list.len())?;
            for item in list.values() {
                encode(registry, writer, element, item, depth + 1)?;
            }
            Ok(())
        }
        (DataType::Map(keys, element), Datum::Map(map)) => {
            writer.length(map.len())?;
            for (key, value) in map.entries() {
                match (keys, key) {
                    (MapKeyType::Integer, DataKey::Integer(i)) => writer.integer(*i)?,
                    (MapKeyType::Text, DataKey::Text(s)) => writer.text(s)?,
                    _ => return Err(DataError::InvalidKey),
                }
                encode(registry, writer, element, value, depth + 1)?;
            }
            Ok(())
        }
        (DataType::Optional(_), Datum::Optional(None)) => writer.byte(0),
        (DataType::Optional(inner), Datum::Optional(Some(value))) => {
            writer.byte(1)?;
            encode(registry, writer, inner, value, depth + 1)
        }
        (DataType::Outcome(ok, _), Datum::Outcome(OutcomeValue::Ok(value))) => {
            writer.byte(0)?;
            encode(registry, writer, ok, value, depth + 1)
        }
        (DataType::Outcome(_, err), Datum::Outcome(OutcomeValue::Err(value))) => {
            writer.byte(1)?;
            encode(registry, writer, err, value, depth + 1)
        }
        _ => Err(DataError::TypeMismatch),
    }
}
fn decode(
    registry: &SchemaRegistry,
    reader: &mut Reader<'_>,
    ty: &DataType,
    depth: usize,
) -> Result<Datum, DataError> {
    reader.node(depth)?;
    let max_text = registry.limits().max_text_bytes;
    match ty {
        DataType::Source(kind) => Ok(Datum::Source(decode_source(
            reader,
            kind,
            depth + 1,
            max_text,
        )?)),
        DataType::Record(id) => {
            let spec = registry.record(*id)?;
            let mut values = Vec::new();
            values
                .try_reserve_exact(spec.len())
                .map_err(|_| DataError::InvalidWire)?;
            for field in spec {
                values.push(decode(registry, reader, &field.ty, depth + 1)?);
            }
            Ok(Datum::Record(registry.new_record(*id, values)?))
        }
        DataType::Variant(id) => {
            let index = reader.length()?;
            let case = registry
                .variants(*id)?
                .get(index)
                .ok_or(DataError::InvalidWire)?;
            let payload = match &case.payload {
                Some(ty) => Some(decode(registry, reader, ty, depth + 1)?),
                None => None,
            };
            Ok(Datum::Variant(
                registry.new_variant(*id, &case.name, payload)?,
            ))
        }
        DataType::List(element) => {
            let count = reader.length()?;
            bounded(
                count,
                registry.limits().max_collection_entries,
                DataResource::CollectionEntries,
            )?;
            bounded(count, reader.limits.max_nodes, DataResource::WireNodes)?;
            let mut values = Vec::new();
            values
                .try_reserve_exact(count)
                .map_err(|_| DataError::InvalidWire)?;
            for _ in 0..count {
                values.push(decode(registry, reader, element, depth + 1)?);
            }
            Ok(Datum::List(
                registry.new_list(element.as_ref().clone(), values)?,
            ))
        }
        DataType::Map(key_type, element) => {
            let count = reader.length()?;
            bounded(
                count,
                registry.limits().max_collection_entries,
                DataResource::CollectionEntries,
            )?;
            bounded(count, reader.limits.max_nodes, DataResource::WireNodes)?;
            let mut values = Vec::new();
            values
                .try_reserve_exact(count)
                .map_err(|_| DataError::InvalidWire)?;
            for _ in 0..count {
                let key = match key_type {
                    MapKeyType::Integer => DataKey::Integer(reader.integer()?),
                    MapKeyType::Text => DataKey::Text(reader.text(max_text)?),
                };
                values.push((key, decode(registry, reader, element, depth + 1)?));
            }
            // Canonical map ordering is enforced below: decoding never silently
            // sorts altered or duplicate serialized keys.
            if values.windows(2).any(|items| items[0].0 >= items[1].0) {
                return Err(DataError::InvalidWire);
            }
            Ok(Datum::Map(registry.new_map(
                *key_type,
                element.as_ref().clone(),
                values,
            )?))
        }
        DataType::Optional(inner) => match reader.byte()? {
            0 => Ok(Datum::none()),
            1 => Ok(Datum::some(decode(registry, reader, inner, depth + 1)?)),
            _ => Err(DataError::InvalidWire),
        },
        DataType::Outcome(ok, err) => match reader.byte()? {
            0 => Ok(Datum::ok(decode(registry, reader, ok, depth + 1)?)),
            1 => Ok(Datum::err(decode(registry, reader, err, depth + 1)?)),
            _ => Err(DataError::InvalidWire),
        },
    }
}
impl SchemaRegistry {
    /// Encode a fully verified datum to canonical bounded bytes.
    pub fn encode_wire(
        &self,
        ty: &DataType,
        value: &Datum,
        limits: WireLimits,
    ) -> Result<Vec<u8>, DataError> {
        self.validate_type(ty)?;
        self.verify(ty, value)?;
        let mut writer = Writer::new(limits);
        writer.append(MAGIC)?;
        encode(self, &mut writer, ty, value, 1)?;
        Ok(writer.bytes)
    }
    /// Decode one full wire message using the caller-owned schema.
    /// Trailing payload and invalid text/Boolean/float data fail closed.
    pub fn decode_wire(
        &self,
        ty: &DataType,
        bytes: &[u8],
        limits: WireLimits,
    ) -> Result<Datum, DataError> {
        self.validate_type(ty)?;
        let mut reader = Reader::new(bytes, limits)?;
        if reader.take(MAGIC.len())? != MAGIC {
            return Err(DataError::InvalidWire);
        }
        let decoded = decode(self, &mut reader, ty, 1)?;
        if !reader.ended() {
            return Err(DataError::InvalidWire);
        }
        self.verify(ty, &decoded)?;
        Ok(decoded)
    }
}
