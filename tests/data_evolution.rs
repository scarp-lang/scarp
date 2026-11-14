//! Schema evolution classification and transitive recompilation impact tests.
use scarp::data_engine::{
    DataType as T, Definition as D, Field as F, SchemaChange as Change,
    SchemaRegistry as Registry, SchemaRisk as Risk, VariantCase as C,
};
use scarp::types::DefinitionId as Id;

fn id(n:u32)->Id { Id{module:120,declaration:n} }
fn record(n:u32,fields:Vec<F>)->D {
    D::record(id(n),format!("Record{n}"),fields)
}
fn base()->Registry{
    let mut r=Registry::default();
    r.register_batch(vec![
        record(1,vec![
            F::new("name",T::string()),
            F::new("age",T::integer()),
        ]),
        D::variant(id(2),"Status",vec![
            C::unit("Pending"),
            C::typed("Ready",T::Record(id(1))),
        ]),
        record(3,vec![
            F::new("status",T::Variant(id(2))),
        ]),
    ]).unwrap();
    r
}
#[test]
fn unchanged_registry_has_no_edits_or_recompilation_impact(){
    let a=base();
    let b=a.clone();
    let comparison=a.compare_schema(&b);
    assert!(comparison.is_identical());
    assert_eq!(comparison.risk,Risk::Compatible);
    assert_eq!(comparison.change_count(),0);
    assert!(comparison.changed_ids.is_empty());
    assert!(a.schema_impact(&b).unwrap().is_empty());
}
#[test]
fn adding_an_unrelated_nominal_definition_is_semantically_additive(){
    let old=base();
    let mut new=old.clone();
    new.register(record(10,vec![F::new("value",T::boolean())])).unwrap();
    let result=old.compare_schema(&new);
    assert_eq!(result.risk,Risk::Compatible);
    assert_eq!(result.changed_ids,vec![id(10)]);
    assert_eq!(result.changes,vec![
        Change::DefinitionAdded{id:id(10),name:"Record10".into()},
    ]);
    assert_eq!(old.schema_impact(&new).unwrap(),vec![id(10)]);
}
#[test]
fn removing_a_nominal_definition_breaks_existing_consumers(){
    let mut old=base();
    old.register(record(10,vec![])).unwrap();
    let result=old.compare_schema(&base());
    assert_eq!(result.risk,Risk::Breaking);
    assert_eq!(result.changes,vec![
        Change::DefinitionRemoved{id:id(10),name:"Record10".into()},
    ]);
}
#[test]
fn renaming_definition_is_a_reported_source_compatibility_change(){
    let old=base();
    let mut new=Registry::default();
    new.register_batch(vec![
        D::record(id(1),"Person",vec![
            F::new("name",T::string()),F::new("age",T::integer()),
        ]),
        D::variant(id(2),"Status",vec![
            C::unit("Pending"),C::typed("Ready",T::Record(id(1))),
        ]),
        record(3,vec![F::new("status",T::Variant(id(2)))]),
    ]).unwrap();
    let diff=old.compare_schema(&new);
    assert_eq!(diff.risk,Risk::RequiresMigration);
    assert_eq!(diff.changes,vec![
        Change::DefinitionRenamed{
            id:id(1),before:"Record1".into(),after:"Person".into(),
        },
    ]);
    assert_eq!(old.schema_impact(&new).unwrap(),vec![id(1),id(2),id(3)]);
}
#[test]
fn field_addition_requires_explicit_data_migration(){
    let old=base();
    let mut new=Registry::default();
    new.register_batch(vec![
        record(1,vec![
            F::new("name",T::string()),F::new("age",T::integer()),
            F::new("active",T::boolean()),
        ]),
        D::variant(id(2),"Status",vec![
            C::unit("Pending"),C::typed("Ready",T::Record(id(1))),
        ]),
        record(3,vec![F::new("status",T::Variant(id(2)))]),
    ]).unwrap();
    let diff=old.compare_schema(&new);
    assert_eq!(diff.risk,Risk::RequiresMigration);
    assert_eq!(diff.changes,vec![Change::FieldAdded{
        id:id(1),name:"active".into(),ty:T::boolean(),
    }]);
}
#[test]
fn removing_fields_is_classified_breaking_by_default(){
    let old=base();
    let mut new=Registry::default();
    new.register_batch(vec![
        record(1,vec![F::new("name",T::string())]),
        D::variant(id(2),"Status",vec![
            C::unit("Pending"),C::typed("Ready",T::Record(id(1))),
        ]),
        record(3,vec![F::new("status",T::Variant(id(2)))]),
    ]).unwrap();
    let diff=old.compare_schema(&new);
    assert_eq!(diff.risk,Risk::Breaking);
    assert_eq!(diff.changes,vec![Change::FieldRemoved{
        id:id(1),name:"age".into(),
    }]);
}
#[test]
fn changing_field_type_is_detected_without_implicit_coercion(){
    let old=base();
    let mut new=Registry::default();
    new.register_batch(vec![
        record(1,vec![F::new("name",T::string()),F::new("age",T::string())]),
        D::variant(id(2),"Status",vec![
            C::unit("Pending"),C::typed("Ready",T::Record(id(1))),
        ]),
        record(3,vec![F::new("status",T::Variant(id(2)))]),
    ]).unwrap();
    let diff=old.compare_schema(&new);
    assert_eq!(diff.risk,Risk::Breaking);
    assert_eq!(diff.changes,vec![Change::FieldTypeChanged{
        id:id(1),name:"age".into(),before:T::integer(),after:T::string(),
    }]);
}
#[test]
fn field_reorder_is_reported_without_false_type_changes(){
    let old=base();
    let mut new=Registry::default();
    new.register_batch(vec![
        record(1,vec![F::new("age",T::integer()),F::new("name",T::string())]),
        D::variant(id(2),"Status",vec![
            C::unit("Pending"),C::typed("Ready",T::Record(id(1))),
        ]),
        record(3,vec![F::new("status",T::Variant(id(2)))]),
    ]).unwrap();
    let diff=old.compare_schema(&new);
    assert_eq!(diff.risk,Risk::RequiresMigration);
    assert_eq!(diff.changes,vec![
        Change::FieldReordered{id:id(1),name:"name".into(),before:0,after:1},
        Change::FieldReordered{id:id(1),name:"age".into(),before:1,after:0},
    ]);
}
#[test]
fn variant_case_addition_is_treated_as_exhaustiveness_breaking(){
    let old=base();
    let mut new=Registry::default();
    new.register_batch(vec![
        record(1,vec![F::new("name",T::string()),F::new("age",T::integer())]),
        D::variant(id(2),"Status",vec![
            C::unit("Pending"),C::typed("Ready",T::Record(id(1))),
            C::typed("Failed",T::string()),
        ]),
        record(3,vec![F::new("status",T::Variant(id(2)))]),
    ]).unwrap();
    let diff=old.compare_schema(&new);
    assert_eq!(diff.risk,Risk::Breaking);
    assert_eq!(diff.changes,vec![
        Change::CaseAdded{id:id(2),name:"Failed".into()},
    ]);
    assert_eq!(old.schema_impact(&new).unwrap(),vec![id(2),id(3)]);
}
#[test]
fn variant_reordered_case_index_is_recognized_by_name(){
    let old=base();
    let mut new=Registry::default();
    new.register_batch(vec![
        record(1,vec![F::new("name",T::string()),F::new("age",T::integer())]),
        D::variant(id(2),"Status",vec![
            C::typed("Ready",T::Record(id(1))),C::unit("Pending"),
        ]),
        record(3,vec![F::new("status",T::Variant(id(2)))]),
    ]).unwrap();
    let diff=old.compare_schema(&new);
    assert_eq!(diff.risk,Risk::RequiresMigration);
    assert_eq!(diff.changes,vec![
        Change::CaseReordered{id:id(2),name:"Pending".into(),before:0,after:1},
        Change::CaseReordered{id:id(2),name:"Ready".into(),before:1,after:0},
    ]);
}
#[test]
fn changed_variant_payload_marks_breaking_type_contract(){
    let old=base();
    let mut new=Registry::default();
    new.register_batch(vec![
        record(1,vec![F::new("name",T::string()),F::new("age",T::integer())]),
        D::variant(id(2),"Status",vec![
            C::unit("Pending"),C::typed("Ready",T::string()),
        ]),
        record(3,vec![F::new("status",T::Variant(id(2)))]),
    ]).unwrap();
    let diff=old.compare_schema(&new);
    assert_eq!(diff.risk,Risk::Breaking);
    assert_eq!(diff.changes,vec![
        Change::CasePayloadChanged{id:id(2),name:"Ready".into()},
    ]);
}
#[test]
fn changing_a_record_into_a_variant_does_not_compare_unrelated_members(){
    let mut old=Registry::default();
    old.register(record(1,vec![F::new("value",T::integer())])).unwrap();
    let mut new=Registry::default();
    new.register(D::variant(id(1),"Record1",vec![C::unit("Empty")])).unwrap();
    let diff=old.compare_schema(&new);
    assert_eq!(diff.changes,vec![Change::DefinitionKindChanged{id:id(1)}]);
    assert_eq!(diff.risk,Risk::Breaking);
}
#[test]
fn changed_ids_are_sorted_by_nominal_identity_not_registration_order(){
    let mut old=Registry::default();
    old.register_batch(vec![record(3,vec![]),record(1,vec![])]).unwrap();
    let mut new=Registry::default();
    new.register_batch(vec![record(2,vec![]),record(1,vec![])]).unwrap();
    let diff=old.compare_schema(&new);
    assert_eq!(diff.changed_ids,vec![id(2),id(3)]);
    assert_eq!(diff.risk,Risk::Breaking);
}
#[test]
fn transitive_impact_uses_old_and_new_graph_edges(){
    let old=base();
    let mut new=Registry::default();
    new.register_batch(vec![
        record(1,vec![F::new("name",T::string()),F::new("age",T::string())]),
        D::variant(id(2),"Status",vec![C::unit("Pending"),C::typed("Ready",T::Record(id(1)))]),
        record(3,vec![F::new("status",T::Variant(id(2)))]),
        record(4,vec![F::new("event",T::Record(id(3)))]),
    ]).unwrap();
    assert_eq!(old.schema_impact(&new).unwrap(),vec![
        id(1),id(2),id(3),id(4),
    ]);
}
#[test]
fn schema_evolution_reports_equal_semantics_across_registration_orders(){
    let old=base();
    let mut new=Registry::default();
    new.register_batch(vec![
        record(3,vec![F::new("status",T::Variant(id(2)))]),
        D::variant(id(2),"Status",vec![C::unit("Pending"),C::typed("Ready",T::Record(id(1)))]),
        record(1,vec![F::new("name",T::string()),F::new("age",T::integer())]),
    ]).unwrap();
    let diff=old.compare_schema(&new);
    assert!(diff.is_identical());
    assert!(old.schema_impact(&new).unwrap().is_empty());
}
