use scarp::data_engine::{
    DataError as E, DataLimits, DataResource as R, DataType as T,
    Definition as D, Field as F, SchemaGraph as G, SchemaRegistry as Registry,
    VariantCase as C,
};
use scarp::types::DefinitionId as Id;

fn id(n:u32)->Id {Id{module:44,declaration:n}}
fn record(n:u32,fields:Vec<F>)->D {D::record(id(n),format!("Record{n}"),fields)}
fn chain()->Registry{
    let mut r=Registry::default();
    r.register_batch(vec![
        record(3,vec![F::new("second",T::Record(id(2)))]),
        record(1,vec![F::new("value",T::integer())]),
        record(2,vec![F::new("first",T::Record(id(1)))]),
    ]).unwrap();
    r
}
#[test]
fn mutual_forward_references_register_atomically(){
    let mut r=Registry::default();
    r.register_batch(vec![
        record(1,vec![F::new("next",T::optional(T::Record(id(2))))]),
        record(2,vec![F::new("next",T::optional(T::Record(id(1))))]),
    ]).unwrap();
    assert_eq!(r.len(),2);
    let g=G::new(&r).unwrap();
    assert_eq!(g.dependencies(id(1)).unwrap(),vec![id(2)]);
    assert_eq!(g.dependencies(id(2)).unwrap(),vec![id(1)]);
    assert_eq!(g.cycles(),vec![vec![id(1),id(2)]]);
}
#[test]
fn self_recursive_variants_are_verified_for_nominal_kind(){
    let mut r=Registry::default();
    r.register(D::variant(id(1),"Linked",vec![
        C::unit("Empty"),
        C::typed("Next",T::optional(T::Variant(id(1)))),
    ])).unwrap();
    let g=G::new(&r).unwrap();
    assert_eq!(g.cycles(),vec![vec![id(1)]]);
    let variant=r.new_variant(id(1),"Empty",None).unwrap();
    assert_eq!(r.case_name(&variant).unwrap(),"Empty");
}
#[test]
fn wrong_self_reference_kind_does_not_bypass_validation(){
    let mut r=Registry::default();
    let bad=D::variant(id(1),"Bad",vec![C::typed("Recursive",T::Record(id(1)))]);
    assert_eq!(r.register(bad),Err(E::InvalidDefinition));
    assert_eq!(r.len(),0);
    let bad=D::record(id(1),"Bad",vec![F::new("self",T::Variant(id(1)))]);
    assert_eq!(r.register(bad),Err(E::InvalidDefinition));
    assert_eq!(r.len(),0);
}
#[test]
fn failed_batch_rolls_back_entire_schema_graph(){
    let mut r=Registry::default();
    r.register(record(1,vec![F::new("a",T::integer())])).unwrap();
    let before=r.definitions().to_vec();
    let result=r.register_batch(vec![
        record(2,vec![F::new("a",T::Record(id(1)))]),
        record(3,vec![F::new("b",T::Record(id(999)))]),
    ]);
    assert_eq!(result,Err(E::UnknownDefinition));
    assert_eq!(r.definitions(),before);
    r.register(record(2,vec![F::new("a",T::Record(id(1)))]))
        .unwrap();
    assert_eq!(r.len(),2);
}
#[test]
fn duplicate_id_in_batch_rejected_with_no_partial_registration(){
    let mut r=Registry::default();
    assert_eq!(r.register_batch(vec![record(1,vec![]),record(1,vec![])]),
        Err(E::DuplicateDefinition));
    assert_eq!(r.len(),0);
}
#[test]
fn empty_batch_is_noop_and_does_not_consume_limits(){
    let mut r=Registry::new(DataLimits{max_definitions:0,..DataLimits::default()});
    r.register_batch(vec![]).unwrap();
    assert_eq!(r.len(),0);
    assert_eq!(r.register(record(1,vec![])),
        Err(E::Limit{resource:R::Definitions,limit:0}));
}
#[test]
fn batch_member_limit_and_type_nodes_are_cumulative(){
    let mut r=Registry::new(DataLimits{
        max_members_per_definition:1,..DataLimits::default()
    });
    assert_eq!(r.register_batch(vec![record(1,vec![
        F::new("a",T::integer()),F::new("b",T::integer()),
    ])]),Err(E::Limit{resource:R::Members,limit:1}));
    assert_eq!(r.len(),0);
    let mut r=Registry::new(DataLimits{max_type_nodes:2,..DataLimits::default()});
    r.register_batch(vec![record(1,vec![F::new("a",T::integer())])]).unwrap();
    assert_eq!(r.register_batch(vec![record(2,vec![
        F::new("a",T::integer()),F::new("b",T::integer()),
    ])]),Err(E::Limit{resource:R::TypeNodes,limit:2}));
    assert_eq!(r.len(),1);
}
#[test]
fn graph_direct_dependencies_are_sorted_by_declaration_order(){
    let r=chain();
    let g=G::new(&r).unwrap();
    assert_eq!(g.identities(),&[id(3),id(1),id(2)]);
    assert_eq!(g.dependencies(id(3)).unwrap(),vec![id(2)]);
    assert_eq!(g.dependencies(id(2)).unwrap(),vec![id(1)]);
    assert_eq!(g.dependencies(id(1)).unwrap(),vec![]);
    assert_eq!(g.dependents(id(1)).unwrap(),vec![id(2)]);
    assert_eq!(g.dependents(id(2)).unwrap(),vec![id(3)]);
    assert_eq!(g.len(),3);
    assert!(!g.is_empty());
}
#[test]
fn graph_dependency_first_order_uses_stable_declaration_tiebreaks(){
    let r=chain();
    let g=G::new(&r).unwrap();
    assert_eq!(g.dependency_order(),Ok(vec![id(1),id(2),id(3)]));
    assert_eq!(g.transitive_dependencies(id(3)).unwrap(),vec![id(1),id(2)]);
    assert_eq!(g.transitive_dependents(id(1)).unwrap(),vec![id(3),id(2)]);
    assert!(g.depends_on(id(3),id(1)).unwrap());
    assert!(!g.depends_on(id(1),id(3)).unwrap());
}
#[test]
fn strongly_connected_components_are_partitioned_deterministically(){
    let mut r=Registry::default();
    r.register_batch(vec![
        record(1,vec![F::new("to2",T::optional(T::Record(id(2))))]),
        record(2,vec![F::new("to1",T::optional(T::Record(id(1))))]),
        record(3,vec![F::new("leaf",T::integer())]),
        record(4,vec![F::new("self",T::optional(T::Record(id(4))))]),
        record(5,vec![F::new("to3",T::Record(id(3)))]),
    ]).unwrap();
    let g=G::new(&r).unwrap();
    assert_eq!(g.strongly_connected_components(),vec![
        vec![id(1),id(2)],vec![id(3)],vec![id(4)],vec![id(5)],
    ]);
    assert_eq!(g.cycles(),vec![vec![id(1),id(2)],vec![id(4)]]);
    assert_eq!(g.dependency_order(),Err(E::CyclicDefinition));
}
#[test]
fn graph_unknown_id_errors_not_silent_empty_result(){
    let g=G::new(&chain()).unwrap();
    assert_eq!(g.dependencies(id(999)),Err(E::UnknownDefinition));
    assert_eq!(g.dependents(id(999)),Err(E::UnknownDefinition));
    assert_eq!(g.transitive_dependencies(id(999)),Err(E::UnknownDefinition));
    assert_eq!(g.transitive_dependents(id(999)),Err(E::UnknownDefinition));
    assert_eq!(g.depends_on(id(1),id(999)),Err(E::UnknownDefinition));
}
#[test]
fn empty_registry_has_empty_graph_and_order(){
    let g=G::new(&Registry::default()).unwrap();
    assert!(g.is_empty());
    assert_eq!(g.cycles(),Vec::<Vec<Id>>::new());
    assert_eq!(g.dependency_order().unwrap(),Vec::<Id>::new());
}
#[test]
fn graph_reflects_type_references_buried_in_map_option_and_outcome(){
    let mut r=Registry::default();
    r.register_batch(vec![
        record(1,vec![
            F::new("nested",T::map(
                scarp::data_engine::MapKeyType::Text,
                T::optional(T::outcome(T::Record(id(2)),T::Variant(id(3))))
            )),
        ]),
        record(2,vec![]),
        D::variant(id(3),"Status",vec![C::unit("Unknown")]),
    ]).unwrap();
    let g=G::new(&r).unwrap();
    assert_eq!(g.dependencies(id(1)).unwrap(),vec![id(2),id(3)]);
    assert_eq!(g.dependency_order().unwrap(),vec![id(2),id(3),id(1)]);
}
#[test]
fn large_bounded_chain_traverses_iteratively(){
    const N:u32=512;
    let mut definitions=Vec::new();
    for n in 0..N {
        definitions.push(record(n,if n==0 {vec![]} else {
            vec![F::new("prev",T::optional(T::Record(id(n-1))))]
        }));
    }
    let mut r=Registry::new(DataLimits{
        max_definitions:N as usize,
        ..DataLimits::default()
    });
    r.register_batch(definitions).unwrap();
    let g=G::new(&r).unwrap();
    assert_eq!(g.transitive_dependencies(id(N-1)).unwrap().len(),(N-1) as usize);
    assert_eq!(g.dependency_order().unwrap().len(),N as usize);
    assert!(g.cycles().is_empty());
}
