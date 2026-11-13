//! Bounded immutable tuple storage and escaped nested display.
use super::Value;
use crate::types::SourceType;
use std::{
    fmt::{self, Write},
    sync::Arc,
};

#[derive(Debug, Clone)]
pub struct TupleValue(Arc<TupleStorage>);
#[derive(Debug)]
struct TupleStorage {
    elements: Box<[Value]>,
    kind: SourceType,
    work: u64,
    capture: usize,
}
impl PartialEq for TupleValue {
    fn eq(&self, other: &Self) -> bool {
        self.0.kind == other.0.kind && self.0.elements == other.0.elements
    }
}
impl Eq for TupleValue {}
impl TupleValue {
    /// Reject invalid arity, depth, type complexity or unit members.
    /// Ownership is consumed; subsequent clones share immutable storage.
    pub fn new(elements: Vec<Value>) -> Option<Self> {
        if elements.is_empty() || elements.len() > crate::types::MAX_TUPLE_ARITY {
            return None;
        }
        let mut types = Vec::new();
        types.try_reserve_exact(elements.len()).ok()?;
        for element in &elements {
            types.push(element.kind());
        }
        let kind = SourceType::tuple(types)?;
        let work = elements
            .iter()
            .fold(1u64, |n, v| n.saturating_add(value_work(v)));
        let capture = elements.iter().fold(16usize, |n, v| {
            n.saturating_add(value_capture(v))
                .saturating_add(std::mem::size_of::<Value>())
        });
        Some(Self(Arc::new(TupleStorage {
            elements: elements.into_boxed_slice(),
            kind,
            work,
            capture,
        })))
    }
    pub fn elements(&self) -> &[Value] {
        &self.0.elements
    }
    pub fn kind(&self) -> &SourceType {
        &self.0.kind
    }
    pub fn len(&self) -> usize {
        self.elements().len()
    }
    pub fn is_empty(&self) -> bool {
        false
    }
    pub(crate) fn work(&self) -> u64 {
        self.0.work
    }
    pub(crate) fn capture_bytes(&self) -> usize {
        self.0.capture
    }
    pub(crate) fn allocation_bytes(elements: &[Value]) -> Option<usize> {
        let nodes = elements
            .iter()
            .try_fold(1usize, |n, v| n.checked_add(v.kind().node_count()))?;
        nodes
            .checked_mul(std::mem::size_of::<SourceType>())?
            .checked_add(elements.len().checked_mul(std::mem::size_of::<Value>())?)?
            .checked_add(std::mem::size_of::<TupleStorage>())
    }
}
fn value_work(value: &Value) -> u64 {
    match value {
        Value::Tuple(t) => t.work(),
        Value::String(s) => (s.len() as u64).saturating_mul(6).saturating_add(2),
        Value::Vector(v) => (v.len() as u64).saturating_mul(32).saturating_add(2),
        Value::Integers(v) => (v.len() as u64).saturating_mul(24).saturating_add(12),
        Value::Tile(t) => (t.len() as u64)
            .saturating_mul(32)
            .saturating_add(t.height() as u64)
            .saturating_add(16),
        Value::BigInt(n) => u64::try_from(n.decimal_digits())
            .unwrap_or(u64::MAX)
            .saturating_add(u64::from(n.is_negative())),
        _ => 32,
    }
}
fn value_capture(value: &Value) -> usize {
    match value {
        Value::Tuple(t) => t.capture_bytes(),
        Value::String(s) => s.len().saturating_mul(6).saturating_add(2),
        Value::Vector(v) => v.len().saturating_mul(8),
        Value::Integers(v) => v.len().saturating_mul(8),
        Value::Tile(t) => t.len().saturating_mul(8).saturating_add(16),
        Value::BigInt(n) => n.payload_bytes().max(
            n.decimal_digits()
                .saturating_add(usize::from(n.is_negative())),
        ),
        _ => 8,
    }
}
fn quoted(text: &str, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.write_char('"')?;
    for ch in text.chars() {
        match ch {
            '"' => f.write_str("\\\"")?,
            '\\' => f.write_str("\\\\")?,
            '\n' => f.write_str("\\n")?,
            '\r' => f.write_str("\\r")?,
            '\t' => f.write_str("\\t")?,
            '\0' => f.write_str("\\0")?,
            c if c.is_control() => write!(f, "\\u{{{:x}}}", c as u32)?,
            c => f.write_char(c)?,
        }
    }
    f.write_char('"')
}
impl fmt::Display for TupleValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_char('(')?;
        for (index, value) in self.elements().iter().enumerate() {
            if index > 0 {
                f.write_str(", ")?;
            }
            if let Value::String(text) = value {
                quoted(text, f)?;
            } else {
                value.fmt(f)?;
            }
        }
        if self.len() == 1 {
            f.write_char(',')?;
        }
        f.write_char(')')
    }
}

#[cfg(test)]
mod bigint_tuple_budgets {
    use super::{TupleValue, Value};
    use crate::numeric::{self, Builtin};

    #[test]
    fn nested_bigint_is_charged_by_magnitude_not_scalar_placeholder() {
        let text = "9".repeat(500);
        let number = numeric::evaluate(Builtin::BigInt, &[Value::from(text.as_str())], 1000)
            .expect("bounded exact input");
        let inner = TupleValue::new(vec![number]).expect("tuple of bigint");
        assert!(
            inner.work() >= 500,
            "inner work covers BigInt decimal formatting"
        );
        assert!(
            inner.capture_bytes() >= 500,
            "inner capture covers decimal magnitude"
        );
        let outer = TupleValue::new(vec![Value::Tuple(inner)]).expect("nested tuple");
        assert!(
            outer.work() >= 500,
            "nested work cannot bypass BigInt charge"
        );
        assert!(
            outer.capture_bytes() >= 500,
            "nested capture cannot bypass BigInt charge"
        );
    }
}
