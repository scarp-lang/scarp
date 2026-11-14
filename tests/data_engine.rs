//! Host data-engine conformance. These APIs are not yet .srp syntax.
use scarp::data_engine::{
    Coverage, DataError as E, DataKey, DataLimits, DataResource as R, DataType as T,
    Datum as V, Definition as D, Field as F, MapKeyType as K, Pattern as P,
    SchemaRegistry as Registry, VariantCase as C,
};
use scarp::types::{DefinitionId as Id, PrimitiveType, SourceType};
use scarp::value::Value;
use std::sync::Arc;

fn id(declaration:u32) -> Id { Id { module: 7, declaration } }
fn person_definition() -> D {
    D::record(id(1), "Person", vec![
        F::new("name",T::string()),
        F::new("age",T::integer()),
        F::new("active",T::boolean()),
    ])
}
fn status_definition() -> D {
    D::variant(id(2), "Status", vec![
        C::unit("Pending"),
        C::typed("Accepted",T::Record(id(1))),
        C::typed("Rejected",T::string()),
    ])
}
fn initialized() -> Registry {
    let mut r = Registry::default();
    r.register(person_definition()).unwrap();
    r.register(status_definition()).unwrap();
    r
}
fn person(r:&Registry, name:&str, age:i64) -> V {
    V::Record(r.new_record(id(1),vec![
        V::text(name),V::integer(age),V::boolean(true),
    ]).unwrap())
}
#[test]
fn register_schemas_with_nominal_identity_and_ordered_metadata() {
    let mut schemas = initialized();
    assert_eq!(schemas.len(),2);
    assert_eq!(schemas.definitions()[0].name,"Person");
    assert_eq!(schemas.definitions()[1].name,"Status");
    assert_eq!(schemas.field_names(id(1)).unwrap(),vec!["name","age","active"]);
    assert_eq!(schemas.variant_names(id(2)).unwrap(),vec!["Pending","Accepted","Rejected"]);
    assert_eq!(schemas.register(person_definition()),Err(E::DuplicateDefinition));
    assert_eq!(schemas.len(),2);
}
#[test]
fn registration_is_transactional_after_bad_reference_and_name() {
    let mut r = Registry::default();
    let bad = D::record(id(1),"wrong-name",vec![]);
    assert!(matches!(r.register(bad),Err(E::InvalidName(_))));
    let bad = D::record(id(1),"Good",vec![F::new("child",T::Record(id(44)))]);
    assert_eq!(r.register(bad),Err(E::UnknownDefinition));
    r.register(person_definition()).unwrap();
    assert_eq!(r.len(),1);
}
#[test]
fn duplicate_fields_and_cases_are_rejected_without_mutation() {
    let mut r = Registry::default();
    let duplicate = D::record(id(4),"Test",vec![
        F::new("name",T::string()),F::new("name",T::integer()),
    ]);
    assert_eq!(r.register(duplicate),Err(E::DuplicateName("name".into())));
    assert_eq!(r.len(),0);
    let duplicate = D::variant(id(4),"Status",vec![C::unit("Ok"),C::unit("Ok")]);
    assert_eq!(r.register(duplicate),Err(E::DuplicateName("Ok".into())));
    assert_eq!(r.len(),0);
    assert_eq!(r.register(D::variant(id(4),"Empty",vec![])),Err(E::InvalidShape));
}
#[test]
fn duplicate_type_identity_never_uses_display_name_as_key() {
    let mut r = Registry::default();
    r.register(D::record(id(1),"Person",vec![])).unwrap();
    r.register(D::record(id(2),"Person",vec![])).unwrap();
    assert_ne!(r.get(id(1)).unwrap().id,r.get(id(2)).unwrap().id);
    assert_eq!(r.len(),2);
}
#[test]
fn record_positional_and_named_values_are_equivalent() {
    let r=initialized();
    let positional = person(&r,"Ada",36);
    let named=r.record_named(id(1),vec![
        ("age".into(),V::integer(36)),
        ("active".into(),V::boolean(true)),
        ("name".into(),V::text("Ada")),
    ]).unwrap();
    assert_eq!(positional,V::Record(named.clone()));
    assert_eq!(r.field(&named,"age").unwrap(),&V::integer(36));
    assert_eq!(r.field(&named,"name").unwrap(),&V::text("Ada"));
    assert_eq!(r.field_names(id(1)).unwrap().len(),named.len());
    assert_eq!(r.field(&named,"missing"),Err(E::UnknownField("missing".into())));
}
#[test]
fn record_named_rejects_omissions_duplicates_unknown_and_mistyped_members() {
    let r=initialized();
    assert_eq!(r.record_named(id(1),vec![]),Err(E::InvalidShape));
    let duplicate=vec![
        ("name".into(),V::text("A")),
        ("name".into(),V::text("B")),
        ("age".into(),V::integer(1)),
    ];
    assert!(matches!(r.record_named(id(1),duplicate),Err(E::DuplicateName(_))));
    let missing=vec![
        ("name".into(),V::text("A")),
        ("age".into(),V::integer(1)),
        ("what".into(),V::integer(2)),
    ];
    assert!(matches!(r.record_named(id(1),missing),Err(E::UnknownField(_))));
    let wrong=vec![V::text("A"),V::boolean(false),V::boolean(true)];
    assert_eq!(r.new_record(id(1),wrong),Err(E::TypeMismatch));
}
#[test]
fn persistent_record_update_does_not_change_previous_snapshots() {
    let r=initialized();
    let original=r.new_record(id(1),vec![V::text("Ada"),V::integer(36),V::boolean(true)]).unwrap();
    let backup=original.clone();
    let changed=r.replace_field(&original,"age",V::integer(37)).unwrap();
    assert_eq!(r.field(&changed,"age").unwrap(),&V::integer(37));
    assert_eq!(r.field(&original,"age").unwrap(),&V::integer(36));
    assert_eq!(backup,original);
    assert_eq!(r.replace_field(&original,"age",V::text("old")),Err(E::TypeMismatch));
    assert_eq!(backup,original);
}
#[test]
fn record_ids_are_nominal_not_structural() {
    let mut r=initialized();
    r.register(D::record(id(3),"Employee",vec![
        F::new("name",T::string()),
        F::new("age",T::integer()),
        F::new("active",T::boolean()),
    ])).unwrap();
    let item=person(&r,"Ada",35);
    assert_eq!(r.verify(&T::Record(id(3)),&item),Err(E::TypeMismatch));
    assert!(r.verify(&T::Record(id(1)),&item).is_ok());
}
#[test]
fn variant_unit_and_payload_cases_validate_exactly() {
    let r=initialized();
    let pending=r.new_variant(id(2),"Pending",None).unwrap();
    assert_eq!(r.case_name(&pending).unwrap(),"Pending");
    let accepted=r.new_variant(id(2),"Accepted",Some(person(&r,"N",19))).unwrap();
    assert_eq!(r.case_name(&accepted).unwrap(),"Accepted");
    assert!(accepted.payload().is_some());
    let rejected=r.new_variant(id(2),"Rejected",Some(V::text("closed"))).unwrap();
    assert_eq!(r.case_name(&rejected).unwrap(),"Rejected");
    assert_eq!(r.new_variant(id(2),"Pending",Some(V::integer(1))),Err(E::InvalidShape));
    assert_eq!(r.new_variant(id(2),"Rejected",None),Err(E::InvalidShape));
    assert_eq!(r.new_variant(id(2),"Accepted",Some(V::text("N"))),Err(E::TypeMismatch));
    assert_eq!(r.new_variant(id(2),"Unknown",None),Err(E::UnknownCase("Unknown".into())));
}
#[test]
fn list_type_validation_handles_empty_and_nonempty() {
    let r=initialized();
    let empty=r.new_list(T::integer(),vec![]).unwrap();
    assert_eq!(empty.len(),0);
    assert!(empty.is_empty());
    let list=r.new_list(T::integer(),vec![V::integer(1),V::integer(2)]).unwrap();
    assert_eq!(list.len(),2);
    assert_eq!(list.get(1),Some(&V::integer(2)));
    assert_eq!(r.new_list(T::string(),vec![V::integer(2)]),Err(E::TypeMismatch));
    assert_ne!(empty.element_type(),&T::string());
    assert_eq!(r.verify(&T::list(T::integer()),&V::List(list)),Ok(()));
}
#[test]
fn list_updates_are_atomic_persistent_and_checked() {
    let r=initialized();
    let original=r.new_list(T::integer(),vec![V::integer(1),V::integer(2)]).unwrap();
    let updated=r.list_push(&original,V::integer(3)).unwrap();
    let changed=r.list_set(&updated,0,V::integer(9)).unwrap();
    let (removed, old)=r.list_remove(&changed,1).unwrap();
    assert_eq!(removed.values(),&[V::integer(9),V::integer(3)]);
    assert_eq!(old,V::integer(2));
    assert_eq!(original.values(),&[V::integer(1),V::integer(2)]);
    assert_eq!(r.list_push(&original,V::text("invalid")),Err(E::TypeMismatch));
    assert_eq!(r.list_set(&original,30,V::integer(9)),Err(E::InvalidShape));
    assert_eq!(r.list_remove(&original,30),Err(E::InvalidShape));
}
#[test]
fn maps_are_sorted_by_key_not_insertion_order() {
    let r=initialized();
    let map=r.new_map(K::Text,T::integer(),vec![
        (DataKey::text("z"),V::integer(26)),
        (DataKey::text("a"),V::integer(1)),
        (DataKey::text("m"),V::integer(13)),
    ]).unwrap();
    let names:Vec<&str>=map.entries().iter().map(|(k,_)|match k {
        DataKey::Text(text)=>text.as_ref(), _=>panic!("wrong key type"),
    }).collect();
    assert_eq!(names,vec!["a","m","z"]);
    assert_eq!(map.get(&DataKey::text("m")),Some(&V::integer(13)));
    assert_eq!(map.get(&DataKey::text("missing")),None);
}
#[test]
fn map_rejects_ambiguous_duplicate_keys_and_invalid_types() {
    let r=initialized();
    assert_eq!(r.new_map(K::Text,T::integer(),vec![
        (DataKey::text("x"),V::integer(1)),
        (DataKey::text("x"),V::integer(2)),
    ]),Err(E::DuplicateKey));
    assert_eq!(r.new_map(K::Text,T::integer(),vec![
        (DataKey::Integer(1),V::integer(1)),
    ]),Err(E::InvalidKey));
    assert_eq!(r.new_map(K::Integer,T::integer(),vec![
        (DataKey::Integer(1),V::text("not integer")),
    ]),Err(E::TypeMismatch));
}
#[test]
fn map_insert_and_remove_preserve_old_snapshots() {
    let r=initialized();
    let empty=r.new_map(K::Integer,T::string(),vec![]).unwrap();
    let a=r.map_insert(&empty,DataKey::Integer(3),V::text("three")).unwrap();
    let b=r.map_insert(&a,DataKey::Integer(-4),V::text("minus four")).unwrap();
    let c=r.map_insert(&b,DataKey::Integer(3),V::text("changed")).unwrap();
    assert_eq!(empty.len(),0);
    assert_eq!(a.len(),1);
    assert_eq!(b.len(),2);
    assert_eq!(c.len(),2);
    assert_eq!(a.get(&DataKey::Integer(3)),Some(&V::text("three")));
    assert_eq!(c.get(&DataKey::Integer(3)),Some(&V::text("changed")));
    let (d,removed)=r.map_remove(&c,&DataKey::Integer(3)).unwrap();
    assert_eq!(removed,Some(V::text("changed")));
    assert_eq!(d.len(),1);
    assert_eq!(c.len(),2);
    assert_eq!(r.map_insert(&c,DataKey::text("bad"),V::text("x")),Err(E::InvalidKey));
    assert_eq!(r.map_insert(&c,DataKey::Integer(1),V::integer(1)),Err(E::TypeMismatch));
}
#[test]
fn nested_records_variants_lists_and_maps_verify_without_source_syntax() {
    let r=initialized();
    let accepted=V::Variant(r.new_variant(id(2),"Accepted",Some(person(&r,"Ada",36))).unwrap());
    let list=V::List(r.new_list(T::Variant(id(2)),vec![accepted.clone()]).unwrap());
    let map=V::Map(r.new_map(K::Text,T::list(T::Variant(id(2))),vec![
        (DataKey::text("sessions"),list.clone()),
    ]).unwrap());
    assert_eq!(r.verify(&T::map(K::Text,T::list(T::Variant(id(2)))),&map),Ok(()));
    assert_eq!(r.verify(&T::map(K::Text,T::integer()),&map),Err(E::TypeMismatch));
}
#[test]
fn options_and_results_differ_from_untyped_nulls() {
    let r=initialized();
    let optional=T::optional(T::integer());
    assert!(r.verify(&optional,&V::none()).is_ok());
    assert!(r.verify(&optional,&V::some(V::integer(6))).is_ok());
    assert_eq!(r.verify(&optional,&V::some(V::text("wrong"))),Err(E::TypeMismatch));
    let ty=T::outcome(T::integer(),T::string());
    let ok=r.make_outcome(T::integer(),T::string(),Ok(V::integer(5))).unwrap();
    let err=r.make_outcome(T::integer(),T::string(),Err(V::text("error"))).unwrap();
    assert_eq!(r.verify(&ty,&ok),Ok(()));
    assert_eq!(r.verify(&ty,&err),Ok(()));
    assert_eq!(r.make_outcome(T::integer(),T::string(),Ok(V::text("bad"))),Err(E::TypeMismatch));
    assert_eq!(r.make_optional(T::integer(),Some(V::text("bad"))),Err(E::TypeMismatch));
}
#[test]
fn matching_selects_fields_and_preserves_binding_values() {
    let r=initialized();
    let record=person(&r,"Ada",36);
    let pattern=P::Record {identity:id(1),fields:vec![
        ("name".into(),P::bind("who")),
        ("age".into(),P::Literal(Value::Integer(36))),
    ]};
    let bindings=r.match_pattern(&pattern,&record).unwrap().unwrap();
    assert_eq!(bindings.get("who"),Some(&V::text("Ada")));
    assert_eq!(bindings.len(),1);
    let pattern=P::Record {identity:id(1),fields:vec![
        ("age".into(),P::Literal(Value::Integer(37))),
    ]};
    assert_eq!(r.match_pattern(&pattern,&record).unwrap(),None);
}
#[test]
fn matching_recursively_selects_variant_payload() {
    let r=initialized();
    let accepted=V::Variant(r.new_variant(id(2),"Accepted",Some(person(&r,"Ada",36))).unwrap());
    let pattern=P::variant(id(2),"Accepted",Some(P::Record {identity:id(1),fields:vec![
        ("name".into(),P::bind("name")),
    ]}));
    let captures=r.match_pattern(&pattern,&accepted).unwrap().unwrap();
    assert_eq!(captures.get("name"),Some(&V::text("Ada")));
    assert_eq!(r.match_pattern(&P::variant(id(2),"Pending",None),&accepted).unwrap(),None);
}
#[test]
fn matching_lists_options_and_outcomes() {
    let r=initialized();
    let list=V::List(r.new_list(T::integer(),vec![V::integer(2),V::integer(3)]).unwrap());
    let p=P::List(vec![P::Literal(Value::Integer(2)),P::bind("second")]);
    assert_eq!(r.match_pattern(&p,&list).unwrap().unwrap().get("second"),Some(&V::integer(3)));
    assert_eq!(r.match_pattern(&P::List(vec![]),&list).unwrap(),None);
    assert!(r.match_pattern(&P::OptionalNone,&V::none()).unwrap().is_some());
    assert!(r.match_pattern(&P::OptionalSome(Box::new(P::Any)),&V::none()).unwrap().is_none());
    assert!(r.match_pattern(&P::OutcomeErr(Box::new(P::Any)),&V::err(V::text("e"))).unwrap().is_some());
    assert!(r.match_pattern(&P::OutcomeOk(Box::new(P::Any)),&V::ok(V::integer(1))).unwrap().is_some());
}
#[test]
fn duplicate_capture_binding_is_explicit_error() {
    let r=initialized();
    let record=person(&r,"Ada",36);
    let p=P::Record { identity:id(1),fields:vec![
        ("age".into(),P::bind("same")),
        ("name".into(),P::bind("same")),
    ]};
    assert_eq!(r.match_pattern(&p,&record),Err(E::DuplicateName("same".into())));
}
#[test]
fn matching_invalid_field_name_errors_instead_of_false_match() {
    let r=initialized();
    let p=P::Record {identity:id(1),fields:vec![
        ("nonexistent".into(),P::Any),
    ]};
    assert_eq!(r.match_pattern(&p,&person(&r,"N",1)),
        Err(E::UnknownField("nonexistent".into())));
}
#[test]
fn variant_coverage_lists_uncovered_cases_in_declaration_order() {
    let r=initialized();
    let one=vec![P::variant(id(2),"Pending",None)];
    assert_eq!(r.variant_coverage(id(2),&one).unwrap(),Coverage{
        exhaustive:false,missing:vec!["Accepted".into(),"Rejected".into()],
        unreachable_arms:vec![],
    });
    let exhaustive=vec![
        P::variant(id(2),"Pending",None),
        P::variant(id(2),"Accepted",Some(P::Any)),
        P::variant(id(2),"Rejected",Some(P::bind("text"))),
    ];
    let checked=r.variant_coverage(id(2),&exhaustive).unwrap();
    assert!(checked.exhaustive);
    assert!(checked.missing.is_empty());
}
#[test]
fn refutable_payload_does_not_falsely_claim_exhaustiveness() {
    let r=initialized();
    let patterns=vec![
        P::variant(id(2),"Pending",None),
        P::variant(id(2),"Accepted",Some(P::Literal(Value::Integer(1)))),
        P::variant(id(2),"Rejected",Some(P::Any)),
    ];
    let result=r.variant_coverage(id(2),&patterns).unwrap();
    assert_eq!(result.missing,vec!["Accepted"]);
}
#[test]
fn variant_coverage_identifies_unreachable_arms() {
    let r=initialized();
    let arms=vec![
        P::variant(id(2),"Pending",None),
        P::variant(id(2),"Pending",None),
        P::Any,
        P::variant(id(2),"Rejected",Some(P::Any)),
    ];
    assert_eq!(r.variant_coverage(id(2),&arms).unwrap().unreachable_arms,vec![1,3]);
}
#[test]
fn invalid_variant_patterns_are_rejected() {
    let r=initialized();
    assert_eq!(r.variant_coverage(id(2),&[P::variant(id(2),"Nope",None)]),
        Err(E::UnknownCase("Nope".into())));
    assert_eq!(r.variant_coverage(id(2),&[P::Literal(Value::Integer(3))]),
        Err(E::InvalidPattern));
    assert_eq!(r.variant_coverage(id(2),&[P::variant(id(2),"Pending",Some(P::Any))]),
        Err(E::InvalidPattern));
}
#[test]
fn zero_member_and_zero_entry_cases_are_supported() {
    let mut r=Registry::default();
    r.register(D::record(id(1),"Empty",vec![])).unwrap();
    let record=r.new_record(id(1),vec![]).unwrap();
    assert!(record.is_empty());
    assert!(r.verify(&T::Record(id(1)),&V::Record(record)).is_ok());
    let empty_list=r.new_list(T::integer(),vec![]).unwrap();
    let empty_map=r.new_map(K::Text,T::boolean(),vec![]).unwrap();
    assert!(empty_list.is_empty());
    assert!(empty_map.is_empty());
}
#[test]
fn exact_definition_and_member_limits_are_enforced() {
    let mut r=Registry::new(DataLimits{
        max_definitions:1,max_members_per_definition:1,..DataLimits::default()
    });
    r.register(D::record(id(1),"One",vec![F::new("a",T::integer())])).unwrap();
    assert_eq!(r.register(D::record(id(2),"Two",vec![])),Err(E::Limit{
        resource:R::Definitions,limit:1
    }));
    let mut r=Registry::new(DataLimits{
        max_members_per_definition:1,..DataLimits::default()
    });
    assert_eq!(r.register(person_definition()),Err(E::Limit{
        resource:R::Members,limit:1
    }));
}
#[test]
fn exact_collection_entry_limit_rejects_overgrowth() {
    let r=Registry::new(DataLimits{max_collection_entries:2,..DataLimits::default()});
    let two=r.new_list(T::integer(),vec![V::integer(1),V::integer(2)]).unwrap();
    assert!(matches!(r.list_push(&two,V::integer(3)),Err(E::Limit{
        resource:R::CollectionEntries,limit:2
    })));
    assert_eq!(two.len(),2);
}
#[test]
fn exact_value_node_limit_fails_before_visible_mutation() {
    let r=Registry::new(DataLimits{max_value_nodes:3,..DataLimits::default()});
    let items=r.new_list(T::integer(),vec![V::integer(1),V::integer(2)]).unwrap();
    assert_eq!(r.list_push(&items,V::integer(3)),Err(E::Limit{
        resource:R::ValueNodes,limit:3
    }));
    assert_eq!(items.len(),2);
}
#[test]
fn depth_limit_prevents_deep_recursive_value_traversal() {
    let r=Registry::new(DataLimits{max_value_depth:2,..DataLimits::default()});
    let value=V::some(V::some(V::integer(1)));
    let ty=T::optional(T::optional(T::integer()));
    assert_eq!(r.verify(&ty,&value),Err(E::Limit{resource:R::ValueDepth,limit:2}));
}
#[test]
fn text_budget_applies_to_key_and_value_utf8_lengths() {
    let r=Registry::new(DataLimits{max_text_bytes:6,..DataLimits::default()});
    assert!(r.new_map(K::Text,T::string(),vec![
        (DataKey::text("a"),V::text("abcde")),
    ]).is_ok());
    assert_eq!(r.new_map(K::Text,T::string(),vec![
        (DataKey::text("aa"),V::text("abcde")),
    ]),Err(E::Limit{resource:R::TextBytes,limit:6}));
    let r=Registry::new(DataLimits{max_text_bytes:3,..DataLimits::default()});
    assert!(r.verify(&T::string(),&V::text("ह")).is_ok());
    assert_eq!(r.verify(&T::string(),&V::text("नम")),Err(E::Limit{
        resource:R::TextBytes,limit:3
    }));
}
#[test]
fn type_depth_and_type_node_quotas_are_checked() {
    let deep=T::optional(T::list(T::optional(T::integer())));
    let r=Registry::new(DataLimits{max_type_depth:3,..DataLimits::default()});
    assert_eq!(r.validate_type(&deep),Err(E::Limit{resource:R::TypeDepth,limit:3}));
    let r=Registry::new(DataLimits{max_type_nodes:2,..DataLimits::default()});
    assert_eq!(r.validate_type(&deep),Err(E::Limit{resource:R::TypeNodes,limit:2}));
}
#[test]
fn match_budget_counts_nested_pattern_nodes() {
    let r=Registry::new(DataLimits{max_pattern_nodes:2,..DataLimits::default()});
    let pattern=P::List(vec![P::Any,P::Any]);
    let list=r.new_list(T::integer(),vec![V::integer(1),V::integer(2)]).unwrap();
    assert_eq!(r.match_pattern(&pattern,&V::List(list)),Err(E::Limit{
        resource:R::PatternNodes,limit:2
    }));
}
#[test]
fn registry_and_persistent_values_are_safe_to_share_between_threads() {
    let r=Arc::new(initialized());
    let value=Arc::new(person(&r,"Ada",36));
    let mut handles=Vec::new();
    for _ in 0..8 {
        let registry=r.clone();
        let snapshot=value.clone();
        handles.push(std::thread::spawn(move || {
            registry.verify(&T::Record(id(1)),&snapshot).unwrap();
            let p=P::Record {identity:id(1),fields:vec![
                ("name".into(),P::bind("name")),
            ]};
            registry.match_pattern(&p,&snapshot).unwrap().unwrap()
        }));
    }
    for handle in handles {
        assert_eq!(handle.join().unwrap().get("name"),Some(&V::text("Ada")));
    }
}
#[test]
fn legacy_source_type_boundary_is_exact_not_implicitly_coerced() {
    let r=Registry::default();
    let array=SourceType::array(PrimitiveType::Integer,2).unwrap();
    let ty=T::Source(array.clone());
    let ordinary=V::integer(4);
    assert_eq!(r.verify(&ty,&ordinary),Err(E::TypeMismatch));
    assert_eq!(r.verify(&T::integer(),&ordinary),Ok(()));
    assert!(r.validate_type(&T::Source(array)).is_ok());
}
#[test]
fn deterministic_sorting_permutation_metamorphic_regression() {
    let r=Registry::default();
    let original:Vec<_>=(-30..=30).rev().map(|i|(DataKey::Integer(i),V::integer(i))).collect();
    let expected=r.new_map(K::Integer,T::integer(),original.clone()).unwrap();
    let mut scrambled=original;
    for i in 0..scrambled.len() {
        let other=(i*37+11)%scrambled.len();
        scrambled.swap(i,other);
    }
    let actual=r.new_map(K::Integer,T::integer(),scrambled).unwrap();
    assert_eq!(expected,actual);
    for i in -30..=30 {
        assert_eq!(actual.get(&DataKey::Integer(i)),Some(&V::integer(i)));
    }
}
