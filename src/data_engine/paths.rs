//! Typed paths and all-or-nothing persistent updates.
//!
//! A path is not executable syntax. It represents a compiler-resolved sequence
//! of field, variant payload and collection accesses. Every traversal verifies
//! the root type before exposing data, and every edit returns a new snapshot.
use super::limits::{bounded, DataError, DataResource};
use super::schema::{DataType, SchemaRegistry};
use super::value::{DataKey, Datum};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathSegment {
    Field(String),
    VariantPayload(String),
    ListIndex(usize),
    MapKey(DataKey),
    OptionalSome,
    OutcomeOk,
    OutcomeErr,
}
impl PathSegment {
    pub fn field(name: impl Into<String>) -> Self {
        Self::Field(name.into())
    }
    pub fn case(name: impl Into<String>) -> Self {
        Self::VariantPayload(name.into())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PatchOperation {
    Replace { path: Vec<PathSegment>, value: Datum },
    ListPush { path: Vec<PathSegment>, value: Datum },
    ListInsert { path: Vec<PathSegment>, index: usize, value: Datum },
    ListRemove { path: Vec<PathSegment>, index: usize },
    MapPut { path: Vec<PathSegment>, key: DataKey, value: Datum },
    MapDelete { path: Vec<PathSegment>, key: DataKey },
}
impl PatchOperation {
    pub fn path(&self) -> &[PathSegment] {
        match self {
            Self::Replace { path, .. }
            | Self::ListPush { path, .. }
            | Self::ListInsert { path, .. }
            | Self::ListRemove { path, .. }
            | Self::MapPut { path, .. }
            | Self::MapDelete { path, .. } => path,
        }
    }
}

impl SchemaRegistry {
    /// Statically compute the exact type of a path without accessing a value.
    /// Runtime indices and keys still require checked traversal.
    pub fn path_type(
        &self,
        root: &DataType,
        path: &[PathSegment],
    ) -> Result<DataType, DataError> {
        self.validate_type(root)?;
        bounded(
            path.len(),
            self.limits().max_value_depth,
            DataResource::ValueDepth,
        )?;
        let mut current = root.clone();
        for segment in path {
            current = match (&current, segment) {
                (DataType::Record(id), PathSegment::Field(name)) => {
                    self.record(*id)?
                        .iter()
                        .find(|field| field.name == *name)
                        .ok_or_else(|| DataError::UnknownField(name.clone()))?
                        .ty
                        .clone()
                }
                (DataType::Variant(id), PathSegment::VariantPayload(name)) => {
                    self.variants(*id)?
                        .iter()
                        .find(|case| case.name == *name)
                        .ok_or_else(|| DataError::UnknownCase(name.clone()))?
                        .payload
                        .clone()
                        .ok_or(DataError::InvalidShape)?
                }
                (DataType::List(element), PathSegment::ListIndex(_)) => {
                    element.as_ref().clone()
                }
                (DataType::Map(key_type, element), PathSegment::MapKey(key))
                    if *key_type == key.kind() =>
                {
                    element.as_ref().clone()
                }
                (DataType::Optional(element), PathSegment::OptionalSome) => {
                    element.as_ref().clone()
                }
                (DataType::Outcome(ok, _), PathSegment::OutcomeOk) => {
                    ok.as_ref().clone()
                }
                (DataType::Outcome(_, err), PathSegment::OutcomeErr) => {
                    err.as_ref().clone()
                }
                _ => return Err(DataError::TypeMismatch),
            };
        }
        self.validate_type(&current)?;
        Ok(current)
    }

    fn step<'a>(
        &self,
        value: &'a Datum,
        segment: &PathSegment,
    ) -> Result<&'a Datum, DataError> {
        match (value, segment) {
            (Datum::Record(record), PathSegment::Field(name)) => self.field(record, name),
            (Datum::Variant(variant), PathSegment::VariantPayload(name)) => {
                if self.case_name(variant)? != name {
                    return Err(DataError::InvalidShape);
                }
                variant.payload().ok_or(DataError::InvalidShape)
            }
            (Datum::List(list), PathSegment::ListIndex(index)) => {
                list.get(*index).ok_or(DataError::InvalidShape)
            }
            (Datum::Map(map), PathSegment::MapKey(key)) => {
                if map.key_type() != key.kind() {
                    return Err(DataError::InvalidKey);
                }
                map.get(key).ok_or(DataError::InvalidShape)
            }
            (Datum::Optional(Some(value)), PathSegment::OptionalSome) => Ok(value),
            (Datum::Outcome(super::value::OutcomeValue::Ok(value)), PathSegment::OutcomeOk) => {
                Ok(value)
            }
            (Datum::Outcome(super::value::OutcomeValue::Err(value)), PathSegment::OutcomeErr) => {
                Ok(value)
            }
            _ => Err(DataError::InvalidShape),
        }
    }

