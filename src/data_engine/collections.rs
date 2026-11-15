//! Verified data construction and persistent collection operations.
//!
//! Every operation either returns a fully checked new value or an error;
//! it never mutates a previously published persistent snapshot. The meter
//! charges logical visited nodes and visible UTF-8 payload, not OS RSS.
use super::limits::{bounded, DataError, DataResource, Meter};
use super::schema::{DataType, DefinitionKind, MapKeyType, SchemaRegistry};
use super::value::{DataKey, Datum, ListValue, MapValue, OutcomeValue, RecordValue, VariantValue};
use crate::types::DefinitionId;
use crate::value::Value;
use std::sync::Arc;

impl SchemaRegistry {
    /// Validate the complete visible graph before exposing a constructed value.
    pub fn verify(&self, ty: &DataType, value: &Datum) -> Result<(), DataError> {
        self.validate_type(ty)?;
        let mut meter = Meter::new(self.limits());
        let mut pending = vec![(ty, value, 1usize)];
        while let Some((expected, actual, depth)) = pending.pop() {
            meter.node(depth)?;
            match (expected, actual) {
                (DataType::Source(kind), Datum::Source(value)) if &value.kind() == kind => {
                    if let Value::String(text) = value {
                        meter.text(text.len())?;
                    }
                }
                (DataType::Record(id), Datum::Record(record)) if id == &record.identity => {
                    let fields = self.record(*id)?;
                    if fields.len() != record.values.len() {
                        return Err(DataError::InvalidShape);
                    }
                    for (field, datum) in fields.iter().zip(record.values.iter()).rev() {
                        pending.push((&field.ty, datum, depth + 1));
                    }
                }
                (DataType::Variant(id), Datum::Variant(variant)) if id == &variant.identity => {
                    let cases = self.variants(*id)?;
                    let case = cases
                        .get(variant.case_index)
                        .ok_or(DataError::InvalidShape)?;
                    match (&case.payload, variant.payload.as_ref()) {
                        (None, None) => (),
                        (Some(t), Some(v)) => pending.push((t, v, depth + 1)),
                        _ => return Err(DataError::InvalidShape),
                    }
                }
                (DataType::List(element), Datum::List(list))
                    if element.as_ref() == &list.element =>
                {
                    meter.entries(list.values.len())?;
                    for item in list.values.iter().rev() {
                        pending.push((element, item, depth + 1));
                    }
                }
                (DataType::Map(keys, element), Datum::Map(map))
                    if keys == &map.keys && element.as_ref() == &map.element =>
                {
                    meter.entries(map.pairs.len())?;
                    let mut previous = None;
                    for (key, item) in map.pairs.iter() {
                        if key.kind() != *keys {
                            return Err(DataError::InvalidKey);
                        }
                        if let Some(prev) = previous {
                            if prev >= key {
                                return Err(DataError::DuplicateKey);
                            }
                        }
                        previous = Some(key);
                        if let DataKey::Text(text) = key {
                            meter.text(text.len())?;
                        }
                        pending.push((element, item, depth + 1));
                    }
                }
                (DataType::Optional(_), Datum::Optional(None)) => (),
                (DataType::Optional(inner), Datum::Optional(Some(value))) => {
                    pending.push((inner, value, depth + 1))
                }
                (DataType::Outcome(ok, _), Datum::Outcome(OutcomeValue::Ok(value))) => {
                    pending.push((ok, value, depth + 1))
                }
                (DataType::Outcome(_, err), Datum::Outcome(OutcomeValue::Err(value))) => {
                    pending.push((err, value, depth + 1))
                }
                _ => return Err(DataError::TypeMismatch),
            }
        }
        Ok(())
    }

