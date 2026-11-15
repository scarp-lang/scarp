//! Exact schema-bound envelope for the experimental logical value codec.
//!
//! The underlying SCDE v1 message checks a caller-provided type, but its bytes
//! do not carry that type's declaration. This opt-in SCDB v2 envelope records a
//! canonical full registry descriptor and root type, preventing equal-layout
//! but different-meaning schemas from silently decoding as one another.
//!
//! Nominal identities are local compiler coordinates, not promised globally
//! stable IDs. The descriptor has to match exactly on both sides. This is a
//! developmental format, not a persistence ABI or a cryptographic signature.
use super::limits::{bounded, DataError, DataResource};
use super::schema::{DataType, DefinitionKind, MapKeyType, SchemaRegistry};
use super::value::Datum;
use super::wire::WireLimits;
use crate::types::{DefinitionId, PrimitiveType, SourceType};

const MAGIC_V2: &[u8; 5] = b"SCDB\x02";

struct DescriptorEncoder {
    bytes: Vec<u8>,
    limits: WireLimits,
    visited: usize,
}
impl DescriptorEncoder {
    fn new(limits: WireLimits) -> Self {
        Self {
            bytes: Vec::new(),
            limits,
            visited: 0,
        }
    }
    fn node(&mut self, depth: usize) -> Result<(), DataError> {
        bounded(depth, self.limits.max_depth, DataResource::WireDepth)?;
        self.visited = self.visited.checked_add(1).ok_or(DataError::Limit {
            resource: DataResource::WireNodes,
            limit: self.limits.max_nodes,
        })?;
        bounded(self.visited, self.limits.max_nodes, DataResource::WireNodes)
    }
    fn bytes(&mut self, data: &[u8]) -> Result<(), DataError> {
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
    fn byte(&mut self, value: u8) -> Result<(), DataError> {
        self.bytes(&[value])
    }
    fn len(&mut self, value: usize) -> Result<(), DataError> {
        let count = u32::try_from(value).map_err(|_| DataError::InvalidWire)?;
        self.bytes(&count.to_le_bytes())
    }
    fn id(&mut self, id: DefinitionId) -> Result<(), DataError> {
        self.bytes(&id.module.to_le_bytes())?;
        self.bytes(&id.declaration.to_le_bytes())
    }
    fn text(&mut self, text: &str) -> Result<(), DataError> {
        self.len(text.len())?;
        self.bytes(text.as_bytes())
    }
    fn primitive(&mut self, primitive: PrimitiveType) -> Result<(), DataError> {
        // PrimitiveType has a stable explicit repr(u8) *within this version*.
        self.byte(primitive as u8)
    }
    fn source(&mut self, ty: &SourceType, depth: usize) -> Result<(), DataError> {
        self.node(depth)?;
        match ty {
            SourceType::Primitive(primitive) => {
                self.byte(0)?;
                self.primitive(*primitive)
            }
            SourceType::Tuple(tuple) => {
                self.byte(1)?;
                self.len(tuple.elements().len())?;
                for item in tuple.elements() {
                    self.source(item, depth + 1)?;
                }
                Ok(())
            }
            SourceType::Array(array) => {
                self.byte(2)?;
                self.len(array.len())?;
                self.source(array.element(), depth + 1)
            }
        }
    }
    fn ty(&mut self, ty: &DataType, depth: usize) -> Result<(), DataError> {
        self.node(depth)?;
        match ty {
            DataType::Source(source) => {
                self.byte(0)?;
                self.source(source, depth + 1)
            }
            DataType::Record(id) => {
                self.byte(1)?;
                self.id(*id)
            }
            DataType::Variant(id) => {
                self.byte(2)?;
                self.id(*id)
            }
            DataType::List(inner) => {
                self.byte(3)?;
                self.ty(inner, depth + 1)
            }
            DataType::Map(keys, inner) => {
                self.byte(4)?;
                self.byte(match keys {
                    MapKeyType::Integer => 0,
                    MapKeyType::Text => 1,
                })?;
                self.ty(inner, depth + 1)
            }
            DataType::Optional(inner) => {
                self.byte(5)?;
                self.ty(inner, depth + 1)
            }
            DataType::Outcome(ok, err) => {
                self.byte(6)?;
                self.ty(ok, depth + 1)?;
                self.ty(err, depth + 1)
            }
        }
    }
}
impl SchemaRegistry {
    /// Exact deterministic metadata, sorted by identity so registration order
    /// cannot create different descriptors for equivalent schemas.
    ///
    /// The descriptor includes all registered definitions, not only reachable
    /// definitions. This intentionally rejects a registry with extra types.
    pub fn wire_schema_descriptor(
        &self,
        root: &DataType,
        limits: WireLimits,
    ) -> Result<Vec<u8>, DataError> {
        self.validate_type(root)?;
        let mut writer = DescriptorEncoder::new(limits);
        writer.bytes(b"SCHEMA\x01")?;
        writer.ty(root, 1)?;
        let mut definitions: Vec<_> = self.definitions().iter().collect();
        definitions.sort_by_key(|d| (d.id.module, d.id.declaration));
        writer.len(definitions.len())?;
        for definition in definitions {
            writer.node(1)?;
            writer.id(definition.id)?;
            writer.text(&definition.name)?;
            match &definition.kind {
                DefinitionKind::Record(fields) => {
                    writer.byte(0)?;
                    writer.len(fields.len())?;
                    for field in fields {
                        writer.node(2)?;
                        writer.text(&field.name)?;
                        writer.ty(&field.ty, 3)?;
                    }
                }
                DefinitionKind::Variant(cases) => {
                    writer.byte(1)?;
                    writer.len(cases.len())?;
                    for case in cases {
                        writer.node(2)?;
                        writer.text(&case.name)?;
                        match &case.payload {
                            None => writer.byte(0)?,
                            Some(ty) => {
                                writer.byte(1)?;
                                writer.ty(ty, 3)?;
                            }
                        }
                    }
                }
            }
        }
        Ok(writer.bytes)
    }

