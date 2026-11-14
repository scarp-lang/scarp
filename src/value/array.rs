//! Shared, immutable fixed-array values.

use super::{
    tuple::{display_nested, value_capture, value_work},
    Value,
};
use crate::types::SourceType;
use std::{fmt, mem::size_of, sync::Arc};

#[derive(Debug, Clone)]
pub struct ArrayValue(Arc<ArrayStorage>);

#[derive(Debug)]
struct ArrayStorage {
    // Keep the fallibly reserved buffer without shrinking or copying it.
    elements: Vec<Value>,
    kind: SourceType,
    work: u64,
    capture: usize,
}

impl ArrayValue {
    /// Build a homogeneous, immutable array with the supplied element type.
    /// Empty arrays retain that type. Invalid types or lengths return `None`.
    pub fn new(element_type: impl Into<SourceType>, elements: Vec<Value>) -> Option<Self> {
        let element_type = element_type.into();
        let kind = SourceType::array(&element_type, elements.len())?;
        if elements.iter().any(|value| value.kind() != element_type) {
            return None;
        }
        Some(Self::from_checked(kind, elements))
    }

    pub fn elements(&self) -> &[Value] {
        &self.0.elements
    }

    pub fn element_type(&self) -> &SourceType {
        self.kind().as_array().expect("array type").element()
    }

    pub fn kind(&self) -> &SourceType {
        &self.0.kind
    }

    pub fn len(&self) -> usize {
        self.0.elements.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.elements.is_empty()
    }

    pub(crate) fn from_checked(kind: SourceType, elements: Vec<Value>) -> Self {
        let array = kind.as_array().expect("array type");
        debug_assert_eq!(array.len(), elements.len());
        let display_bytes = if elements.is_empty() {
            "[array: <>]".len() + array.element().display_bytes()
        } else {
            "[array: ]".len() + 2 * (elements.len() - 1)
        };
        let work = elements.iter().fold(
            display_bytes as u64 + 2 * kind.node_count() as u64,
            |total, value| total.saturating_add(value_work(value)),
        );
        let capture = elements.iter().fold(display_bytes, |total, value| {
            total
                .saturating_add(value_capture(value))
                .saturating_add(size_of::<Value>())
        });
        Self(Arc::new(ArrayStorage {
            elements,
            kind,
            work,
            capture,
        }))
    }

    pub(crate) fn allocation_bytes(element: &SourceType, length: usize) -> Option<usize> {
        element
            .node_count()
            .checked_add(1)?
            .checked_mul(size_of::<SourceType>())?
            .checked_add(length.checked_mul(size_of::<Value>())?)?
            .checked_add(size_of::<ArrayStorage>())
    }

    pub(crate) fn work(&self) -> u64 {
        self.0.work
    }

    pub(crate) fn capture_bytes(&self) -> usize {
        self.0.capture
    }
}

impl PartialEq for ArrayValue {
    fn eq(&self, other: &Self) -> bool {
        self.kind() == other.kind() && self.elements() == other.elements()
    }
}
impl Eq for ArrayValue {}

impl fmt::Display for ArrayValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("[array: ")?;
        if self.is_empty() {
            write!(f, "<{}>", self.element_type())?;
        } else {
            for (index, value) in self.elements().iter().enumerate() {
                if index != 0 {
                    f.write_str(", ")?;
                }
                display_nested(value, f)?;
            }
        }
        f.write_str("]")
    }
}
