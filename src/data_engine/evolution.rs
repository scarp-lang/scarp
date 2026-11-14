//! Deterministic nominal schema evolution diagnostics and invalidation impact.
//!
//! This module compares schema declarations by existing compiler DefinitionId.
//! It does not infer a migration from equal struct layout or reinterpret a
//! variant case index; all changes are reported with explicit semantic risk.
use super::limits::DataError;
use super::schema::{DataType, Definition, DefinitionKind, SchemaRegistry};
use super::schema_graph::SchemaGraph;
use crate::types::DefinitionId;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SchemaRisk {
    /// Types are unchanged or only previously absent declarations were added.
    Compatible,
    /// Existing data and/or source code require explicit adaptation.
    RequiresMigration,
    /// Removal, kind/type change or exhaustiveness-changing variant edit.
    Breaking,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SchemaChange {
    DefinitionAdded {
        id: DefinitionId,
        name: String,
    },
    DefinitionRemoved {
        id: DefinitionId,
        name: String,
    },
    DefinitionRenamed {
        id: DefinitionId,
        before: String,
        after: String,
    },
    DefinitionKindChanged {
        id: DefinitionId,
    },
    FieldAdded {
        id: DefinitionId,
        name: String,
        ty: DataType,
    },
    FieldRemoved {
        id: DefinitionId,
        name: String,
    },
    FieldTypeChanged {
        id: DefinitionId,
        name: String,
        before: DataType,
        after: DataType,
    },
    FieldReordered {
        id: DefinitionId,
        name: String,
        before: usize,
        after: usize,
    },
    CaseAdded {
        id: DefinitionId,
        name: String,
    },
    CaseRemoved {
        id: DefinitionId,
        name: String,
    },
    CasePayloadChanged {
        id: DefinitionId,
        name: String,
    },
    CaseReordered {
        id: DefinitionId,
        name: String,
        before: usize,
        after: usize,
    },
}
impl SchemaChange {
    pub fn definition(&self) -> DefinitionId {
        match self {
            Self::DefinitionAdded { id, .. }
            | Self::DefinitionRemoved { id, .. }
            | Self::DefinitionRenamed { id, .. }
            | Self::DefinitionKindChanged { id }
            | Self::FieldAdded { id, .. }
            | Self::FieldRemoved { id, .. }
            | Self::FieldTypeChanged { id, .. }
            | Self::FieldReordered { id, .. }
            | Self::CaseAdded { id, .. }
            | Self::CaseRemoved { id, .. }
            | Self::CasePayloadChanged { id, .. }
            | Self::CaseReordered { id, .. } => *id,
        }
    }
    pub fn risk(&self) -> SchemaRisk {
        match self {
            Self::DefinitionAdded { .. } => SchemaRisk::Compatible,
            Self::DefinitionRenamed { .. }
            | Self::FieldAdded { .. }
            | Self::FieldReordered { .. }
            | Self::CaseReordered { .. } => SchemaRisk::RequiresMigration,
            Self::DefinitionRemoved { .. }
            | Self::DefinitionKindChanged { .. }
            | Self::FieldRemoved { .. }
            | Self::FieldTypeChanged { .. }
            | Self::CaseAdded { .. }
            | Self::CaseRemoved { .. }
            | Self::CasePayloadChanged { .. } => SchemaRisk::Breaking,
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaComparison {
    pub changes: Vec<SchemaChange>,
    pub risk: SchemaRisk,
    /// Directly altered nominal definitions sorted by module/declaration ID.
    pub changed_ids: Vec<DefinitionId>,
}
impl SchemaComparison {
    pub fn is_identical(&self) -> bool {
        self.changes.is_empty()
    }
    pub fn change_count(&self) -> usize {
        self.changes.len()
    }
}
fn id_key(id: DefinitionId) -> (u32, u32) {
    (id.module, id.declaration)
}
fn changed_id(key: (u32, u32)) -> DefinitionId {
    DefinitionId {
        module: key.0,
        declaration: key.1,
    }
}
fn named_fields(
    before: &[(String, DataType)],
    after: &[(String, DataType)],
    id: DefinitionId,
    changes: &mut Vec<SchemaChange>,
) {
    for (old_index, (name, old_type)) in before.iter().enumerate() {
        match after.iter().position(|(other, _)| other == name) {
            None => changes.push(SchemaChange::FieldRemoved {
                id,
                name: name.clone(),
            }),
            Some(new_index) => {
                let new_type = &after[new_index].1;
                if old_type != new_type {
                    changes.push(SchemaChange::FieldTypeChanged {
                        id,
                        name: name.clone(),
                        before: old_type.clone(),
                        after: new_type.clone(),
                    });
                }
                if old_index != new_index {
                    changes.push(SchemaChange::FieldReordered {
                        id,
                        name: name.clone(),
                        before: old_index,
                        after: new_index,
                    });
                }
            }
        }
    }
    for (name, ty) in after {
        if !before.iter().any(|(old_name, _)| old_name == name) {
            changes.push(SchemaChange::FieldAdded {
                id,
                name: name.clone(),
                ty: ty.clone(),
            });
        }
    }
}
fn compare_definition(before: &Definition, after: &Definition, changes: &mut Vec<SchemaChange>) {
    let id = before.id;
    if before.name != after.name {
        changes.push(SchemaChange::DefinitionRenamed {
            id,
            before: before.name.clone(),
            after: after.name.clone(),
        });
    }
    match (&before.kind, &after.kind) {
        (DefinitionKind::Record(old), DefinitionKind::Record(new)) => {
            let old: Vec<_> = old.iter().map(|f| (f.name.clone(), f.ty.clone())).collect();
            let new: Vec<_> = new.iter().map(|f| (f.name.clone(), f.ty.clone())).collect();
            named_fields(&old, &new, id, changes);
        }
        (DefinitionKind::Variant(old), DefinitionKind::Variant(new)) => {
            for (index, case) in old.iter().enumerate() {
                match new.iter().position(|new_case| new_case.name == case.name) {
                    None => changes.push(SchemaChange::CaseRemoved {
                        id,
                        name: case.name.clone(),
                    }),
                    Some(new_index) => {
                        if case.payload != new[new_index].payload {
                            changes.push(SchemaChange::CasePayloadChanged {
                                id,
                                name: case.name.clone(),
                            });
                        }
                        if index != new_index {
                            changes.push(SchemaChange::CaseReordered {
                                id,
                                name: case.name.clone(),
                                before: index,
                                after: new_index,
                            });
                        }
                    }
                }
            }
            for case in new {
                if !old.iter().any(|before| before.name == case.name) {
                    changes.push(SchemaChange::CaseAdded {
                        id,
                        name: case.name.clone(),
                    });
                }
            }
        }
        _ => changes.push(SchemaChange::DefinitionKindChanged { id }),
    }
}
impl SchemaRegistry {
    /// Compare against another complete registry; changes are ordered first by
    /// nominal ID and then by declaration membership, not hashmap iteration.
    pub fn compare_schema(&self, target: &SchemaRegistry) -> SchemaComparison {
        let mut ids = BTreeSet::new();
        let mut old = BTreeMap::new();
        let mut new = BTreeMap::new();
        for definition in self.definitions() {
            let key = id_key(definition.id);
            ids.insert(key);
            old.insert(key, definition);
        }
        for definition in target.definitions() {
            let key = id_key(definition.id);
            ids.insert(key);
            new.insert(key, definition);
        }
        let mut changes = Vec::new();
        let mut changed_ids = Vec::new();
        for key in ids {
            let before = changes.len();
            match (old.get(&key), new.get(&key)) {
                (Some(before), Some(after)) => compare_definition(before, after, &mut changes),
                (Some(before), None) => changes.push(SchemaChange::DefinitionRemoved {
                    id: before.id,
                    name: before.name.clone(),
                }),
                (None, Some(after)) => changes.push(SchemaChange::DefinitionAdded {
                    id: after.id,
                    name: after.name.clone(),
                }),
                (None, None) => unreachable!("union must refer to at least one declaration"),
            }
            if changes.len() != before {
                changed_ids.push(changed_id(key));
            }
        }
        let risk = changes
            .iter()
            .map(SchemaChange::risk)
            .max()
            .unwrap_or(SchemaRisk::Compatible);
        SchemaComparison {
            changes,
            risk,
            changed_ids,
        }
    }
    /// Collect direct schema changes and all transitively dependent nominal
    /// definitions in either the old or new registry, in stable ID order.
    /// Removed definitions still appear in the impact set.
    pub fn schema_impact(&self, target: &SchemaRegistry) -> Result<Vec<DefinitionId>, DataError> {
        let comparison = self.compare_schema(target);
        let before = SchemaGraph::new(self)?;
        let after = SchemaGraph::new(target)?;
        let mut impacted = BTreeSet::new();
        for id in comparison.changed_ids {
            impacted.insert(id_key(id));
            for graph in [&before, &after] {
                if graph.identities().contains(&id) {
                    for dependent in graph.transitive_dependents(id)? {
                        impacted.insert(id_key(dependent));
                    }
                }
            }
        }
        Ok(impacted.into_iter().map(changed_id).collect())
    }
}
