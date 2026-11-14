//! Experimental host-side Scarp data engine.
//!
//! This module establishes checked semantics for nominal records, tagged
//! variants, dynamic typed lists, maps, option/result values and patterns.
//! It does not claim that these types can yet be written in .srp source.
//! The source parser, resolver and interpreter will adopt this model only
//! after the language syntax and effect/ownership contracts are reviewed.
pub mod collections;
pub mod limits;
pub mod pattern;
pub mod paths;
pub mod schema;
pub mod value;

pub use limits::{DataError, DataLimits, DataResource};
pub use pattern::{Bindings, Coverage, Pattern};
pub use paths::{PatchOperation, PathSegment};
pub use schema::{
    DataType, Definition, DefinitionKind, Field, MapKeyType, SchemaRegistry, VariantCase,
};
pub use value::{DataKey, Datum, ListValue, MapValue, OutcomeValue, RecordValue, VariantValue};
