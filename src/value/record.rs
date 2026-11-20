//! Shared immutable source record values.
use super::{
    tuple::{display_nested, value_capture, value_work},
    Value,
};
use crate::types::{RecordType, SourceType};
use std::{fmt, mem::size_of, sync::Arc};

#[derive(Debug, Clone)]
pub struct RecordValue(Arc<Storage>);

#[derive(Debug)]
struct Storage {
    kind: SourceType,
    fields: Vec<Value>,
    work: u64,
    capture_bytes: usize,
}

impl RecordValue {
    pub fn kind(&self) -> &SourceType {
        &self.0.kind
    }

    pub fn fields(&self) -> &[Value] {
        &self.0.fields
    }

    pub fn layout(&self) -> &RecordType {
        self.kind()
            .as_record()
            .expect("record value stores a record layout")
    }

    pub(crate) fn from_checked(kind: SourceType, fields: Vec<Value>) -> Self {
        let layout = kind.as_record().expect("resolved layout");
        debug_assert_eq!(layout.fields().len(), fields.len());
        let syntax_bytes = layout
            .fields()
            .iter()
            .fold(layout.name().len() + "new  {}".len(), |total, (name, _)| {
                total + name.len() + 4
            });
        let work = fields.iter().fold(syntax_bytes as u64, |total, value| {
            total.saturating_add(value_work(value))
        });
        let capture_bytes = fields.iter().fold(syntax_bytes, |total, value| {
            total
                .saturating_add(value_capture(value))
                .saturating_add(size_of::<Value>())
        });
        Self(Arc::new(Storage {
            kind,
            fields,
            work,
            capture_bytes,
        }))
    }

    pub(crate) fn allocation_bytes(fields: usize) -> usize {
        // Bounded by the resolver's field limit, including both Arc counters.
        size_of::<Storage>() + 2 * size_of::<usize>() + fields * size_of::<Value>()
    }

    pub(crate) fn work(&self) -> u64 {
        self.0.work
    }

    pub(crate) fn capture_bytes(&self) -> usize {
        self.0.capture_bytes
    }
}

impl PartialEq for RecordValue {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
            || (self.kind() == other.kind() && self.fields() == other.fields())
    }
}
impl Eq for RecordValue {}

impl fmt::Display for RecordValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "new {} {{", self.layout().name())?;
        for (index, ((name, _), value)) in
            self.layout().fields().iter().zip(self.fields()).enumerate()
        {
            if index != 0 {
                f.write_str(", ")?;
            }
            write!(f, "{name}: ")?;
            display_nested(value, f)?;
        }
        f.write_str("}")
    }
}
