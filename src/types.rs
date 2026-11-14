//! Semantic type identities. Source-language legality is checked separately.
mod source;
pub use source::{
    ArrayType, SourceType, TupleType, MAX_ARRAY_LENGTH, MAX_COMPOSITE_DEPTH,
    MAX_EXPANDED_VALUE_NODES, MAX_SOURCE_TYPE_NODES, MAX_TUPLE_ARITY, MAX_TUPLE_DEPTH,
};

use std::{
    collections::HashMap,
    fmt,
    sync::{
        atomic::{AtomicU32, Ordering},
        Arc,
    },
};

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PrimitiveType {
    Unit,
    Integer,
    Float,
    Vector,
    Integers,
    Tile,
    Boolean,
    String,
    BigInt,
}

impl PrimitiveType {
    pub const ALL: &'static [Self] = &[
        Self::Unit,
        Self::Integer,
        Self::Float,
        Self::Vector,
        Self::Integers,
        Self::Tile,
        Self::Boolean,
        Self::String,
        Self::BigInt,
    ];
    pub const fn id(self) -> TypeId {
        TypeId(self as u64)
    }
    pub(crate) fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "unit" => Self::Unit,
            "integer" => Self::Integer,
            "float" => Self::Float,
            "vector" => Self::Vector,
            "integers" => Self::Integers,
            "tile" => Self::Tile,
            "boolean" => Self::Boolean,
            "string" => Self::String,
            "bigint" => Self::BigInt,
            _ => return None,
        })
    }
    pub(crate) fn is_numeric(self) -> bool {
        matches!(self, Self::Integer | Self::Float | Self::BigInt)
    }
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::Unit => "unit",
            Self::Integer => "integer",
            Self::Float => "float",
            Self::Vector => "vector",
            Self::Integers => "integers",
            Self::Tile => "tile",
            Self::Boolean => "boolean",
            Self::String => "string",
            Self::BigInt => "bigint",
        }
    }
}
impl fmt::Display for PrimitiveType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

// Primitive identities are universal. Composite handles include a private arena
// identity, so equal local indices from unrelated registries cannot alias.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct TypeId(u64);
impl fmt::Debug for TypeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("TypeId").field(&self.index()).finish()
    }
}
impl TypeId {
    pub fn index(self) -> usize {
        (self.0 & u64::from(u32::MAX)) as usize
    }
    pub fn primitive(self) -> Option<PrimitiveType> {
        (self.0 < PrimitiveType::ALL.len() as u64).then(|| PrimitiveType::ALL[self.index()])
    }
    fn arena(self) -> u32 {
        (self.0 >> 32) as u32
    }
}
impl fmt::Display for TypeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(kind) = self.primitive() {
            kind.fmt(f)
        } else {
            write!(f, "type#{}", self.index())
        }
    }
}

pub(crate) fn type_mismatch(
    span: crate::diagnostics::Span,
    expected: impl fmt::Display,
    found: impl fmt::Display,
) -> crate::diagnostics::Diagnostic {
    crate::diagnostics::Diagnostic::new(crate::diagnostics::DiagnosticKind::TypeMismatch,span,format!("expected {expected}, found {found}"))
        .with_help("keep primitive types consistent; numeric operands must share a numeric kind, logic requires booleans")
}
impl From<PrimitiveType> for TypeId {
    fn from(value: PrimitiveType) -> Self {
        value.id()
    }
}

/// Compiler-assigned declaration identity, scoped to a registry, not a name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DefinitionId {
    pub module: u32,
    pub declaration: u32,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FunctionEffect {
    Pure,
    Impure,
}

