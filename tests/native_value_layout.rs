use scarp::{
    native_object::{
        plan_x86_64_value_layout as plan, NativeLayoutError as Error, NativeLayoutShape as Shape,
        NativeValueLayout as Layout,
    },
    types::{DefinitionId, PrimitiveType as Primitive, TypeKind, TypeLimits, TypeRegistryBuilder},
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
    let limit = i64::MAX as u64;
    let enormous = types.intern(sequence(Primitive::Boolean, limit)).unwrap();
    let past_limit = types
        .intern(sequence(Primitive::Boolean, limit + 1))
        .unwrap();
    let product_overflow = types
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
    assert_eq!(plan(&registry, enormous).unwrap().size, limit);
    for id in [past_limit, product_overflow, pad, sum] {
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

#[test]
fn plans_resolved_variants_with_one_shared_payload_slot() {
    let source = r#"
        record Payload { flag: boolean, count: integer }
        variant Flag { Off, On }
        variant Packet { Empty, Bit(boolean), Data(record Payload) }
        variant TypedEmpty { Missing, Empty(array<integer, 0>) }
        bind flags = new Flag::On();
        bind packet = new Packet::Empty();
        bind array = [array: packet, packet];
        bind nested = (flags, array);
        bind empty = new TypedEmpty::Missing();
    "#;
    let program = scarp::compile(source).unwrap();
    let registry = program.type_registry();
    let ids = program.binding_type_ids();
    assert_eq!(plan(registry, ids[0]).unwrap(), tagged(4, 4, 2, None, 0));
    assert_eq!(
        plan(registry, ids[1]).unwrap(),
        tagged(24, 8, 3, Some(8), 16)
    );
    assert_eq!(
        plan(registry, ids[2]).unwrap(),
        Layout {
            size: 48,
            alignment: 8,
            shape: Shape::Array {
                length: 2,
                stride: 24
            },
        },
    );
    assert_eq!(
        plan(registry, ids[3]).unwrap(),
        Layout {
            size: 56,
            alignment: 8,
            shape: Shape::Fields {
                offsets: vec![0, 8]
            },
        },
    );
    assert_eq!(plan(registry, ids[4]).unwrap(), tagged(8, 8, 2, Some(8), 0));
}

#[test]
fn plans_builtin_specializations_and_rejects_heap_backed_inactive_cases() {
    let program = scarp::compile(
        r#"
        bind a = new option<boolean>::None();
        bind b = new result<boolean, integer>::Ok(true);
        bind c = new option<result<boolean, integer>>::None();
        bind d = new result<integer, string>::Ok(3);
        variant V { Empty, Message(string) }
        bind e = new V::Empty();
    "#,
    )
    .unwrap();
    let registry = program.type_registry();
    let ids = program.binding_type_ids();
    assert_eq!(plan(registry, ids[0]).unwrap(), tagged(8, 4, 2, Some(4), 1));
    assert_eq!(
        plan(registry, ids[1]).unwrap(),
        tagged(16, 8, 2, Some(8), 8)
    );
    assert_eq!(
        plan(registry, ids[2]).unwrap(),
        tagged(24, 8, 2, Some(8), 16)
    );
    for &id in &ids[3..] {
        assert_eq!(
            plan(registry, id),
            Err(Error::UnsupportedType(Primitive::String.id()))
        );
    }
}

#[test]
fn checks_tagged_sizes_and_counts_payloadless_cases_toward_the_budget() {
    let mut types = TypeRegistryBuilder::new(TypeLimits {
        max_arity: 65_537,
        ..TypeLimits::default()
    })
    .unwrap();
    let mut next_id = 0;
    let mut variant = |types: &mut TypeRegistryBuilder, cases: Vec<_>| {
        let definition = DefinitionId {
            module: 0,
            declaration: next_id,
        };
        next_id += 1;
        types
            .intern(TypeKind::Variant {
                definition,
                cases: cases.into_boxed_slice(),
            })
            .unwrap()
    };
    let empty = variant(&mut types, vec![]);
    let limit = variant(&mut types, vec![None; 65_536]);
    let past_limit = variant(&mut types, vec![None; 65_537]);
    let nested_limit = types.intern(TypeKind::Tuple(Box::new([limit]))).unwrap();
    let size_limit = i64::MAX as u64;
    let [largest_aligned, unaligned, too_large] =
        [size_limit - 7, size_limit - 4, size_limit].map(|length| {
            let enormous = types
                .intern(TypeKind::Sequence {
                    element: Primitive::Boolean.id(),
                    length: Some(length),
                })
                .unwrap();
            variant(&mut types, vec![None, Some(enormous)])
        });
    let registry = types.finish();
    assert_eq!(plan(&registry, empty), Err(Error::UnsupportedType(empty)));
    assert_eq!(
        plan(&registry, limit).unwrap(),
        tagged(4, 4, 65_536, None, 0)
    );
    for id in [past_limit, nested_limit] {
        assert_eq!(plan(&registry, id), Err(Error::Complexity));
    }
    assert_eq!(
        plan(&registry, largest_aligned).unwrap(),
        tagged(size_limit - 3, 4, 2, Some(4), size_limit - 7)
    );
    for id in [unaligned, too_large] {
        assert_eq!(plan(&registry, id), Err(Error::SizeOverflow));
    }
}

fn tagged(
    size: u64,
    alignment: u64,
    case_count: u32,
    payload_offset: Option<u64>,
    payload_size: u64,
) -> Layout {
    Layout {
        size,
        alignment,
        shape: Shape::Tagged {
            case_count,
            payload_offset,
            payload_size,
        },
    }
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[test]
fn nested_layout_matches_an_independent_sysv_c11_layout() {
    use std::{fs, path::PathBuf, process::Command};

    let cc = std::env::var_os("SCARP_CC").unwrap_or_else(|| "cc".into());
    if Command::new(&cc).arg("--version").output().is_err() {
        assert!(
            std::env::var_os("SCARP_REQUIRE_C").is_none(),
            "C compiler is required for the Linux x86-64 layout comparison"
        );
        return;
    }

    let program = scarp::compile(
        r#"
        record Cell { active: boolean, weight: float, count: integer }
        variant Packet { Empty, Bit(boolean), Data(array<record Cell, 2>) }
        bind cell = new Cell { count: 3, active: true, weight: 2.5 };
        bind packet = new Packet::Empty();
        bind pair = (true, packet);
        "#,
    )
    .unwrap();
    let registry = program.type_registry();
    let [cell, packet, pair] = <[scarp::types::TypeId; 3]>::try_from(program.binding_type_ids())
        .unwrap()
        .map(|id| plan(registry, id).unwrap());

    let Shape::Fields {
        offsets: cell_offsets,
    } = cell.shape
    else {
        panic!("expected record offsets");
    };
    let Shape::Tagged {
        payload_offset: Some(payload),
        payload_size,
        ..
    } = packet.shape
    else {
        panic!("expected a tagged payload layout");
    };
    let Shape::Fields {
        offsets: pair_offsets,
    } = pair.shape
    else {
        panic!("expected tuple offsets");
    };

    struct Temp(PathBuf);
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    let directory = Temp(
        std::env::temp_dir().join(format!("scarp-native-value-layout-{}", std::process::id())),
    );
    fs::create_dir(&directory.0).unwrap();
    let input = directory.0.join("layout.c");
    let executable = directory.0.join("layout");
    fs::write(
        &input,
        r#"#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
struct cell { bool active; double weight; int64_t count; };
union payload { bool bit; struct cell data[2]; };
struct packet { uint32_t tag; union payload payload; };
struct pair { bool first; struct packet second; };
int main(void) {
    printf("%zu %zu %zu %zu %zu\n", sizeof(struct cell), _Alignof(struct cell),
           offsetof(struct cell, active), offsetof(struct cell, weight), offsetof(struct cell, count));
    printf("%zu %zu %zu %zu\n", sizeof(struct packet), _Alignof(struct packet),
           offsetof(struct packet, payload), sizeof(union payload));
    printf("%zu %zu %zu %zu\n", sizeof(struct pair), _Alignof(struct pair),
           offsetof(struct pair, first), offsetof(struct pair, second));
    return 0;
}
"#,
    )
    .unwrap();
    let built = Command::new(cc)
        .args(["-std=c11", "-Wall", "-Wextra", "-Werror"])
        .arg(input)
        .arg("-o")
        .arg(&executable)
        .output()
        .unwrap();
    assert!(
        built.status.success(),
        "{}",
        String::from_utf8_lossy(&built.stderr)
    );
    let output = Command::new(executable).output().unwrap();
    assert!(output.status.success(), "{:?}", output.status);
    let expected = format!(
        "{} {} {} {} {}\n{} {} {payload} {payload_size}\n{} {} {} {}\n",
        cell.size,
        cell.alignment,
        cell_offsets[0],
        cell_offsets[1],
        cell_offsets[2],
        packet.size,
        packet.alignment,
        pair.size,
        pair.alignment,
        pair_offsets[0],
        pair_offsets[1],
    );
    assert_eq!(output.stdout, expected.as_bytes());
}
