//! Deterministic, caller-owned limits for experimental composite values.
use std::fmt;

/// A host may lower these limits, but never bypass type or shape validation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DataLimits {
    pub max_definitions: usize,
    pub max_members_per_definition: usize,
    pub max_type_depth: usize,
    pub max_type_nodes: usize,
    pub max_value_depth: usize,
    pub max_value_nodes: usize,
    pub max_collection_entries: usize,
    pub max_text_bytes: usize,
    pub max_pattern_nodes: usize,
    pub max_specializations: usize,
}
impl Default for DataLimits {
    fn default() -> Self {
        Self {
            max_definitions: 1024,
            max_members_per_definition: 256,
            max_type_depth: 32,
            max_type_nodes: 8192,
            max_value_depth: 64,
            max_value_nodes: 65_536,
            max_collection_entries: 4096,
            max_text_bytes: 8 * 1024 * 1024,
            max_pattern_nodes: 8192,
            max_specializations: 1024,
        }
    }
}

/// Logical limits are not claims about the allocator's RSS usage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataResource {
    Definitions,
    Members,
    TypeDepth,
    TypeNodes,
    ValueDepth,
    ValueNodes,
    CollectionEntries,
    TextBytes,
    PatternNodes,
    Specializations,
    QuerySteps,
    QueryScan,
    QueryOutput,
    WireBytes,
    WireNodes,
    WireDepth,
    IndexRows,
    IndexKeys,
    IndexKeyBytes,
    IndexPairs,
    DiffNodes,
    DiffOperations,
    DiffDepth,
    MigrationNodes,
    MigrationDepth,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DataError {
    Limit {
        resource: DataResource,
        limit: usize,
    },
    InvalidName(String),
    DuplicateName(String),
    DuplicateDefinition,
    UnknownDefinition,
    UnknownField(String),
    UnknownCase(String),
    InvalidDefinition,
    TypeMismatch,
    InvalidShape,
    InvalidKey,
    DuplicateKey,
    InvalidPattern,
    CyclicDefinition,
    ArithmeticOverflow,
    InvalidWire,
    UnsupportedWireType,
    DroppedField(String),
    MissingMigrationDefault(String),
    UnboundParameter,
    ArityMismatch,
    UnsatisfiedConstraint,
}

impl fmt::Display for DataError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Limit { resource, limit } => {
                write!(f, "data engine {resource:?} limit {limit} exceeded")
            }
            Self::InvalidName(n) => write!(f, "invalid identifier: {n}"),
            Self::DuplicateName(n) => write!(f, "duplicate name: {n}"),
            Self::DuplicateDefinition => f.write_str("definition identity already exists"),
            Self::UnknownDefinition => f.write_str("unknown nominal type identity"),
            Self::UnknownField(n) => write!(f, "unknown field: {n}"),
            Self::UnknownCase(n) => write!(f, "unknown variant case: {n}"),
            Self::InvalidDefinition => f.write_str("definition kind is not appropriate"),
            Self::TypeMismatch => f.write_str("composite value type mismatch"),
            Self::InvalidShape => f.write_str("composite value has invalid shape"),
            Self::InvalidKey => f.write_str("map key is incompatible with declared key type"),
            Self::DuplicateKey => f.write_str("duplicate map key"),
            Self::InvalidPattern => f.write_str("invalid or ambiguous pattern"),
            Self::CyclicDefinition => f.write_str("nominal declaration dependency cycle"),
            Self::ArithmeticOverflow => f.write_str("query integer arithmetic overflow"),
            Self::InvalidWire => f.write_str("invalid or noncanonical binary data"),
            Self::UnsupportedWireType => {
                f.write_str("source type not supported by experimental binary format")
            }
            Self::DroppedField(name) => write!(f, "migration would remove field without permission: {name}"),
            Self::MissingMigrationDefault(name) => write!(f, "migration requires explicit default for field: {name}"),
            Self::UnboundParameter => f.write_str("unbound generic type parameter"),
            Self::ArityMismatch => f.write_str("generic function argument count mismatch"),
            Self::UnsatisfiedConstraint => f.write_str("unsatisfied generic type constraint"),
        }
    }
}
impl std::error::Error for DataError {}

pub(crate) fn bounded(value: usize, limit: usize, resource: DataResource) -> Result<(), DataError> {
    if value > limit {
        Err(DataError::Limit { resource, limit })
    } else {
        Ok(())
    }
}

pub(crate) fn identifier(name: &str) -> Result<(), DataError> {
    let mut chars = name.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => (),
        _ => return Err(DataError::InvalidName(name.to_owned())),
    }
    if chars.any(|c| !(c.is_ascii_alphanumeric() || c == '_')) {
        return Err(DataError::InvalidName(name.to_owned()));
    }
    Ok(())
}

#[derive(Debug)]
pub(crate) struct Meter {
    limits: DataLimits,
    nodes: usize,
    bytes: usize,
}
impl Meter {
    pub(crate) fn new(limits: DataLimits) -> Self {
        Self {
            limits,
            nodes: 0,
            bytes: 0,
        }
    }
    pub(crate) fn node(&mut self, depth: usize) -> Result<(), DataError> {
        bounded(depth, self.limits.max_value_depth, DataResource::ValueDepth)?;
        self.nodes = self.nodes.checked_add(1).ok_or(DataError::Limit {
            resource: DataResource::ValueNodes,
            limit: self.limits.max_value_nodes,
        })?;
        bounded(
            self.nodes,
            self.limits.max_value_nodes,
            DataResource::ValueNodes,
        )
    }
    pub(crate) fn text(&mut self, length: usize) -> Result<(), DataError> {
        self.bytes = self.bytes.checked_add(length).ok_or(DataError::Limit {
            resource: DataResource::TextBytes,
            limit: self.limits.max_text_bytes,
        })?;
        bounded(
            self.bytes,
            self.limits.max_text_bytes,
            DataResource::TextBytes,
        )
    }
    pub(crate) fn entries(&self, count: usize) -> Result<(), DataError> {
        bounded(
            count,
            self.limits.max_collection_entries,
            DataResource::CollectionEntries,
        )
    }
}
