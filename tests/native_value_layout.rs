use scarp::{
    native_object::{
        plan_x86_64_value_layout as plan, NativeLayoutError as Error, NativeLayoutShape as Shape,
        NativeValueLayout as Layout,
    },
    types::{PrimitiveType as Primitive, TypeKind, TypeLimits, TypeRegistryBuilder},
};

#[test]
fn lays_out_resolved_record_fields_by_declaration_order_and_pads_fixed_arrays() {
    let source = r#"
        record Cell { ready: boolean, weight: float, count: integer, seen: boolean }
        bind first = new Cell { weight: 2.5, seen: false, ready: true, count: 3 };
        bind pair = [array: first, first];
        bind tuple = (true, pair, 7);
        bind empty: array<record Cell, 0> = [array: <record Cell>];
    "#;
    let program = scarp::compile(source).unwrap();
    let registry = program.type_registry();
    let ids = program.binding_type_ids();
    let expected = [
        Layout {
            size: 32,
            alignment: 8,
            shape: Shape::Fields {
                offsets: vec![0, 8, 16, 24],
            },
        },
        Layout {
            size: 64,
            alignment: 8,
            shape: Shape::Array {
                length: 2,
                stride: 32,
            },
        },
        Layout {
            size: 80,
            alignment: 8,
            shape: Shape::Fields {
                offsets: vec![0, 8, 72],
            },
        },
        Layout {
            size: 0,
            alignment: 8,
            shape: Shape::Array {
                length: 0,
                stride: 32,
            },
        },
    ];
    assert_eq!(ids.len(), expected.len());
    for (&id, expected) in ids.iter().zip(expected) {
        assert_eq!(plan(registry, id).unwrap(), expected);
    }
}

#[test]
fn empty_arrays_keep_alignment_and_size_checks_cover_product_padding_and_total() {
    let mut types = TypeRegistryBuilder::new(TypeLimits::default()).unwrap();
    let sequence = |element: Primitive, length| TypeKind::Sequence {
        element: element.id(),
        length: Some(length),
    };
    let enormous = types
        .intern(sequence(Primitive::Boolean, u64::MAX))
        .unwrap();
    let overflow = types
        .intern(sequence(Primitive::Integer, u64::MAX))
        .unwrap();
    let empty = types.intern(sequence(Primitive::Integer, 0)).unwrap();
    let pad = types
        .intern(TypeKind::Tuple(Box::new([enormous, empty])))
        .unwrap();
    let sum = types
        .intern(TypeKind::Tuple(Box::new([
            enormous,
            Primitive::Boolean.id(),
        ])))
        .unwrap();
    let padded = types
        .intern(TypeKind::Tuple(Box::new([Primitive::Boolean.id(), empty])))
        .unwrap();
    let registry = types.finish();
    assert_eq!(plan(&registry, enormous).unwrap().size, u64::MAX);
    for id in [overflow, pad, sum] {
        assert_eq!(plan(&registry, id), Err(Error::SizeOverflow));
    }
    assert_eq!(
        plan(&registry, padded).unwrap(),
        Layout {
            size: 8,
            alignment: 8,
            shape: Shape::Fields {
                offsets: vec![0, 8]
            }
        }
    );
}

#[test]
fn rejects_foreign_handles_dynamic_and_heap_types_even_in_empty_arrays() {
    let mut arena = TypeRegistryBuilder::new(TypeLimits::default()).unwrap();
    let mut other = TypeRegistryBuilder::new(TypeLimits::default()).unwrap();
    let descriptor = TypeKind::Tuple(Box::new([Primitive::Integer.id()]));
    let local = arena.intern(descriptor.clone()).unwrap();
    let foreign = other.intern(descriptor).unwrap();
    assert_eq!(local.index(), foreign.index());
    let dynamic = arena
        .intern(TypeKind::Sequence {
            element: Primitive::Integer.id(),
            length: None,
        })
        .unwrap();
    let zero_strings = arena
        .intern(TypeKind::Sequence {
            element: Primitive::String.id(),
            length: Some(0),
        })
        .unwrap();
    let registry = arena.finish();
    assert_eq!(plan(&registry, foreign), Err(Error::UnknownType(foreign)));
    assert_eq!(
        plan(&registry, dynamic),
        Err(Error::UnsupportedType(dynamic))
    );
    assert_eq!(
        plan(&registry, zero_strings),
        Err(Error::UnsupportedType(Primitive::String.id()))
    );
    assert_eq!(
        plan(&registry, Primitive::Decimal.id()),
        Err(Error::UnsupportedType(Primitive::Decimal.id()))
    );
}

#[test]
fn depth_and_component_limits_bound_host_created_type_graphs() {
    let mut arena = TypeRegistryBuilder::new(TypeLimits {
        max_depth: 70,
        ..TypeLimits::default()
    })
    .unwrap();
    let mut deep = Primitive::Boolean.id();
    for _ in 0..64 {
        deep = arena
            .intern(TypeKind::Sequence {
                element: deep,
                length: Some(1),
            })
            .unwrap();
    }
    let mut wide_children = Vec::new();
    for length in 0..128 {
        let child = arena
            .intern(TypeKind::Sequence {
                element: Primitive::Boolean.id(),
                length: Some(length),
            })
            .unwrap();
        wide_children.push(
            arena
                .intern(TypeKind::Tuple(vec![child; 1024].into_boxed_slice()))
                .unwrap(),
        );
    }
    let wide = arena
        .intern(TypeKind::Tuple(wide_children.into_boxed_slice()))
        .unwrap();
    let arena = arena.finish();
    assert_eq!(plan(&arena, deep), Err(Error::Complexity));
    assert_eq!(plan(&arena, wide), Err(Error::Complexity));
}
