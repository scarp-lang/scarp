//! Deterministic semantic diffs and transactional patch roundtrips.
use scarp::data_engine::{
    DataError as E, DataKey, DataResource as R, DataType as T, Datum as V,
    Definition, DiffLimits as L, Field, MapKeyType as K, PatchOperation as Op,
    PathSegment as S, SchemaRegistry as Registry, VariantCase,
};
use scarp::types::DefinitionId as Id;

fn id(n:u32)->Id {Id{module:90,declaration:n}}
fn schema()->Registry {
    let mut r=Registry::default();
    r.register_batch(vec![
        Definition::record(id(1),"Person",vec![
            Field::new("name",T::string()),
            Field::new("age",T::integer()),
        ]),
        Definition::record(id(2),"Group",vec![
            Field::new("title",T::string()),
            Field::new("people",T::list(T::Record(id(1)))),
        ]),
        Definition::variant(id(3),"Status",vec![
            VariantCase::unit("Pending"),
            VariantCase::typed("Ready",T::Record(id(1))),
            VariantCase::typed("Failed",T::string()),
        ]),
    ]).unwrap();
    r
}
fn person(r:&Registry,name:&str,age:i64)->V {
    V::Record(r.new_record(id(1),vec![V::text(name),V::integer(age)]).unwrap())
}
fn group(r:&Registry,title:&str,people:Vec<V>)->V {
    V::Record(r.new_record(id(2),vec![
        V::text(title),
        V::List(r.new_list(T::Record(id(1)),people).unwrap()),
    ]).unwrap())
}
fn materialize(r:&Registry,ty:&T,initial:&V,wanted:&V)->Vec<Op>{
    let diff=r.reconcile_data(ty,initial,wanted,L::default()).unwrap();
    assert_eq!(r.apply_patch(ty,initial,&diff.operations).unwrap(),*wanted);
    diff.operations
}
#[test]
fn equality_requires_zero_patch_operations(){
    let r=schema();
    let a=group(&r,"A",vec![person(&r,"Ada",10)]);
    let d=r.reconcile_data(&T::Record(id(2)),&a,&a,L::default()).unwrap();
    assert!(d.is_empty());
    assert_eq!(d.len(),0);
    assert_eq!(d.visited,1);
}
#[test]
fn scalar_replacement_is_exact_single_operation(){
    let r=schema();
    let diff=materialize(&r,&T::integer(),&V::integer(3),&V::integer(4));
    assert_eq!(diff,vec![Op::Replace{path:vec![],value:V::integer(4)}]);
}
#[test]
fn record_field_update_uses_named_typed_path(){
    let r=schema();
    let a=person(&r,"Ada",21);
    let b=person(&r,"Ada",22);
    let diff=materialize(&r,&T::Record(id(1)),&a,&b);
    assert_eq!(diff,vec![Op::Replace{
        path:vec![S::field("age")],
        value:V::integer(22),
    }]);
}
#[test]
fn record_multiple_fields_follow_declaration_order(){
    let r=schema();
    let a=person(&r,"Ada",21);
    let b=person(&r,"Mira",22);
    let diff=materialize(&r,&T::Record(id(1)),&a,&b);
    assert_eq!(diff,vec![
        Op::Replace{path:vec![S::field("name")],value:V::text("Mira")},
        Op::Replace{path:vec![S::field("age")],value:V::integer(22)},
    ]);
}
#[test]
fn list_only_appends_extra_suffix(){
    let r=schema();
    let ty=T::list(T::integer());
    let a=V::List(r.new_list(T::integer(),vec![V::integer(1)]).unwrap());
    let b=V::List(r.new_list(T::integer(),vec![
        V::integer(1),V::integer(2),V::integer(3),
    ]).unwrap());
    let diff=materialize(&r,&ty,&a,&b);
    assert_eq!(diff,vec![
        Op::ListPush{path:vec![],value:V::integer(2)},
        Op::ListPush{path:vec![],value:V::integer(3)},
    ]);
}
#[test]
fn list_shrinks_by_descending_tail_indices(){
    let r=schema();
    let ty=T::list(T::integer());
    let a=V::List(r.new_list(T::integer(),vec![
        V::integer(1),V::integer(2),V::integer(3),
    ]).unwrap());
    let b=V::List(r.new_list(T::integer(),vec![V::integer(1)]).unwrap());
    assert_eq!(materialize(&r,&ty,&a,&b),vec![
        Op::ListRemove{path:vec![],index:2},
        Op::ListRemove{path:vec![],index:1},
    ]);
}
#[test]
fn list_overlapping_prefix_is_patched_before_extra_pushes(){
    let r=schema();
    let ty=T::list(T::integer());
    let a=V::List(r.new_list(T::integer(),vec![
        V::integer(1),V::integer(2),
    ]).unwrap());
    let b=V::List(r.new_list(T::integer(),vec![
        V::integer(3),V::integer(2),V::integer(4),
    ]).unwrap());
    assert_eq!(materialize(&r,&ty,&a,&b),vec![
        Op::Replace{path:vec![S::ListIndex(0)],value:V::integer(3)},
        Op::ListPush{path:vec![],value:V::integer(4)},
    ]);
}
#[test]
fn nested_list_of_records_only_replaces_changed_leaf(){
    let r=schema();
    let ty=T::Record(id(2));
    let a=group(&r,"A",vec![person(&r,"Ada",21),person(&r,"Mira",18)]);
    let b=group(&r,"A",vec![person(&r,"Ada",21),person(&r,"Mira",19)]);
    assert_eq!(materialize(&r,&ty,&a,&b),vec![Op::Replace{
        path:vec![S::field("people"),S::ListIndex(1),S::field("age")],
        value:V::integer(19),
    }]);
}
#[test]
fn diff_map_insert_delete_and_recursive_update(){
    let r=schema();
    let ty=T::map(K::Integer,T::Record(id(1)));
    let a=V::Map(r.new_map(K::Integer,T::Record(id(1)),vec![
        (DataKey::Integer(1),person(&r,"Ada",21)),
        (DataKey::Integer(2),person(&r,"Mira",20)),
    ]).unwrap());
    let b=V::Map(r.new_map(K::Integer,T::Record(id(1)),vec![
        (DataKey::Integer(2),person(&r,"Mira",21)),
        (DataKey::Integer(3),person(&r,"Zoe",22)),
    ]).unwrap());
    let diff=materialize(&r,&ty,&a,&b);
    assert_eq!(diff,vec![
        Op::MapDelete{path:vec![],key:DataKey::Integer(1)},
        Op::Replace{
            path:vec![S::MapKey(DataKey::Integer(2)),S::field("age")],
            value:V::integer(21),
        },
        Op::MapPut{
            path:vec![],key:DataKey::Integer(3),value:person(&r,"Zoe",22),
        },
    ]);
}
#[test]
fn optional_some_diff_only_touches_payload(){
    let r=schema();
    let ty=T::optional(T::integer());
    let a=V::some(V::integer(2));
    let b=V::some(V::integer(3));
    assert_eq!(materialize(&r,&ty,&a,&b),vec![Op::Replace{
        path:vec![S::OptionalSome],value:V::integer(3),
    }]);
}
#[test]
fn optional_case_change_replaces_entire_optional(){
    let r=schema();
    let ty=T::optional(T::integer());
    let a=V::none();
    let b=V::some(V::integer(3));
    assert_eq!(materialize(&r,&ty,&a,&b),vec![Op::Replace{
        path:vec![],value:b,
    }]);
}
#[test]
fn result_same_case_diff_touches_only_payload(){
    let r=schema();
    let ty=T::outcome(T::integer(),T::string());
    let a=V::err(V::text("one"));
    let b=V::err(V::text("two"));
    assert_eq!(materialize(&r,&ty,&a,&b),vec![Op::Replace{
        path:vec![S::OutcomeErr],value:V::text("two"),
    }]);
}
#[test]
fn result_case_change_replaces_parent(){
    let r=schema();
    let ty=T::outcome(T::integer(),T::string());
    let a=V::ok(V::integer(1));
    let b=V::err(V::text("bad"));
    assert_eq!(materialize(&r,&ty,&a,&b),vec![Op::Replace{
        path:vec![],value:b,
    }]);
}
#[test]
fn variant_same_case_updates_its_nested_nominal_payload(){
    let r=schema();
    let ty=T::Variant(id(3));
    let a=V::Variant(r.new_variant(id(3),"Ready",Some(person(&r,"Ada",21))).unwrap());
    let b=V::Variant(r.new_variant(id(3),"Ready",Some(person(&r,"Ada",22))).unwrap());
    assert_eq!(materialize(&r,&ty,&a,&b),vec![Op::Replace{
        path:vec![S::case("Ready"),S::field("age")],value:V::integer(22),
    }]);
}
#[test]
fn variant_case_change_replaces_whole_variant(){
    let r=schema();
    let ty=T::Variant(id(3));
    let a=V::Variant(r.new_variant(id(3),"Pending",None).unwrap());
    let b=V::Variant(r.new_variant(id(3),"Failed",Some(V::text("no"))).unwrap());
    assert_eq!(materialize(&r,&ty,&a,&b),vec![Op::Replace{
        path:vec![],value:b,
    }]);
}
#[test]
fn diff_rejects_mistyped_target_before_any_operation(){
    let r=schema();
    assert_eq!(r.diff_data(&T::integer(),&V::integer(2),&V::text("bad"),L::default()),
        Err(E::TypeMismatch));
}
#[test]
fn exact_zero_diff_operation_budget_allows_equality(){
    let r=schema();
    let a=person(&r,"Ada",21);
    assert!(r.diff_data(&T::Record(id(1)),&a,&a,L{
        max_operations:0,..L::default()
    }).unwrap().is_empty());
}
#[test]
fn maximum_diff_operations_rejects_partial_result(){
    let r=schema();
    let a=person(&r,"Ada",21);
    let b=person(&r,"Mira",22);
    assert_eq!(r.diff_data(&T::Record(id(1)),&a,&b,L{
        max_operations:1,..L::default()
    }),Err(E::Limit{resource:R::DiffOperations,limit:1}));
    assert_eq!(a,person(&r,"Ada",21));
}
#[test]
fn maximum_visited_nodes_limits_deep_traversal(){
    let r=schema();
    let a=person(&r,"Ada",21);
    let b=person(&r,"Mira",22);
    assert_eq!(r.diff_data(&T::Record(id(1)),&a,&b,L{
        max_visited:2,..L::default()
    }),Err(E::Limit{resource:R::DiffNodes,limit:2}));
}
#[test]
fn maximum_diff_depth_prevents_excessive_recursion(){
    let r=schema();
    let a=group(&r,"A",vec![person(&r,"Ada",21)]);
    let b=group(&r,"A",vec![person(&r,"Ada",22)]);
    assert_eq!(r.diff_data(&T::Record(id(2)),&a,&b,L{
        max_depth:3,..L::default()
    }),Err(E::Limit{resource:R::DiffDepth,limit:3}));
}
#[test]
fn ordered_map_changes_are_canonical_regardless_of_input_insertion_order(){
    let r=schema();
    let ty=T::map(K::Text,T::integer());
    let before=V::Map(r.new_map(K::Text,T::integer(),vec![
        (DataKey::text("z"),V::integer(1)),
        (DataKey::text("a"),V::integer(2)),
    ]).unwrap());
    let after=V::Map(r.new_map(K::Text,T::integer(),vec![
        (DataKey::text("a"),V::integer(3)),
        (DataKey::text("b"),V::integer(4)),
    ]).unwrap());
    assert_eq!(materialize(&r,&ty,&before,&after),vec![
        Op::Replace{path:vec![S::MapKey(DataKey::text("a"))],value:V::integer(3)},
        Op::MapPut{path:vec![],key:DataKey::text("b"),value:V::integer(4)},
        Op::MapDelete{path:vec![],key:DataKey::text("z")},
    ]);
}
#[test]
fn a_patch_roundtrip_can_be_repeated_for_multiple_target_snapshots(){
    let r=schema();
    let ty=T::Record(id(2));
    let mut current=group(&r,"A",vec![]);
    for version in 0..12 {
        let target=group(&r,"B",vec![
            person(&r,"Ada",version),
            person(&r,"Mira",version+1),
        ]);
        let operations=materialize(&r,&ty,&current,&target);
        assert_eq!(r.apply_patch(&ty,&current,&operations).unwrap(),target);
        current=target;
    }
}
#[test]
fn repeated_generation_is_deterministic_and_non_mutating(){
    let r=schema();
    let ty=T::Record(id(2));
    let a=group(&r,"One",vec![person(&r,"Ada",21)]);
    let b=group(&r,"Two",vec![person(&r,"Ada",22),person(&r,"Mira",23)]);
    let first=r.diff_data(&ty,&a,&b,L::default()).unwrap();
    let second=r.diff_data(&ty,&a,&b,L::default()).unwrap();
    assert_eq!(first,second);
    assert_eq!(r.apply_patch(&ty,&a,&first.operations).unwrap(),b);
    assert_eq!(a,group(&r,"One",vec![person(&r,"Ada",21)]));
}
