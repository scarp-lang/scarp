//! Read-only value inspection: typed paths and deterministic traversal order.
use scarp::data_engine::{
    DataError as E, DataKey, DataResource as R, DataType as T, Datum as V,
    Definition as D, Field, InspectKind, InspectionLimits as L, MapKeyType as K,
    PathSegment as S, SchemaRegistry as Registry, VariantCase as C,
};
use scarp::types::DefinitionId as Id;

fn id(n:u32)->Id {Id{module:135,declaration:n}}
fn registry()->Registry{
    let mut r=Registry::default();
    r.register_batch(vec![
        D::record(id(1),"Person",vec![
            Field::new("name",T::string()),
            Field::new("age",T::integer()),
        ]),
        D::variant(id(2),"Status",vec![
            C::unit("Pending"),
            C::typed("Ready",T::Record(id(1))),
        ]),
    ]).unwrap();
    r
}
fn person(r:&Registry,name:&str,age:i64)->V{
    V::Record(r.new_record(id(1),vec![V::text(name),V::integer(age)]).unwrap())
}
fn fixture(r:&Registry)->(T,V){
    let ty=T::map(K::Text,T::list(T::Variant(id(2))));
    let list=r.new_list(T::Variant(id(2)),vec![
        V::Variant(r.new_variant(id(2),"Pending",None).unwrap()),
        V::Variant(r.new_variant(id(2),"Ready",Some(person(r,"Ada",21))).unwrap()),
    ]).unwrap();
    let map=V::Map(r.new_map(K::Text,T::list(T::Variant(id(2))),vec![
        (DataKey::text("other"),V::List(r.new_list(T::Variant(id(2)),vec![]).unwrap())),
        (DataKey::text("team"),V::List(list)),
    ]).unwrap());
    (ty,map)
}
#[test]
fn primitive_inspection_includes_root_and_correct_text_bytes(){
    let r=registry();
    let report=r.inspect_data(&T::string(),&V::text("हिंदी"),L::default()).unwrap();
    assert_eq!(report.nodes.len(),1);
    assert_eq!(report.nodes[0].kind,InspectKind::Source);
    assert!(report.nodes[0].path.is_empty());
    assert_eq!(report.stats.scalar_nodes,1);
    assert_eq!(report.stats.visible_text_bytes,"हिंदी".len());
    assert_eq!(report.stats.max_depth,1);
}
#[test]
fn record_inspection_visits_fields_in_schema_order(){
    let r=registry();
    let report=r.inspect_data(&T::Record(id(1)),&person(&r,"Ada",21),L::default()).unwrap();
    assert_eq!(report.nodes.len(),3);
    assert_eq!(report.nodes[0].kind,InspectKind::Record);
    assert_eq!(report.nodes[1].path,vec![S::field("name")]);
    assert_eq!(report.nodes[2].path,vec![S::field("age")]);
    assert_eq!(report.stats.record_nodes,1);
    assert_eq!(report.stats.scalar_nodes,2);
    assert_eq!(report.stats.visible_text_bytes,3);
    assert_eq!(report.stats.max_depth,2);
}
#[test]
fn nested_map_list_variant_paths_follow_sorted_map_key_order(){
    let r=registry();
    let (ty,datum)=fixture(&r);
    let report=r.inspect_data(&ty,&datum,L::default()).unwrap();
    assert_eq!(report.nodes[0].kind,InspectKind::Map);
    assert_eq!(report.nodes[1].path,vec![S::MapKey(DataKey::text("other"))]);
    assert_eq!(report.nodes[2].path,vec![S::MapKey(DataKey::text("team"))]);
    assert_eq!(report.nodes[3].path,vec![
        S::MapKey(DataKey::text("team")),S::ListIndex(0),
    ]);
    assert_eq!(report.nodes[4].path,vec![
        S::MapKey(DataKey::text("team")),S::ListIndex(1),
    ]);
    assert_eq!(report.nodes[5].path,vec![
        S::MapKey(DataKey::text("team")),S::ListIndex(1),S::case("Ready"),
    ]);
    assert_eq!(report.nodes[6].path,vec![
        S::MapKey(DataKey::text("team")),S::ListIndex(1),
        S::case("Ready"),S::field("name"),
    ]);
    assert_eq!(report.nodes[7].path,vec![
        S::MapKey(DataKey::text("team")),S::ListIndex(1),
        S::case("Ready"),S::field("age"),
    ]);
    assert_eq!(report.stats.nodes,8);
    assert_eq!(report.stats.map_nodes,1);
    assert_eq!(report.stats.map_entries,2);
    assert_eq!(report.stats.list_nodes,2);
    assert_eq!(report.stats.list_elements,2);
    assert_eq!(report.stats.variant_nodes,2);
    assert_eq!(report.stats.record_nodes,1);
    assert_eq!(report.stats.scalar_nodes,2);
}
#[test]
fn inspection_paths_can_be_reused_for_verified_selection(){
    let r=registry();
    let (ty,datum)=fixture(&r);
    let report=r.inspect_data(&ty,&datum,L::default()).unwrap();
    for node in report.nodes {
        let selected=r.select_path(&ty,&datum,&node.path).unwrap();
        assert!(r.verify(&node.ty,selected).is_ok());
    }
}
#[test]
fn report_lookup_finds_exact_path_not_case_insensitive_names(){
    let r=registry();
    let (ty,datum)=fixture(&r);
    let report=r.inspect_data(&ty,&datum,L::default()).unwrap();
    let path=vec![S::MapKey(DataKey::text("team"))];
    assert_eq!(report.node_at(&path).unwrap().kind,InspectKind::List);
    assert!(report.node_at(&[S::MapKey(DataKey::text("TEAM"))]).is_none());
}
#[test]
fn optional_and_outcome_cases_are_recorded_as_distinct_node_kinds(){
    let r=registry();
    let ty=T::optional(T::outcome(T::integer(),T::string()));
    let a=V::none();
    let report=r.inspect_data(&ty,&a,L::default()).unwrap();
    assert_eq!(report.nodes.len(),1);
    assert_eq!(report.nodes[0].kind,InspectKind::OptionalNone);
    let b=V::some(V::err(V::text("bad")));
    let report=r.inspect_data(&ty,&b,L::default()).unwrap();
    assert_eq!(report.nodes.iter().map(|n|n.kind).collect::<Vec<_>>(),vec![
        InspectKind::OptionalSome,InspectKind::OutcomeErr,InspectKind::Source,
    ]);
    assert_eq!(report.nodes[2].path,vec![S::OptionalSome,S::OutcomeErr]);
    assert_eq!(report.stats.optional_some,1);
    assert_eq!(report.stats.outcome_err,1);
    let c=V::some(V::ok(V::integer(1)));
    let report=r.inspect_data(&ty,&c,L::default()).unwrap();
    assert_eq!(report.nodes[1].kind,InspectKind::OutcomeOk);
}
#[test]
fn strict_node_budget_rejects_partially_assembled_report(){
    let r=registry();
    let (ty,datum)=fixture(&r);
    assert_eq!(r.inspect_data(&ty,&datum,L{
        max_nodes:3,..L::default()
    }),Err(E::Limit{resource:R::InspectionNodes,limit:3}));
}
#[test]
fn exact_node_budget_accepts_only_complete_preorder(){
    let r=registry();
    let (ty,datum)=fixture(&r);
    assert_eq!(r.inspect_data(&ty,&datum,L{
        max_nodes:8,..L::default()
    }).unwrap().stats.nodes,8);
}
#[test]
fn strict_depth_budget_limits_nested_variant_and_field_paths(){
    let r=registry();
    let (ty,datum)=fixture(&r);
    assert_eq!(r.inspect_data(&ty,&datum,L{
        max_depth:4,..L::default()
    }),Err(E::Limit{resource:R::InspectionDepth,limit:4}));
    assert_eq!(r.inspect_data(&ty,&datum,L{
        max_depth:5,..L::default()
    }).unwrap().stats.max_depth,5);
}
#[test]
fn empty_map_and_list_report_only_their_container_nodes(){
    let r=registry();
    let empty_map=V::Map(r.new_map(K::Integer,T::integer(),vec![]).unwrap());
    let report=r.inspect_data(&T::map(K::Integer,T::integer()),&empty_map,L::default()).unwrap();
    assert_eq!(report.nodes.len(),1);
    assert_eq!(report.stats.map_entries,0);
    let empty_list=V::List(r.new_list(T::integer(),vec![]).unwrap());
    let report=r.inspect_data(&T::list(T::integer()),&empty_list,L::default()).unwrap();
    assert_eq!(report.nodes.len(),1);
    assert_eq!(report.stats.list_elements,0);
}
#[test]
fn inspection_rejects_wrong_root_type_before_exposing_metadata(){
    let r=registry();
    assert_eq!(r.inspect_data(&T::integer(),&V::text("not integer"),L::default()),
        Err(E::TypeMismatch));
}
#[test]
fn cloned_immutable_source_produces_equal_inspection_reports(){
    let r=registry();
    let (ty,datum)=fixture(&r);
    let first=r.inspect_data(&ty,&datum,L::default()).unwrap();
    let second=r.clone().inspect_data(&ty,&datum.clone(),L::default()).unwrap();
    assert_eq!(first,second);
}
#[test]
fn report_does_not_copy_user_string_contents_into_node_metadata(){
    let r=registry();
    let value=V::text("a private host string");
    let report=r.inspect_data(&T::string(),&value,L::default()).unwrap();
    assert_eq!(report.nodes[0].ty,T::string());
    assert!(report.nodes[0].path.is_empty());
    assert_eq!(report.stats.visible_text_bytes,"a private host string".len());
}
#[test]
fn nested_text_bytes_sum_tracks_all_visible_strings(){
    let r=registry();
    let value=V::List(r.new_list(T::string(),vec![
        V::text("ह"),V::text("अ"),V::text("A"),
    ]).unwrap());
    let report=r.inspect_data(&T::list(T::string()),&value,L::default()).unwrap();
    assert_eq!(report.stats.visible_text_bytes,"हअA".len());
    assert_eq!(report.stats.scalar_nodes,3);
    assert_eq!(report.stats.nodes,4);
}
#[test]
fn inspecting_an_updated_snapshot_never_alters_old_one(){
    let r=registry();
    let original=person(&r,"Ada",21);
    let updated=r.replace_path(&T::Record(id(1)),&original,
        &[S::field("age")],V::integer(22)).unwrap();
    let before=r.inspect_data(&T::Record(id(1)),&original,L::default()).unwrap();
    let after=r.inspect_data(&T::Record(id(1)),&updated,L::default()).unwrap();
    assert_eq!(before,after);
    assert_eq!(r.select_path(&T::Record(id(1)),&original,&[S::field("age")]),
        Ok(&V::integer(21)));
    assert_eq!(r.select_path(&T::Record(id(1)),&updated,&[S::field("age")]),
        Ok(&V::integer(22)));
}
