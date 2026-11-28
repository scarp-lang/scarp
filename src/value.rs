//! Runtime values, finite numerical storage and inferred binding types.

use std::{
    fmt,
    sync::{Arc, OnceLock},
};

use crate::diagnostics::{Diagnostic, DiagnosticKind, Span};

pub use crate::types::PrimitiveType as ValueType;
use crate::types::SourceType;
mod array;
pub(crate) mod fixed_integer;
pub use fixed_integer::FixedInteger;
mod list;
mod map;
mod record;
mod tensor;
mod tuple;
mod variant;
pub use array::ArrayValue;
pub use list::ListValue;
pub use map::MapValue;
pub(crate) use map::{key_compare, key_work};
pub use record::RecordValue;
pub(crate) use tensor::element_count as tensor_element_count;
pub use tensor::{NumericTensor, TensorValue, MAX_TENSOR_RANK};
pub use tuple::TupleValue;
pub use variant::VariantValue;

pub const MAX_TEXT_BYTES: usize = 1024 * 1024;

/// A finite IEEE-754 binary64 scalar. Private storage excludes NaN and infinity.
/// Equality is numeric: signed zeros compare equal; `get().to_bits()` and
/// display retain their sign. This type deliberately does not implement Hash.
#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FiniteFloat(f64);

// Finite numeric equality is reflexive, symmetric and transitive. NaN, the
// floating-point counterexample to Eq, cannot be constructed through this API.
impl Eq for FiniteFloat {}

impl FiniteFloat {
    /// Reject NaN and either infinity, including values supplied by a Rust host.
    pub fn new(value: f64) -> Option<Self> {
        if value.is_finite() {
            Some(Self(value))
        } else {
            None
        }
    }

    /// Inspect the finite payload without permitting mutation of its invariant.
    pub fn get(self) -> f64 {
        self.0
    }
}

impl fmt::Display for FiniteFloat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Shortest round-trip digits, a float marker, and preserved signed zero.
        write!(f, "{:?}", self.0)
    }
}

/// Maximum elements in either a source-created or host-created numeric vector.
pub const MAX_VECTOR_LENGTH: usize = 65_536;

/// Immutable finite elements with an optional visible window into shared
/// storage. Value identity is its visible numbers, never its backing buffer.
/// No user-visible pointer, lifetime or ownership operation is exposed.
#[derive(Debug, Clone)]
pub struct NumericVector {
    storage: Arc<Vec<FiniteFloat>>,
    offset: usize,
    length: usize,
}

impl PartialEq for NumericVector {
    fn eq(&self, other: &Self) -> bool {
        self.as_slice() == other.as_slice()
    }
}
impl Eq for NumericVector {}

impl NumericVector {
    /// Reject oversized/nonfinite input or failed buffer reservation.
    pub fn new(values: &[f64]) -> Option<Self> {
        if values.len() > MAX_VECTOR_LENGTH {
            return None;
        }
        let mut finite = Vec::new();
        finite.try_reserve_exact(values.len()).ok()?;
        for &value in values {
            finite.push(FiniteFloat::new(value)?);
        }
        Self::from_finite(finite)
    }

    /// Consume an already checked finite buffer without copying its elements.
    pub fn from_finite(values: Vec<FiniteFloat>) -> Option<Self> {
        if values.len() > MAX_VECTOR_LENGTH {
            return None;
        }
        let length = values.len();
        let storage = if values.is_empty() {
            // Canonical immutable empty storage avoids repeated allocations and
            // lets an empty view release a much larger original backing buffer.
            static EMPTY: OnceLock<Arc<Vec<FiniteFloat>>> = OnceLock::new();
            Arc::clone(EMPTY.get_or_init(|| Arc::new(Vec::new())))
        } else {
            Arc::new(values)
        };
        Some(Self {
            length,
            offset: 0,
            storage,
        })
    }

    /// Select a contiguous range without copying numerical elements.
    /// Nonempty windows share the original allocation. Empty windows instead
    /// share canonical empty storage and cannot pin the source buffer.
    pub fn window(&self, start: usize, length: usize) -> Option<Self> {
        if start.checked_add(length)? > self.length {
            return None;
        }
        if length == 0 {
            return Self::from_finite(Vec::new());
        }
        Some(Self {
            storage: Arc::clone(&self.storage),
            offset: self.offset.checked_add(start)?,
            length,
        })
    }

