//! Nominal schema registration, portable source types and checked declaration
//! boundaries. Identity uses the existing compiler DefinitionId, never spelling.
use super::limits::{bounded, identifier, DataError, DataLimits, DataResource};
use crate::types::{DefinitionId, PrimitiveType, SourceType};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MapKeyType {
    Integer,
    Text,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum DataType {
    /// Reuses the real Scarp scalar / tuple / fixed-array type boundary.
    Source(SourceType),
    Record(DefinitionId),
    Variant(DefinitionId),
    List(Box<DataType>),
    Map(MapKeyType, Box<DataType>),
    Optional(Box<DataType>),
    Outcome(Box<DataType>, Box<DataType>),
}
impl DataType {
    pub fn integer() -> Self {
        Self::Source(PrimitiveType::Integer.into())
    }
    pub fn boolean() -> Self {
        Self::Source(PrimitiveType::Boolean.into())
    }
    pub fn string() -> Self {
        Self::Source(PrimitiveType::String.into())
    }
    pub fn list(inner: Self) -> Self {
        Self::List(Box::new(inner))
    }
    pub fn optional(inner: Self) -> Self {
        Self::Optional(Box::new(inner))
    }
    pub fn map(keys: MapKeyType, values: Self) -> Self {
        Self::Map(keys, Box::new(values))
    }
    pub fn outcome(ok: Self, err: Self) -> Self {
        Self::Outcome(Box::new(ok), Box::new(err))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Field {
    pub name: String,
    pub ty: DataType,
}
impl Field {
    pub fn new(name: impl Into<String>, ty: DataType) -> Self {
        Self {
            name: name.into(),
            ty,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct VariantCase {
    pub name: String,
    pub payload: Option<DataType>,
}
impl VariantCase {
    pub fn unit(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            payload: None,
        }
    }
    pub fn typed(name: impl Into<String>, ty: DataType) -> Self {
        Self {
            name: name.into(),
            payload: Some(ty),
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum DefinitionKind {
    Record(Vec<Field>),
    Variant(Vec<VariantCase>),
}
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Definition {
    pub id: DefinitionId,
    pub name: String,
    pub kind: DefinitionKind,
}
impl Definition {
    pub fn record(id: DefinitionId, name: impl Into<String>, fields: Vec<Field>) -> Self {
        Self {
            id,
            name: name.into(),
            kind: DefinitionKind::Record(fields),
        }
    }
    pub fn variant(id: DefinitionId, name: impl Into<String>, cases: Vec<VariantCase>) -> Self {
        Self {
            id,
            name: name.into(),
            kind: DefinitionKind::Variant(cases),
        }
    }
    pub fn member_count(&self) -> usize {
        match &self.kind {
            DefinitionKind::Record(fields) => fields.len(),
            DefinitionKind::Variant(cases) => cases.len(),
        }
    }
}

/// Explicit registry; instances are immutable once handed to a value context.
/// Registration checks all referenced types before mutating the registry.
#[derive(Debug, Clone)]
pub struct SchemaRegistry {
    limits: DataLimits,
    definitions: Vec<Definition>,
    indices: HashMap<DefinitionId, usize>,
    total_type_nodes: usize,
}
impl Default for SchemaRegistry {
    fn default() -> Self {
        Self::new(DataLimits::default())
    }
}
impl SchemaRegistry {
    pub fn new(limits: DataLimits) -> Self {
        Self {
            limits,
            definitions: Vec::new(),
            indices: HashMap::new(),
            total_type_nodes: 0,
        }
    }
    pub fn limits(&self) -> DataLimits {
        self.limits
    }
    pub fn len(&self) -> usize {
        self.definitions.len()
    }
    pub fn is_empty(&self) -> bool {
        self.definitions.is_empty()
    }
    pub fn definitions(&self) -> &[Definition] {
        &self.definitions
    }
    pub fn get(&self, id: DefinitionId) -> Option<&Definition> {
        self.indices
            .get(&id)
            .and_then(|&index| self.definitions.get(index))
    }
    pub fn record(&self, id: DefinitionId) -> Result<&[Field], DataError> {
        match &self.get(id).ok_or(DataError::UnknownDefinition)?.kind {
            DefinitionKind::Record(fields) => Ok(fields),
            _ => Err(DataError::InvalidDefinition),
        }
    }
    pub fn variants(&self, id: DefinitionId) -> Result<&[VariantCase], DataError> {
        match &self.get(id).ok_or(DataError::UnknownDefinition)?.kind {
            DefinitionKind::Variant(cases) => Ok(cases),
            _ => Err(DataError::InvalidDefinition),
        }
    }
    /// Register one declaration. Self-recursive references are permitted,
    /// but the referenced identity must have the appropriate nominal kind.
    pub fn register(&mut self, definition: Definition) -> Result<(), DataError> {
        self.register_batch(vec![definition])
    }

    /// Atomically register mutually dependent declarations in one graph.
    /// Forward references are checked against a private candidate registry;
    /// failure leaves all existing definitions and quotas unchanged.
    pub fn register_batch(&mut self, definitions: Vec<Definition>) -> Result<(), DataError> {
        if definitions.is_empty() {
            return Ok(());
        }
        let prospective = self
            .definitions
            .len()
            .checked_add(definitions.len())
            .ok_or(DataError::Limit {
                resource: DataResource::Definitions,
                limit: self.limits.max_definitions,
            })?;
        bounded(
            prospective,
            self.limits.max_definitions,
            DataResource::Definitions,
        )?;
        let mut staged = self.clone();
        for definition in &definitions {
            identifier(&definition.name)?;
            if staged.indices.contains_key(&definition.id) {
                return Err(DataError::DuplicateDefinition);
            }
            let index = staged.definitions.len();
            staged.indices.insert(definition.id, index);
            staged.definitions.push(definition.clone());
        }
        let mut nodes = self.total_type_nodes;
        for definition in &definitions {
            bounded(
                definition.member_count(),
                self.limits.max_members_per_definition,
                DataResource::Members,
            )?;
            let mut names = HashSet::new();
            match &definition.kind {
                DefinitionKind::Record(fields) => {
                    for field in fields {
                        identifier(&field.name)?;
                        if !names.insert(field.name.as_str()) {
                            return Err(DataError::DuplicateName(field.name.clone()));
                        }
                        staged.validate_type_inner(&field.ty, &mut nodes)?;
                    }
                }
                DefinitionKind::Variant(cases) => {
                    if cases.is_empty() {
                        return Err(DataError::InvalidShape);
                    }
                    for case in cases {
                        identifier(&case.name)?;
                        if !names.insert(case.name.as_str()) {
                            return Err(DataError::DuplicateName(case.name.clone()));
                        }
                        if let Some(ty) = &case.payload {
                            staged.validate_type_inner(ty, &mut nodes)?;
                        }
                    }
                }
            }
        }
        staged.total_type_nodes = nodes;
        *self = staged;
        Ok(())
    }
    /// Validate a host type without allocating or adding a definition.
    pub fn validate_type(&self, ty: &DataType) -> Result<(), DataError> {
        let mut nodes = 0;
        self.validate_type_inner(ty, &mut nodes)
    }
    fn validate_type_inner(&self, ty: &DataType, nodes: &mut usize) -> Result<(), DataError> {
        let mut pending = vec![(ty, 1usize)];
        while let Some((part, depth)) = pending.pop() {
            bounded(depth, self.limits.max_type_depth, DataResource::TypeDepth)?;
            *nodes = nodes.checked_add(1).ok_or(DataError::Limit {
                resource: DataResource::TypeNodes,
                limit: self.limits.max_type_nodes,
            })?;
            bounded(*nodes, self.limits.max_type_nodes, DataResource::TypeNodes)?;
            match part {
                DataType::Source(source) => {
                    if source == &SourceType::Primitive(PrimitiveType::Unit) {
                        return Err(DataError::InvalidDefinition);
                    }
                }
                DataType::Record(id) => {
                    self.record(*id)?;
                }
                DataType::Variant(id) => {
                    self.variants(*id)?;
                }
                DataType::List(inner) | DataType::Optional(inner) | DataType::Map(_, inner) => {
                    pending.push((inner, depth + 1));
                }
                DataType::Outcome(ok, err) => {
                    pending.push((err, depth + 1));
                    pending.push((ok, depth + 1));
                }
            }
        }
        Ok(())
    }
}
