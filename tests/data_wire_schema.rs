//! Exact-schema v2 envelope acceptance and malformed metadata regression tests.
use scarp::data_engine::{
    DataError as E, DataKey, DataResource as R, DataType as T, Datum as V,
    Definition as D, Field, MapKeyType as K, SchemaRegistry as Registry,
    VariantCase as C, WireLimits,
};
use scarp::types::{DefinitionId as Id, PrimitiveType as P, SourceType};
use scarp::value::{ArrayValue, TupleValue, Value};

fn id(n: u32) -> Id {
    Id { module: 70, declaration: n }
}
fn registry() -> Registry {
    let mut r = Registry::default();
    r.register_batch(vec![
        D::record(id(1), "Student", vec![
            Field::new("name", T::string()),
            Field::new("age", T::integer()),
        ]),
        D::variant(id(2), "Reply", vec![
            C::unit("Pending"),
            C::typed("Accepted", T::Record(id(1))),
            C::typed("Failed", T::string()),
        ]),
    ]).unwrap();
    r
}
fn student(r: &Registry, name: &str, age: i64) -> V {
    V::Record(r.new_record(id(1), vec![V::text(name), V::integer(age)]).unwrap())
}
fn roundtrip(r: &Registry, ty: T, value: V) -> Vec<u8> {
    let bytes = r.encode_schema_wire(&ty, &value, WireLimits::default()).unwrap();
    assert_eq!(&bytes[..5], b"SCDB\x02");
    assert_eq!(
        r.decode_schema_wire(&ty, &bytes, WireLimits::default()).unwrap(),
        value
    );
    assert_eq!(
        r.encode_schema_wire(&ty, &value, WireLimits::default()).unwrap(),
        bytes
    );
    bytes
}
#[test]
fn schema_roundtrips_simple_primitives_without_implicit_coercion() {
    let r = registry();
    roundtrip(&r, T::integer(), V::integer(i64::MIN));
    roundtrip(&r, T::boolean(), V::boolean(true));
    roundtrip(&r, T::string(), V::text("हिंदी 😀"));
}
#[test]
fn schema_roundtrips_nominal_records_and_variants() {
    let r = registry();
    roundtrip(&r, T::Record(id(1)), student(&r, "Ada", 21));
    for value in [
        V::Variant(r.new_variant(id(2), "Pending", None).unwrap()),
        V::Variant(r.new_variant(id(2), "Accepted", Some(student(&r, "Ada", 21))).unwrap()),
        V::Variant(r.new_variant(id(2), "Failed", Some(V::text("error"))).unwrap()),
    ] {
        roundtrip(&r, T::Variant(id(2)), value);
    }
}
#[test]
fn nested_result_list_maps_roundtrip_with_exhaustive_type_validation() {
    let r = registry();
    let accepted = V::Variant(
        r.new_variant(id(2), "Accepted", Some(student(&r, "Ada", 21))).unwrap()
    );
    let ty = T::map(K::Integer, T::optional(T::outcome(T::Variant(id(2)), T::string())));
    let value = V::Map(r.new_map(
        K::Integer,
        T::optional(T::outcome(T::Variant(id(2)), T::string())),
        vec![
            (DataKey::Integer(-1), V::none()),
            (DataKey::Integer(2), V::some(V::ok(accepted))),
            (DataKey::Integer(10), V::some(V::err(V::text("retry")))),
        ],
    ).unwrap());
    roundtrip(&r, ty, value);
}
#[test]
fn schema_descriptor_is_stable_across_separate_registry_objects() {
    let a = registry();
    let b = registry();
    let ty = T::Record(id(1));
    let left = a.wire_schema_descriptor(&ty, WireLimits::default()).unwrap();
    let right = b.wire_schema_descriptor(&ty, WireLimits::default()).unwrap();
    assert_eq!(left, right);
    assert!(left.starts_with(b"SCHEMA\x01"));
    assert!(!left.is_empty());
}
#[test]
fn schema_descriptor_is_not_affected_by_definition_registration_order() {
    let a = registry();
    let mut b = Registry::default();
    b.register_batch(vec![
        D::variant(id(2), "Reply", vec![
            C::unit("Pending"),
            C::typed("Accepted", T::Record(id(1))),
            C::typed("Failed", T::string()),
        ]),
        D::record(id(1), "Student", vec![
            Field::new("name", T::string()),
            Field::new("age", T::integer()),
        ]),
    ]).unwrap();
    let ty = T::Variant(id(2));
    assert_eq!(
        a.wire_schema_descriptor(&ty, WireLimits::default()),
        b.wire_schema_descriptor(&ty, WireLimits::default())
    );
    let value = V::Variant(a.new_variant(id(2), "Pending", None).unwrap());
    let bytes = a.encode_schema_wire(&ty, &value, WireLimits::default()).unwrap();
    assert_eq!(b.decode_schema_wire(&ty, &bytes, WireLimits::default()), Ok(value));
}
#[test]
fn changed_field_name_with_identical_binary_layout_is_rejected() {
    let a = registry();
    let mut b = Registry::default();
    b.register_batch(vec![
        D::record(id(1), "Student", vec![
            Field::new("name", T::string()),
            Field::new("years", T::integer()),
        ]),
        D::variant(id(2), "Reply", vec![
            C::unit("Pending"),
            C::typed("Accepted", T::Record(id(1))),
            C::typed("Failed", T::string()),
        ]),
    ]).unwrap();
    let ty = T::Record(id(1));
    let bytes = a.encode_schema_wire(&ty, &student(&a, "Ada", 21), WireLimits::default())
        .unwrap();
    assert_eq!(
        b.decode_schema_wire(&ty, &bytes, WireLimits::default()),
        Err(E::InvalidWire)
    );
}
#[test]
fn changed_nominal_display_name_is_rejected_even_with_same_fields() {
    let a = registry();
    let mut b = Registry::default();
    b.register_batch(vec![
        D::record(id(1), "Person", vec![
            Field::new("name", T::string()),
            Field::new("age", T::integer()),
        ]),
        D::variant(id(2), "Reply", vec![
            C::unit("Pending"),
            C::typed("Accepted", T::Record(id(1))),
            C::typed("Failed", T::string()),
        ]),
    ]).unwrap();
    let ty = T::Record(id(1));
    let bytes = a.encode_schema_wire(&ty, &student(&a, "Ada", 21), WireLimits::default())
        .unwrap();
    assert_eq!(
        b.decode_schema_wire(&ty, &bytes, WireLimits::default()),
        Err(E::InvalidWire)
    );
}
#[test]
fn altered_variant_case_order_is_rejected() {
    let a = registry();
    let mut b = Registry::default();
    b.register_batch(vec![
        D::record(id(1), "Student", vec![
            Field::new("name", T::string()),
            Field::new("age", T::integer()),
        ]),
        D::variant(id(2), "Reply", vec![
            C::unit("Pending"),
            C::typed("Failed", T::string()),
            C::typed("Accepted", T::Record(id(1))),
        ]),
    ]).unwrap();
    let ty = T::Variant(id(2));
    let value = V::Variant(a.new_variant(id(2), "Pending", None).unwrap());
    let bytes = a.encode_schema_wire(&ty, &value, WireLimits::default()).unwrap();
    assert_eq!(
        b.decode_schema_wire(&ty, &bytes, WireLimits::default()),
        Err(E::InvalidWire)
    );
}
#[test]
fn unused_definition_is_part_of_explicit_registry_version() {
    let a = registry();
    let mut b = registry();
    b.register(D::record(id(10), "Extra", vec![])).unwrap();
    let ty = T::integer();
    let bytes = a.encode_schema_wire(&ty, &V::integer(1), WireLimits::default()).unwrap();
    assert_eq!(
        b.decode_schema_wire(&ty, &bytes, WireLimits::default()),
        Err(E::InvalidWire)
    );
}
#[test]
fn distinct_root_type_with_same_registry_is_rejected() {
    let r = registry();
    let bytes = roundtrip(&r, T::integer(), V::integer(1));
    assert_eq!(
        r.decode_schema_wire(&T::boolean(), &bytes, WireLimits::default()),
        Err(E::InvalidWire)
    );
}
#[test]
fn descriptor_includes_source_tuple_and_array_structure() {
    let r = registry();
    let tuple = SourceType::tuple(vec![
        P::Integer.into(), P::String.into(),
    ]).unwrap();
    let tuple_value = V::Source(Value::Tuple(TupleValue::new(vec![
        Value::Integer(2), Value::String("OK".into()),
    ]).unwrap()));
    roundtrip(&r, T::Source(tuple), tuple_value);
    let array = SourceType::array(P::Integer, 2).unwrap();
    let array_value = V::Source(Value::Array(ArrayValue::new(P::Integer, vec![
        Value::Integer(3), Value::Integer(-1),
    ]).unwrap()));
    roundtrip(&r, T::Source(array), array_value);
}
#[test]
fn schema_prefix_rejects_original_v1_without_guessing_version() {
    let r = registry();
    let bytes = r.encode_wire(&T::integer(), &V::integer(1), WireLimits::default()).unwrap();
    assert_eq!(
        r.decode_schema_wire(&T::integer(), &bytes, WireLimits::default()),
        Err(E::InvalidWire)
    );
}
#[test]
fn original_v1_decoder_does_not_accept_v2_envelope() {
    let r = registry();
    let bytes = roundtrip(&r, T::integer(), V::integer(1));
    assert_eq!(
        r.decode_wire(&T::integer(), &bytes, WireLimits::default()),
        Err(E::InvalidWire)
    );
}
#[test]
fn modified_descriptor_byte_fails_closed() {
    let r = registry();
    let mut bytes = roundtrip(&r, T::integer(), V::integer(3));
    bytes[12] ^= 1;
    assert_eq!(
        r.decode_schema_wire(&T::integer(), &bytes, WireLimits::default()),
        Err(E::InvalidWire)
    );
}
#[test]
fn altered_descriptor_length_fails_closed() {
    let r = registry();
    let original = roundtrip(&r, T::integer(), V::integer(3));
    for invalid in [0u32, 1, u32::MAX] {
        let mut bytes = original.clone();
        bytes[5..9].copy_from_slice(&invalid.to_le_bytes());
        assert_eq!(
            r.decode_schema_wire(&T::integer(), &bytes, WireLimits::default()),
            Err(E::InvalidWire)
        );
    }
}
#[test]
fn truncated_v2_input_fails_at_every_position() {
    let r = registry();
    let bytes = roundtrip(&r, T::Record(id(1)), student(&r, "Ada", 21));
    for end in 0..bytes.len() {
        assert_eq!(
            r.decode_schema_wire(&T::Record(id(1)), &bytes[..end], WireLimits::default()),
            Err(E::InvalidWire)
        );
    }
}
#[test]
fn trailing_bytes_are_rejected_inside_inner_payload() {
    let r = registry();
    let mut bytes = roundtrip(&r, T::integer(), V::integer(3));
    bytes.push(42);
    assert_eq!(
        r.decode_schema_wire(&T::integer(), &bytes, WireLimits::default()),
        Err(E::InvalidWire)
    );
}
#[test]
fn byte_limit_covers_header_descriptor_and_payload() {
    let r = registry();
    let ty = T::integer();
    let bytes = roundtrip(&r, ty.clone(), V::integer(1));
    let small = WireLimits {
        max_bytes: bytes.len() - 1,
        ..WireLimits::default()
    };
    assert_eq!(
        r.encode_schema_wire(&ty, &V::integer(1), small),
        Err(E::Limit { resource: R::WireBytes, limit: small.max_bytes })
    );
    assert_eq!(
        r.decode_schema_wire(&ty, &bytes, small),
        Err(E::Limit { resource: R::WireBytes, limit: small.max_bytes })
    );
    assert_eq!(
        r.encode_schema_wire(&ty, &V::integer(1), WireLimits {
            max_bytes: bytes.len(),
            ..WireLimits::default()
        }).unwrap(), bytes
    );
}
#[test]
fn metadata_node_limit_is_not_bypassed_for_simple_payload() {
    let r = registry();
    let ty = T::integer();
    let limits = WireLimits {
        max_nodes: 1,
        ..WireLimits::default()
    };
    assert!(matches!(
        r.encode_schema_wire(&ty, &V::integer(1), limits),
        Err(E::Limit { resource: R::WireNodes, limit: 1 })
    ));
}
#[test]
fn v2_roundtrip_is_repeatable_from_independent_registry_clone() {
    let a = registry();
    let b = a.clone();
    let ty = T::Record(id(1));
    let value = student(&a, "Ada", 21);
    let bytes = roundtrip(&a, ty.clone(), value.clone());
    assert_eq!(b.decode_schema_wire(&ty, &bytes, WireLimits::default()), Ok(value));
}
