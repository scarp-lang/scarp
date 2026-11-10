//! Portable structural source types. Composite TypeIds remain arena-local.
use super::{PrimitiveType, TypeId, TypeKind, TypeRegistryBuilder, TypeRegistryError};
use std::{fmt, sync::Arc};

pub const MAX_TUPLE_ARITY: usize = 32;
pub const MAX_TUPLE_DEPTH: usize = 32;
pub const MAX_SOURCE_TYPE_NODES: usize = 4096;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum SourceType {
    Primitive(PrimitiveType),
    Tuple(Arc<TupleType>),
}

/// Checked structural metadata; private fields prevent invalid host shapes.
#[derive(Debug, PartialEq, Eq, Hash)]
pub struct TupleType {
    elements: Box<[SourceType]>,
    depth: usize,
    nodes: usize,
}
impl TupleType {
    pub fn elements(&self) -> &[SourceType] {
        &self.elements
    }
    pub fn depth(&self) -> usize {
        self.depth
    }
    pub fn nodes(&self) -> usize {
        self.nodes
    }
}
impl SourceType {
    pub fn tuple(elements: Vec<Self>) -> Option<Self> {
        if elements.is_empty()
            || elements.len() > MAX_TUPLE_ARITY
            || elements.iter().any(|t| t == &PrimitiveType::Unit)
        {
            return None;
        }
        let depth = 1 + elements.iter().map(Self::tuple_depth).max().unwrap_or(0);
        let nodes = elements
            .iter()
            .try_fold(1usize, |n, t| n.checked_add(t.node_count()))?;
        if depth > MAX_TUPLE_DEPTH || nodes > MAX_SOURCE_TYPE_NODES {
            return None;
        }
        Some(Self::Tuple(Arc::new(TupleType {
            elements: elements.into_boxed_slice(),
            depth,
            nodes,
        })))
    }
    pub fn as_primitive(&self) -> Option<PrimitiveType> {
        match self {
            Self::Primitive(p) => Some(*p),
            _ => None,
        }
    }
    pub fn tuple_elements(&self) -> Option<&[Self]> {
        match self {
            Self::Tuple(t) => Some(t.elements()),
            _ => None,
        }
    }
    pub fn tuple_depth(&self) -> usize {
        match self {
            Self::Tuple(t) => t.depth,
            _ => 0,
        }
    }
    pub fn node_count(&self) -> usize {
        match self {
            Self::Tuple(t) => t.nodes,
            _ => 1,
        }
    }
    pub(crate) fn is_numeric(&self) -> bool {
        self.as_primitive().is_some_and(PrimitiveType::is_numeric)
    }
    pub(crate) fn intern(
        &self,
        registry: &mut TypeRegistryBuilder,
    ) -> Result<TypeId, TypeRegistryError> {
        match self {
            Self::Primitive(p) => Ok(p.id()),
            Self::Tuple(t) => {
                let elements = t
                    .elements
                    .iter()
                    .map(|e| e.intern(registry))
                    .collect::<Result<Vec<_>, _>>()?;
                registry.intern(TypeKind::Tuple(elements.into()))
            }
        }
    }
}
impl From<PrimitiveType> for SourceType {
    fn from(p: PrimitiveType) -> Self {
        Self::Primitive(p)
    }
}
impl From<&SourceType> for SourceType {
    fn from(t: &SourceType) -> Self {
        t.clone()
    }
}
impl PartialEq<PrimitiveType> for SourceType {
    fn eq(&self, p: &PrimitiveType) -> bool {
        self.as_primitive() == Some(*p)
    }
}
impl PartialEq<SourceType> for PrimitiveType {
    fn eq(&self, t: &SourceType) -> bool {
        t == self
    }
}
impl fmt::Display for SourceType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Primitive(p) => p.fmt(f),
            Self::Tuple(t) => {
                f.write_str("tuple<")?;
                for (index, element) in t.elements.iter().enumerate() {
                    if index > 0 {
                        f.write_str(", ")?;
                    }
                    element.fmt(f)?;
                }
                f.write_str(">")
            }
        }
    }
}