    pub fn as_slice(&self) -> &[FiniteFloat] {
        &self.storage[self.offset..self.offset + self.length]
    }
    pub fn len(&self) -> usize {
        self.length
    }
    pub fn is_empty(&self) -> bool {
        self.length == 0
    }
}

impl fmt::Display for NumericVector {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("[")?;
        for (index, value) in self.as_slice().iter().enumerate() {
            if index > 0 {
                f.write_str(", ")?;
            }
            value.fmt(f)?;
        }
        f.write_str("]")
    }
}

#[derive(Debug, Clone)]
pub struct IntegerVector {
    storage: Arc<Vec<i64>>,
    offset: usize,
    length: usize,
}
impl PartialEq for IntegerVector {
    fn eq(&self, other: &Self) -> bool {
        self.as_slice() == other.as_slice()
    }
}
impl Eq for IntegerVector {}
impl IntegerVector {
    pub fn new(values: &[i64]) -> Option<Self> {
        if values.len() > MAX_VECTOR_LENGTH {
            return None;
        }
        let mut owned = Vec::new();
        owned.try_reserve_exact(values.len()).ok()?;
        owned.extend_from_slice(values);
        Self::from_values(owned)
    }
    pub fn from_values(values: Vec<i64>) -> Option<Self> {
        if values.len() > MAX_VECTOR_LENGTH {
            return None;
        }
        let length = values.len();
        let storage = if values.is_empty() {
            static EMPTY: OnceLock<Arc<Vec<i64>>> = OnceLock::new();
            Arc::clone(EMPTY.get_or_init(|| Arc::new(Vec::new())))
        } else {
            Arc::new(values)
        };
        Some(Self {
            length,
            offset: 0,
            storage,
        })
    }
    /// Nonempty windows share storage; empty windows detach from their source.
    pub fn window(&self, start: usize, length: usize) -> Option<Self> {
        if start.checked_add(length)? > self.length {
            return None;
        }
        if length == 0 {
            return Self::from_values(Vec::new());
        }
        Some(Self {
            storage: Arc::clone(&self.storage),
            offset: self.offset.checked_add(start)?,
            length,
        })
    }
    pub fn as_slice(&self) -> &[i64] {
        &self.storage[self.offset..self.offset + self.length]
    }
    pub fn len(&self) -> usize {
        self.length
    }
    pub fn is_empty(&self) -> bool {
        self.length == 0
    }
}
impl fmt::Display for IntegerVector {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("[integer:")?;
        for (index, value) in self.as_slice().iter().enumerate() {
            f.write_str(if index == 0 { " " } else { ", " })?;
            value.fmt(f)?;
        }
        f.write_str("]")
    }
}

/// Immutable row-major rectangular numerical value with an explicit shape.
/// The cells reuse a finite vector's shared backing storage without copying.
/// Zero-sized shapes are allowed and remain distinguishable by their axes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NumericTile {
    cells: NumericVector,
    height: usize,
    width: usize,
}

impl NumericTile {
    /// Preserve the original immutable vector without copying its elements.
    /// Both dimensions and their product are bounded by MAX_VECTOR_LENGTH.
    pub fn from_vector(cells: NumericVector, height: usize, width: usize) -> Option<Self> {
        if height > MAX_VECTOR_LENGTH || width > MAX_VECTOR_LENGTH {
            return None;
        }
        if height.checked_mul(width)? != cells.len() {
            return None;
        }
        Some(Self {
            cells,
            height,
            width,
        })
    }

    pub fn new(values: &[f64], height: usize, width: usize) -> Option<Self> {
        if height > MAX_VECTOR_LENGTH
            || width > MAX_VECTOR_LENGTH
            || height.checked_mul(width)? != values.len()
        {
            return None;
        }
        Self::from_vector(NumericVector::new(values)?, height, width)
    }

    pub fn height(&self) -> usize {
        self.height
    }

    pub fn width(&self) -> usize {
        self.width
    }

    pub fn len(&self) -> usize {
        self.cells.len()
    }