// Children are IDs rather than owned subtrees. Fixed arrays use Sequence
// with a length; a length-free Sequence remains a tooling descriptor.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TypeKind {
    Primitive(PrimitiveType),
    Tuple(Box<[TypeId]>),
    Sequence {
        element: TypeId,
        length: Option<u64>,
    },
    Applied {
        constructor: DefinitionId,
        arguments: Box<[TypeId]>,
    },
    Parameter {
        binder: DefinitionId,
        index: u32,
    },
    Function {
        parameters: Box<[TypeId]>,
        result: TypeId,
        effect: FunctionEffect,
    },
    Tensor {
        element: TypeId,
        dimensions: Box<[Option<u64>]>,
    },
}
impl TypeKind {
    fn children(&self) -> impl Iterator<Item = TypeId> + '_ {
        let (single, others): (Option<TypeId>, &[TypeId]) = match self {
            Self::Tuple(values) => (None, values),
            Self::Sequence { element, .. } | Self::Tensor { element, .. } => (Some(*element), &[]),
            Self::Applied { arguments, .. } => (None, arguments),
            Self::Function {
                parameters, result, ..
            } => (Some(*result), parameters),
            _ => (None, &[]),
        };
        single.into_iter().chain(others.iter().copied())
    }
    fn width(&self) -> usize {
        match self {
            Self::Tuple(v) => v.len(),
            Self::Applied { arguments, .. } => arguments.len(),
            Self::Function { parameters, .. } => parameters.len(),
            Self::Tensor { dimensions, .. } => dimensions.len(),
            _ => 0,
        }
    }
    fn components(&self) -> Option<usize> {
        let children = self.children().count();
        children.checked_add(if let Self::Tensor { dimensions, .. } = self {
            dimensions.len()
        } else {
            0
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TypeLimits {
    /// Unique types, including the eight universal primitives.
    pub max_types: usize,
    /// Retained child references and tensor axis descriptors, not process bytes.
    pub max_components: usize,
    pub max_depth: usize,
    pub max_arity: usize,
}
impl Default for TypeLimits {
    fn default() -> Self {
        Self {
            max_types: 65_536,
            max_components: 8_388_608,
            max_depth: 64,
            max_arity: 1024,
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypeResource {
    Types,
    Components,
    Depth,
    Arity,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypeRegistryError {
    InvalidLimits,
    UnknownType(TypeId),
    Limit {
        resource: TypeResource,
        limit: usize,
    },
    Allocation,
    IdentityExhausted,
}
impl fmt::Display for TypeRegistryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidLimits => f.write_str(
                "type limits require room for all primitives, positive depth and 32-bit indices",
            ),
            Self::UnknownType(_) => f.write_str("type handle does not belong to this registry"),
            Self::Limit { resource, limit } => {
                write!(f, "semantic type {resource:?} limit of {limit} exceeded")
            }
            Self::Allocation => f.write_str("unable to reserve semantic type storage"),
            Self::IdentityExhausted => f.write_str("semantic type registry identities exhausted"),
        }
    }
}
impl std::error::Error for TypeRegistryError {}

static PRIMITIVES: [TypeKind; 9] = [
    TypeKind::Primitive(PrimitiveType::Unit),
    TypeKind::Primitive(PrimitiveType::Integer),
    TypeKind::Primitive(PrimitiveType::Float),
    TypeKind::Primitive(PrimitiveType::Vector),
    TypeKind::Primitive(PrimitiveType::Integers),
    TypeKind::Primitive(PrimitiveType::Tile),
    TypeKind::Primitive(PrimitiveType::Boolean),
    TypeKind::Primitive(PrimitiveType::String),
    TypeKind::Primitive(PrimitiveType::BigInt),
];
static NEXT_ARENA: AtomicU32 = AtomicU32::new(1);

#[derive(Debug)]
struct Entry {
    kind: Arc<TypeKind>,
    depth: usize,
}
#[derive(Debug)]
struct Storage {
    arena: u32,
    entries: Vec<Entry>,
}

#[derive(Clone)]
pub struct TypeRegistry {
    storage: Arc<Storage>,
}
impl fmt::Debug for TypeRegistry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_map().entries(self.iter()).finish()
    }
}
impl Default for TypeRegistry {
    fn default() -> Self {
        Self {
            storage: Arc::new(Storage {
                arena: 0,
                entries: Vec::new(),
            }),
        }
    }
}
impl TypeRegistry {
    pub fn len(&self) -> usize {
        PRIMITIVES.len() + self.storage.entries.len()
    }
    pub fn is_empty(&self) -> bool {
        false
    }
    pub fn get(&self, id: TypeId) -> Option<&TypeKind> {
        if id.primitive().is_some() {
            return PRIMITIVES.get(id.index());
        }
        self.entry(id).map(|entry| entry.kind.as_ref())
    }
    fn entry(&self, id: TypeId) -> Option<&Entry> {
        (id.arena() == self.storage.arena)
            .then(|| id.index().checked_sub(PRIMITIVES.len()))
            .flatten()
            .and_then(|index| self.storage.entries.get(index))
    }
    pub fn depth(&self, id: TypeId) -> Option<usize> {
        if id.primitive().is_some() {
            Some(1)
        } else {
            self.entry(id).map(|entry| entry.depth)
        }
    }
    pub fn equivalent(&self, left: TypeId, right: TypeId) -> Result<bool, TypeRegistryError> {
        for id in [left, right] {
            self.get(id).ok_or(TypeRegistryError::UnknownType(id))?;
        }
        Ok(left == right)
    }
    pub fn iter(&self) -> impl Iterator<Item = (TypeId, &TypeKind)> {
        PRIMITIVES
            .iter()
            .enumerate()
            .map(|(index, kind)| (TypeId(index as u64), kind))
            .chain(
                self.storage
                    .entries
                    .iter()
                    .enumerate()
                    .map(|(index, entry)| {
                        (
                            TypeId(
                                (u64::from(self.storage.arena) << 32)
                                    | (index + PRIMITIVES.len()) as u64,
                            ),
                            entry.kind.as_ref(),
                        )
                    }),
            )
    }
}

pub struct TypeRegistryBuilder {
    storage: Storage,
    index: HashMap<Arc<TypeKind>, TypeId>,
    limits: TypeLimits,
    components: usize,
}
impl TypeRegistryBuilder {
    pub fn new(limits: TypeLimits) -> Result<Self, TypeRegistryError> {
        if limits.max_types < PRIMITIVES.len()
            || limits.max_types > u32::MAX as usize
            || limits.max_depth == 0
        {
            return Err(TypeRegistryError::InvalidLimits);
        }
        let mut arena = NEXT_ARENA.load(Ordering::Relaxed);
        loop {
            let next = arena
                .checked_add(1)
                .ok_or(TypeRegistryError::IdentityExhausted)?;
            match NEXT_ARENA.compare_exchange_weak(
                arena,
                next,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(actual) => arena = actual,
            }
        }
        Ok(Self {
            storage: Storage {
                arena,
                entries: Vec::new(),
            },
            index: HashMap::new(),
            limits,
            components: 0,
        })
    }
    pub fn len(&self) -> usize {
        PRIMITIVES.len() + self.storage.entries.len()
    }
    pub fn is_empty(&self) -> bool {
        false
    }
    pub fn component_count(&self) -> usize {
        self.components
    }
    fn depth(&self, id: TypeId) -> Option<usize> {
        if id.primitive().is_some() {
            return Some(1);
        }
        if id.arena() != self.storage.arena {
            return None;
        }
        self.storage
            .entries
            .get(id.index().checked_sub(PRIMITIVES.len())?)
            .map(|entry| entry.depth)
    }
    pub fn intern(&mut self, kind: TypeKind) -> Result<TypeId, TypeRegistryError> {
        if let TypeKind::Primitive(kind) = kind {
            return Ok(kind.id());
        }
        let limit = |resource, limit| TypeRegistryError::Limit { resource, limit };
        if kind.width() > self.limits.max_arity {
            return Err(limit(TypeResource::Arity, self.limits.max_arity));
        }
        let mut depth = 1usize;
        for child in kind.children() {
            let child = self
                .depth(child)
                .ok_or(TypeRegistryError::UnknownType(child))?;
            depth = depth.max(
                child
                    .checked_add(1)
                    .ok_or(limit(TypeResource::Depth, self.limits.max_depth))?,
            );
        }
        if depth > self.limits.max_depth {
            return Err(limit(TypeResource::Depth, self.limits.max_depth));
        }
        if let Some(&id) = self.index.get(&kind) {
            return Ok(id);
        }
        if self.len() >= self.limits.max_types {
            return Err(limit(TypeResource::Types, self.limits.max_types));
        }
        let components = kind
            .components()
            .and_then(|count| self.components.checked_add(count))
            .filter(|count| *count <= self.limits.max_components)
            .ok_or(limit(TypeResource::Components, self.limits.max_components))?;
        self.storage
            .entries
            .try_reserve(1)
            .map_err(|_| TypeRegistryError::Allocation)?;
        self.index
            .try_reserve(1)
            .map_err(|_| TypeRegistryError::Allocation)?;
        let id = TypeId((u64::from(self.storage.arena) << 32) | self.len() as u64);
        let kind = Arc::new(kind);
        self.storage.entries.push(Entry {
            kind: Arc::clone(&kind),
            depth,
        });
        self.index.insert(kind, id);
        self.components = components;
        Ok(id)
    }
    pub fn finish(self) -> TypeRegistry {
        TypeRegistry {
            storage: Arc::new(self.storage),
        }
    }
}
