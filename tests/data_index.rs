use scarp::data_engine::{
    DataError as E, DataIndex as I, DataKey, DataResource as R, DataType as T, Datum as V,
    Definition, Field, IndexLimits as L, MapKeyType as K, PathSegment as S,
    SchemaRegistry as Registry,
};
use scarp::types::DefinitionId as Id;

fn id(n: u32) -> Id {
    Id {
        module: 64,
        declaration: n,
    }
}
fn registry() -> Registry {
    let mut r = Registry::default();
    r.register(Definition::record(
        id(1),
        "Person",
        vec![
            Field::new("name", T::string()),
            Field::new("team", T::integer()),
        ],
    ))
    .unwrap();
    r
}
fn person(r: &Registry, name: &str, team: i64) -> V {
    V::Record(
        r.new_record(id(1), vec![V::text(name), V::integer(team)])
            .unwrap(),
    )
}
fn list(r: &Registry) -> scarp::data_engine::ListValue {
    r.new_list(
        T::Record(id(1)),
        vec![
            person(r, "Ada", 1),
            person(r, "Mira", 2),
            person(r, "Ada", 3),
            person(r, "Nina", 1),
            person(r, "Áda", 2),
            person(r, "", 0),
        ],
    )
    .unwrap()
}
fn index(r: &Registry, field: &str) -> I {
    I::build(r, &list(r), vec![S::field(field)], L::default()).unwrap()
}
#[test]
fn stable_name_index_uses_lexicographic_byte_order() {
    let r = registry();
    let idx = index(&r, "name");
    let names: Vec<_> = idx.unique_keys().cloned().collect();
    assert_eq!(
        names,
        vec![
            DataKey::text(""),
            DataKey::text("Ada"),
            DataKey::text("Mira"),
            DataKey::text("Nina"),
            DataKey::text("Áda"),
        ]
    );
    assert_eq!(idx.stats().indexed_rows, 6);
    assert_eq!(idx.stats().distinct_keys, 5);
    assert_eq!(idx.stats().key_text_bytes, "AdaMiraAdaNinaÁda".len());
}
#[test]
fn equality_lookup_returns_source_positions_not_key_sort_rank() {
    let r = registry();
    let idx = index(&r, "name");
    let a = idx.find(&DataKey::text("Ada")).unwrap();
    assert_eq!(a.iter().map(|v| v.position).collect::<Vec<_>>(), vec![0, 2]);
    assert!(idx.find(&DataKey::text("Unknown")).unwrap().is_empty());
    assert_eq!(idx.find(&DataKey::Integer(1)), Err(E::InvalidKey));
}
#[test]
fn materializing_an_index_hit_reproduces_original_typed_row() {
    let r = registry();
    let idx = index(&r, "name");
    let hits = idx.find(&DataKey::text("Ada")).unwrap();
    let result = idx.materialize(&r, &hits).unwrap();
    assert_eq!(result.element_type(), &T::Record(id(1)));
    assert_eq!(
        result.values(),
        &[person(&r, "Ada", 1), person(&r, "Ada", 3),]
    );
}
#[test]
fn integer_range_is_inclusive_and_sorted_by_key_then_source_position() {
    let r = registry();
    let idx = index(&r, "team");
    assert_eq!(idx.key_type(), K::Integer);
    let all = idx.integer_range(None, None).unwrap();
    assert_eq!(all.len(), 6);
    let selected = idx.integer_range(Some(1), Some(2)).unwrap();
    let positions: Vec<_> = selected.iter().map(|v| v.position).collect();
    assert_eq!(positions, vec![0, 3, 1, 4]);
    assert_eq!(idx.integer_range(Some(3), Some(2)), Err(E::InvalidShape));
}
#[test]
fn text_prefix_is_case_sensitive_and_unicode_safe() {
    let r = registry();
    let idx = index(&r, "name");
    let selected = idx.text_prefix("A").unwrap();
    assert_eq!(
        selected.iter().map(|x| x.position).collect::<Vec<_>>(),
        vec![0, 2]
    );
    assert_eq!(idx.text_prefix("Á").unwrap().len(), 1);
    assert!(idx.text_prefix("á").unwrap().is_empty());
    assert_eq!(idx.text_prefix("").unwrap().len(), 6);
}
#[test]
fn index_uses_exact_declared_path_type() {
    let r = registry();
    assert_eq!(
        I::build(&r, &list(&r), vec![S::field("unknown")], L::default()),
        Err(E::UnknownField("unknown".into()))
    );
    let idx = index(&r, "name");
    assert_eq!(idx.path(), &[S::field("name")]);
    assert_eq!(idx.element_type(), &T::Record(id(1)));
    assert_eq!(idx.source().len(), 6);
}
#[test]
fn duplicate_index_hits_materialize_as_explicit_duplicate_rows() {
    let r = registry();
    let idx = index(&r, "name");
    let a = idx.find(&DataKey::text("Ada")).unwrap();
    let hits = vec![a[1].clone(), a[0].clone(), a[1].clone()];
    let result = idx.materialize(&r, &hits).unwrap();
    assert_eq!(
        result.values(),
        &[
            person(&r, "Ada", 3),
            person(&r, "Ada", 1),
            person(&r, "Ada", 3),
        ]
    );
}
#[test]
fn index_materialization_rejects_tampered_hit_key() {
    let r = registry();
    let idx = index(&r, "name");
    let mut a = idx.find(&DataKey::text("Ada")).unwrap();
    a[0].key = DataKey::text("Nina");
    assert_eq!(idx.materialize(&r, &a), Err(E::InvalidKey));
    a[0].position = 100;
    assert_eq!(idx.materialize(&r, &a), Err(E::InvalidShape));
}
#[test]
fn index_snapshot_is_not_modified_when_source_list_changes() {
    let r = registry();
    let original = list(&r);
    let idx = I::build(&r, &original, vec![S::field("name")], L::default()).unwrap();
    let changed = r.list_push(&original, person(&r, "Ada", 90)).unwrap();
    assert_eq!(changed.len(), 7);
    assert_eq!(idx.source().len(), 6);
    assert_eq!(idx.find(&DataKey::text("Ada")).unwrap().len(), 2);
    let new_index = I::build(&r, &changed, vec![S::field("name")], L::default()).unwrap();
    assert_eq!(new_index.find(&DataKey::text("Ada")).unwrap().len(), 3);
}
#[test]
fn exact_max_row_limit_rejects_overflow() {
    let r = registry();
    assert_eq!(
        I::build(
            &r,
            &list(&r),
            vec![S::field("name")],
            L {
                max_rows: 5,
                ..L::default()
            }
        ),
        Err(E::Limit {
            resource: R::IndexRows,
            limit: 5
        })
    );
    assert!(I::build(
        &r,
        &list(&r),
        vec![S::field("name")],
        L {
            max_rows: 6,
            ..L::default()
        }
    )
    .is_ok());
}
#[test]
fn exact_distinct_key_limit_rejects_fifth_unique_key() {
    let r = registry();
    assert_eq!(
        I::build(
            &r,
            &list(&r),
            vec![S::field("name")],
            L {
                max_distinct_keys: 4,
                ..L::default()
            }
        ),
        Err(E::Limit {
            resource: R::IndexKeys,
            limit: 4
        })
    );
}
#[test]
fn exact_key_text_budget_counts_duplicate_visible_values() {
    let r = registry();
    let idx = index(&r, "name");
    let len = idx.stats().key_text_bytes;
    assert_eq!(
        I::build(
            &r,
            &list(&r),
            vec![S::field("name")],
            L {
                max_key_bytes: len - 1,
                ..L::default()
            }
        ),
        Err(E::Limit {
            resource: R::IndexKeyBytes,
            limit: len - 1
        })
    );
    assert!(I::build(
        &r,
        &list(&r),
        vec![S::field("name")],
        L {
            max_key_bytes: len,
            ..L::default()
        }
    )
    .is_ok());
}
#[test]
fn integer_index_does_not_charge_text_key_bytes() {
    let r = registry();
    let idx = I::build(
        &r,
        &list(&r),
        vec![S::field("team")],
        L {
            max_key_bytes: 0,
            ..L::default()
        },
    )
    .unwrap();
    assert_eq!(idx.stats().key_text_bytes, 0);
}
#[test]
fn deterministic_self_join_counts_cartesian_duplicates() {
    let r = registry();
    let idx = index(&r, "name");
    let joined = idx.equijoin(&idx, L::default()).unwrap();
    // One key occurs twice; all others once: 2*2 + 4*1 = 8.
    assert_eq!(joined.rows.len(), 8);
    let ada: Vec<_> = joined
        .rows
        .iter()
        .filter(|item| item.key == DataKey::text("Ada"))
        .collect();
    assert_eq!(ada.len(), 4);
    assert_eq!(
        ada.iter()
            .map(|r| (r.left_position, r.right_position))
            .collect::<Vec<_>>(),
        vec![(0, 0), (0, 2), (2, 0), (2, 2)]
    );
}
#[test]
fn self_join_returns_original_immutable_rows() {
    let r = registry();
    let idx = index(&r, "name");
    let joined = idx.equijoin(&idx, L::default()).unwrap();
    let first = joined
        .rows
        .iter()
        .find(|r| r.key == DataKey::text("Ada"))
        .unwrap();
    assert_eq!(first.left, person(&r, "Ada", 1));
    assert_eq!(first.right, person(&r, "Ada", 1));
    assert!(joined.compared_keys >= 1);
}
#[test]
fn join_on_two_different_lists_matches_only_shared_keys() {
    let r = registry();
    let left = index(&r, "name");
    let right_list = r
        .new_list(
            T::Record(id(1)),
            vec![
                person(&r, "Ada", 99),
                person(&r, "Mira", 77),
                person(&r, "Ada", 88),
            ],
        )
        .unwrap();
    let right = I::build(&r, &right_list, vec![S::field("name")], L::default()).unwrap();
    let output = left.equijoin(&right, L::default()).unwrap();
    let pairs: Vec<_> = output
        .rows
        .iter()
        .map(|row| (row.left_position, row.right_position))
        .collect();
    assert_eq!(pairs, vec![(0, 0), (0, 2), (2, 0), (2, 2), (1, 1)]);
}
#[test]
fn join_rejects_mismatched_index_key_types() {
    let r = registry();
    let a = index(&r, "name");
    let b = index(&r, "team");
    assert_eq!(a.equijoin(&b, L::default()), Err(E::TypeMismatch));
}
#[test]
fn join_pair_budget_is_enforced_before_cartesian_allocation() {
    let r = registry();
    let idx = index(&r, "name");
    assert_eq!(
        idx.equijoin(
            &idx,
            L {
                max_join_pairs: 7,
                ..L::default()
            }
        ),
        Err(E::Limit {
            resource: R::IndexPairs,
            limit: 7
        })
    );
    assert_eq!(
        idx.equijoin(
            &idx,
            L {
                max_join_pairs: 8,
                ..L::default()
            }
        )
        .unwrap()
        .rows
        .len(),
        8
    );
}
#[test]
fn join_distinct_key_budget_is_separate_from_pair_budget() {
    let r = registry();
    let idx = index(&r, "name");
    assert_eq!(
        idx.equijoin(
            &idx,
            L {
                max_distinct_keys: 2,
                ..L::default()
            }
        ),
        Err(E::Limit {
            resource: R::IndexKeys,
            limit: 2
        })
    );
}
#[test]
fn unknown_index_key_never_reads_or_constructs_a_value() {
    let r = registry();
    let idx = index(&r, "team");
    assert!(idx.find(&DataKey::Integer(999)).unwrap().is_empty());
    assert_eq!(idx.occurrences(&DataKey::Integer(999)).unwrap(), &[]);
}
#[test]
fn index_matches_filtered_scan_for_exact_integer_key() {
    let r = registry();
    let source = list(&r);
    let idx = index(&r, "team");
    let materialized = idx
        .materialize(&r, &idx.find(&DataKey::Integer(1)).unwrap())
        .unwrap();
    let scanned: Vec<V> = source
        .values()
        .iter()
        .filter(|datum| r.field(datum_as_record(datum), "team").unwrap() == &V::integer(1))
        .cloned()
        .collect();
    assert_eq!(materialized.values(), scanned);
}
fn datum_as_record(datum: &V) -> &scarp::data_engine::RecordValue {
    match datum {
        V::Record(v) => v,
        _ => panic!("test expects record"),
    }
}
#[test]
fn map_key_data_type_is_not_mistaken_for_data_index_keys() {
    let r = registry();
    let map = r
        .new_map(
            K::Text,
            T::integer(),
            vec![(DataKey::text("x"), V::integer(5))],
        )
        .unwrap();
    assert_eq!(map.len(), 1);
    let idx = index(&r, "name");
    assert_eq!(idx.key_type(), K::Text);
}
