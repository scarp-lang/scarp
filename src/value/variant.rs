//! Immutable tagged source values. Clones share any payload.
use super::{
    tuple::{display_nested, value_capture, value_work},
    Value,
};
use crate::types::{SourceType, VariantType};
use std::{fmt, mem::size_of, sync::Arc};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VariantValue {
    kind: SourceType,
    tag: u8,
    payload: Option<Arc<Value>>,
}

impl VariantValue {
    pub(crate) fn from_checked(kind: SourceType, tag: u8, payload: Option<Value>) -> Self {
        debug_assert_eq!(
            kind.as_variant()
                .and_then(|variant| variant.cases()[usize::from(tag)].1.as_ref()),
            payload.as_ref().map(Value::kind).as_ref(),
        );
        Self {
            kind,
            tag,
            payload: payload.map(Arc::new),
        }
    }

    pub fn kind(&self) -> &SourceType {
        &self.kind
    }

    pub fn layout(&self) -> &VariantType {
        self.kind.as_variant().expect("variant layout")
    }

    pub fn tag(&self) -> u8 {
        self.tag
    }

    pub fn payload(&self) -> Option<&Value> {
        self.payload.as_deref()
    }

    pub(crate) fn allocation_bytes() -> usize {
        size_of::<Value>() + 2 * size_of::<usize>()
    }

    pub(crate) fn work(&self) -> u64 {
        (self.syntax_bytes() as u64).saturating_add(self.payload().map_or(0, value_work))
    }

    pub(crate) fn capture_bytes(&self) -> usize {
        self.syntax_bytes()
            .saturating_add(self.payload().map_or(0, |payload| {
                size_of::<Value>().saturating_add(value_capture(payload))
            }))
    }

    fn case_name(&self) -> &str {
        &self.layout().cases()[usize::from(self.tag)].0
    }

    fn syntax_bytes(&self) -> usize {
        "new ::()".len() + self.layout().name().len() + self.case_name().len()
    }
}

impl fmt::Display for VariantValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "new {}::{}(", self.layout().name(), self.case_name())?;
        if let Some(payload) = self.payload() {
            display_nested(payload, f)?;
        }
        f.write_str(")")
    }
}
