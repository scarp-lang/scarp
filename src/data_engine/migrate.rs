//! Explicit, bounded migration between two nominal schema revisions.
//!
//! This host-only API never infers potentially lossy casts or silently drops
//! a field. New fields require caller-provided checked defaults; removing a
//! field requires explicit permission. Every rebuilt value is verified by the
//! target registry before being exposed. It is not an on-disk ABI migrator.
use super::limits::{bounded, DataError, DataResource};
use super::schema::{DataType, SchemaRegistry};
use super::value::{Datum, OutcomeValue};
use crate::types::DefinitionId;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct MigrationOptions {
    pub allow_drop_fields: bool,
    pub max_nodes: usize,
    pub max_depth: usize,
    defaults: HashMap<(DefinitionId, String), Datum>,
}
impl Default for MigrationOptions {
    fn default() -> Self {
        Self {
            allow_drop_fields: false,
            max_nodes: 65_536,
            max_depth: 32,
            defaults: HashMap::new(),
        }
    }
}
impl MigrationOptions {
    pub fn allow_dropping_fields(mut self) -> Self {
        self.allow_drop_fields = true;
        self
    }
    pub fn with_node_limit(mut self, limit: usize) -> Self {
        self.max_nodes = limit;
        self
    }
    pub fn with_depth_limit(mut self, limit: usize) -> Self {
        self.max_depth = limit;
        self
    }
    pub fn with_default(
        mut self,
        definition: DefinitionId,
        field: impl Into<String>,
        value: Datum,
    ) -> Self {
        self.defaults.insert((definition, field.into()), value);
        self
    }
    pub fn get_default(&self, definition: DefinitionId, field: &str) -> Option<&Datum> {
        self.defaults.get(&(definition, field.to_owned()))
    }
    pub fn default_count(&self) -> usize {
        self.defaults.len()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MigrationStats {
    pub visited: usize,
    pub inserted_fields: usize,
    pub removed_fields: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigrationResult {
    pub value: Datum,
    pub stats: MigrationStats,
}

struct Migrator<'a> {
    source: &'a SchemaRegistry,
    target: &'a SchemaRegistry,
    options: &'a MigrationOptions,
    stats: MigrationStats,
}
impl Migrator<'_> {
    fn node(&mut self, depth: usize) -> Result<(), DataError> {
        bounded(depth, self.options.max_depth, DataResource::MigrationDepth)?;
        self.stats.visited = self.stats.visited.checked_add(1).ok_or(DataError::Limit {
            resource: DataResource::MigrationNodes,
            limit: self.options.max_nodes,
        })?;
        bounded(
            self.stats.visited,
            self.options.max_nodes,
            DataResource::MigrationNodes,
        )
    }
    fn insert_field(&mut self) -> Result<(), DataError> {
        self.stats.inserted_fields =
            self.stats
                .inserted_fields
                .checked_add(1)
                .ok_or(DataError::Limit {
                    resource: DataResource::MigrationNodes,
                    limit: self.options.max_nodes,
                })?;
        bounded(
            self.stats.inserted_fields,
            self.options.max_nodes,
            DataResource::MigrationNodes,
        )
    }
    fn remove_field(&mut self) -> Result<(), DataError> {
        self.stats.removed_fields =
            self.stats
                .removed_fields
                .checked_add(1)
                .ok_or(DataError::Limit {
                    resource: DataResource::MigrationNodes,
                    limit: self.options.max_nodes,
                })?;
        bounded(
            self.stats.removed_fields,
            self.options.max_nodes,
            DataResource::MigrationNodes,
        )
    }
    fn migrate(
        &mut self,
        old_ty: &DataType,
        new_ty: &DataType,
        value: &Datum,
        depth: usize,
    ) -> Result<Datum, DataError> {
        self.node(depth)?;
        match (old_ty, new_ty, value) {
            (DataType::Source(a), DataType::Source(b), Datum::Source(source)) if a == b => {
                Ok(Datum::Source(source.clone()))
            }
            (DataType::Record(old_id), DataType::Record(new_id), Datum::Record(old_record))
                if old_id == new_id && old_record.identity() == *old_id =>
            {
                let old_fields = self.source.record(*old_id)?.to_vec();
                let new_fields = self.target.record(*new_id)?.to_vec();
                if old_record.values().len() != old_fields.len() {
                    return Err(DataError::InvalidShape);
                }
                if !self.options.allow_drop_fields {
                    for old in &old_fields {
                        if !new_fields.iter().any(|new| new.name == old.name) {
                            return Err(DataError::DroppedField(old.name.clone()));
                        }
                    }
                } else {
                    for old in &old_fields {
                        if !new_fields.iter().any(|new| new.name == old.name) {
                            self.remove_field()?;
                        }
                    }
                }
                let mut values = Vec::new();
                values
                    .try_reserve_exact(new_fields.len())
                    .map_err(|_| DataError::InvalidShape)?;
                for new in new_fields {
                    let migrated = if let Some(index) =
                        old_fields.iter().position(|old| old.name == new.name)
                    {
                        self.migrate(
                            &old_fields[index].ty,
                            &new.ty,
                            &old_record.values()[index],
                            depth + 1,
                        )?
                    } else {
                        let default = self
                            .options
                            .get_default(*new_id, &new.name)
                            .ok_or_else(|| DataError::MissingMigrationDefault(new.name.clone()))?
                            .clone();
                        self.target.verify(&new.ty, &default)?;
                        self.insert_field()?;
                        default
                    };
                    values.push(migrated);
                }
                Ok(Datum::Record(self.target.new_record(*new_id, values)?))
            }
            (DataType::Variant(old_id), DataType::Variant(new_id), Datum::Variant(old_variant))
                if old_id == new_id && old_variant.identity() == *old_id =>
            {
                let old_case = self
                    .source
                    .variants(*old_id)?
                    .get(old_variant.case_index())
                    .ok_or(DataError::InvalidShape)?
                    .clone();
                let new_case = self
                    .target
                    .variants(*new_id)?
                    .iter()
                    .find(|c| c.name == old_case.name)
                    .ok_or_else(|| DataError::UnknownCase(old_case.name.clone()))?
                    .clone();
                let payload = match (&old_case.payload, &new_case.payload, old_variant.payload()) {
                    (None, None, None) => None,
                    (Some(a), Some(b), Some(payload)) => {
                        Some(self.migrate(a, b, payload, depth + 1)?)
                    }
                    _ => return Err(DataError::TypeMismatch),
                };
                Ok(Datum::Variant(self.target.new_variant(
                    *new_id,
                    &new_case.name,
                    payload,
                )?))
            }
            (DataType::List(a), DataType::List(b), Datum::List(list))
                if a.as_ref() == list.element_type() =>
            {
                let mut converted = Vec::new();
                converted
                    .try_reserve_exact(list.len())
                    .map_err(|_| DataError::InvalidShape)?;
                for value in list.values() {
                    converted.push(self.migrate(a, b, value, depth + 1)?);
                }
                Ok(Datum::List(
                    self.target.new_list(b.as_ref().clone(), converted)?,
                ))
            }
            (DataType::Map(old_key, a), DataType::Map(new_key, b), Datum::Map(map))
                if old_key == new_key && old_key == &map.key_type() =>
            {
                let mut converted = Vec::new();
                converted
                    .try_reserve_exact(map.len())
                    .map_err(|_| DataError::InvalidShape)?;
                for (key, value) in map.entries() {
                    converted.push((key.clone(), self.migrate(a, b, value, depth + 1)?));
                }
                Ok(Datum::Map(self.target.new_map(
                    *new_key,
                    b.as_ref().clone(),
                    converted,
                )?))
            }
            (DataType::Optional(_), DataType::Optional(_), Datum::Optional(None)) => {
                Ok(Datum::none())
            }
            (DataType::Optional(a), DataType::Optional(b), Datum::Optional(Some(value))) => {
                Ok(Datum::some(self.migrate(a, b, value, depth + 1)?))
            }
            (
                DataType::Outcome(a, _),
                DataType::Outcome(b, _),
                Datum::Outcome(OutcomeValue::Ok(v)),
            ) => Ok(Datum::ok(self.migrate(a, b, v, depth + 1)?)),
            (
                DataType::Outcome(_, a),
                DataType::Outcome(_, b),
                Datum::Outcome(OutcomeValue::Err(v)),
            ) => Ok(Datum::err(self.migrate(a, b, v, depth + 1)?)),
            _ => Err(DataError::TypeMismatch),
        }
    }
}
impl SchemaRegistry {
    /// Transform a source snapshot against another independently validated
    /// registry. Uses exact field/case identity by name and never mutates
    /// either input registry or the source value.
    pub fn migrate_data(
        &self,
        target: &SchemaRegistry,
        source_type: &DataType,
        target_type: &DataType,
        source_value: &Datum,
        options: &MigrationOptions,
    ) -> Result<MigrationResult, DataError> {
        self.verify(source_type, source_value)?;
        target.validate_type(target_type)?;
        let mut migrator = Migrator {
            source: self,
            target,
            options,
            stats: MigrationStats::default(),
        };
        let value = migrator.migrate(source_type, target_type, source_value, 1)?;
        target.verify(target_type, &value)?;
        Ok(MigrationResult {
            value,
            stats: migrator.stats,
        })
    }
}