    /// Type-check a root, follow a path and verify the destination type.
    pub fn select_path<'a>(
        &self,
        root_type: &DataType,
        root: &'a Datum,
        path: &[PathSegment],
    ) -> Result<&'a Datum, DataError> {
        let destination = self.path_type(root_type, path)?;
        self.verify(root_type, root)?;
        let mut current = root;
        for segment in path {
            current = self.step(current, segment)?;
        }
        self.verify(&destination, current)?;
        Ok(current)
    }

    /// Replace a leaf. The old root is never mutated, even when validation
    /// fails halfway through reconstruction.
    pub fn replace_path(
        &self,
        root_type: &DataType,
        root: &Datum,
        path: &[PathSegment],
        replacement: Datum,
    ) -> Result<Datum, DataError> {
        let destination = self.path_type(root_type, path)?;
        self.verify(root_type, root)?;
        self.verify(&destination, &replacement)?;
        let mut trail = Vec::new();
        let mut current = root;
        for segment in path {
            trail.push((current, segment));
            current = self.step(current, segment)?;
        }
        let mut updated = replacement;
        for (parent, segment) in trail.into_iter().rev() {
            updated = match (parent, segment) {
                (Datum::Record(record), PathSegment::Field(name)) => {
                    Datum::Record(self.replace_field(record, name, updated)?)
                }
                (Datum::Variant(variant), PathSegment::VariantPayload(case)) => {
                    if self.case_name(variant)? != case {
                        return Err(DataError::InvalidShape);
                    }
                    Datum::Variant(self.new_variant(variant.identity(), case, Some(updated))?)
                }
                (Datum::List(list), PathSegment::ListIndex(index)) => {
                    Datum::List(self.list_set(list, *index, updated)?)
                }
                (Datum::Map(map), PathSegment::MapKey(key)) => {
                    if map.get(key).is_none() {
                        return Err(DataError::InvalidShape);
                    }
                    Datum::Map(self.map_insert(map, key.clone(), updated)?)
                }
                (Datum::Optional(Some(_)), PathSegment::OptionalSome) => Datum::some(updated),
                (Datum::Outcome(super::value::OutcomeValue::Ok(_)), PathSegment::OutcomeOk) => {
                    Datum::ok(updated)
                }
                (Datum::Outcome(super::value::OutcomeValue::Err(_)), PathSegment::OutcomeErr) => {
                    Datum::err(updated)
                }
                _ => return Err(DataError::InvalidShape),
            };
        }
        self.verify(root_type, &updated)?;
        Ok(updated)
    }

    /// Execute an ordered batch against a temporary root. If any operation
    /// fails the caller receives no replacement root, with no side effects on
    /// the original immutable snapshot.
    pub fn apply_patch(
        &self,
        root_type: &DataType,
        root: &Datum,
        operations: &[PatchOperation],
    ) -> Result<Datum, DataError> {
        bounded(
            operations.len(),
            self.limits().max_collection_entries,
            DataResource::CollectionEntries,
        )?;
        self.verify(root_type, root)?;
        let mut next = root.clone();
        for operation in operations {
            let path = operation.path();
            let updated = match operation {
                PatchOperation::Replace { value, .. } => value.clone(),
                PatchOperation::ListPush { value, .. } => {
                    let leaf = self.select_path(root_type, &next, path)?;
                    match leaf {
                        Datum::List(list) => {
                            Datum::List(self.list_push(list, value.clone())?)
                        }
                        _ => return Err(DataError::TypeMismatch),
                    }
                }
                PatchOperation::ListInsert { index, value, .. } => {
                    let leaf = self.select_path(root_type, &next, path)?;
                    match leaf {
                        Datum::List(list) => {
                            if *index > list.len() {
                                return Err(DataError::InvalidShape);
                            }
                            let mut values = list.values().to_vec();
                            values.insert(*index, value.clone());
                            Datum::List(self.new_list(list.element_type().clone(), values)?)
                        }
                        _ => return Err(DataError::TypeMismatch),
                    }
                }
                PatchOperation::ListRemove { index, .. } => {
                    let leaf = self.select_path(root_type, &next, path)?;
                    match leaf {
                        Datum::List(list) => {
                            Datum::List(self.list_remove(list, *index)?.0)
                        }
                        _ => return Err(DataError::TypeMismatch),
                    }
                }
                PatchOperation::MapPut { key, value, .. } => {
                    let leaf = self.select_path(root_type, &next, path)?;
                    match leaf {
                        Datum::Map(map) => Datum::Map(self.map_insert(map, key.clone(), value.clone())?),
                        _ => return Err(DataError::TypeMismatch),
                    }
                }
                PatchOperation::MapDelete { key, .. } => {
                    let leaf = self.select_path(root_type, &next, path)?;
                    match leaf {
                        Datum::Map(map) => Datum::Map(self.map_remove(map, key)?.0),
                        _ => return Err(DataError::TypeMismatch),
                    }
                }
            };
            next = self.replace_path(root_type, &next, path, updated)?;
        }
        Ok(next)
    }
}
