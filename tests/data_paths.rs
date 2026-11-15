use scarp::data_engine::{
    DataError as E, DataKey, DataType as T, Datum as V, Definition, Field, MapKeyType as K,
    PatchOperation as Op, PathSegment as S, SchemaRegistry as Registry, VariantCase,
};
use scarp::types::DefinitionId as Id;

fn id(n: u32) -> Id {
    Id {
        module: 21,
        declaration: n,
    }
}
fn schema() -> Registry {
    let mut r = Registry::default();
    r.register(Definition::record(
        id(1),
        "Person",
        vec![
            Field::new("name", T::string()),
            Field::new("age", T::integer()),
        ],
    ))
    .unwrap();
    r.register(Definition::variant(
        id(2),
        "Decision",
        vec![
            VariantCase::unit("Pending"),
            VariantCase::typed("Accepted", T::Record(id(1))),
            VariantCase::typed("Rejected", T::string()),
        ],
    ))
    .unwrap();
    r
}
fn person(r: &Registry, name: &str, age: i64) -> V {
    V::Record(
        r.new_record(id(1), vec![V::text(name), V::integer(age)])
            .unwrap(),
    )
}
fn nested(r: &Registry) -> (T, V) {
    let accepted = V::Variant(
        r.new_variant(id(2), "Accepted", Some(person(r, "Ada", 36)))
            .unwrap(),
    );
    let all = V::List(r.new_list(T::Variant(id(2)), vec![accepted]).unwrap());
    let top = V::Map(
        r.new_map(
            K::Text,
            T::list(T::Variant(id(2))),
            vec![(DataKey::text("group"), all)],
        )
        .unwrap(),
    );
    (T::map(K::Text, T::list(T::Variant(id(2)))), top)
}
fn age_path() -> Vec<S> {
    vec![
        S::MapKey(DataKey::text("group")),
        S::ListIndex(0),
        S::case("Accepted"),
        S::field("age"),
    ]
}
#[test]
fn path_type_infers_every_nested_step() {
    let r = schema();
    let (ty, _) = nested(&r);
    let prefix = vec![S::MapKey(DataKey::text("group"))];
    assert_eq!(
        r.path_type(&ty, &prefix).unwrap(),
        T::list(T::Variant(id(2)))
    );
    assert_eq!(r.path_type(&ty, &age_path()).unwrap(), T::integer());
    assert_eq!(r.path_type(&ty, &[]).unwrap(), ty);
}
#[test]
fn nested_select_uses_strict_runtime_case_and_index() {
    let r = schema();
    let (ty, root) = nested(&r);
    assert_eq!(r.select_path(&ty, &root, &age_path()), Ok(&V::integer(36)));
    let wrong_index = vec![S::MapKey(DataKey::text("group")), S::ListIndex(1)];
    assert_eq!(
        r.select_path(&ty, &root, &wrong_index),
        Err(E::InvalidShape)
    );
    let wrong_key = vec![S::MapKey(DataKey::text("missing"))];
    assert_eq!(r.select_path(&ty, &root, &wrong_key), Err(E::InvalidShape));
    let wrong_case = vec![
        S::MapKey(DataKey::text("group")),
        S::ListIndex(0),
        S::case("Rejected"),
    ];
    assert_eq!(r.select_path(&ty, &root, &wrong_case), Err(E::InvalidShape));
}
#[test]
fn replacing_deep_leaf_preserves_old_map_list_variant_record() {
    let r = schema();
    let (ty, original) = nested(&r);
    let updated = r
        .replace_path(&ty, &original, &age_path(), V::integer(37))
        .unwrap();
    assert_eq!(
        r.select_path(&ty, &original, &age_path()),
        Ok(&V::integer(36))
    );
    assert_eq!(
        r.select_path(&ty, &updated, &age_path()),
        Ok(&V::integer(37))
    );
    assert_ne!(updated, original);
    assert_eq!(
        r.replace_path(&ty, &original, &age_path(), V::text("wrong")),
        Err(E::TypeMismatch)
    );
    assert_eq!(
        r.select_path(&ty, &original, &age_path()),
        Ok(&V::integer(36))
    );
}
#[test]
fn static_path_type_rejects_unknown_fields_and_wrong_segment_type() {
    let r = schema();
    let (ty, _) = nested(&r);
    let mut path = age_path();
    path.pop();
    path.push(S::field("missing"));
    assert_eq!(
        r.path_type(&ty, &path),
        Err(E::UnknownField("missing".into()))
    );
    assert_eq!(r.path_type(&ty, &[S::ListIndex(0)]), Err(E::TypeMismatch));
    assert_eq!(
        r.path_type(&ty, &[S::MapKey(DataKey::Integer(0))]),
        Err(E::TypeMismatch)
    );
    assert_eq!(r.path_type(&ty, &[S::field("age")]), Err(E::TypeMismatch));
}
#[test]
fn replacing_entire_root_requires_original_root_type() {
    let r = schema();
    let (ty, original) = nested(&r);
    let replacement = original.clone();
    assert_eq!(
        r.replace_path(&ty, &original, &[], replacement.clone()),
        Ok(replacement)
    );
    assert_eq!(
        r.replace_path(&ty, &original, &[], V::integer(1)),
        Err(E::TypeMismatch)
    );
    assert_eq!(r.select_path(&ty, &original, &[]), Ok(&original));
}
#[test]
fn optional_and_outcome_paths_keep_cases_distinct() {
    let r = schema();
    let opt = T::optional(T::integer());
    let some = V::some(V::integer(9));
    assert_eq!(
        r.select_path(&opt, &some, &[S::OptionalSome]),
        Ok(&V::integer(9))
    );
    assert_eq!(
        r.select_path(&opt, &V::none(), &[S::OptionalSome]),
        Err(E::InvalidShape)
    );
    let replaced = r
        .replace_path(&opt, &some, &[S::OptionalSome], V::integer(10))
        .unwrap();
    assert_eq!(replaced, V::some(V::integer(10)));
    assert_eq!(some, V::some(V::integer(9)));
    let outcome = T::outcome(T::integer(), T::string());
    let ok = V::ok(V::integer(1));
    assert_eq!(
        r.select_path(&outcome, &ok, &[S::OutcomeOk]),
        Ok(&V::integer(1))
    );
    assert_eq!(
        r.select_path(&outcome, &ok, &[S::OutcomeErr]),
        Err(E::InvalidShape)
    );
    let err = V::err(V::text("stop"));
    let changed = r
        .replace_path(&outcome, &err, &[S::OutcomeErr], V::text("retry"))
        .unwrap();
    assert_eq!(changed, V::err(V::text("retry")));
    assert_eq!(err, V::err(V::text("stop")));
}
#[test]
fn patch_can_insert_remove_and_push_typed_list() {
    let r = schema();
    let ty = T::list(T::integer());
    let original = V::List(r.new_list(T::integer(), vec![V::integer(1)]).unwrap());
    let patched = r
        .apply_patch(
            &ty,
            &original,
            &[
                Op::ListPush {
                    path: vec![],
                    value: V::integer(3),
                },
                Op::ListInsert {
                    path: vec![],
                    index: 1,
                    value: V::integer(2),
                },
                Op::ListRemove {
                    path: vec![],
                    index: 0,
                },
            ],
        )
        .unwrap();
    let want = V::List(
        r.new_list(T::integer(), vec![V::integer(2), V::integer(3)])
            .unwrap(),
    );
    assert_eq!(patched, want);
    assert_eq!(
        r.select_path(&ty, &original, &[S::ListIndex(0)]),
        Ok(&V::integer(1))
    );
    assert_eq!(
        r.select_path(&ty, &original, &[S::ListIndex(1)]),
        Err(E::InvalidShape)
    );
}
#[test]
fn patch_can_put_delete_and_update_maps_in_order() {
    let r = schema();
    let ty = T::map(K::Text, T::integer());
    let original = V::Map(r.new_map(K::Text, T::integer(), vec![]).unwrap());
    let updated = r
        .apply_patch(
            &ty,
            &original,
            &[
                Op::MapPut {
                    path: vec![],
                    key: DataKey::text("second"),
                    value: V::integer(2),
                },
                Op::MapPut {
                    path: vec![],
                    key: DataKey::text("first"),
                    value: V::integer(1),
                },
                Op::MapPut {
                    path: vec![],
                    key: DataKey::text("second"),
                    value: V::integer(3),
                },
                Op::MapDelete {
                    path: vec![],
                    key: DataKey::text("first"),
                },
            ],
        )
        .unwrap();
    assert_eq!(
        r.select_path(&ty, &updated, &[S::MapKey(DataKey::text("second"))]),
        Ok(&V::integer(3))
    );
    assert_eq!(
        r.select_path(&ty, &updated, &[S::MapKey(DataKey::text("first"))]),
        Err(E::InvalidShape)
    );
    assert_eq!(
        original,
        V::Map(r.new_map(K::Text, T::integer(), vec![]).unwrap())
    );
}
#[test]
fn patch_is_all_or_nothing_if_operation_after_success_fails() {
    let r = schema();
    let ty = T::list(T::integer());
    let original = V::List(r.new_list(T::integer(), vec![V::integer(1)]).unwrap());
    let result = r.apply_patch(
        &ty,
        &original,
        &[
            Op::ListPush {
                path: vec![],
                value: V::integer(2),
            },
            Op::ListPush {
                path: vec![],
                value: V::text("mistyped"),
            },
        ],
    );
    assert_eq!(result, Err(E::TypeMismatch));
    assert_eq!(
        r.select_path(&ty, &original, &[S::ListIndex(0)]),
        Ok(&V::integer(1))
    );
}
#[test]
fn path_on_wrong_source_type_rejects_before_value_selection() {
    let r = schema();
    let (ty, datum) = nested(&r);
    assert_eq!(
        r.select_path(&T::integer(), &datum, &[]),
        Err(E::TypeMismatch)
    );
    assert_eq!(
        r.replace_path(&T::integer(), &datum, &[], V::integer(0)),
        Err(E::TypeMismatch)
    );
    assert!(r.select_path(&ty, &datum, &age_path()).is_ok());
}
#[test]
fn patch_paths_are_not_reinterpreted_as_insertions() {
    let r = schema();
    let (ty, root) = nested(&r);
    let nonexistent = vec![S::MapKey(DataKey::text("missing"))];
    assert_eq!(
        r.replace_path(&ty, &root, &nonexistent, V::integer(1)),
        Err(E::TypeMismatch)
    );
    let empty = r.apply_patch(&ty, &root, &[]).unwrap();
    assert_eq!(empty, root);
}
