use scarp::data_engine::{
    DataError as E, DataKey, DataResource as R, DataType as T, Datum as V, Definition, Field,
    MapKeyType as K, PathSegment as S, Predicate as P, QueryLimits, QueryPlan,
    SchemaRegistry as Registry, VariantCase,
};
use scarp::types::DefinitionId as Id;

fn id(n: u32) -> Id {
    Id {
        module: 61,
        declaration: n,
    }
}
fn registry() -> Registry {
    let mut r = Registry::default();
    r.register(Definition::record(
        id(1),
        "Student",
        vec![
            Field::new("name", T::string()),
            Field::new("age", T::integer()),
            Field::new("active", T::boolean()),
            Field::new("note", T::optional(T::string())),
        ],
    ))
    .unwrap();
    r.register(Definition::variant(
        id(2),
        "Status",
        vec![
            VariantCase::unit("Waiting"),
            VariantCase::typed("Ready", T::integer()),
        ],
    ))
    .unwrap();
    r
}
fn row(r: &Registry, name: &str, age: i64, active: bool, note: Option<&str>) -> V {
    V::Record(
        r.new_record(
            id(1),
            vec![
                V::text(name),
                V::integer(age),
                V::boolean(active),
                note.map(|n| V::some(V::text(n))).unwrap_or_else(V::none),
            ],
        )
        .unwrap(),
    )
}
fn source(r: &Registry) -> scarp::data_engine::ListValue {
    r.new_list(
        T::Record(id(1)),
        vec![
            row(r, "Zoe", 18, true, Some("हिंदी")),
            row(r, "Ada", 21, false, None),
            row(r, "Zoe", 24, true, Some("Rust")),
            row(r, "Mira", 21, true, Some("Scarp")),
        ],
    )
    .unwrap()
}
fn field(name: &str) -> Vec<S> {
    vec![S::field(name)]
}
fn plan(p: P, projection: Vec<S>) -> QueryPlan {
    QueryPlan {
        predicate: p,
        projection,
        limit: None,
    }
}
#[test]
fn filtering_with_integer_range_projects_exact_names() {
    let r = registry();
    let q = plan(P::range(field("age"), Some(20), Some(24)), field("name"));
    let result = q.execute(&r, &source(&r), QueryLimits::default()).unwrap();
    assert_eq!(result.rows.element_type(), &T::string());
    assert_eq!(
        result.rows.values(),
        &[V::text("Ada"), V::text("Zoe"), V::text("Mira")]
    );
    assert_eq!(result.stats.scanned, 4);
    assert_eq!(result.stats.matched, 3);
}
#[test]
fn exact_equality_preserves_nominal_type_validation() {
    let r = registry();
    let q = plan(P::equals(field("active"), V::boolean(true)), field("name"));
    assert_eq!(
        q.execute(&r, &source(&r), QueryLimits::default())
            .unwrap()
            .rows
            .values(),
        &[V::text("Zoe"), V::text("Zoe"), V::text("Mira")]
    );
    let bad = plan(P::equals(field("active"), V::integer(1)), field("name"));
    assert_eq!(
        bad.count(&r, &source(&r), QueryLimits::default()),
        Err(E::TypeMismatch)
    );
}
#[test]
fn text_contains_queries_unicode_without_ascii_assumptions() {
    let r = registry();
    let p = P::contains(field("name"), "Zo");
    assert_eq!(
        plan(p, field("age"))
            .execute(&r, &source(&r), QueryLimits::default())
            .unwrap()
            .rows
            .values(),
        &[V::integer(18), V::integer(24)]
    );
    let p = P::contains(vec![S::field("note"), S::OptionalSome], "हिं");
    assert_eq!(
        plan(p, field("name")).execute(&r, &source(&r), QueryLimits::default()),
        Err(E::InvalidShape)
    );
    // Optional fields require a case guard before a nested payload query.
    let p = P::OptionalPresent {
        path: field("note"),
    };
    assert_eq!(
        plan(p, field("name"))
            .execute(&r, &source(&r), QueryLimits::default())
            .unwrap()
            .rows
            .values(),
        &[V::text("Zoe"), V::text("Zoe"), V::text("Mira")]
    );
}
#[test]
fn boolean_predicates_short_circuit_left_to_right() {
    let r = registry();
    let all = plan(
        P::All(vec![
            P::False,
            P::contains(vec![S::field("note"), S::OptionalSome], "missing"),
        ]),
        field("name"),
    );
    let result = all
        .execute(&r, &source(&r), QueryLimits::default())
        .unwrap();
    assert!(result.rows.is_empty());
    let any = plan(
        P::Any(vec![P::True, P::equals(field("age"), V::integer(999))]),
        field("name"),
    );
    assert_eq!(
        any.count(&r, &source(&r), QueryLimits::default())
            .unwrap()
            .matched,
        4
    );
    // Each scanned row charges one scan and only the predicates actually run.
    assert_eq!(
        any.count(&r, &source(&r), QueryLimits::default())
            .unwrap()
            .work,
        12
    );
}
#[test]
fn not_and_empty_boolean_combinators_are_deterministic() {
    let r = registry();
    let q = plan(
        P::Not(Box::new(P::equals(field("active"), V::boolean(true)))),
        field("name"),
    );
    assert_eq!(
        q.execute(&r, &source(&r), QueryLimits::default())
            .unwrap()
            .rows
            .values(),
        &[V::text("Ada")]
    );
    let q = plan(P::All(vec![]), field("name"));
    assert_eq!(
        q.count(&r, &source(&r), QueryLimits::default())
            .unwrap()
            .matched,
        4
    );
    let q = plan(P::Any(vec![]), field("name"));
    assert_eq!(
        q.count(&r, &source(&r), QueryLimits::default())
            .unwrap()
            .matched,
        0
    );
}
#[test]
fn typed_group_count_is_key_sorted_and_non_destructive() {
    let r = registry();
    let q = plan(P::True, field("name"));
    let (map, stats) = q
        .group_count(&r, &source(&r), QueryLimits::default())
        .unwrap();
    assert_eq!(stats.matched, 4);
    assert_eq!(map.len(), 3);
    assert_eq!(map.get(&DataKey::text("Ada")), Some(&V::integer(1)));
    assert_eq!(map.get(&DataKey::text("Mira")), Some(&V::integer(1)));
    assert_eq!(map.get(&DataKey::text("Zoe")), Some(&V::integer(2)));
    let keys: Vec<_> = map.entries().iter().map(|(key, _)| key.clone()).collect();
    assert_eq!(
        keys,
        vec![
            DataKey::text("Ada"),
            DataKey::text("Mira"),
            DataKey::text("Zoe")
        ]
    );
    assert_eq!(map.key_type(), K::Text);
}
#[test]
fn integer_group_count_can_group_on_an_integer_path() {
    let r = registry();
    let q = plan(P::True, field("age"));
    let (map, _) = q
        .group_count(&r, &source(&r), QueryLimits::default())
        .unwrap();
    assert_eq!(map.key_type(), K::Integer);
    assert_eq!(map.get(&DataKey::Integer(21)), Some(&V::integer(2)));
    assert_eq!(map.get(&DataKey::Integer(18)), Some(&V::integer(1)));
    assert_eq!(map.get(&DataKey::Integer(24)), Some(&V::integer(1)));
}
#[test]
fn group_count_rejects_non_keyable_projection() {
    let r = registry();
    let q = plan(P::True, field("active"));
    assert_eq!(
        q.group_count(&r, &source(&r), QueryLimits::default()),
        Err(E::InvalidKey)
    );
    let q = plan(P::True, vec![]);
    assert_eq!(
        q.group_count(&r, &source(&r), QueryLimits::default()),
        Err(E::InvalidKey)
    );
}
#[test]
fn checked_integer_sum_returns_result_and_stable_statistics() {
    let r = registry();
    let q = plan(P::equals(field("active"), V::boolean(true)), field("age"));
    let (sum, stats) = q
        .sum_integer(&r, &source(&r), QueryLimits::default())
        .unwrap();
    assert_eq!(sum, 63);
    assert_eq!(stats.matched, 3);
    assert_eq!(stats.scanned, 4);
}
#[test]
fn integer_sum_overflow_is_an_error_without_partial_result() {
    let mut r = Registry::default();
    r.register(Definition::record(
        id(1),
        "Value",
        vec![Field::new("n", T::integer())],
    ))
    .unwrap();
    let src = r
        .new_list(
            T::Record(id(1)),
            vec![
                V::Record(r.new_record(id(1), vec![V::integer(i64::MAX)]).unwrap()),
                V::Record(r.new_record(id(1), vec![V::integer(1)]).unwrap()),
            ],
        )
        .unwrap();
    let q = plan(P::True, field("n"));
    assert_eq!(
        q.sum_integer(&r, &src, QueryLimits::default()),
        Err(E::ArithmeticOverflow)
    );
    assert_eq!(src.len(), 2);
}
#[test]
fn empty_result_still_has_exact_projected_type() {
    let r = registry();
    let q = plan(P::False, field("age"));
    let output = q.execute(&r, &source(&r), QueryLimits::default()).unwrap();
    assert_eq!(output.rows.len(), 0);
    assert_eq!(output.rows.element_type(), &T::integer());
    assert_eq!(output.stats.scanned, 4);
    assert_eq!(output.stats.matched, 0);
}
#[test]
fn query_limit_stops_after_selected_matches_not_scanned_rows() {
    let r = registry();
    let mut q = plan(P::equals(field("active"), V::boolean(true)), field("name"));
    q.limit = Some(2);
    let output = q.execute(&r, &source(&r), QueryLimits::default()).unwrap();
    assert_eq!(output.rows.values(), &[V::text("Zoe"), V::text("Zoe")]);
    assert_eq!(output.stats.matched, 2);
    assert_eq!(output.stats.scanned, 3);
    q.limit = Some(0);
    let output = q.execute(&r, &source(&r), QueryLimits::default()).unwrap();
    assert_eq!(output.stats.scanned, 0);
}
#[test]
fn invalid_range_bounds_are_rejected_before_evaluation() {
    let r = registry();
    let q = plan(P::range(field("age"), Some(30), Some(20)), field("name"));
    assert_eq!(
        q.count(&r, &source(&r), QueryLimits::default()),
        Err(E::InvalidShape)
    );
    let q = plan(P::range(field("name"), Some(1), Some(5)), field("age"));
    assert_eq!(
        q.count(&r, &source(&r), QueryLimits::default()),
        Err(E::TypeMismatch)
    );
}
#[test]
fn invalid_projection_is_checked_even_when_no_rows_would_match() {
    let r = registry();
    let q = plan(P::False, field("unknown"));
    assert_eq!(
        q.count(&r, &source(&r), QueryLimits::default()),
        Err(E::UnknownField("unknown".into()))
    );
}
#[test]
fn work_budget_counts_only_evaluated_predicate_branches() {
    let r = registry();
    let q = plan(P::Any(vec![P::True, P::False]), field("age"));
    let limits = QueryLimits {
        max_steps: 7,
        ..QueryLimits::default()
    };
    assert_eq!(
        q.count(&r, &source(&r), limits),
        Err(E::Limit {
            resource: R::QuerySteps,
            limit: 7
        })
    );
    let limits = QueryLimits {
        max_steps: 12,
        ..QueryLimits::default()
    };
    assert_eq!(q.count(&r, &source(&r), limits).unwrap().work, 12);
}
#[test]
fn maximum_scanned_rows_is_checked_at_first_over_limit_row() {
    let r = registry();
    let q = plan(P::False, field("name"));
    assert_eq!(
        q.count(
            &r,
            &source(&r),
            QueryLimits {
                max_scanned_rows: 2,
                ..QueryLimits::default()
            }
        ),
        Err(E::Limit {
            resource: R::QueryScan,
            limit: 2
        })
    );
    assert_eq!(
        q.count(
            &r,
            &source(&r),
            QueryLimits {
                max_scanned_rows: 4,
                ..QueryLimits::default()
            }
        )
        .unwrap()
        .scanned,
        4
    );
}
#[test]
fn output_quota_enforced_without_changing_original_list() {
    let r = registry();
    let src = source(&r);
    let q = plan(P::True, field("name"));
    assert_eq!(
        q.execute(
            &r,
            &src,
            QueryLimits {
                max_returned_rows: 2,
                ..QueryLimits::default()
            }
        ),
        Err(E::Limit {
            resource: R::QueryOutput,
            limit: 2
        })
    );
    assert_eq!(src.len(), 4);
}
#[test]
fn predicate_depth_and_node_count_are_bounded() {
    let r = registry();
    let q = plan(
        P::Not(Box::new(P::Not(Box::new(P::Not(Box::new(P::True)))))),
        field("age"),
    );
    assert_eq!(
        q.count(
            &r,
            &source(&r),
            QueryLimits {
                max_predicate_depth: 3,
                ..QueryLimits::default()
            }
        ),
        Err(E::Limit {
            resource: R::PatternNodes,
            limit: 3
        })
    );
    assert_eq!(
        q.count(
            &r,
            &source(&r),
            QueryLimits {
                max_predicate_nodes: 2,
                ..QueryLimits::default()
            }
        ),
        Err(E::Limit {
            resource: R::PatternNodes,
            limit: 2
        })
    );
}
#[test]
fn variant_case_predicate_matches_correct_nominal_branch() {
    let r = registry();
    let a = V::Variant(r.new_variant(id(2), "Waiting", None).unwrap());
    let b = V::Variant(r.new_variant(id(2), "Ready", Some(V::integer(5))).unwrap());
    let src = r
        .new_list(T::Variant(id(2)), vec![a, b.clone(), b])
        .unwrap();
    let q = plan(
        P::VariantCase {
            path: vec![],
            case: "Ready".into(),
        },
        vec![],
    );
    assert_eq!(
        q.count(&r, &src, QueryLimits::default()).unwrap().matched,
        2
    );
    let q = plan(
        P::VariantCase {
            path: vec![],
            case: "Missing".into(),
        },
        vec![],
    );
    assert_eq!(
        q.count(&r, &src, QueryLimits::default()),
        Err(E::UnknownCase("Missing".into()))
    );
}
#[test]
fn boolean_and_numeric_types_remain_distinct_in_query_preflight() {
    let r = registry();
    let q = plan(P::equals(field("age"), V::boolean(true)), field("name"));
    assert_eq!(
        q.count(&r, &source(&r), QueryLimits::default()),
        Err(E::TypeMismatch)
    );
    let q = plan(P::OptionalPresent { path: field("age") }, field("name"));
    assert_eq!(
        q.count(&r, &source(&r), QueryLimits::default()),
        Err(E::TypeMismatch)
    );
}
#[test]
fn query_determinism_is_independent_of_caller_owned_snapshots() {
    let r = registry();
    let src = source(&r);
    let q = plan(P::range(field("age"), Some(18), Some(21)), field("name"));
    let a = q.execute(&r, &src, QueryLimits::default()).unwrap();
    let b = q.execute(&r, &src, QueryLimits::default()).unwrap();
    assert_eq!(a, b);
    assert_eq!(src.len(), 4);
}
#[test]
fn query_counts_match_aggregation_of_key_groups() {
    let r = registry();
    let src = source(&r);
    let q = plan(P::True, field("name"));
    let (map, stats) = q.group_count(&r, &src, QueryLimits::default()).unwrap();
    let count: i64 = map
        .entries()
        .iter()
        .map(|(_, v)| match v {
            V::Source(scarp::value::Value::Integer(i)) => *i,
            _ => panic!("wrong count type"),
        })
        .sum();
    assert_eq!(count, stats.matched as i64);
}
#[test]
fn always_true_default_query_projects_original_rows() {
    let r = registry();
    let src = source(&r);
    let a = QueryPlan::default()
        .execute(&r, &src, QueryLimits::default())
        .unwrap();
    assert_eq!(a.rows, src);
    assert_eq!(a.stats.scanned, 4);
}