    pub fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }

    pub fn cells(&self) -> &[FiniteFloat] {
        self.cells.as_slice()
    }

    pub fn as_vector(&self) -> &NumericVector {
        &self.cells
    }

    pub fn cell(&self, row: usize, column: usize) -> Option<FiniteFloat> {
        if row >= self.height || column >= self.width {
            return None;
        }
        self.cells
            .as_slice()
            .get(row.checked_mul(self.width)?.checked_add(column)?)
            .copied()
    }

    pub fn row(&self, row: usize) -> Option<&[FiniteFloat]> {
        if row >= self.height {
            return None;
        }
        let start = row.checked_mul(self.width)?;
        self.cells
            .as_slice()
            .get(start..start.checked_add(self.width)?)
    }
}

impl fmt::Display for NumericTile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "tile({}x{})[", self.height, self.width)?;
        for row in 0..self.height {
            if row != 0 {
                f.write_str(", ")?;
            }
            f.write_str("[")?;
            for (index, value) in self
                .row(row)
                .expect("validated tile row")
                .iter()
                .enumerate()
            {
                if index != 0 {
                    f.write_str(", ")?;
                }
                value.fmt(f)?;
            }
            f.write_str("]")?;
        }
        f.write_str("]")
    }
}

pub(crate) fn finite_number(value: f64, span: Span) -> Result<FiniteFloat, Diagnostic> {
    FiniteFloat::new(value).ok_or_else(|| {
        Diagnostic::new(
            DiagnosticKind::NonFiniteNumber,
            span,
            "numeric operation produced a non-finite result",
        )
        .with_help("keep every intermediate result finite")
    })
}

/// Strings share immutable storage. Reassignment replaces a value and cannot
/// mutate its copies or values already captured for output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    /// Internal marker; rejected in ordinary value contexts.
    Unit,
    Integer(i64),
    FixedInteger(FixedInteger),
    Float(FiniteFloat),
    Vector(NumericVector),
    Integers(IntegerVector),
    Tile(NumericTile),
    Tensor(TensorValue),
    Boolean(bool),
    String(Arc<str>),
    Tuple(TupleValue),
    BigInt(crate::bigint::BigInt),
    Decimal(crate::decimal::Decimal),
    Array(ArrayValue),
    List(ListValue),
    Map(MapValue),
    Record(RecordValue),
    Variant(VariantValue),
}

impl Value {
    /// Logical string bytes count each contained occurrence, including shared
    /// values. Immutable aggregate storage caches the fold at construction so
    /// host admission does not traverse a potentially large shared value graph.
    pub(crate) fn host_utf8_bytes(&self) -> usize {
        let mut value = self;
        while let Self::Variant(variant) = value {
            match variant.payload() {
                Some(payload) => value = payload,
                None => return 0,
            }
        }
        match value {
            Self::String(text) => text.len(),
            Self::Tuple(tuple) => tuple.host_utf8_bytes(),
            Self::Array(array) => array.host_utf8_bytes(),
            Self::List(list) => list.host_utf8_bytes(),
            Self::Map(map) => map.host_utf8_bytes(),
            Self::Record(record) => record.host_utf8_bytes(),
            _ => 0,
        }
    }

    /// Exact structural value type; use as_primitive() for legacy primitive classification.
    pub fn kind(&self) -> SourceType {
        match self {
            Self::Tuple(value) => value.kind().clone(),
            Self::BigInt(_) => ValueType::BigInt.into(),
            Self::Decimal(_) => ValueType::Decimal.into(),
            Self::Array(value) => value.kind().clone(),
            Self::List(value) => value.kind().clone(),
            Self::Map(value) => value.kind().clone(),
            Self::Record(value) => value.kind().clone(),
            Self::Variant(value) => value.kind().clone(),
            Self::Unit => ValueType::Unit.into(),
            Self::Integer(_) => ValueType::Integer.into(),
            Self::FixedInteger(value) => value.kind().into(),
            Self::Float(_) => ValueType::Float.into(),
            Self::Vector(_) => ValueType::Vector.into(),
            Self::Integers(_) => ValueType::Integers.into(),
            Self::Tile(_) => ValueType::Tile.into(),
            Self::Tensor(value) => value.kind().clone(),
            Self::Boolean(_) => ValueType::Boolean.into(),
            Self::String(_) => ValueType::String.into(),
        }
    }

