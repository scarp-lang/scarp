//! Immutable, homogeneously typed source-level dynamic lists.
//!
//! Cloning a ListValue shares the entire immutable snapshot. Functional
//! update operations are separate bounded allocations (not alias mutations).
use super::{
    tuple::{display_nested, value_capture, value_work},
    Value,
};
use crate::types::{SourceType, MAX_LIST_LENGTH};
use std::{fmt, mem::size_of, sync::Arc};

#[derive(Debug, Clone)]
pub struct ListValue(Arc<ListStorage>);

#[derive(Debug)]
struct ListStorage {
    elements: Vec<Value>,
    kind: SourceType,
    work: u64,
    capture: usize,
}

impl ListValue {
    pub fn new(element_type: impl Into<SourceType>, elements: Vec<Value>) -> Option<Self> {
        let element_type = element_type.into();
        if element_type.has_nominal_ref() || elements.len() > MAX_LIST_LENGTH {
            return None;
        }
        let kind = SourceType::list(&element_type)?;
        if elements.iter().any(|value| value.kind() != element_type) {
            return None;
        }
        Some(Self::from_checked(kind, elements))
    }

    pub fn kind(&self) -> &SourceType {
        &self.0.kind
    }

    pub fn element_type(&self) -> &SourceType {
        self.kind().as_list().expect("list type").element()
    }

    pub fn elements(&self) -> &[Value] {
        &self.0.elements
    }

    pub fn len(&self) -> usize {
        self.0.elements.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.elements.is_empty()
    }

    pub(crate) fn from_checked(kind: SourceType, elements: Vec<Value>) -> Self {
        debug_assert!(kind.as_list().is_some());
        debug_assert!(elements.len() <= MAX_LIST_LENGTH);
        let display_bytes = if elements.is_empty() {
            "[list: <>]".len() + kind.as_list().unwrap().element().display_bytes()
        } else {
            "[list: ]".len() + 2 * (elements.len() - 1)
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
        Self(Arc::new(ListStorage {
            elements,
            kind,
            work,
            capture,
        }))
    }

    pub(crate) fn allocation_bytes(element: &SourceType, length: usize) -> Option<usize> {
        if length > MAX_LIST_LENGTH {
            return None;
        }
        element
            .node_count()
            .checked_add(1)?
            .checked_mul(size_of::<SourceType>())?
            .checked_add(length.checked_mul(size_of::<Value>())?)?
            .checked_add(size_of::<ListStorage>())
    }

    pub(crate) fn work(&self) -> u64 {
        self.0.work
    }

    pub(crate) fn capture_bytes(&self) -> usize {
        self.0.capture
    }
}

impl PartialEq for ListValue {
    fn eq(&self, other: &Self) -> bool {
        self.kind() == other.kind() && self.elements() == other.elements()
    }
}
impl Eq for ListValue {}

impl fmt::Display for ListValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("[list: ")?;
        if self.is_empty() {
            write!(f, "<{}>", self.element_type())?;
        } else {
            for (index, value) in self.elements().iter().enumerate() {
                if index > 0 {
                    f.write_str(", ")?;
                }
                display_nested(value, f)?;
            }
        }
        f.write_str("]")
    }
}