    /// Schema-order fields; caller cannot reorder after validation.
    pub fn new_record(
        &self,
        id: DefinitionId,
        values: Vec<Datum>,
    ) -> Result<RecordValue, DataError> {
        let fields = self.record(id)?;
        if fields.len() != values.len() {
            return Err(DataError::InvalidShape);
        }
        let record = RecordValue {
            identity: id,
            values: values.into(),
        };
        self.verify(&DataType::Record(id), &Datum::Record(record.clone()))?;
        Ok(record)
    }
    /// Named fields are canonicalized into declaration order, preventing
    /// accidentally swapping equal-typed fields.
    pub fn record_named(
        &self,
        id: DefinitionId,
        named: Vec<(String, Datum)>,
    ) -> Result<RecordValue, DataError> {
        let fields = self.record(id)?;
        if named.len() != fields.len() {
            return Err(DataError::InvalidShape);
        }
        let mut resolved: Vec<Option<Datum>> = vec![None; fields.len()];
        for (name, value) in named {
            let index = fields
                .iter()
                .position(|field| field.name == name)
                .ok_or(DataError::UnknownField(name))?;
            if resolved[index].replace(value).is_some() {
                return Err(DataError::DuplicateName(fields[index].name.clone()));
            }
        }
        let ordered = resolved
            .into_iter()
            .collect::<Option<Vec<_>>>()
            .ok_or(DataError::InvalidShape)?;
        self.new_record(id, ordered)
    }
    pub fn field<'a>(&self, record: &'a RecordValue, name: &str) -> Result<&'a Datum, DataError> {
        let fields = self.record(record.identity)?;
        let index = fields
            .iter()
            .position(|field| field.name == name)
            .ok_or_else(|| DataError::UnknownField(name.to_owned()))?;
        record.values.get(index).ok_or(DataError::InvalidShape)
    }
    pub fn replace_field(
        &self,
        record: &RecordValue,
        name: &str,
        value: Datum,
    ) -> Result<RecordValue, DataError> {
        let fields = self.record(record.identity)?;
        let index = fields
            .iter()
            .position(|field| field.name == name)
            .ok_or_else(|| DataError::UnknownField(name.to_owned()))?;
        self.verify(&fields[index].ty, &value)?;
        let mut values = record.values.to_vec();
        values[index] = value;
        self.new_record(record.identity, values)
    }
    pub fn new_variant(
        &self,
        id: DefinitionId,
        case: &str,
        payload: Option<Datum>,
    ) -> Result<VariantValue, DataError> {
        let variants = self.variants(id)?;
        let index = variants
            .iter()
            .position(|v| v.name == case)
            .ok_or_else(|| DataError::UnknownCase(case.to_owned()))?;
        let variant = VariantValue {
            identity: id,
            case_index: index,
            payload: payload.map(Arc::new),
        };
        self.verify(&DataType::Variant(id), &Datum::Variant(variant.clone()))?;
        Ok(variant)
    }
    pub fn case_name<'a>(&'a self, value: &VariantValue) -> Result<&'a str, DataError> {
        self.variants(value.identity)?
            .get(value.case_index)
            .map(|c| c.name.as_str())
            .ok_or(DataError::InvalidShape)
    }
    pub fn new_list(&self, element: DataType, values: Vec<Datum>) -> Result<ListValue, DataError> {
        bounded(
            values.len(),
            self.limits().max_collection_entries,
            DataResource::CollectionEntries,
        )?;
        let list = ListValue {
            element,
            values: values.into(),
        };
        self.verify(
            &DataType::list(list.element.clone()),
            &Datum::List(list.clone()),
        )?;
        Ok(list)
    }
    pub fn list_push(&self, list: &ListValue, value: Datum) -> Result<ListValue, DataError> {
        let mut next = list.values.to_vec();
        next.push(value);
        self.new_list(list.element.clone(), next)
    }
    pub fn list_set(
        &self,
        list: &ListValue,
        index: usize,
        value: Datum,
    ) -> Result<ListValue, DataError> {
        if index >= list.values.len() {
            return Err(DataError::InvalidShape);
        }
        let mut next = list.values.to_vec();
        next[index] = value;
        self.new_list(list.element.clone(), next)
    }
    pub fn list_remove(
        &self,
        list: &ListValue,
        index: usize,
    ) -> Result<(ListValue, Datum), DataError> {
        if index >= list.values.len() {
            return Err(DataError::InvalidShape);
        }
        let mut next = list.values.to_vec();
        let removed = next.remove(index);
        Ok((self.new_list(list.element.clone(), next)?, removed))
    }
    /// Deterministic ascending key order, independent of insertion order.
    /// Duplicate input is an error, never silently "last value wins".
    pub fn new_map(
        &self,
        keys: MapKeyType,
        element: DataType,
        mut entries: Vec<(DataKey, Datum)>,
    ) -> Result<MapValue, DataError> {
        bounded(
            entries.len(),
            self.limits().max_collection_entries,
            DataResource::CollectionEntries,
        )?;
        if entries.iter().any(|(key, _)| key.kind() != keys) {
            return Err(DataError::InvalidKey);
        }
        entries.sort_by(|(a, _), (b, _)| a.cmp(b));
        if entries.windows(2).any(|items| items[0].0 == items[1].0) {
            return Err(DataError::DuplicateKey);
        }
        let map = MapValue {
            keys,
            element,
            pairs: entries.into(),
        };
        self.verify(
            &DataType::map(map.keys, map.element.clone()),
            &Datum::Map(map.clone()),
        )?;
        Ok(map)
    }
    pub fn map_insert(
        &self,
        map: &MapValue,
        key: DataKey,
        value: Datum,
    ) -> Result<MapValue, DataError> {
        if key.kind() != map.keys {
            return Err(DataError::InvalidKey);
        }
        self.verify(&map.element, &value)?;
        let mut entries = map.pairs.to_vec();
        match entries.binary_search_by(|(existing, _)| existing.cmp(&key)) {
            Ok(index) => entries[index].1 = value,
            Err(index) => entries.insert(index, (key, value)),
        }
        self.new_map(map.keys, map.element.clone(), entries)
    }
    pub fn map_remove(
        &self,
        map: &MapValue,
        key: &DataKey,
    ) -> Result<(MapValue, Option<Datum>), DataError> {
        if key.kind() != map.keys {
            return Err(DataError::InvalidKey);
        }
        let mut entries = map.pairs.to_vec();
        let removed = match entries.binary_search_by(|(existing, _)| existing.cmp(key)) {
            Ok(index) => Some(entries.remove(index).1),
            Err(_) => None,
        };
        let updated = self.new_map(map.keys, map.element.clone(), entries)?;
        Ok((updated, removed))
    }
    /// This is a checked host API, not a source-level effect or a new budget
    /// inside the existing interpreter. Interpreter adoption requires its own
    /// accounting design and tests.
    pub fn make_optional(
        &self,
        element: DataType,
        value: Option<Datum>,
    ) -> Result<Datum, DataError> {
        let constructed = Datum::Optional(value.map(Arc::new));
        self.verify(&DataType::optional(element), &constructed)?;
        Ok(constructed)
    }
    pub fn make_outcome(
        &self,
        ok: DataType,
        err: DataType,
        value: Result<Datum, Datum>,
    ) -> Result<Datum, DataError> {
        let constructed = match value {
            Ok(v) => Datum::Outcome(OutcomeValue::Ok(Arc::new(v))),
            Err(v) => Datum::Outcome(OutcomeValue::Err(Arc::new(v))),
        };
        self.verify(&DataType::outcome(ok, err), &constructed)?;
        Ok(constructed)
    }
    /// Enumerate user-accessible schema field names without exposing internal
    /// arena indices or guessing at module visibility.
    pub fn field_names(&self, id: DefinitionId) -> Result<Vec<&str>, DataError> {
        Ok(self.record(id)?.iter().map(|f| f.name.as_str()).collect())
    }
    pub fn variant_names(&self, id: DefinitionId) -> Result<Vec<&str>, DataError> {
        Ok(self.variants(id)?.iter().map(|c| c.name.as_str()).collect())
    }
    pub fn check_record_identity(
        &self,
        record: &RecordValue,
        id: DefinitionId,
    ) -> Result<(), DataError> {
        match self.get(id).ok_or(DataError::UnknownDefinition)?.kind {
            DefinitionKind::Record(_) if record.identity == id => Ok(()),
            DefinitionKind::Record(_) => Err(DataError::TypeMismatch),
            _ => Err(DataError::InvalidDefinition),
        }
    }
}
