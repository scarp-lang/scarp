//! Portable structural source types. Composite TypeIds remain arena-local.
use super::{PrimitiveType, TypeId, TypeKind, TypeRegistryBuilder, TypeRegistryError};
use std::{fmt, sync::Arc};

pub const MAX_TUPLE_ARITY: usize = 32;
pub const MAX_RECORD_FIELDS: usize = 32;
pub const MAX_VARIANT_CASES: usize = 32;
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
    /// A record name in parsed syntax, resolved before semantic interning.
    RecordRef(Arc<str>),
    Record(Arc<RecordType>),
    /// A variant name in parsed syntax, resolved before semantic interning.
    VariantRef(Arc<str>),
    Variant(Arc<VariantType>),
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum NominalKind {
    Record,
    Variant,
}
impl NominalKind {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Record => "record",
            Self::Variant => "variant",
        }
    }

    pub(crate) fn reference(self, name: Arc<str>) -> SourceType {
        match self {
            Self::Record => SourceType::RecordRef(name),
            Self::Variant => SourceType::VariantRef(name),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct Shape {
    depth: usize,
    nodes: usize,
    expanded: usize,
    display_bytes: usize,
    contains_array: bool,
    has_nominal_ref: bool,
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

/// Immutable layout owned by one resolved program.
#[derive(Debug)]
pub struct RecordType {
    id: TypeId,
    declaration: crate::diagnostics::Span,
    name: Arc<str>,
    fields: Box<[(String, SourceType)]>,
    shape: Shape,
}
impl RecordType {
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn fields(&self) -> &[(String, SourceType)] {
        &self.fields
    }
    pub(crate) fn declaration_span(&self) -> crate::diagnostics::Span {
        self.declaration
    }
    pub fn type_id(&self) -> TypeId {
        self.id
    }
}
impl PartialEq for RecordType {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}
impl Eq for RecordType {}
impl std::hash::Hash for RecordType {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

/// Immutable nominal case table scoped to one compiled program.
#[derive(Debug)]
pub struct VariantType {
    id: TypeId,
    declaration: crate::diagnostics::Span,
    name: Arc<str>,
    cases: Box<[(String, Option<SourceType>)]>,
    shape: Shape,
}
impl VariantType {
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn cases(&self) -> &[(String, Option<SourceType>)] {
        &self.cases
    }
    pub fn type_id(&self) -> TypeId {
        self.id
    }
    pub(crate) fn declaration_span(&self) -> crate::diagnostics::Span {
        self.declaration
    }
}
impl PartialEq for VariantType {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}
impl Eq for VariantType {}
impl std::hash::Hash for VariantType {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

impl SourceType {
    pub(crate) fn variant(
        id: TypeId,
        declaration: crate::diagnostics::Span,
        name: Arc<str>,
        cases: Vec<(String, Option<Self>)>,
    ) -> Option<Self> {
        if cases.is_empty() || cases.len() > MAX_VARIANT_CASES {
            return None;
        }
        let mut shape = Shape {
            depth: 1,
            nodes: 1 + cases.len(),
            expanded: 1,
            display_bytes: "variant ".len() + name.len(),
            contains_array: false,
            has_nominal_ref: false,
        };
        for (_, field) in &cases {
            if let Some(child) = field.as_ref().map(Self::shape) {
                shape.depth = shape.depth.max(child.depth + 1);
                shape.nodes = shape.nodes.checked_add(child.nodes)?;
                shape.expanded = shape.expanded.max(child.expanded.checked_add(1)?);
                shape.contains_array |= child.contains_array;
            }
        }
        Some(Self::Variant(Arc::new(VariantType {
            id,
            declaration,
            name,
            cases: cases.into_boxed_slice(),
            shape: shape.checked()?,
        })))
    }

    pub fn as_variant(&self) -> Option<&VariantType> {
        match self {
            Self::Variant(variant) => Some(variant),
            _ => None,
        }
    }

    pub(crate) fn nominal_declaration(&self) -> Option<crate::diagnostics::Span> {
        match self {
            Self::Record(record) => Some(record.declaration_span()),
            Self::Variant(variant) => Some(variant.declaration_span()),
            _ => None,
        }
    }

    pub(crate) fn record(
        id: TypeId,
        declaration: crate::diagnostics::Span,
        name: Arc<str>,
        fields: Vec<(String, Self)>,
    ) -> Option<Self> {
        if fields.len() > MAX_RECORD_FIELDS {
            return None;
        }
        let mut shape = Shape {
            depth: 1,
            nodes: 1,
            expanded: 1,
            display_bytes: "record ".len() + name.len(),
            contains_array: false,
            has_nominal_ref: false,
        };
        for (_, field) in &fields {
            let child = field.shape();
            shape.depth = shape.depth.max(child.depth + 1);
            shape.nodes = shape.nodes.checked_add(child.nodes)?;
            shape.expanded = shape.expanded.checked_add(child.expanded)?;
            shape.contains_array |= child.contains_array;
        }
        Some(Self::Record(Arc::new(RecordType {
            id,
            declaration,
            name,
            fields: fields.into_boxed_slice(),
            shape: shape.checked()?,
        })))
    }

    pub fn as_record(&self) -> Option<&RecordType> {
        match self {
            Self::Record(record) => Some(record),
            _ => None,
        }
    }

    // Rebuild through the checked constructors so a nominal field cannot hide
    // an oversized shape in a tuple or a zero-length array.
    pub(crate) fn map_nominal_refs(
        &self,
        mapper: &mut impl FnMut(
            NominalKind,
            &str,
            usize,
        ) -> Result<Self, crate::diagnostics::Diagnostic>,
        too_large: &impl Fn() -> crate::diagnostics::Diagnostic,
    ) -> Result<Self, crate::diagnostics::Diagnostic> {
        self.map_nominal_refs_at(0, mapper, too_large)
    }

    fn map_nominal_refs_at(
        &self,
        depth: usize,
        mapper: &mut impl FnMut(
            NominalKind,
            &str,
            usize,
        ) -> Result<Self, crate::diagnostics::Diagnostic>,
        too_large: &impl Fn() -> crate::diagnostics::Diagnostic,
    ) -> Result<Self, crate::diagnostics::Diagnostic> {
        match self {
            Self::RecordRef(name) => mapper(NominalKind::Record, name, depth),
            Self::VariantRef(name) => mapper(NominalKind::Variant, name, depth),
            Self::Tuple(tuple) if tuple.shape.has_nominal_ref => {
                let items = tuple
                    .elements
                    .iter()
                    .map(|t| t.map_nominal_refs_at(depth + 1, mapper, too_large))
                    .collect::<Result<Vec<_>, _>>()?;
                Self::tuple(items).ok_or_else(too_large)
            }
            Self::Array(array) if array.shape.has_nominal_ref => Self::array(
                array
                    .element
                    .map_nominal_refs_at(depth + 1, mapper, too_large)?,
                array.length,
            )
            .ok_or_else(too_large),
            _ => Ok(self.clone()),
        }
    }

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
            has_nominal_ref: false,
        };
        for child in elements.iter().map(Self::shape) {
            shape.depth = shape.depth.max(child.depth + 1);
            shape.nodes = shape.nodes.checked_add(child.nodes)?;
            shape.expanded = shape.expanded.checked_add(child.expanded)?;
            shape.display_bytes = shape.display_bytes.checked_add(child.display_bytes)?;
            shape.contains_array |= child.contains_array;
            shape.has_nominal_ref |= child.has_nominal_ref;
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
            has_nominal_ref: child.has_nominal_ref,
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

    pub(crate) fn has_nominal_ref(&self) -> bool {
        self.shape().has_nominal_ref
    }

    pub(crate) fn display_bytes(&self) -> usize {
        self.shape().display_bytes
    }

    fn shape(&self) -> Shape {
        match self {
            Self::Tuple(t) => t.shape,
            Self::Array(a) => a.shape,
            Self::Record(r) => r.shape,
            Self::Variant(v) => v.shape,
            Self::RecordRef(name) | Self::VariantRef(name) => Shape {
                depth: 0,
                nodes: 1,
                expanded: 1,
                display_bytes: if matches!(self, Self::RecordRef(_)) {
                    "record ".len()
                } else {
                    "variant ".len()
                } + name.len(),
                contains_array: false,
                has_nominal_ref: true,
            },
            Self::Primitive(p) => Shape {
                depth: 0,
                nodes: 1,
                expanded: 1,
                display_bytes: p.name().len(),
                contains_array: false,
                has_nominal_ref: false,
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
            Self::RecordRef(_) => Err(TypeRegistryError::UnresolvedRecord),
            Self::VariantRef(_) => Err(TypeRegistryError::UnresolvedVariant),
            Self::Variant(variant) => registry
                .depth(variant.id)
                .map(|_| variant.id)
                .ok_or(TypeRegistryError::UnknownType(variant.id)),
            Self::Record(record) => registry
                .depth(record.id)
                .map(|_| record.id)
                .ok_or(TypeRegistryError::UnknownType(record.id)),
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
            Self::RecordRef(name) => write!(f, "record {name}"),
            Self::Record(record) => write!(f, "record {}", record.name()),
            Self::VariantRef(name) => write!(f, "variant {name}"),
            Self::Variant(variant) => write!(f, "variant {}", variant.name()),
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
