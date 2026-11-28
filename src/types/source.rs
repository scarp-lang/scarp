//! Portable structural source types. Composite TypeIds remain arena-local.
use super::{
    DefinitionId, PrimitiveType, TypeConstraint, TypeId, TypeKind, TypeRegistryBuilder,
    TypeRegistryError,
};
use std::{fmt, sync::Arc};

pub const MAX_TUPLE_ARITY: usize = 32;
pub const MAX_RECORD_FIELDS: usize = 32;
pub const MAX_VARIANT_CASES: usize = 32;
pub const MAX_COMPOSITE_DEPTH: usize = 32;
pub const MAX_TUPLE_DEPTH: usize = MAX_COMPOSITE_DEPTH;
pub const MAX_SOURCE_TYPE_NODES: usize = 4096;
pub const MAX_ARRAY_LENGTH: usize = 4096;
/// Maximum elements in an immutable source-level typed dynamic list.
pub const MAX_LIST_LENGTH: usize = 4096;
/// Maximum entries in one sorted, immutable source-level map.
pub const MAX_MAP_ENTRIES: usize = 1024;
pub const MAX_EXPANDED_VALUE_NODES: usize = 65_536;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum SourceType {
    Primitive(PrimitiveType),
    Parameter(Arc<TypeParameter>),
    Tuple(Arc<TupleType>),
    Array(Arc<ArrayType>),
    Tensor(Arc<TensorType>),
    List(Arc<ListType>),
    Map(Arc<MapType>),
    /// A record name in parsed syntax, resolved before semantic interning.
    RecordRef(Arc<str>),
    Record(Arc<RecordType>),
    /// A variant name in parsed syntax, resolved before semantic interning.
    VariantRef(Arc<str>),
    VariantApplication(Arc<VariantApplication>),
    Variant(Arc<VariantType>),
}

/// A lexical type parameter. Parsed references have no semantic binder yet.
#[derive(Debug, PartialEq, Eq, Hash)]
pub struct TypeParameter {
    pub(crate) id: Option<TypeId>,
    pub(crate) index: usize,
    pub(crate) name: Arc<str>,
    pub(crate) constraint: Option<TypeConstraint>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum BuiltinVariant {
    Option,
    Result,
}

impl BuiltinVariant {
    pub(crate) fn definition(self) -> DefinitionId {
        match self {
            Self::Option => DefinitionId::BUILTIN_OPTION,
            Self::Result => DefinitionId::BUILTIN_RESULT,
        }
    }

    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Option => "option",
            Self::Result => "result",
        }
    }

    pub(crate) fn cases(
        self,
        first: SourceType,
        second: Option<SourceType>,
    ) -> Vec<(String, Option<SourceType>)> {
        match self {
            Self::Option => vec![("None".into(), None), ("Some".into(), Some(first))],
            Self::Result => vec![("Ok".into(), Some(first)), ("Err".into(), second)],
        }
    }
}

/// Parsed Option/Result application whose arguments still contain type parameters.
#[derive(Debug, PartialEq, Eq, Hash)]
pub struct VariantApplication {
    family: BuiltinVariant,
    first: SourceType,
    second: Option<SourceType>,
    shape: Shape,
}

impl fmt::Display for VariantApplication {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}<{}", self.family.name(), self.first)?;
        if let Some(second) = &self.second {
            write!(f, ", {second}")?;
        }
        f.write_str(">")
    }
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
    has_parameter: bool,
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

/// A validated shape for an immutable finite float tensor.
#[derive(Debug, PartialEq, Eq, Hash)]
pub struct TensorType {
    dimensions: Box<[usize]>,
    cells: usize,
}

impl TensorType {
    pub fn dimensions(&self) -> &[usize] {
        &self.dimensions
    }

    pub fn cell_count(&self) -> usize {
        self.cells
    }
}

/// A dynamically sized, immutable list with one exact element type.
/// The runtime limits visible entries separately from this type descriptor.
#[derive(Debug, PartialEq, Eq, Hash)]
pub struct ListType {
    element: SourceType,
    shape: Shape,
}
impl ListType {
    pub fn element(&self) -> &SourceType {
        &self.element
    }
}