    /// Encode a value with a canonical exact schema envelope. The embedded
    /// v1 payload retains its independent validation and bounds.
    pub fn encode_schema_wire(
        &self,
        root: &DataType,
        value: &Datum,
        limits: WireLimits,
    ) -> Result<Vec<u8>, DataError> {
        let descriptor = self.wire_schema_descriptor(root, limits)?;
        let message = self.encode_wire(root, value, limits)?;
        let descriptor_length =
            u32::try_from(descriptor.len()).map_err(|_| DataError::InvalidWire)?;
        let prefix_size = MAGIC_V2
            .len()
            .checked_add(4)
            .and_then(|v| v.checked_add(descriptor.len()))
            .ok_or(DataError::Limit {
                resource: DataResource::WireBytes,
                limit: limits.max_bytes,
            })?;
        let total = prefix_size
            .checked_add(message.len())
            .ok_or(DataError::Limit {
                resource: DataResource::WireBytes,
                limit: limits.max_bytes,
            })?;
        bounded(total, limits.max_bytes, DataResource::WireBytes)?;
        let mut output = Vec::new();
        output
            .try_reserve_exact(total)
            .map_err(|_| DataError::Limit {
                resource: DataResource::WireBytes,
                limit: limits.max_bytes,
            })?;
        output.extend_from_slice(MAGIC_V2);
        output.extend_from_slice(&descriptor_length.to_le_bytes());
        output.extend_from_slice(&descriptor);
        output.extend_from_slice(&message);
        Ok(output)
    }

    /// Reject mismatched root types, field names/order, variant declarations,
    /// unrelated registry entries and trailing/altered value payload bytes.
    ///
    /// For arbitrary untrusted files this is not an authenticity check or a
    /// replacement for independently trusted schema/version negotiation.
    pub fn decode_schema_wire(
        &self,
        root: &DataType,
        bytes: &[u8],
        limits: WireLimits,
    ) -> Result<Datum, DataError> {
        bounded(bytes.len(), limits.max_bytes, DataResource::WireBytes)?;
        let expected = self.wire_schema_descriptor(root, limits)?;
        if !bytes.starts_with(MAGIC_V2) {
            return Err(DataError::InvalidWire);
        }
        let offset = MAGIC_V2.len();
        let length = bytes
            .get(offset..offset + 4)
            .ok_or(DataError::InvalidWire)?;
        let declared =
            u32::from_le_bytes(length.try_into().map_err(|_| DataError::InvalidWire)?) as usize;
        if declared != expected.len() {
            return Err(DataError::InvalidWire);
        }
        let start = offset + 4;
        let end = start.checked_add(declared).ok_or(DataError::InvalidWire)?;
        if bytes.get(start..end) != Some(expected.as_slice()) {
            return Err(DataError::InvalidWire);
        }
        let payload = bytes.get(end..).ok_or(DataError::InvalidWire)?;
        self.decode_wire(root, payload, limits)
    }
}