    pub fn as_tuple(&self) -> Option<&TupleValue> {
        if let Self::Tuple(value) = self {
            Some(value)
        } else {
            None
        }
    }

    pub fn as_array(&self) -> Option<&ArrayValue> {
        match self {
            Self::Array(value) => Some(value),
            _ => None,
        }
    }

    pub fn as_list(&self) -> Option<&ListValue> {
        match self {
            Self::List(value) => Some(value),
            _ => None,
        }
    }

    pub fn as_map(&self) -> Option<&MapValue> {
        match self {
            Self::Map(value) => Some(value),
            _ => None,
        }
    }

    pub(crate) fn composite_work(&self) -> Option<u64> {
        match self {
            Self::Tuple(value) => Some(value.work()),
            Self::Tensor(value) => Some(value.work()),
            Self::Array(value) => Some(value.work()),
            Self::List(value) => Some(value.work()),
            Self::Map(value) => Some(value.work()),
            Self::Record(value) => Some(value.work()),
            Self::Variant(value) => Some(value.work()),
            _ => None,
        }
    }

    pub fn as_fixed_integer(&self) -> Option<FixedInteger> {
        match self {
            Self::FixedInteger(value) => Some(*value),
            _ => None,
        }
    }

    pub fn as_integer(&self) -> Option<i64> {
        match self {
            Self::Integer(value) => Some(*value),
            _ => None,
        }
    }

    pub fn as_bigint(&self) -> Option<&crate::bigint::BigInt> {
        match self {
            Self::BigInt(value) => Some(value),
            _ => None,
        }
    }

    pub fn as_decimal(&self) -> Option<crate::decimal::Decimal> {
        match self {
            Self::Decimal(value) => Some(*value),
            _ => None,
        }
    }

    pub fn as_float(&self) -> Option<f64> {
        match self {
            Self::Float(value) => Some(value.get()),
            _ => None,
        }
    }

    pub fn as_vector(&self) -> Option<&NumericVector> {
        match self {
            Self::Vector(value) => Some(value),
            _ => None,
        }
    }

    pub fn as_tensor(&self) -> Option<&NumericTensor> {
        match self {
            Self::Tensor(value) => Some(value.tensor()),
            _ => None,
        }
    }

    pub fn as_tile(&self) -> Option<&NumericTile> {
        match self {
            Self::Tile(value) => Some(value),
            _ => None,
        }
    }
    pub fn as_integers(&self) -> Option<&IntegerVector> {
        match self {
            Self::Integers(value) => Some(value),
            _ => None,
        }
    }
    pub(crate) fn require_integers(&self, span: Span) -> Result<&IntegerVector, Diagnostic> {
        self.as_integers()
            .ok_or_else(|| type_mismatch(span, ValueType::Integers, self.kind()))
    }

    pub(crate) fn require_tile(&self, span: Span) -> Result<&NumericTile, Diagnostic> {
        self.as_tile()
            .ok_or_else(|| type_mismatch(span, ValueType::Tile, self.kind()))
    }

    pub(crate) fn require_vector(&self, span: Span) -> Result<&NumericVector, Diagnostic> {
        self.as_vector()
            .ok_or_else(|| type_mismatch(span, ValueType::Vector, self.kind()))
    }

    pub(crate) fn require_float(&self, span: Span) -> Result<f64, Diagnostic> {
        self.as_float()
            .ok_or_else(|| type_mismatch(span, ValueType::Float, self.kind()))
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(value) => Some(value),
            _ => None,
        }
    }

    pub fn as_boolean(&self) -> Option<bool> {
        match self {
            Self::Boolean(value) => Some(*value),
            _ => None,
        }
    }

    pub(crate) fn require_boolean(&self, span: Span) -> Result<bool, Diagnostic> {
        self.as_boolean()
            .ok_or_else(|| type_mismatch(span, ValueType::Boolean, self.kind()))
    }

    pub(crate) fn require_integer(&self, span: Span) -> Result<i64, Diagnostic> {
        self.as_integer()
            .ok_or_else(|| type_mismatch(span, ValueType::Integer, self.kind()))
    }
}