/// Deterministic typed map. Key types are integer, boolean or string.
/// Values use the same bounded composite type model as lists and arrays.
#[derive(Debug, PartialEq, Eq, Hash)]
pub struct MapType {
    key: SourceType,
    value: SourceType,
    shape: Shape,
}
impl MapType {
    pub fn key(&self) -> &SourceType {
        &self.key
    }
    pub fn value(&self) -> &SourceType {
        &self.value
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
    pub(crate) fn builtin_arguments(
        &self,
    ) -> Option<(BuiltinVariant, &SourceType, Option<&SourceType>)> {
        match self.cases.as_ref() {
            [(_, None), (_, Some(first))] if self.name.starts_with("option<") => {
                Some((BuiltinVariant::Option, first, None))
            }
            [(_, Some(first)), (_, Some(second))] if self.name.starts_with("result<") => {
                Some((BuiltinVariant::Result, first, Some(second)))
            }
            _ => None,
        }
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
    pub(crate) fn variant_application(
        family: BuiltinVariant,
        first: Self,
        second: Option<Self>,
    ) -> Option<Self> {
        let mut shape = Shape {
            depth: 1,
            nodes: 3,
            expanded: 1,
            display_bytes: "variant <>".len() + family.name().len(),
            contains_array: false,
            has_nominal_ref: false,
            has_parameter: false,
        };
        for child in std::iter::once(&first)
            .chain(second.iter())
            .map(Self::shape)
        {
            shape.depth = shape.depth.max(child.depth + 1);
            shape.nodes = shape.nodes.checked_add(child.nodes)?;
            shape.expanded = shape.expanded.max(child.expanded.checked_add(1)?);
            shape.display_bytes = shape.display_bytes.checked_add(child.display_bytes + 2)?;
            shape.contains_array |= child.contains_array;
            shape.has_nominal_ref |= child.has_nominal_ref;
            shape.has_parameter |= child.has_parameter;
        }
        Some(Self::VariantApplication(Arc::new(VariantApplication {
            family,
            first,
            second,
            shape: shape.checked()?,
        })))
    }

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
            has_parameter: false,
        };
        for (_, field) in &cases {
            if let Some(child) = field.as_ref().map(Self::shape) {
                shape.depth = shape.depth.max(child.depth + 1);
                shape.nodes = shape.nodes.checked_add(child.nodes)?;
                shape.expanded = shape.expanded.max(child.expanded.checked_add(1)?);
                shape.contains_array |= child.contains_array;
                shape.has_parameter |= child.has_parameter;
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
            has_parameter: false,
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
            Self::VariantApplication(application) if application.shape.has_nominal_ref => {
                let first = application
                    .first
                    .map_nominal_refs_at(depth + 1, mapper, too_large)?;
                let second = application
                    .second
                    .as_ref()
                    .map(|second| second.map_nominal_refs_at(depth + 1, mapper, too_large))
                    .transpose()?;
                Self::variant_application(application.family, first, second).ok_or_else(too_large)
            }

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
            Self::List(list) if list.shape.has_nominal_ref => Self::list(
                list.element
                    .map_nominal_refs_at(depth + 1, mapper, too_large)?,
            )
            .ok_or_else(too_large),
            Self::Map(map) if map.shape.has_nominal_ref => Self::map(
                map.key.map_nominal_refs_at(depth + 1, mapper, too_large)?,
                map.value
                    .map_nominal_refs_at(depth + 1, mapper, too_large)?,
            )
            .ok_or_else(too_large),
            _ => Ok(self.clone()),
        }
    }

    pub(crate) fn has_parameter(&self) -> bool {
        self.shape().has_parameter
    }

    pub(crate) fn map_parameters<E>(
        &self,
        mapper: &mut impl FnMut(&TypeParameter) -> Result<Self, E>,
        variant: &mut impl FnMut(BuiltinVariant, Self, Option<Self>) -> Result<Self, E>,
        too_large: &impl Fn() -> E,
    ) -> Result<Self, E> {
        if !self.has_parameter() {
            return Ok(self.clone());
        }
        let mut map = |ty: &Self| ty.map_parameters(mapper, variant, too_large);
        let application = match self {
            Self::Parameter(parameter) => return mapper(parameter),
            Self::Tuple(tuple) => {
                return Self::tuple(
                    tuple
                        .elements
                        .iter()
                        .map(map)
                        .collect::<Result<Vec<_>, _>>()?,
                )
                .ok_or_else(too_large)
            }
            Self::Array(array) => {
                return Self::array(map(&array.element)?, array.length).ok_or_else(too_large)
            }
            Self::List(list) => return Self::list(map(&list.element)?).ok_or_else(too_large),
            Self::Map(map_type) => {
                return Self::map(&map_type.key, map(&map_type.value)?).ok_or_else(too_large)
            }
            Self::VariantApplication(application) => Some((
                application.family,
                &application.first,
                application.second.as_ref(),
            )),
            Self::Variant(application) => application.builtin_arguments(),
            _ => None,
        };
        let Some((family, first, second)) = application else {
            return Ok(self.clone());
        };
        let first = map(first)?;
        let second = second.map(map).transpose()?;
        variant(family, first, second)
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
            has_parameter: false,
        };
        for child in elements.iter().map(Self::shape) {
            shape.depth = shape.depth.max(child.depth + 1);
            shape.nodes = shape.nodes.checked_add(child.nodes)?;
            shape.expanded = shape.expanded.checked_add(child.expanded)?;
            shape.display_bytes = shape.display_bytes.checked_add(child.display_bytes)?;
            shape.contains_array |= child.contains_array;
            shape.has_nominal_ref |= child.has_nominal_ref;
            shape.has_parameter |= child.has_parameter;
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
            has_parameter: child.has_parameter,
        }
        .checked()?;
        Some(Self::Array(Arc::new(ArrayType {
            element,
            length,
            shape,
        })))
    }
    /// Build a bounded type descriptor; list length is checked at runtime.
    pub fn list(element: impl Into<Self>) -> Option<Self> {
        let element = element.into();
        if element == PrimitiveType::Unit {
            return None;
        }
        let child = element.shape();
        let shape = Shape {
            depth: child.depth.checked_add(1)?,
            nodes: child.nodes.checked_add(1)?,
            expanded: child.expanded.checked_add(1)?,
            display_bytes: child.display_bytes.checked_add("list<>".len())?,
            contains_array: true,
            has_nominal_ref: child.has_nominal_ref,
            has_parameter: child.has_parameter,
        }
        .checked()?;
        Some(Self::List(Arc::new(ListType { element, shape })))
    }

