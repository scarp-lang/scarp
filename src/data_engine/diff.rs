//! Bounded deterministic structural difference between two checked data graphs.
//!
//! Diffs produce ordered PatchOperation values accepted by apply_patch. The
//! representation is host-only and makes no promise about a stable patch ABI.
//! No mutable borrowing, aliasing or user-visible garbage collection is added.
use super::limits::{bounded, DataError, DataResource};
use super::paths::{PatchOperation, PathSegment};
use super::schema::{DataType, SchemaRegistry};
use super::value::{Datum, OutcomeValue};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiffLimits {
    pub max_visited: usize,
    pub max_operations: usize,
    pub max_depth: usize,
}
impl Default for DiffLimits {
    fn default() -> Self {
        Self {
            max_visited: 65_536,
            max_operations: 4096,
            max_depth: 32,
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataDiff {
    pub operations: Vec<PatchOperation>,
    pub visited: usize,
}
impl DataDiff {
    pub fn is_empty(&self) -> bool {
        self.operations.is_empty()
    }
    pub fn len(&self) -> usize {
        self.operations.len()
    }
}
struct Differ<'a> {
    registry: &'a SchemaRegistry,
    limits: DiffLimits,
    visited: usize,
    operations: Vec<PatchOperation>,
}
impl Differ<'_> {
    fn visit(&mut self, depth: usize) -> Result<(), DataError> {
        bounded(depth, self.limits.max_depth, DataResource::DiffDepth)?;
        self.visited = self.visited.checked_add(1).ok_or(DataError::Limit {
            resource: DataResource::DiffNodes,
            limit: self.limits.max_visited,
        })?;
        bounded(self.visited, self.limits.max_visited, DataResource::DiffNodes)
    }
    fn emit(&mut self, op: PatchOperation) -> Result<(), DataError> {
        bounded(
            self.operations.len().saturating_add(1),
            self.limits.max_operations,
            DataResource::DiffOperations,
        )?;
        self.operations.push(op);
        Ok(())
    }
    fn replace(&mut self, path: &[PathSegment], value: &Datum) -> Result<(), DataError> {
        self.emit(PatchOperation::Replace {
            path: path.to_vec(),
            value: value.clone(),
        })
    }
    fn compare(
        &mut self,
        ty: &DataType,
        before: &Datum,
        after: &Datum,
        path: &mut Vec<PathSegment>,
        depth: usize,
    ) -> Result<(), DataError> {
        self.visit(depth)?;
        if before == after {
            return Ok(());
        }
        match (ty, before, after) {
            (DataType::Record(id), Datum::Record(a), Datum::Record(b))
                if a.identity() == *id && b.identity() == *id =>
            {
                let fields = self.registry.record(*id)?;
                if a.len() != fields.len() || b.len() != fields.len() {
                    return Err(DataError::InvalidShape);
                }
                for (index, field) in fields.iter().enumerate() {
                    path.push(PathSegment::Field(field.name.clone()));
                    self.compare(
                        &field.ty,
                        &a.values()[index],
                        &b.values()[index],
                        path,
                        depth + 1,
                    )?;
                    path.pop();
                }
                Ok(())
            }
            (DataType::Variant(id), Datum::Variant(a), Datum::Variant(b))
                if a.identity() == *id && b.identity() == *id =>
            {
                if a.case_index() != b.case_index() {
                    return self.replace(path, after);
                }
                let case = self
                    .registry
                    .variants(*id)?
                    .get(a.case_index())
                    .ok_or(DataError::InvalidShape)?;
                match (&case.payload, a.payload(), b.payload()) {
                    (None, None, None) => Ok(()),
                    (Some(inner), Some(left), Some(right)) => {
                        path.push(PathSegment::VariantPayload(case.name.clone()));
                        self.compare(inner, left, right, path, depth + 1)?;
                        path.pop();
                        Ok(())
                    }
                    _ => Err(DataError::InvalidShape),
                }
            }
            (DataType::Optional(inner), Datum::Optional(a), Datum::Optional(b)) => {
                match (a, b) {
                    (Some(left), Some(right)) => {
                        path.push(PathSegment::OptionalSome);
                        self.compare(inner, left, right, path, depth + 1)?;
                        path.pop();
                        Ok(())
                    }
                    _ => self.replace(path, after),
                }
            }
            (DataType::Outcome(ok, err), Datum::Outcome(a), Datum::Outcome(b)) => {
                match (a, b) {
                    (OutcomeValue::Ok(left), OutcomeValue::Ok(right)) => {
                        path.push(PathSegment::OutcomeOk);
                        self.compare(ok, left, right, path, depth + 1)?;
                        path.pop();
                        Ok(())
                    }
                    (OutcomeValue::Err(left), OutcomeValue::Err(right)) => {
                        path.push(PathSegment::OutcomeErr);
                        self.compare(err, left, right, path, depth + 1)?;
                        path.pop();
                        Ok(())
                    }
                    _ => self.replace(path, after),
                }
            }
            (DataType::List(inner), Datum::List(a), Datum::List(b)) => {
                let overlap = a.len().min(b.len());
                for index in 0..overlap {
                    path.push(PathSegment::ListIndex(index));
                    self.compare(
                        inner,
                        &a.values()[index],
                        &b.values()[index],
                        path,
                        depth + 1,
                    )?;
                    path.pop();
                }
                // Remove from the tail downwards, preserving every existing
                // index within the overlapping unchanged prefix.
                for index in (b.len()..a.len()).rev() {
                    self.emit(PatchOperation::ListRemove {
                        path: path.clone(),
                        index,
                    })?;
                }
                // A push always appends after the existing prefix and any
                // prior tail removals.
                for item in &b.values()[overlap..] {
                    self.emit(PatchOperation::ListPush {
                        path: path.clone(),
                        value: item.clone(),
                    })?;
                }
                Ok(())
            }
            (DataType::Map(_, inner), Datum::Map(a), Datum::Map(b)) => {
                let (left, right) = (a.entries(), b.entries());
                let (mut i, mut j) = (0usize, 0usize);
                while i < left.len() || j < right.len() {
                    match (left.get(i), right.get(j)) {
                        (Some((old_key, _)), None) => {
                            self.emit(PatchOperation::MapDelete {
                                path: path.clone(),
                                key: old_key.clone(),
                            })?;
                            i += 1;
                        }
                        (None, Some((new_key, new_value))) => {
                            self.emit(PatchOperation::MapPut {
                                path: path.clone(),
                                key: new_key.clone(),
                                value: new_value.clone(),
                            })?;
                            j += 1;
                        }
                        (Some((old_key, old_value)), Some((new_key, new_value))) => {
                            use std::cmp::Ordering;
                            match old_key.cmp(new_key) {
                                Ordering::Less => {
                                    self.emit(PatchOperation::MapDelete {
                                        path: path.clone(),
                                        key: old_key.clone(),
                                    })?;
                                    i += 1;
                                }
                                Ordering::Greater => {
                                    self.emit(PatchOperation::MapPut {
                                        path: path.clone(),
                                        key: new_key.clone(),
                                        value: new_value.clone(),
                                    })?;
                                    j += 1;
                                }
                                Ordering::Equal => {
                                    path.push(PathSegment::MapKey(old_key.clone()));
                                    self.compare(inner, old_value, new_value, path, depth + 1)?;
                                    path.pop();
                                    i += 1;
                                    j += 1;
                                }
                            }
                        }
                        (None, None) => break,
                    }
                }
                Ok(())
            }
            _ => self.replace(path, after),
        }
    }
}
impl SchemaRegistry {
    /// Generate a deterministic ordered patch. Both operands must be verified
    /// against the same declared root type before visiting any difference.
    pub fn diff_data(
        &self,
        root_type: &DataType,
        before: &Datum,
        after: &Datum,
        limits: DiffLimits,
    ) -> Result<DataDiff, DataError> {
        self.verify(root_type, before)?;
        self.verify(root_type, after)?;
        let mut differ = Differ {
            registry: self,
            limits,
            visited: 0,
            operations: Vec::new(),
        };
        differ.compare(root_type, before, after, &mut Vec::new(), 1)?;
        Ok(DataDiff {
            operations: differ.operations,
            visited: differ.visited,
        })
    }
    /// Verify the patch roundtrip exactly and return an immutable new root.
    /// Failure never publishes partially changed data.
    pub fn reconcile_data(
        &self,
        root_type: &DataType,
        before: &Datum,
        after: &Datum,
        limits: DiffLimits,
    ) -> Result<DataDiff, DataError> {
        let diff = self.diff_data(root_type, before, after, limits)?;
        let rebuilt = self.apply_patch(root_type, before, &diff.operations)?;
        if &rebuilt != after {
            return Err(DataError::InvalidShape);
        }
        Ok(diff)
    }
}
