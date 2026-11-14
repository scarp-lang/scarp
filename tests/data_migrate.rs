//! Host schema evolution with explicit defaults and no implicit data loss.
use scarp::data_engine::{
    DataError as E, DataKey, DataResource as R, DataType as T, Datum as V, Definition as D, Field,
    MapKeyType as K, MigrationOptions as Options, SchemaRegistry as Registry, VariantCase as C,
};
use scarp::types::DefinitionId as Id;

fn id(n: u32) -> Id {
    Id {
        module: 100,
        declaration: n,
    }
}
fn old_schema() -> Registry {
    let mut r = Registry::default();
    r.register_batch(vec![
        D::record(
            id(1),
            "Student",
            vec![
                Field::new("name", T::string()),
                Field::new("age", T::integer()),
            ],
        ),
        D::variant(
            id(2),
            "Event",
            vec![C::unit("Pending"), C::typed("Done", T::Record(id(1)))],
        ),
    ])
    .unwrap();
    r
}
fn new_schema() -> Registry {
    let mut r = Registry::default();
    r.register_batch(vec![
        D::record(
            id(1),
            "Student",
            vec![
                Field::new("age", T::integer()),
                Field::new("active", T::boolean()),
                Field::new("name", T::string()),
            ],
        ),
        D::variant(
            id(2),
            "Event",
            vec![
                C::typed("Done", T::Record(id(1))),
                C::unit("Pending"),
                C::typed("Failed", T::string()),
            ],
        ),
    ])
    .unwrap();
    r
}
fn options() -> Options {
    Options::default().with_default(id(1), "active", V::boolean(false))
}
fn student(r: &Registry, name: &str, age: i64) -> V {
    V::Record(
        r.new_record(id(1), vec![V::text(name), V::integer(age)])
            .unwrap(),
    )
}
fn newer(r: &Registry, name: &str, age: i64) -> V {
    V::Record(
        r.new_record(
            id(1),
            vec![V::integer(age), V::boolean(false), V::text(name)],
        )
        .unwrap(),
    )
}
#[test]
fn added_field_requires_explicit_default() {
    let old = old_schema();
    let new = new_schema();
    let original = student(&old, "Ada", 21);
    assert_eq!(
        old.migrate_data(
            &new,
            &T::Record(id(1)),
            &T::Record(id(1)),
            &original,
            &Options::default()
        ),
        Err(E::MissingMigrationDefault("active".into()))
    );
}
#[test]
fn reordered_fields_and_addition_preserve_logical_values() {
    let old = old_schema();
    let new = new_schema();
    let source = student(&old, "Ada", 21);
    let result = old
        .migrate_data(
            &new,
            &T::Record(id(1)),
            &T::Record(id(1)),
            &source,
            &options(),
        )
        .unwrap();
    assert_eq!(result.value, newer(&new, "Ada", 21));
    assert_eq!(result.stats.inserted_fields, 1);
    assert_eq!(result.stats.removed_fields, 0);
    assert_eq!(result.stats.visited, 3);
    assert_eq!(source, student(&old, "Ada", 21));
}
#[test]
fn migrated_list_reuses_typed_element_schema() {
    let old = old_schema();
    let new = new_schema();
    let original = V::List(
        old.new_list(
            T::Record(id(1)),
            vec![student(&old, "Ada", 21), student(&old, "Mira", 22)],
        )
        .unwrap(),
    );
    let result = old
        .migrate_data(
            &new,
            &T::list(T::Record(id(1))),
            &T::list(T::Record(id(1))),
            &original,
            &options(),
        )
        .unwrap();
    let expected = V::List(
        new.new_list(
            T::Record(id(1)),
            vec![newer(&new, "Ada", 21), newer(&new, "Mira", 22)],
        )
        .unwrap(),
    );
    assert_eq!(result.value, expected);
    assert_eq!(result.stats.inserted_fields, 2);
}
#[test]
fn empty_typed_list_does_not_require_a_default_value_instance() {
    let old = old_schema();
    let new = new_schema();
    let source = V::List(old.new_list(T::Record(id(1)), vec![]).unwrap());
    let result = old
        .migrate_data(
            &new,
            &T::list(T::Record(id(1))),
            &T::list(T::Record(id(1))),
            &source,
            &Options::default(),
        )
        .unwrap();
    assert_eq!(
        result.value,
        V::List(new.new_list(T::Record(id(1)), vec![]).unwrap())
    );
    assert_eq!(result.stats.inserted_fields, 0);
}
#[test]
fn variant_case_reordering_uses_name_not_old_numeric_index() {
    let old = old_schema();
    let new = new_schema();
    let source = V::Variant(old.new_variant(id(2), "Pending", None).unwrap());
    let result = old
        .migrate_data(
            &new,
            &T::Variant(id(2)),
            &T::Variant(id(2)),
            &source,
            &Options::default(),
        )
        .unwrap();
    assert_eq!(
        result.value,
        V::Variant(new.new_variant(id(2), "Pending", None).unwrap())
    );
}
#[test]
fn variant_payload_migrates_record_fields_recursively() {
    let old = old_schema();
    let new = new_schema();
    let source = V::Variant(
        old.new_variant(id(2), "Done", Some(student(&old, "Ada", 21)))
            .unwrap(),
    );
    let result = old
        .migrate_data(
            &new,
            &T::Variant(id(2)),
            &T::Variant(id(2)),
            &source,
            &options(),
        )
        .unwrap();
    assert_eq!(
        result.value,
        V::Variant(
            new.new_variant(id(2), "Done", Some(newer(&new, "Ada", 21)))
                .unwrap()
        )
    );
    assert_eq!(result.stats.inserted_fields, 1);
}
#[test]
fn map_values_are_migrated_without_changing_key_order() {
    let old = old_schema();
    let new = new_schema();
    let ty = T::map(K::Text, T::Record(id(1)));
    let source = V::Map(
        old.new_map(
            K::Text,
            T::Record(id(1)),
            vec![
                (DataKey::text("z"), student(&old, "Zoe", 24)),
                (DataKey::text("a"), student(&old, "Ada", 21)),
            ],
        )
        .unwrap(),
    );
    let result = old
        .migrate_data(&new, &ty, &ty, &source, &options())
        .unwrap();
    let expected = V::Map(
        new.new_map(
            K::Text,
            T::Record(id(1)),
            vec![
                (DataKey::text("z"), newer(&new, "Zoe", 24)),
                (DataKey::text("a"), newer(&new, "Ada", 21)),
            ],
        )
        .unwrap(),
    );
    assert_eq!(result.value, expected);
    assert_eq!(result.stats.inserted_fields, 2);
}
#[test]
fn optional_and_result_payloads_migrate_their_nested_types() {
    let old = old_schema();
    let new = new_schema();
    let inner = T::Record(id(1));
    let optional = T::optional(inner.clone());
    let source = V::some(student(&old, "Ada", 21));
    assert_eq!(
        old.migrate_data(&new, &optional, &optional, &source, &options())
            .unwrap()
            .value,
        V::some(newer(&new, "Ada", 21))
    );
    let outcome = T::outcome(inner, T::string());
    let source = V::ok(student(&old, "Ada", 21));
    assert_eq!(
        old.migrate_data(&new, &outcome, &outcome, &source, &options())
            .unwrap()
            .value,
        V::ok(newer(&new, "Ada", 21))
    );
    let unchanged = V::err(V::text("failed"));
    assert_eq!(
        old.migrate_data(&new, &outcome, &outcome, &unchanged, &Options::default())
            .unwrap()
            .value,
        unchanged
    );
}
#[test]
fn none_optional_preserves_case_even_with_missing_field_default() {
    let old = old_schema();
    let new = new_schema();
    let ty = T::optional(T::Record(id(1)));
    assert_eq!(
        old.migrate_data(&new, &ty, &ty, &V::none(), &Options::default())
            .unwrap()
            .value,
        V::none()
    );
}
#[test]
fn removal_requires_explicit_opt_in() {
    let old = new_schema();
    let new = old_schema();
    let source = newer(&old, "Ada", 21);
    assert_eq!(
        old.migrate_data(
            &new,
            &T::Record(id(1)),
            &T::Record(id(1)),
            &source,
            &Options::default()
        ),
        Err(E::DroppedField("active".into()))
    );
    let options = Options::default().allow_dropping_fields();
    let result = old
        .migrate_data(
            &new,
            &T::Record(id(1)),
            &T::Record(id(1)),
            &source,
            &options,
        )
        .unwrap();
    assert_eq!(result.value, student(&new, "Ada", 21));
    assert_eq!(result.stats.removed_fields, 1);
}
#[test]
fn default_value_is_typechecked_before_it_can_enter_a_record() {
    let old = old_schema();
    let new = new_schema();
    let invalid = Options::default().with_default(id(1), "active", V::integer(1));
    assert_eq!(invalid.default_count(), 1);
    assert_eq!(invalid.get_default(id(1), "active"), Some(&V::integer(1)));
    assert_eq!(
        old.migrate_data(
            &new,
            &T::Record(id(1)),
            &T::Record(id(1)),
            &student(&old, "Ada", 21),
            &invalid
        ),
        Err(E::TypeMismatch)
    );
}
#[test]
fn changed_source_field_type_is_not_implicitly_cast() {
    let old = old_schema();
    let mut changed = Registry::default();
    changed
        .register(D::record(
            id(1),
            "Student",
            vec![
                Field::new("name", T::string()),
                Field::new("age", T::string()),
            ],
        ))
        .unwrap();
    let source = student(&old, "Ada", 21);
    assert_eq!(
        old.migrate_data(
            &changed,
            &T::Record(id(1)),
            &T::Record(id(1)),
            &source,
            &Options::default()
        ),
        Err(E::TypeMismatch)
    );
}
#[test]
fn removed_variant_case_is_rejected_instead_of_guessing_a_new_case() {
    let old = new_schema();
    let new = old_schema();
    let source = V::Variant(
        old.new_variant(id(2), "Failed", Some(V::text("bad")))
            .unwrap(),
    );
    assert_eq!(
        old.migrate_data(
            &new,
            &T::Variant(id(2)),
            &T::Variant(id(2)),
            &source,
            &Options::default()
        ),
        Err(E::UnknownCase("Failed".into()))
    );
}
#[test]
fn wrong_nominal_id_is_not_equated_with_same_layout() {
    let old = old_schema();
    let mut new = Registry::default();
    new.register(D::record(
        id(3),
        "Student",
        vec![
            Field::new("name", T::string()),
            Field::new("age", T::integer()),
        ],
    ))
    .unwrap();
    let source = student(&old, "Ada", 21);
    assert_eq!(
        old.migrate_data(
            &new,
            &T::Record(id(1)),
            &T::Record(id(3)),
            &source,
            &Options::default()
        ),
        Err(E::TypeMismatch)
    );
}
#[test]
fn strict_node_budget_applies_to_migration_graph() {
    let old = old_schema();
    let new = new_schema();
    assert_eq!(
        old.migrate_data(
            &new,
            &T::Record(id(1)),
            &T::Record(id(1)),
            &student(&old, "Ada", 21),
            &options().with_node_limit(2)
        ),
        Err(E::Limit {
            resource: R::MigrationNodes,
            limit: 2
        })
    );
}
#[test]
fn strict_depth_budget_covers_nested_lists_and_records() {
    let old = old_schema();
    let new = new_schema();
    let ty = T::list(T::Record(id(1)));
    let source = V::List(
        old.new_list(T::Record(id(1)), vec![student(&old, "Ada", 21)])
            .unwrap(),
    );
    assert_eq!(
        old.migrate_data(&new, &ty, &ty, &source, &options().with_depth_limit(2)),
        Err(E::Limit {
            resource: R::MigrationDepth,
            limit: 2
        })
    );
}
#[test]
fn no_schema_change_yields_equal_value_with_zero_insertions_and_removals() {
    let old = old_schema();
    let new = old.clone();
    let source = student(&old, "Ada", 21);
    let result = old
        .migrate_data(
            &new,
            &T::Record(id(1)),
            &T::Record(id(1)),
            &source,
            &Options::default(),
        )
        .unwrap();
    assert_eq!(result.value, source);
    assert_eq!(result.stats.inserted_fields, 0);
    assert_eq!(result.stats.removed_fields, 0);
}
#[test]
fn invalid_source_value_is_rejected_before_defaults_are_checked() {
    let old = old_schema();
    let new = new_schema();
    assert_eq!(
        old.migrate_data(
            &new,
            &T::Record(id(1)),
            &T::Record(id(1)),
            &V::integer(99),
            &Options::default()
        ),
        Err(E::TypeMismatch)
    );
}
#[test]
fn nested_migration_is_reproducible_across_independent_registries() {
    let old = old_schema();
    let new = new_schema();
    let value = student(&old, "Ada", 21);
    let one = old
        .migrate_data(
            &new,
            &T::Record(id(1)),
            &T::Record(id(1)),
            &value,
            &options(),
        )
        .unwrap();
    let two = old
        .clone()
        .migrate_data(
            &new.clone(),
            &T::Record(id(1)),
            &T::Record(id(1)),
            &value,
            &options(),
        )
        .unwrap();
    assert_eq!(one, two);
}