    pub fn as_list(&self) -> Option<&ListType> {
        match self {
            Self::List(value) => Some(value),
            _ => None,
        }
    }

    /// Keys have a canonical total order and cannot be floating-point or
    /// composite types. Values may be any bounded non-unit source type.
    pub fn map(key: impl Into<Self>, value: impl Into<Self>) -> Option<Self> {
        let key = key.into();
        let value = value.into();
        if !matches!(
            key,
            Self::Primitive(PrimitiveType::Integer)
                | Self::Primitive(PrimitiveType::Boolean)
                | Self::Primitive(PrimitiveType::String)
        ) || value == PrimitiveType::Unit
        {
            return None;
        }
        let key_shape = key.shape();
        let value_shape = value.shape();
        let shape = Shape {
            depth: key_shape.depth.max(value_shape.depth).checked_add(1)?,
            nodes: key_shape
                .nodes
                .checked_add(value_shape.nodes)?
                .checked_add(1)?,
            expanded: value_shape.expanded.checked_add(2)?,
            display_bytes: key_shape
                .display_bytes
                .checked_add(value_shape.display_bytes)?
                .checked_add("map<, >".len())?,
            contains_array: true,
            has_nominal_ref: value_shape.has_nominal_ref,
            has_parameter: value_shape.has_parameter,
        }
        .checked()?;
        Some(Self::Map(Arc::new(MapType { key, value, shape })))
    }

