//! Experimental host-side Scarp data engine.
//!
//! This module establishes checked semantics for nominal records, tagged
//! variants, dynamic typed lists, maps, option/result values and patterns.
//! It does not claim that these types can yet be written in .srp source.
//! The source parser, resolver and interpreter will adopt this model only
//! after the language syntax and effect/ownership contracts are reviewed.
pub mod collections;
pub mod generics;
pub mod index;
pub mod limits;
pub mod paths;
pub mod pattern;
pub mod query;
pub mod schema;
pub mod schema_graph;
pub mod value;
pub mod wire;

pub use index::{DataIndex, IndexHit, IndexLimits, IndexStats, JoinResult, JoinRow};
pub use generics::{
    Constraint, GenericSignature, Specialization, SpecializationCache, TypeExpression,
};
pub use limits::{DataError, DataLimits, DataResource};
pub use paths::{PatchOperation, PathSegment};
pub use pattern::{Bindings, Coverage, Pattern};
pub use query::{Predicate, QueryLimits, QueryPlan, QueryResult, QueryStats};
pub use schema::{
    DataType, Definition, DefinitionKind, Field, MapKeyType, SchemaRegistry, VariantCase,
};
pub use schema_graph::SchemaGraph;
pub use wire::WireLimits;
pub use value::{DataKey, Datum, ListValue, MapValue, OutcomeValue, RecordValue, VariantValue};
