//! Bounded first-order generic specialization for experimental composite types.
//!
//! This is not a second Scarp source type checker. It is a checked host model
//! for substitution and monomorphization that will eventually consume the
//! compiler's own resolved type identities and function declarations.
use super::limits::{bounded, DataError, DataResource};
use super::schema::{DataType, Definition, MapKeyType, SchemaRegistry};
use crate::types::{DefinitionId, PrimitiveType, SourceType};
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TypeExpression {
    Concrete(DataType),
    Variable(u32),
    List(Box<TypeExpression>),
    Map(MapKeyType, Box<TypeExpression>),
    Optional(Box<TypeExpression>),
    Outcome(Box<TypeExpression>, Box<TypeExpression>),
}
impl TypeExpression {
    pub fn concrete(ty: DataType) -> Self {
        Self::Concrete(ty)
    }
    pub fn var(index: u32) -> Self {
        Self::Variable(index)
    }
    pub fn list(inner: Self) -> Self {
        Self::List(Box::new(inner))
    }
    pub fn map(key: MapKeyType, value: Self) -> Self {
        Self::Map(key, Box::new(value))
    }
    pub fn optional(inner: Self) -> Self {
        Self::Optional(Box::new(inner))
    }
    pub fn outcome(ok: Self, err: Self) -> Self {
        Self::Outcome(Box::new(ok), Box::new(err))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Constraint {
    Any,
    SourcePrimitive,
    Numeric,
    ValidMapKey,
    Collection,
    RecordWithField(String),
    VariantWithCase(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct GenericSignature {
    pub identity: DefinitionId,
    pub type_parameters: u32,
    pub inputs: Vec<TypeExpression>,
    pub result: TypeExpression,
    /// Each constraint applies to one quantified type-parameter index.
    pub constraints: Vec<(u32, Constraint)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Specialization {
    pub identity: DefinitionId,
    /// Index-aligned type arguments, not process-local numeric TypeId indices.
    pub types: Vec<DataType>,
    pub result: DataType,
}

impl GenericSignature {
    /// Preflight never mutates global or caller-owned compiler state.
    pub fn validate(&self, registry: &SchemaRegistry) -> Result<(), DataError> {
        let limits = registry.limits();
        bounded(
            self.inputs.len(),
            limits.max_members_per_definition,
            DataResource::Members,
        )?;
        bounded(
            self.type_parameters as usize,
            limits.max_members_per_definition,
            DataResource::Members,
        )?;
        let mut count = 0usize;
        for expr in self.inputs.iter().chain(std::iter::once(&self.result)) {
            let mut pending = vec![(expr, 1usize)];
            while let Some((current, depth)) = pending.pop() {
                bounded(depth, limits.max_type_depth, DataResource::TypeDepth)?;
                count = count.checked_add(1).ok_or(DataError::Limit {
                    resource: DataResource::TypeNodes,
                    limit: limits.max_type_nodes,
                })?;
                bounded(count, limits.max_type_nodes, DataResource::TypeNodes)?;
                match current {
                    TypeExpression::Concrete(ty) => registry.validate_type(ty)?,
                    TypeExpression::Variable(index) => {
                        if *index >= self.type_parameters {
                            return Err(DataError::UnboundParameter);
                        }
                    }
                    TypeExpression::List(inner)
                    | TypeExpression::Map(_, inner)
                    | TypeExpression::Optional(inner) => pending.push((inner, depth + 1)),
                    TypeExpression::Outcome(ok, err) => {
                        pending.push((err, depth + 1));
                        pending.push((ok, depth + 1));
                    }
                }
            }
        }
        bounded(
            self.constraints.len(),
            limits.max_type_nodes,
            DataResource::TypeNodes,
        )?;
        for (index, constraint) in &self.constraints {
            if *index >= self.type_parameters {
                return Err(DataError::UnboundParameter);
            }
            if let Constraint::RecordWithField(name) | Constraint::VariantWithCase(name) =
                constraint
            {
                super::limits::identifier(name)?;
            }
        }
        Ok(())
    }

    /// Infer type parameters from actual monomorphic arguments, then construct
    /// a checked return type. Equal parameter occurrences must unify exactly.
    pub fn infer(
        &self,
        registry: &SchemaRegistry,
        actual: &[DataType],
    ) -> Result<Specialization, DataError> {
        self.validate(registry)?;
        if actual.len() != self.inputs.len() {
            return Err(DataError::ArityMismatch);
        }
        let mut bound: HashMap<u32, DataType> = HashMap::new();
        let mut work = 0usize;
        for (formal, concrete) in self.inputs.iter().zip(actual) {
            registry.validate_type(concrete)?;
            let mut pending = vec![(formal, concrete)];
            while let Some((template, value)) = pending.pop() {
                work = work.checked_add(1).ok_or(DataError::Limit {
                    resource: DataResource::TypeNodes,
                    limit: registry.limits().max_type_nodes,
                })?;
                bounded(
                    work,
                    registry.limits().max_type_nodes,
                    DataResource::TypeNodes,
                )?;
                match (template, value) {
                    (TypeExpression::Concrete(required), given) if required == given => (),
                    (TypeExpression::Variable(index), given) => match bound.get(index) {
                        Some(previous) if previous != given => return Err(DataError::TypeMismatch),
                        None => {
                            bound.insert(*index, given.clone());
                        }
                        _ => (),
                    },
                    (TypeExpression::List(inner), DataType::List(actual))
                    | (TypeExpression::Optional(inner), DataType::Optional(actual)) => {
                        pending.push((inner, actual));
                    }
                    (TypeExpression::Map(expected, inner), DataType::Map(actual, element))
                        if expected == actual =>
                    {
                        pending.push((inner, element))
                    }
                    (TypeExpression::Outcome(ok, err), DataType::Outcome(a, b)) => {
                        pending.push((err, b));
                        pending.push((ok, a));
                    }
                    _ => return Err(DataError::TypeMismatch),
                }
            }
        }
        let mut types = Vec::new();
        types
            .try_reserve_exact(self.type_parameters as usize)
            .map_err(|_| DataError::Limit {
                resource: DataResource::TypeNodes,
                limit: registry.limits().max_type_nodes,
            })?;
        for index in 0..self.type_parameters {
            types.push(bound.remove(&index).ok_or(DataError::UnboundParameter)?);
        }
        for (index, constraint) in &self.constraints {
            if !constraint.accepts(registry, &types[*index as usize])? {
                return Err(DataError::UnsatisfiedConstraint);
            }
        }
        let output = self.result.instantiate(registry, &types)?;
        registry.validate_type(&output)?;
        Ok(Specialization {
            identity: self.identity,
            types,
            result: output,
        })
    }
}
impl TypeExpression {
    fn instantiate(
        &self,
        registry: &SchemaRegistry,
        substitutions: &[DataType],
    ) -> Result<DataType, DataError> {
        fn substitute(
            expr: &TypeExpression,
            args: &[DataType],
            depth: usize,
            registry: &SchemaRegistry,
            steps: &mut usize,
        ) -> Result<DataType, DataError> {
            let limits = registry.limits();
            bounded(depth, limits.max_type_depth, DataResource::TypeDepth)?;
            *steps = steps.checked_add(1).ok_or(DataError::Limit {
                resource: DataResource::TypeNodes,
                limit: limits.max_type_nodes,
            })?;
            bounded(*steps, limits.max_type_nodes, DataResource::TypeNodes)?;
            let child = |e: &TypeExpression, steps: &mut usize| {
                substitute(e, args, depth + 1, registry, steps)
            };
            let result = match expr {
                TypeExpression::Concrete(t) => t.clone(),
                TypeExpression::Variable(index) => args
                    .get(*index as usize)
                    .cloned()
                    .ok_or(DataError::UnboundParameter)?,
                TypeExpression::List(e) => DataType::list(child(e, steps)?),
                TypeExpression::Map(k, e) => DataType::map(*k, child(e, steps)?),
                TypeExpression::Optional(e) => DataType::optional(child(e, steps)?),
                TypeExpression::Outcome(ok, err) => {
                    let first = child(ok, steps)?;
                    let second = child(err, steps)?;
                    DataType::outcome(first, second)
                }
            };
            Ok(result)
        }
        let mut steps = 0;
        let result = substitute(self, substitutions, 1, registry, &mut steps)?;
        registry.validate_type(&result)?;
        Ok(result)
    }
}
impl Constraint {
    pub fn accepts(&self, registry: &SchemaRegistry, actual: &DataType) -> Result<bool, DataError> {
        registry.validate_type(actual)?;
        Ok(match (self, actual) {
            (Self::Any, _) => true,
            (Self::SourcePrimitive, DataType::Source(SourceType::Primitive(p))) => {
                *p != PrimitiveType::Unit
            }
            (Self::Numeric, DataType::Source(SourceType::Primitive(p))) => matches!(
                p,
                PrimitiveType::Integer | PrimitiveType::Float | PrimitiveType::BigInt
            ),
            (Self::ValidMapKey, DataType::Source(SourceType::Primitive(p))) => {
                matches!(p, PrimitiveType::Integer | PrimitiveType::String)
            }
            (Self::Collection, DataType::List(_) | DataType::Map(_, _)) => true,
            (Self::RecordWithField(name), DataType::Record(id)) => registry
                .record(*id)?
                .iter()
                .any(|field| &field.name == name),
            (Self::VariantWithCase(name), DataType::Variant(id)) => registry
                .variants(*id)?
                .iter()
                .any(|case| &case.name == name),
            _ => false,
        })
    }
}

/// Explicit specialization cache, keyed by the *entire* generic signature and
/// concrete arguments: sharing a DefinitionId with a changed signature cannot
/// poison a previous cache entry. No hidden global state or unbounded growth.
#[derive(Debug, Clone)]
pub struct SpecializationCache {
    max_entries: usize,
    entries: HashMap<(GenericSignature, Vec<DataType>, Vec<Definition>), Specialization>,
}
impl SpecializationCache {
    pub fn new(max_entries: usize) -> Self {
        Self {
            max_entries,
            entries: HashMap::new(),
        }
    }
    pub fn len(&self) -> usize {
        self.entries.len()
    }
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
    pub fn clear(&mut self) {
        self.entries.clear();
    }
    pub fn resolve(
        &mut self,
        registry: &SchemaRegistry,
        signature: &GenericSignature,
        actual: &[DataType],
    ) -> Result<Specialization, DataError> {
        // Revalidate the signature and all concrete input types even on a
        // cache hit: no unchecked schema identity or budget bypass.
        signature.validate(registry)?;
        for ty in actual {
            registry.validate_type(ty)?;
        }
        let key = (
            signature.clone(),
            actual.to_vec(),
            registry.definitions().to_vec(),
        );
        if let Some(specialization) = self.entries.get(&key) {
            registry.validate_type(&specialization.result)?;
            return Ok(specialization.clone());
        }
        bounded(
            self.entries.len().saturating_add(1),
            self.max_entries,
            DataResource::Specializations,
        )?;
        let inferred = signature.infer(registry, actual)?;
        self.entries.insert(key, inferred.clone());
        Ok(inferred)
    }
}