impl From<&str> for Value {
    fn from(value: &str) -> Self {
        Self::String(Arc::from(value))
    }
}

impl From<String> for Value {
    fn from(value: String) -> Self {
        Self::String(Arc::from(value))
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Tuple(value) => value.fmt(f),
            Self::BigInt(value) => value.fmt(f),
            Self::Decimal(value) => value.fmt(f),
            Self::Array(value) => value.fmt(f),
            Self::List(value) => value.fmt(f),
            Self::Map(value) => value.fmt(f),
            Self::Record(value) => value.fmt(f),
            Self::Variant(value) => value.fmt(f),
            Self::Unit => f.write_str("unit"),
            Self::Integer(value) => value.fmt(f),
            Self::FixedInteger(value) => value.fmt(f),
            Self::Float(value) => value.fmt(f),
            Self::Vector(value) => value.fmt(f),
            Self::Integers(value) => value.fmt(f),
            Self::Tile(value) => value.fmt(f),
            Self::Tensor(value) => value.fmt(f),
            Self::Boolean(value) => value.fmt(f),
            Self::String(value) => f.write_str(value),
        }
    }
}

pub(crate) use crate::types::type_mismatch;

#[cfg(test)]
mod lifetime_storage_tests {
    use super::{IntegerVector, NumericVector, TupleValue, Value};
    use std::sync::Arc;

    #[test]
    fn empty_numeric_windows_do_not_retain_large_backing_buffers() {
        let source = NumericVector::new(&vec![1.5; 8192]).unwrap();
        let backing = Arc::downgrade(&source.storage);
        let nonempty = source.window(2, 4).unwrap();
        let empty = source.window(2, 0).unwrap();
        let at_end = source.window(source.len(), 0).unwrap();
        let empty_clone = empty.clone();

        assert_eq!(nonempty.len(), 4);
        assert_eq!(empty.len(), 0);
        assert_eq!(empty.as_slice(), &[]);
        assert!(Arc::ptr_eq(&empty.storage, &at_end.storage));
        assert_eq!(Arc::strong_count(&source.storage), 2);
        assert!(source.window(source.len() + 1, 0).is_none());
        assert!(source.window(usize::MAX, 1).is_none());

        drop(source);
        assert!(
            backing.upgrade().is_some(),
            "nonempty view retains the source"
        );
        drop(nonempty);
        assert!(
            backing.upgrade().is_none(),
            "empty views must not pin source"
        );
        assert_eq!(empty_clone, at_end);
    }

    #[test]
    fn empty_integer_windows_detach_including_nested_windows() {
        let source = IntegerVector::new(&[1, 2, 3, 4]).unwrap();
        let backing = Arc::downgrade(&source.storage);
        let nonempty = source.window(1, 2).unwrap();
        let empty = nonempty.window(nonempty.len(), 0).unwrap();
        let independent_empty = IntegerVector::from_values(Vec::new()).unwrap();

        assert!(Arc::ptr_eq(&empty.storage, &independent_empty.storage));
        assert_eq!(empty.as_slice(), &[]);
        assert_eq!(nonempty.as_slice(), &[2, 3]);
        assert_eq!(Arc::strong_count(&source.storage), 2);
        assert!(nonempty.window(3, 0).is_none());

        drop(source);
        assert!(backing.upgrade().is_some());
        drop(nonempty);
        assert!(backing.upgrade().is_none());
        assert_eq!(empty.len(), 0);
    }

    #[test]
    fn nested_composite_aliases_retain_nonempty_storage_until_final_drop() {
        let source = NumericVector::new(&[2.0, 3.0]).unwrap();
        let backing = Arc::downgrade(&source.storage);
        let tuple = TupleValue::new(vec![Value::Vector(source.clone())]).unwrap();
        let alias = tuple.clone();
        drop(source);
        drop(tuple);
        assert!(
            backing.upgrade().is_some(),
            "tuple alias keeps vector alive"
        );
        drop(alias);
        assert!(
            backing.upgrade().is_none(),
            "final composite drop releases vector"
        );
    }
}
