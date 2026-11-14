//! Bounded structural matching and conservative tagged-variant arm coverage.
//! Not a parser rule; future source matching can lower to these operations.
use super::limits::{bounded, identifier, DataError, DataResource};
use super::schema::SchemaRegistry;
use super::value::{Datum, OutcomeValue};
use crate::types::DefinitionId;
use crate::value::Value;
use std::collections::{BTreeMap, HashSet};

/// Every matched leaf preserves immutable owned snapshots in bindings.
/// Duplicate variable binders are rejected rather than implicitly compared.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pattern {
    Any,
    Bind(String),
    Literal(Value),
    Record {
        identity: DefinitionId,
        fields: Vec<(String, Pattern)>,
    },
    Variant {
        identity: DefinitionId,
        case: String,
        payload: Option<Box<Pattern>>,
    },
    List(Vec<Pattern>),
    OptionalNone,
    OptionalSome(Box<Pattern>),
    OutcomeOk(Box<Pattern>),
    OutcomeErr(Box<Pattern>),
}
impl Pattern {
    pub fn variant(id: DefinitionId, case: impl Into<String>, payload: Option<Self>) -> Self {
        Self::Variant {
            identity: id,
            case: case.into(),
            payload: payload.map(Box::new),
        }
    }
    pub fn bind(name: impl Into<String>) -> Self {
        Self::Bind(name.into())
    }
    /// Only wildcard/binder payload patterns are unconditionally covering.
    pub fn is_irrefutable(&self) -> bool {
        matches!(self, Self::Any | Self::Bind(_))
    }
}
pub type Bindings = BTreeMap<String, Datum>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Coverage {
    pub exhaustive: bool,
    pub missing: Vec<String>,
    /// Unreachable because a previous unguarded pattern already covers it.
    pub unreachable_arms: Vec<usize>,
}

impl SchemaRegistry {
    /// Match without partial bindings or mutation on a nonmatch or error.
    /// Distinct work accounting from program execution is deliberate.
    pub fn match_pattern(
        &self,
        pattern: &Pattern,
        value: &Datum,
    ) -> Result<Option<Bindings>, DataError> {
        let mut captures = BTreeMap::new();
        let mut work = 0usize;
        let mut pending = vec![(pattern, value, 1usize)];
        while let Some((pat, val, depth)) = pending.pop() {
            work = work.checked_add(1).ok_or(DataError::Limit {
                resource: DataResource::PatternNodes,
                limit: self.limits().max_pattern_nodes,
            })?;
            bounded(
                work,
                self.limits().max_pattern_nodes,
                DataResource::PatternNodes,
            )?;
            bounded(
                depth,
                self.limits().max_value_depth,
                DataResource::ValueDepth,
            )?;
            match (pat, val) {
                (Pattern::Any, _) => (),
                (Pattern::Bind(name), found) => {
                    identifier(name)?;
                    if captures.insert(name.clone(), found.clone()).is_some() {
                        return Err(DataError::DuplicateName(name.clone()));
                    }
                }
                (Pattern::Literal(needle), Datum::Source(found)) if needle == found => (),
                (Pattern::Literal(_), _) => return Ok(None),
                (Pattern::Record { identity, fields }, Datum::Record(record))
                    if identity == &record.identity() =>
                {
                    let spec = self.record(*identity)?;
                    let mut used = HashSet::new();
                    for (name, nested) in fields.iter().rev() {
                        if !used.insert(name.as_str()) {
                            return Err(DataError::DuplicateName(name.clone()));
                        }
                        let index = spec
                            .iter()
                            .position(|field| field.name == *name)
                            .ok_or_else(|| DataError::UnknownField(name.clone()))?;
                        let nested_value =
                            record.values().get(index).ok_or(DataError::InvalidShape)?;
                        pending.push((nested, nested_value, depth + 1));
                    }
                }
                (Pattern::Record { .. }, _) => return Ok(None),
                (
                    Pattern::Variant {
                        identity,
                        case,
                        payload,
                    },
                    Datum::Variant(variant),
                ) if identity == &variant.identity() => {
                    let cases = self.variants(*identity)?;
                    let index = cases
                        .iter()
                        .position(|c| c.name == *case)
                        .ok_or_else(|| DataError::UnknownCase(case.clone()))?;
                    if index != variant.case_index() {
                        return Ok(None);
                    }
                    match (payload, variant.payload()) {
                        (None, None) => (),
                        (Some(p), Some(v)) => pending.push((p, v, depth + 1)),
                        _ => return Err(DataError::InvalidPattern),
                    }
                }
                (Pattern::Variant { .. }, _) => return Ok(None),
                (Pattern::List(elements), Datum::List(list)) => {
                    if elements.len() != list.len() {
                        return Ok(None);
                    }
                    for (pat, item) in elements.iter().zip(list.values()).rev() {
                        pending.push((pat, item, depth + 1));
                    }
                }
                (Pattern::List(_), _) => return Ok(None),
                (Pattern::OptionalNone, Datum::Optional(None)) => (),
                (Pattern::OptionalSome(p), Datum::Optional(Some(v))) => {
                    pending.push((p, v, depth + 1))
                }
                (Pattern::OptionalNone | Pattern::OptionalSome(_), _) => return Ok(None),
                (Pattern::OutcomeOk(p), Datum::Outcome(OutcomeValue::Ok(v))) => {
                    pending.push((p, v, depth + 1))
                }
                (Pattern::OutcomeErr(p), Datum::Outcome(OutcomeValue::Err(v))) => {
                    pending.push((p, v, depth + 1))
                }
                (Pattern::OutcomeOk(_) | Pattern::OutcomeErr(_), _) => return Ok(None),
            }
        }
        Ok(Some(captures))
    }

