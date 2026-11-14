//! Deterministic, bounded, read-only inspection of verified composite values.
//!
//! This metadata traversal is intended for future diagnostics, editor views,
//! debug inspectors and test tooling. It is independent of .srp source syntax
//! and never calls user code, performs I/O or reveals mutable storage.
use super::limits::{bounded, DataError, DataResource};
use super::paths::PathSegment;
use super::schema::{DataType, SchemaRegistry};
use super::value::{Datum, OutcomeValue};
use crate::value::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InspectionLimits {
    pub max_nodes: usize,
    pub max_depth: usize,
}
impl Default for InspectionLimits {
    fn default() -> Self {
        Self {
            max_nodes: 65_536,
            max_depth: 64,
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InspectKind {
    Source,
    Record,
    Variant,
    List,
    Map,
    OptionalNone,
    OptionalSome,
    OutcomeOk,
    OutcomeErr,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InspectedNode {
    pub path: Vec<PathSegment>,
    pub ty: DataType,
    pub kind: InspectKind,
    pub depth: usize,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct InspectionStats {
    pub nodes: usize,
    pub scalar_nodes: usize,
    pub record_nodes: usize,
    pub variant_nodes: usize,
    pub list_nodes: usize,
    pub map_nodes: usize,
    pub list_elements: usize,
    pub map_entries: usize,
    pub optional_some: usize,
    pub optional_none: usize,
    pub outcome_ok: usize,
    pub outcome_err: usize,
    pub visible_text_bytes: usize,
    pub max_depth: usize,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InspectionReport {
    pub nodes: Vec<InspectedNode>,
    pub stats: InspectionStats,
}
impl InspectionReport {
    pub fn node_at(&self, path: &[PathSegment]) -> Option<&InspectedNode> {
        self.nodes.iter().find(|node| node.path == path)
    }
}
fn checked_add(
    old: &mut usize,
    amount: usize,
    limit: usize,
    resource: DataResource,
) -> Result<(), DataError> {
    *old = old
        .checked_add(amount)
        .ok_or(DataError::Limit { resource, limit })?;
    bounded(*old, limit, resource)
}
impl SchemaRegistry {
    /// Traverse a validated data graph in stable preorder, with schema-order
    /// record fields and ascending map keys. Each node has a root-relative
    /// typed path compatible with select_path.
    ///
    /// Source tuples and fixed arrays are intentionally treated as opaque
    /// legacy source values pending a source-level path contract.
    pub fn inspect_data(
        &self,
        root_type: &DataType,
        root: &Datum,
        limits: InspectionLimits,
    ) -> Result<InspectionReport, DataError> {
        self.verify(root_type, root)?;
        self.validate_type(root_type)?;
        let mut stats = InspectionStats::default();
        let mut nodes = Vec::new();
        let mut pending = vec![(root_type.clone(), root, Vec::<PathSegment>::new(), 1usize)];
        while let Some((ty, value, path, depth)) = pending.pop() {
            bounded(depth, limits.max_depth, DataResource::InspectionDepth)?;
            checked_add(
                &mut stats.nodes,
                1,
                limits.max_nodes,
                DataResource::InspectionNodes,
            )?;
            stats.max_depth = stats.max_depth.max(depth);
            let kind = match (&ty, value) {
                (DataType::Source(_), Datum::Source(source)) => {
                    stats.scalar_nodes += 1;
                    if let Value::String(text) = source {
                        checked_add(
                            &mut stats.visible_text_bytes,
                            text.len(),
                            self.limits().max_text_bytes,
                            DataResource::TextBytes,
                        )?;
                    }
                    InspectKind::Source
                }
                (DataType::Record(id), Datum::Record(record)) => {
                    stats.record_nodes += 1;
                    let fields = self.record(*id)?;
                    for (field, value) in fields.iter().zip(record.values()).rev() {
                        let mut child_path = path.clone();
                        child_path.push(PathSegment::Field(field.name.clone()));
                        pending.push((field.ty.clone(), value, child_path, depth + 1));
                    }
                    InspectKind::Record
                }
                (DataType::Variant(id), Datum::Variant(variant)) => {
                    stats.variant_nodes += 1;
                    let case = self
                        .variants(*id)?
                        .get(variant.case_index())
                        .ok_or(DataError::InvalidShape)?;
                    if let (Some(ty), Some(payload)) = (&case.payload, variant.payload()) {
                        let mut child_path = path.clone();
                        child_path.push(PathSegment::VariantPayload(case.name.clone()));
                        pending.push((ty.clone(), payload, child_path, depth + 1));
                    }
                    InspectKind::Variant
                }
                (DataType::List(element), Datum::List(list)) => {
                    stats.list_nodes += 1;
                    checked_add(
                        &mut stats.list_elements,
                        list.len(),
                        self.limits().max_value_nodes,
                        DataResource::InspectionNodes,
                    )?;
                    for (index, value) in list.values().iter().enumerate().rev() {
                        let mut child_path = path.clone();
                        child_path.push(PathSegment::ListIndex(index));
                        pending.push((element.as_ref().clone(), value, child_path, depth + 1));
                    }
                    InspectKind::List
                }
                (DataType::Map(_, element), Datum::Map(map)) => {
                    stats.map_nodes += 1;
                    checked_add(
                        &mut stats.map_entries,
                        map.len(),
                        self.limits().max_value_nodes,
                        DataResource::InspectionNodes,
                    )?;
                    for (key, value) in map.entries().iter().rev() {
                        let mut child_path = path.clone();
                        child_path.push(PathSegment::MapKey(key.clone()));
                        pending.push((element.as_ref().clone(), value, child_path, depth + 1));
                    }
                    InspectKind::Map
                }
                (DataType::Optional(_), Datum::Optional(None)) => {
                    stats.optional_none += 1;
                    InspectKind::OptionalNone
                }
                (DataType::Optional(element), Datum::Optional(Some(value))) => {
                    stats.optional_some += 1;
                    let mut child_path = path.clone();
                    child_path.push(PathSegment::OptionalSome);
                    pending.push((element.as_ref().clone(), value, child_path, depth + 1));
                    InspectKind::OptionalSome
                }
                (DataType::Outcome(ok, _), Datum::Outcome(OutcomeValue::Ok(value))) => {
                    stats.outcome_ok += 1;
                    let mut child_path = path.clone();
                    child_path.push(PathSegment::OutcomeOk);
                    pending.push((ok.as_ref().clone(), value, child_path, depth + 1));
                    InspectKind::OutcomeOk
                }
                (DataType::Outcome(_, err), Datum::Outcome(OutcomeValue::Err(value))) => {
                    stats.outcome_err += 1;
                    let mut child_path = path.clone();
                    child_path.push(PathSegment::OutcomeErr);
                    pending.push((err.as_ref().clone(), value, child_path, depth + 1));
                    InspectKind::OutcomeErr
                }
                _ => return Err(DataError::TypeMismatch),
            };
            nodes.push(InspectedNode {
                path,
                ty,
                kind,
                depth,
            });
        }
        Ok(InspectionReport { nodes, stats })
    }
}