    pub fn as_map(&self) -> Option<&MapType> {
        match self {
            Self::Map(map) => Some(map),
            _ => None,
        }
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
    pub fn tensor(dimensions: &[usize]) -> Option<Self> {
        let cells = crate::value::tensor_element_count(dimensions)?;
        Some(Self::Tensor(Arc::new(TensorType {
            dimensions: dimensions.into(),
            cells,
        })))
    }

    pub fn as_tensor(&self) -> Option<&TensorType> {
        match self {
            Self::Tensor(tensor) => Some(tensor),
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
            Self::Tensor(t) => Shape {
                depth: 1,
                nodes: 2 + t.dimensions.len(),
                expanded: 1,
                display_bytes: "tensor<float>".len() + 7 * t.dimensions.len(),
                contains_array: false,
                has_nominal_ref: false,
                has_parameter: false,
            },
            Self::List(l) => l.shape,
            Self::Map(m) => m.shape,
            Self::Record(r) => r.shape,
            Self::Variant(v) => v.shape,
            Self::VariantApplication(a) => a.shape,
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
                has_parameter: false,
            },
            Self::Parameter(parameter) => Shape {
                depth: 0,
                nodes: 1,
                expanded: 1,
                display_bytes: parameter.name.len(),
                contains_array: false,
                has_nominal_ref: false,
                has_parameter: true,
            },
            Self::Primitive(p) => Shape {
                depth: 0,
                nodes: 1,
                expanded: 1,
                display_bytes: p.name().len(),
                contains_array: false,
                has_nominal_ref: false,
                has_parameter: false,
            },
        }
    }
    pub(crate) fn is_numeric(&self) -> bool {
        self.as_primitive().is_some_and(PrimitiveType::is_numeric)
            || matches!(self, Self::Parameter(p) if p.constraint == Some(TypeConstraint::Numeric))
    }
    pub(crate) fn is_ordered(&self) -> bool {
        self.is_numeric()
            || matches!(self, Self::Parameter(p) if p.constraint == Some(TypeConstraint::Comparable))
            || self
                .tuple_elements()
                .is_some_and(|items| items.iter().all(Self::is_ordered))
    }

    pub(crate) fn intern(
        &self,
        registry: &mut TypeRegistryBuilder,
    ) -> Result<TypeId, TypeRegistryError> {
        match self {
            Self::Primitive(p) => Ok(p.id()),
            Self::Parameter(parameter) => {
                let id = parameter.id.ok_or(TypeRegistryError::UnresolvedParameter)?;
                registry
                    .depth(id)
                    .map(|_| id)
                    .ok_or(TypeRegistryError::UnknownType(id))
            }
            Self::RecordRef(_) => Err(TypeRegistryError::UnresolvedRecord),
            Self::VariantRef(_) | Self::VariantApplication(_) => {
                Err(TypeRegistryError::UnresolvedVariant)
            }
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
            Self::Tensor(t) => registry.intern(TypeKind::Tensor {
                element: PrimitiveType::Float.id(),
                dimensions: t.dimensions.iter().map(|&n| Some(n as u64)).collect(),
            }),
            Self::Array(a) => {
                let element = a.element.intern(registry)?;
                registry.intern(TypeKind::Sequence {
                    element,
                    length: Some(a.length as u64),
                })
            }
            Self::List(l) => {
                let element = l.element.intern(registry)?;
                registry.intern(TypeKind::Sequence {
                    element,
                    length: None,
                })
            }
            Self::Map(map) => {
                let key = map.key.intern(registry)?;
                let value = map.value.intern(registry)?;
                registry.intern(TypeKind::Mapping { key, value })
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
            Self::Parameter(parameter) => f.write_str(&parameter.name),
            Self::RecordRef(name) => write!(f, "record {name}"),
            Self::Record(record) => write!(f, "record {}", record.name()),
            Self::VariantRef(name) => write!(f, "variant {name}"),
            Self::VariantApplication(application) => write!(f, "variant {application}"),
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
            Self::Tensor(t) => {
                f.write_str("tensor<float")?;
                for axis in t.dimensions.iter() {
                    write!(f, ", {axis}")?;
                }
                f.write_str(">")
            }
            Self::List(l) => write!(f, "list<{}>", l.element),
            Self::Map(map) => write!(f, "map<{}, {}>", map.key, map.value),
        }
    }
}