    /// Conservatively analyze *unguarded* top-level arms for one nominal
    /// variant. A refutable payload cannot cover its entire case.
    pub fn variant_coverage(
        &self,
        id: DefinitionId,
        arms: &[Pattern],
    ) -> Result<Coverage, DataError> {
        let cases = self.variants(id)?;
        bounded(
            arms.len(),
            self.limits().max_pattern_nodes,
            DataResource::PatternNodes,
        )?;
        let mut completely_covered = HashSet::new();
        let mut catch_all = false;
        let mut unreachable_arms = Vec::new();
        for (index, pat) in arms.iter().enumerate() {
            if catch_all || completely_covered.len() == cases.len() {
                unreachable_arms.push(index);
                continue;
            }
            match pat {
                Pattern::Any | Pattern::Bind(_) => {
                    if let Pattern::Bind(name) = pat {
                        identifier(name)?;
                    }
                    catch_all = true;
                }
                Pattern::Variant {
                    identity,
                    case,
                    payload,
                } => {
                    if *identity != id {
                        return Err(DataError::InvalidPattern);
                    }
                    let schema = cases
                        .iter()
                        .find(|c| c.name == *case)
                        .ok_or_else(|| DataError::UnknownCase(case.clone()))?;
                    let covers = match (&schema.payload, payload) {
                        (None, None) => true,
                        (Some(_), Some(p)) => p.is_irrefutable(),
                        (None, Some(_)) | (Some(_), None) => return Err(DataError::InvalidPattern),
                    };
                    if covers && !completely_covered.insert(case.as_str()) {
                        unreachable_arms.push(index);
                    }
                }
                _ => return Err(DataError::InvalidPattern),
            }
        }
        let missing: Vec<String> = if catch_all {
            Vec::new()
        } else {
            cases
                .iter()
                .filter(|c| !completely_covered.contains(c.name.as_str()))
                .map(|c| c.name.clone())
                .collect()
        };
        Ok(Coverage {
            exhaustive: missing.is_empty(),
            missing,
            unreachable_arms,
        })
    }
}
