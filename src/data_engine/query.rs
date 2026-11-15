//! Pure typed selection, filtering and aggregation over persistent lists.
//!
//! Predicates are explicit immutable host plans, not evaluated Scarp source.
//! Preflight rejects invalid type paths before inspecting any selected row.
//! Execution owns a deterministic query-work meter and has no I/O effects.
use super::limits::{bounded, DataError, DataResource};
use super::paths::PathSegment;
use super::schema::{DataType, MapKeyType, SchemaRegistry};
use super::value::{DataKey, Datum, ListValue, MapValue};
use crate::types::{PrimitiveType, SourceType};
use crate::value::Value;
use std::collections::BTreeMap;
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Predicate {
    True,
    False,
    Equals {
        path: Vec<PathSegment>,
        value: Datum,
    },
    IntegerRange {
        path: Vec<PathSegment>,
        minimum: Option<i64>,
        maximum: Option<i64>,
    },
    TextContains {
        path: Vec<PathSegment>,
        needle: Arc<str>,
    },
    OptionalPresent {
        path: Vec<PathSegment>,
    },
    VariantCase {
        path: Vec<PathSegment>,
        case: String,
    },
    All(Vec<Predicate>),
    Any(Vec<Predicate>),
    Not(Box<Predicate>),
}
impl Predicate {
    pub fn equals(path: Vec<PathSegment>, value: Datum) -> Self {
        Self::Equals { path, value }
    }
    pub fn range(path: Vec<PathSegment>, minimum: Option<i64>, maximum: Option<i64>) -> Self {
        Self::IntegerRange {
            path,
            minimum,
            maximum,
        }
    }
    pub fn contains(path: Vec<PathSegment>, needle: impl Into<Arc<str>>) -> Self {
        Self::TextContains {
            path,
            needle: needle.into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QueryLimits {
    pub max_steps: u64,
    pub max_scanned_rows: usize,
    pub max_returned_rows: usize,
    pub max_predicate_depth: usize,
    pub max_predicate_nodes: usize,
}
impl Default for QueryLimits {
    fn default() -> Self {
        Self {
            max_steps: 1_000_000,
            max_scanned_rows: 4096,
            max_returned_rows: 4096,
            max_predicate_depth: 32,
            max_predicate_nodes: 4096,
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct QueryStats {
    pub scanned: usize,
    pub matched: usize,
    pub work: u64,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueryPlan {
    pub predicate: Predicate,
    pub projection: Vec<PathSegment>,
    /// Maximum successful matches processed. None means the entire list.
    pub limit: Option<usize>,
}
impl Default for QueryPlan {
    fn default() -> Self {
        Self {
            predicate: Predicate::True,
            projection: Vec::new(),
            limit: None,
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueryResult {
    pub rows: ListValue,
    pub stats: QueryStats,
}
#[derive(Debug, Clone)]
struct QueryMeter {
    limits: QueryLimits,
    stats: QueryStats,
}
impl QueryMeter {
    fn new(limits: QueryLimits) -> Self {
        Self {
            limits,
            stats: QueryStats::default(),
        }
    }
    fn step(&mut self) -> Result<(), DataError> {
        self.stats.work = self.stats.work.checked_add(1).ok_or(DataError::Limit {
            resource: DataResource::QuerySteps,
            limit: usize::try_from(self.limits.max_steps).unwrap_or(usize::MAX),
        })?;
        if self.stats.work > self.limits.max_steps {
            return Err(DataError::Limit {
                resource: DataResource::QuerySteps,
                limit: usize::try_from(self.limits.max_steps).unwrap_or(usize::MAX),
            });
        }
        Ok(())
    }
    fn scan(&mut self) -> Result<(), DataError> {
        self.stats.scanned = self.stats.scanned.checked_add(1).ok_or(DataError::Limit {
            resource: DataResource::QueryScan,
            limit: self.limits.max_scanned_rows,
        })?;
        bounded(
            self.stats.scanned,
            self.limits.max_scanned_rows,
            DataResource::QueryScan,
        )?;
        self.step()
    }
    fn matched(&mut self) -> Result<(), DataError> {
        self.stats.matched = self.stats.matched.checked_add(1).ok_or(DataError::Limit {
            resource: DataResource::QueryOutput,
            limit: self.limits.max_returned_rows,
        })?;
        bounded(
            self.stats.matched,
            self.limits.max_returned_rows,
            DataResource::QueryOutput,
        )
    }
}
impl Predicate {
    /// Check all branches, including those that may be short-circuited later.
    pub fn preflight(
        &self,
        registry: &SchemaRegistry,
        root: &DataType,
        limits: QueryLimits,
    ) -> Result<(), DataError> {
        registry.validate_type(root)?;
        let mut nodes = 0usize;
        let mut pending = vec![(self, 1usize)];
        while let Some((predicate, depth)) = pending.pop() {
            bounded(
                depth,
                limits.max_predicate_depth,
                DataResource::PatternNodes,
            )?;
            nodes = nodes.checked_add(1).ok_or(DataError::Limit {
                resource: DataResource::PatternNodes,
                limit: limits.max_predicate_nodes,
            })?;
            bounded(
                nodes,
                limits.max_predicate_nodes,
                DataResource::PatternNodes,
            )?;
            match predicate {
                Predicate::True | Predicate::False => (),
                Predicate::Equals { path, value } => {
                    let ty = registry.path_type(root, path)?;
                    registry.verify(&ty, value)?;
                }
                Predicate::IntegerRange {
                    path,
                    minimum,
                    maximum,
                } => {
                    if registry.path_type(root, path)? != DataType::integer() {
                        return Err(DataError::TypeMismatch);
                    }
                    if let (Some(min), Some(max)) = (minimum, maximum) {
                        if min > max {
                            return Err(DataError::InvalidShape);
                        }
                    }
                }
                Predicate::TextContains { path, needle } => {
                    if registry.path_type(root, path)? != DataType::string() {
                        return Err(DataError::TypeMismatch);
                    }
                    bounded(
                        needle.len(),
                        registry.limits().max_text_bytes,
                        DataResource::TextBytes,
                    )?;
                }
                Predicate::OptionalPresent { path } => {
                    if !matches!(registry.path_type(root, path)?, DataType::Optional(_)) {
                        return Err(DataError::TypeMismatch);
                    }
                }
                Predicate::VariantCase { path, case } => {
                    let ty = registry.path_type(root, path)?;
                    match ty {
                        DataType::Variant(id) => {
                            if !registry.variants(id)?.iter().any(|c| c.name == *case) {
                                return Err(DataError::UnknownCase(case.clone()));
                            }
                        }
                        _ => return Err(DataError::TypeMismatch),
                    }
                }
                Predicate::All(items) | Predicate::Any(items) => {
                    for child in items.iter().rev() {
                        pending.push((child, depth + 1));
                    }
                }
                Predicate::Not(child) => pending.push((child, depth + 1)),
            }
        }
        Ok(())
    }
    fn evaluate(
        &self,
        registry: &SchemaRegistry,
        root: &DataType,
        row: &Datum,
        meter: &mut QueryMeter,
        depth: usize,
    ) -> Result<bool, DataError> {
        bounded(
            depth,
            meter.limits.max_predicate_depth,
            DataResource::PatternNodes,
        )?;
        meter.step()?;
        Ok(match self {
            Predicate::True => true,
            Predicate::False => false,
            Predicate::Equals { path, value } => registry.select_path(root, row, path)? == value,
            Predicate::IntegerRange {
                path,
                minimum,
                maximum,
            } => {
                let item = registry.select_path(root, row, path)?;
                match item {
                    Datum::Source(Value::Integer(x)) => {
                        minimum.map_or(true, |min| *x >= min)
                            && maximum.map_or(true, |max| *x <= max)
                    }
                    _ => return Err(DataError::TypeMismatch),
                }
            }
            Predicate::TextContains { path, needle } => {
                let item = registry.select_path(root, row, path)?;
                match item {
                    Datum::Source(Value::String(text)) => text.contains(needle.as_ref()),
                    _ => return Err(DataError::TypeMismatch),
                }
            }
            Predicate::OptionalPresent { path } => {
                matches!(
                    registry.select_path(root, row, path)?,
                    Datum::Optional(Some(_))
                )
            }
            Predicate::VariantCase { path, case } => {
                match registry.select_path(root, row, path)? {
                    Datum::Variant(value) => registry.case_name(value)? == case,
                    _ => return Err(DataError::TypeMismatch),
                }
            }
            Predicate::All(items) => {
                let mut matches = true;
                for child in items {
                    if !child.evaluate(registry, root, row, meter, depth + 1)? {
                        matches = false;
                        break;
                    }
                }
                matches
            }
            Predicate::Any(items) => {
                let mut matches = false;
                for child in items {
                    if child.evaluate(registry, root, row, meter, depth + 1)? {
                        matches = true;
                        break;
                    }
                }
                matches
            }
            Predicate::Not(child) => !child.evaluate(registry, root, row, meter, depth + 1)?,
        })
    }
}
impl QueryPlan {
    pub fn projection_type(
        &self,
        registry: &SchemaRegistry,
        element: &DataType,
    ) -> Result<DataType, DataError> {
        registry.path_type(element, &self.projection)
    }
    pub fn preflight(
        &self,
        registry: &SchemaRegistry,
        element: &DataType,
        limits: QueryLimits,
    ) -> Result<(), DataError> {
        self.predicate.preflight(registry, element, limits)?;
        self.projection_type(registry, element)?;
        Ok(())
    }
    fn visit<F>(
        &self,
        registry: &SchemaRegistry,
        source: &ListValue,
        limits: QueryLimits,
        mut visitor: F,
    ) -> Result<QueryStats, DataError>
    where
        F: FnMut(&Datum) -> Result<(), DataError>,
    {
        self.preflight(registry, source.element_type(), limits)?;
        registry.verify(
            &DataType::list(source.element_type().clone()),
            &Datum::List(source.clone()),
        )?;
        let mut meter = QueryMeter::new(limits);
        for row in source.values() {
            if self.limit.is_some_and(|limit| meter.stats.matched >= limit) {
                break;
            }
            meter.scan()?;
            if self
                .predicate
                .evaluate(registry, source.element_type(), row, &mut meter, 1)?
            {
                meter.matched()?;
                visitor(row)?;
            }
        }
        Ok(meter.stats)
    }
    pub fn execute(
        &self,
        registry: &SchemaRegistry,
        source: &ListValue,
        limits: QueryLimits,
    ) -> Result<QueryResult, DataError> {
        let selected_type = self.projection_type(registry, source.element_type())?;
        let mut values = Vec::new();
        let stats = self.visit(registry, source, limits, |row| {
            let item = registry.select_path(source.element_type(), row, &self.projection)?;
            values.push(item.clone());
            Ok(())
        })?;
        let rows = registry.new_list(selected_type, values)?;
        Ok(QueryResult { rows, stats })
    }
    pub fn count(
        &self,
        registry: &SchemaRegistry,
        source: &ListValue,
        limits: QueryLimits,
    ) -> Result<QueryStats, DataError> {
        self.visit(registry, source, limits, |_| Ok(()))
    }
    /// Checked signed-64 reduction over projected integer values, with no
    /// silent saturation or implicit BigInt/float promotion.
    pub fn sum_integer(
        &self,
        registry: &SchemaRegistry,
        source: &ListValue,
        limits: QueryLimits,
    ) -> Result<(i64, QueryStats), DataError> {
        if self.projection_type(registry, source.element_type())? != DataType::integer() {
            return Err(DataError::TypeMismatch);
        }
        let mut total = 0i64;
        let stats = self.visit(registry, source, limits, |row| {
            let item = registry.select_path(source.element_type(), row, &self.projection)?;
            match item {
                Datum::Source(Value::Integer(value)) => {
                    total = total
                        .checked_add(*value)
                        .ok_or(DataError::ArithmeticOverflow)?;
                    Ok(())
                }
                _ => Err(DataError::TypeMismatch),
            }
        })?;
        Ok((total, stats))
    }
    /// Group selected rows by an integer/text projection, yielding a
    /// deterministic typed map of exact counts.
    pub fn group_count(
        &self,
        registry: &SchemaRegistry,
        source: &ListValue,
        limits: QueryLimits,
    ) -> Result<(MapValue, QueryStats), DataError> {
        let ty = self.projection_type(registry, source.element_type())?;
        let keys = match &ty {
            DataType::Source(SourceType::Primitive(PrimitiveType::Integer)) => MapKeyType::Integer,
            DataType::Source(SourceType::Primitive(PrimitiveType::String)) => MapKeyType::Text,
            _ => return Err(DataError::InvalidKey),
        };
        let mut counts: BTreeMap<DataKey, i64> = BTreeMap::new();
        let stats = self.visit(registry, source, limits, |row| {
            let item = registry.select_path(source.element_type(), row, &self.projection)?;
            let key = match item {
                Datum::Source(Value::Integer(v)) => DataKey::Integer(*v),
                Datum::Source(Value::String(v)) => DataKey::Text(v.clone()),
                _ => return Err(DataError::InvalidKey),
            };
            let count = counts.entry(key).or_insert(0);
            *count = count.checked_add(1).ok_or(DataError::ArithmeticOverflow)?;
            Ok(())
        })?;
        let entries = counts
            .into_iter()
            .map(|(key, count)| (key, Datum::integer(count)))
            .collect();
        Ok((registry.new_map(keys, DataType::integer(), entries)?, stats))
    }
}
