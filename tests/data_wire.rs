//! Host-only typed wire codec conformance and malformed-byte regressions.
use scarp::data_engine::{
    DataError as E, DataKey, DataResource as R, DataType as T, Datum as V,
    Definition, Field, MapKeyType as K, SchemaRegistry as Registry,
    VariantCase, WireLimits,
};
use scarp::types::{DefinitionId as Id, PrimitiveType as P, SourceType};
use scarp::value::{ArrayValue,FiniteFloat,TupleValue,Value};

fn id(n:u32)->Id{Id{module:55,declaration:n}}
fn registry()->Registry{
    let mut r=Registry::default();
    r.register_batch(vec![
        Definition::record(id(1),"Person",vec![
            Field::new("name",T::string()),
            Field::new("age",T::integer()),
        ]),
        Definition::variant(id(2),"Reply",vec![
            VariantCase::unit("Pending"),
            VariantCase::typed("Accepted",T::Record(id(1))),
            VariantCase::typed("Error",T::string()),
        ]),
    ]).unwrap();
    r
}
fn person(r:&Registry,name:&str,age:i64)->V{
    V::Record(r.new_record(id(1),vec![V::text(name),V::integer(age)]).unwrap())
}
fn roundtrip(r:&Registry,t:T,v:V)->Vec<u8>{
    let raw=r.encode_wire(&t,&v,WireLimits::default()).unwrap();
    assert_eq!(&raw[..5],b"SCDE\x01");
    let decoded=r.decode_wire(&t,&raw,WireLimits::default()).unwrap();
    assert_eq!(decoded,v);
    assert_eq!(r.encode_wire(&t,&decoded,WireLimits::default()).unwrap(),raw);
    raw
}
#[test]
fn integer_boundary_values_roundtrip_byte_identically(){
    let r=registry();
    for i in [i64::MIN,-1,0,1,i64::MAX]{
        let raw=roundtrip(&r,T::integer(),V::integer(i));
        assert_eq!(&raw[5..],&i.to_le_bytes());
    }
}
#[test]
fn bool_encoding_is_exact_one_bit_semantically(){
    let r=registry();
    assert_eq!(roundtrip(&r,T::boolean(),V::boolean(false)),b"SCDE\x01\x00");
    assert_eq!(roundtrip(&r,T::boolean(),V::boolean(true)),b"SCDE\x01\x01");
}
#[test]
fn text_is_raw_utf8_and_length_delimited(){
    let r=registry();
    for text in ["","hello","नमस्ते","🧮","e\u{301}"]{
        let raw=roundtrip(&r,T::string(),V::text(text));
        assert_eq!(u32::from_le_bytes(raw[5..9].try_into().unwrap()) as usize,text.len());
        assert_eq!(&raw[9..],text.as_bytes());
    }
}
#[test]
fn binary64_is_finite_and_preserves_negative_zero(){
    let r=registry();
    let ty=T::Source(P::Float.into());
    for f in [-0.0,0.0,1.5,-42.25,f64::MAX,f64::MIN_POSITIVE] {
        let raw=roundtrip(&r,ty.clone(),V::Source(Value::Float(FiniteFloat::new(f).unwrap())));
        assert_eq!(&raw[5..],&f.to_bits().to_le_bytes());
    }
}
#[test]
fn records_have_schema_order_not_named_map_order(){
    let r=registry();
    let a=person(&r,"Ada",36);
    let raw=roundtrip(&r,T::Record(id(1)),a);
    assert!(raw.windows(3).any(|w|w==b"Ada"));
    assert!(raw.len()>5+3+8);
}
#[test]
fn unit_variant_has_only_case_index_and_no_payload(){
    let r=registry();
    let p=V::Variant(r.new_variant(id(2),"Pending",None).unwrap());
    assert_eq!(roundtrip(&r,T::Variant(id(2)),p),b"SCDE\x01\x00\x00\x00\x00");
}
#[test]
fn variants_with_payload_roundtrip(){
    let r=registry();
    let p=V::Variant(r.new_variant(id(2),"Accepted",Some(person(&r,"Ada",21))).unwrap());
    let raw=roundtrip(&r,T::Variant(id(2)),p);
    assert_eq!(&raw[5..9],&1u32.to_le_bytes());
    let e=V::Variant(r.new_variant(id(2),"Error",Some(V::text("bad"))).unwrap());
    let raw=roundtrip(&r,T::Variant(id(2)),e);
    assert_eq!(&raw[5..9],&2u32.to_le_bytes());
}
#[test]
fn empty_and_nonempty_lists_roundtrip(){
    let r=registry();
    let empty=V::List(r.new_list(T::integer(),vec![]).unwrap());
    assert_eq!(roundtrip(&r,T::list(T::integer()),empty),b"SCDE\x01\x00\x00\x00\x00");
    let filled=V::List(r.new_list(T::integer(),vec![
        V::integer(-5),V::integer(9),V::integer(2),
    ]).unwrap());
    let raw=roundtrip(&r,T::list(T::integer()),filled);
    assert_eq!(u32::from_le_bytes(raw[5..9].try_into().unwrap()),3);
}
#[test]
fn sorted_maps_roundtrip_and_preserve_exact_key_order(){
    let r=registry();
    let map=V::Map(r.new_map(K::Text,T::integer(),vec![
        (DataKey::text("z"),V::integer(2)),
        (DataKey::text("a"),V::integer(1)),
    ]).unwrap());
    let raw=roundtrip(&r,T::map(K::Text,T::integer()),map.clone());
    let decoded=r.decode_wire(&T::map(K::Text,T::integer()),&raw,WireLimits::default()).unwrap();
    assert_eq!(decoded,map);
}
#[test]
fn integer_maps_encode_keys_before_values(){
    let r=registry();
    let t=T::map(K::Integer,T::boolean());
    let map=V::Map(r.new_map(K::Integer,T::boolean(),vec![
        (DataKey::Integer(9),V::boolean(true)),
        (DataKey::Integer(-4),V::boolean(false)),
    ]).unwrap());
    let raw=roundtrip(&r,t,map);
    assert_eq!(&raw[9..17],&(-4i64).to_le_bytes());
    assert_eq!(raw[17],0);
    assert_eq!(&raw[18..26],&(9i64).to_le_bytes());
    assert_eq!(raw[26],1);
}
#[test]
fn option_and_outcome_cases_are_distinct(){
    let r=registry();
    assert_eq!(roundtrip(&r,T::optional(T::integer()),V::none()),b"SCDE\x01\x00");
    assert_eq!(roundtrip(&r,T::optional(T::integer()),V::some(V::integer(9)))[5],1);
    assert_eq!(roundtrip(&r,T::outcome(T::integer(),T::string()),V::ok(V::integer(1)))[5],0);
    assert_eq!(roundtrip(&r,T::outcome(T::integer(),T::string()),V::err(V::text("bad")))[5],1);
}
#[test]
fn source_tuples_are_decoded_with_exact_shape(){
    let r=registry();
    let ty=SourceType::tuple(vec![P::String.into(),P::Integer.into()]).unwrap();
    let value=Value::Tuple(TupleValue::new(vec![
        Value::String("hello".into()),Value::Integer(42),
    ]).unwrap());
    roundtrip(&r,T::Source(ty),V::Source(value));
}
#[test]
fn nested_fixed_arrays_support_zero_length(){
    let r=registry();
    let kind=SourceType::array(P::Integer,0).unwrap();
    let value=Value::Array(ArrayValue::new(P::Integer,vec![]).unwrap());
    roundtrip(&r,T::Source(kind),V::Source(value));
    let kind=SourceType::array(P::String,2).unwrap();
    let value=Value::Array(ArrayValue::new(P::String,vec![
        Value::String("a".into()),Value::String("b".into()),
    ]).unwrap());
    roundtrip(&r,T::Source(kind),V::Source(value));
}
#[test]
fn tuple_array_inside_nominal_record_is_supported(){
    let mut r=registry();
    let kind=SourceType::array(P::Integer,2).unwrap();
    r.register(Definition::record(id(3),"Coordinates",vec![
        Field::new("numbers",T::Source(kind)),
    ])).unwrap();
    let value=V::Record(r.new_record(id(3),vec![
        V::Source(Value::Array(ArrayValue::new(P::Integer,vec![
            Value::Integer(-1),Value::Integer(7),
        ]).unwrap())),
    ]).unwrap());
    roundtrip(&r,T::Record(id(3)),value);
}
#[test]
fn nested_list_map_result_record_roundtrips(){
    let r=registry();
    let accepted=V::Variant(r.new_variant(id(2),"Accepted",
        Some(person(&r,"Ada",36))).unwrap());
    let list=V::List(r.new_list(T::Variant(id(2)),vec![
        accepted,V::Variant(r.new_variant(id(2),"Pending",None).unwrap()),
    ]).unwrap());
    let t=T::map(K::Text,T::list(T::Variant(id(2))));
    let map=V::Map(r.new_map(K::Text,T::list(T::Variant(id(2))),vec![
        (DataKey::text("events"),list),
    ]).unwrap());
    roundtrip(&r,t,map);
}
#[test]
fn malformed_magic_and_version_are_rejected(){
    let r=registry();
    for data in [b"".as_slice(),b"SCDE".as_slice(),b"SCDE\x02\x01".as_slice(),
        b"AAAAA\x01".as_slice()] {
        assert_eq!(r.decode_wire(&T::boolean(),data,WireLimits::default()),
            Err(E::InvalidWire));
    }
}
#[test]
fn truncated_wire_values_fail_at_every_cut_position(){
    let r=registry();
    let t=T::Record(id(1));
    let raw=roundtrip(&r,t.clone(),person(&r,"Ada",36));
    for cut in 0..raw.len(){
        assert_eq!(r.decode_wire(&t,&raw[..cut],WireLimits::default()),
            Err(E::InvalidWire));
    }
}
#[test]
fn trailing_bytes_are_forbidden(){
    let r=registry();
    let mut raw=roundtrip(&r,T::integer(),V::integer(5));
    raw.extend_from_slice(&[0,0]);
    assert_eq!(r.decode_wire(&T::integer(),&raw,WireLimits::default()),
        Err(E::InvalidWire));
}
#[test]
fn invalid_bool_codes_are_rejected(){
    let r=registry();
    for code in [2,3,254,255] {
        let mut raw=b"SCDE\x01".to_vec();
        raw.push(code);
        assert_eq!(r.decode_wire(&T::boolean(),&raw,WireLimits::default()),
            Err(E::InvalidWire));
    }
}
#[test]
fn nonfinite_float_payloads_are_rejected(){
    let r=registry();
    let t=T::Source(P::Float.into());
    for bits in [f64::NAN.to_bits(),f64::INFINITY.to_bits(),f64::NEG_INFINITY.to_bits()] {
        let mut raw=b"SCDE\x01".to_vec();
        raw.extend_from_slice(&bits.to_le_bytes());
        assert_eq!(r.decode_wire(&t,&raw,WireLimits::default()),Err(E::InvalidWire));
    }
}
#[test]
fn malformed_utf8_is_rejected(){
    let r=registry();
    let mut raw=b"SCDE\x01".to_vec();
    raw.extend_from_slice(&2u32.to_le_bytes());
    raw.extend_from_slice(&[0xC3,0x28]);
    assert_eq!(r.decode_wire(&T::string(),&raw,WireLimits::default()),
        Err(E::InvalidWire));
}
#[test]
fn too_large_declared_string_is_rejected_without_allocating(){
    let r=registry();
    let mut raw=b"SCDE\x01".to_vec();
    raw.extend_from_slice(&u32::MAX.to_le_bytes());
    assert!(r.decode_wire(&T::string(),&raw,WireLimits::default()).is_err());
}
#[test]
fn duplicate_or_unsorted_serialized_map_keys_fail_closed(){
    let r=registry();
    let raw=roundtrip(&r,T::map(K::Integer,T::boolean()),
        V::Map(r.new_map(K::Integer,T::boolean(),vec![
            (DataKey::Integer(1),V::boolean(false)),
            (DataKey::Integer(2),V::boolean(true)),
        ]).unwrap()));
    let mut duplicate=raw.clone();
    duplicate[18..26].copy_from_slice(&1i64.to_le_bytes());
    assert_eq!(r.decode_wire(&T::map(K::Integer,T::boolean()),&duplicate,
        WireLimits::default()),Err(E::InvalidWire));
    let mut reversed=raw;
    reversed[9..17].copy_from_slice(&3i64.to_le_bytes());
    assert_eq!(r.decode_wire(&T::map(K::Integer,T::boolean()),&reversed,
        WireLimits::default()),Err(E::InvalidWire));
}
#[test]
fn oversized_declared_collection_is_rejected_before_allocation(){
    let r=registry();
    let mut raw=b"SCDE\x01".to_vec();
    raw.extend_from_slice(&u32::MAX.to_le_bytes());
    assert!(r.decode_wire(&T::list(T::integer()),&raw,WireLimits::default()).is_err());
}
#[test]
fn wire_exact_bytes_limit_covers_header_and_payload(){
    let r=registry();
    let value=V::integer(5);
    assert_eq!(r.encode_wire(&T::integer(),&value,WireLimits{
        max_bytes:12,..WireLimits::default()
    }),Err(E::Limit{resource:R::WireBytes,limit:12}));
    assert!(r.encode_wire(&T::integer(),&value,WireLimits{
        max_bytes:13,..WireLimits::default()
    }).is_ok());
}
#[test]
fn decode_fails_preflight_on_oversized_raw_message(){
    let r=registry();
    let raw=roundtrip(&r,T::integer(),V::integer(1));
    assert_eq!(r.decode_wire(&T::integer(),&raw,WireLimits{
        max_bytes:12,..WireLimits::default()
    }),Err(E::Limit{resource:R::WireBytes,limit:12}));
}
#[test]
fn logical_wire_nodes_are_separately_bounded(){
    let r=registry();
    let value=V::some(V::integer(1));
    let ty=T::optional(T::integer());
    assert_eq!(r.encode_wire(&ty,&value,WireLimits{
        max_nodes:2,..WireLimits::default()
    }),Err(E::Limit{resource:R::WireNodes,limit:2}));
    assert!(r.encode_wire(&ty,&value,WireLimits{
        max_nodes:3,..WireLimits::default()
    }).is_ok());
}
#[test]
fn recursion_depth_is_bounded_before_a_deep_decode(){
    let r=registry();
    let ty=T::optional(T::optional(T::integer()));
    let v=V::some(V::some(V::integer(1)));
    let raw=roundtrip(&r,ty.clone(),v);
    assert_eq!(r.decode_wire(&ty,&raw,WireLimits{
        max_depth:2,..WireLimits::default()
    }),Err(E::Limit{resource:R::WireDepth,limit:2}));
}
#[test]
fn unsupported_host_numeric_payloads_are_not_silently_cast(){
    let r=registry();
    let kind=T::Source(P::Vector.into());
    let value=V::Source(Value::Vector(scarp::value::NumericVector::new(&[1.0,2.0]).unwrap()));
    assert_eq!(r.encode_wire(&kind,&value,WireLimits::default()),
        Err(E::UnsupportedWireType));
}
#[test]
fn altered_schema_cannot_silently_reinterpret_record_type(){
    let r=registry();
    let raw=roundtrip(&r,T::Record(id(1)),person(&r,"Ada",36));
    let mut other=Registry::default();
    other.register(Definition::record(id(1),"Person",vec![
        Field::new("age",T::integer()),Field::new("name",T::string()),
    ])).unwrap();
    assert!(other.decode_wire(&T::Record(id(1)),&raw,WireLimits::default()).is_err());
}
#[test]
fn value_preflight_does_not_encode_wrong_nominal_types(){
    let r=registry();
    assert_eq!(r.encode_wire(&T::Record(id(1)),&V::integer(7),
        WireLimits::default()),Err(E::TypeMismatch));
}
