//! Portable structural source types. Composite TypeIds remain arena-local.
use super::{PrimitiveType, TypeId, TypeKind, TypeRegistryBuilder, TypeRegistryError};
use std::{fmt, sync::Arc};

pub const MAX_TUPLE_ARITY: usize = 32;
pub const MAX_COMPOSITE_DEPTH: usize = 32;
pub const MAX_TUPLE_DEPTH: usize = MAX_COMPOSITE_DEPTH;
pub const MAX_SOURCE_TYPE_NODES: usize = 4096;
pub const MAX_ARRAY_LENGTH: usize = 4096;
pub const MAX_EXPANDED_VALUE_NODES: usize = 65_536;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum SourceType {
    Primitive(PrimitiveType),
    Tuple(Arc<TupleType>),
    Array(Arc<ArrayType>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct Shape {
    depth: usize,
    nodes: usize,
    expanded: usize,
    display_bytes: usize,
    contains_array: bool,
}

impl Shape {
    fn checked(self) -> Option<Self> {
        (self.depth <= MAX_COMPOSITE_DEPTH
            && self.nodes <= MAX_SOURCE_TYPE_NODES
            && self.expanded <= MAX_EXPANDED_VALUE_NODES)
            .then_some(self)
    }
}

/// Checked structural metadata; private fields prevent invalid host shapes.
#[derive(Debug, PartialEq, Eq, Hash)]
pub struct TupleType {
    elements: Box<[SourceType]>,
    tuple_depth: usize,
    shape: Shape,
}
impl TupleType {
    pub fn elements(&self) -> &[SourceType] {
        &self.elements
    }
    pub fn depth(&self) -> usize {
        self.tuple_depth
    }
    pub fn nodes(&self) -> usize {
        self.shape.nodes
    }
}

/// Checked type of an immutable, fixed-length array.
#[derive(Debug, PartialEq, Eq, Hash)]
pub struct ArrayType {
    element: SourceType,
    length: usize,
    shape: Shape,
}

impl ArrayType {
    pub fn element(&self) -> &SourceType {
        &self.element
    }

    pub fn len(&self) -> usize {
        self.length
    }

    pub fn is_empty(&self) -> bool {
        self.length == 0
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
        let tuple_depth = 1 + elements.iter().map(Self::tuple_depth).max().unwrap_or(0);
        let mut shape = Shape {
            depth: 1,
            nodes: 1,
            expanded: 1,
            display_bytes: "tuple<>".len() + 2 * (elements.len() - 1),
            contains_array: false,
        };
        for child in elements.iter().map(Self::shape) {
            shape.depth = shape.depth.max(child.depth + 1);
            shape.nodes = shape.nodes.checked_add(child.nodes)?;
            shape.expanded = shape.expanded.checked_add(child.expanded)?;
            shape.display_bytes = shape.display_bytes.checked_add(child.display_bytes)?;
            shape.contains_array |= child.contains_array;
        }
        let shape = shape.checked()?;
        Some(Self::Tuple(Arc::new(TupleType {
            elements: elements.into_boxed_slice(),
            tuple_depth,
            shape,
        })))
    }

    /// Return a fixed array type if its element type and combined shape fit.
    /// Even a zero-length array retains and validates the full element type.
    pub fn array(element: impl Into<Self>, length: usize) -> Option<Self> {
        let element = element.into();
        if length > MAX_ARRAY_LENGTH || element == PrimitiveType::Unit {
            return None;
        }
        let child = element.shape();
        let digits = length.checked_ilog10().unwrap_or(0) as usize + 1;
        let shape = Shape {
            depth: child.depth + 1,
            nodes: child.nodes.checked_add(1)?,
            expanded: length.checked_mul(child.expanded)?.checked_add(1)?,
            display_bytes: child
                .display_bytes
                .checked_add("array<, >".len() + digits)?,
            contains_array: true,
        }
        .checked()?;
        Some(Self::Array(Arc::new(ArrayType {
            element,
            length,
            shape,
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
    pub fn as_array(&self) -> Option<&ArrayType> {
        match self {
            Self::Array(array) => Some(array),
            _ => None,
        }
    }

    /// Depth along tuple-only edges, with non-tuple types at depth zero.
    pub fn tuple_depth(&self) -> usize {
        match self {
            Self::Tuple(t) => t.depth(),
            _ => 0,
        }
    }

    pub fn structural_depth(&self) -> usize {
        self.shape().depth
    }

    pub fn node_count(&self) -> usize {
        self.shape().nodes
    }

    pub fn expanded_value_nodes(&self) -> usize {
        self.shape().expanded
    }

    pub fn contains_array(&self) -> bool {
        self.shape().contains_array
    }

    pub(crate) fn display_bytes(&self) -> usize {
        self.shape().display_bytes
    }

    fn shape(&self) -> Shape {
        match self {
            Self::Tuple(t) => t.shape,
            Self::Array(a) => a.shape,
            Self::Primitive(p) => Shape {
                depth: 0,
                nodes: 1,
                expanded: 1,
                display_bytes: p.name().len(),
                contains_array: false,
            },
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
            Self::Array(a) => {
                let element = a.element.intern(registry)?;
                registry.intern(TypeKind::Sequence {
                    element,
                    length: Some(a.length as u64),
                })
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
            Self::Array(a) => write!(f, "array<{}, {}>", a.element, a.length),
        }
    }
}
