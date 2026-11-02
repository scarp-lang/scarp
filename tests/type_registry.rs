use scarp::types::{
    DefinitionId, FunctionEffect, PrimitiveType as P, TypeId, TypeKind as T, TypeLimits,
    TypeRegistry, TypeRegistryBuilder as Builder, TypeRegistryError as Error,
    TypeResource as Resource,
};
use scarp::value::ValueType;

fn builder() -> Builder {
    Builder::new(TypeLimits::default()).unwrap()
}
fn sequence(element: TypeId, length: Option<u64>) -> T {
    T::Sequence { element, length }
}
fn tuple(values: &[TypeId]) -> T {
    T::Tuple(values.into())
}

#[test]
fn primitive_handles_and_legacy_value_type_paths_are_compatible() {
    let mut builder = builder();
    let registry = TypeRegistry::default();
    assert_eq!(registry.len(), 8);
    assert!(!registry.is_empty());
    let mut ids = std::collections::HashSet::new();
    for &kind in P::ALL {
        let legacy: ValueType = kind;
        assert_eq!(legacy.to_string(), kind.to_string());
        let id = builder.intern(T::Primitive(legacy)).unwrap();
        assert!(ids.insert(id));
        assert_eq!(id, kind.id());
        assert_eq!(id.primitive(), Some(kind));
        assert_eq!(registry.get(id), Some(&T::Primitive(kind)));
        assert_eq!(registry.depth(id), Some(1));
    }
    assert_eq!(builder.len(), 8);
    assert_eq!(builder.component_count(), 0);
    assert_eq!(registry.iter().count(), 8);
    assert_eq!(scarp::value::Value::Integer(1).kind(), P::Integer);
}

#[test]
fn canonicalization_uses_structure_and_canonical_child_identity() {
    let mut b = builder();
    let integer = P::Integer.id();
    let a = b.intern(sequence(integer, None)).unwrap();
    let alias = b.intern(sequence(integer, None)).unwrap();
    assert_eq!(a, alias);
    let pair = b.intern(tuple(&[a, alias])).unwrap();
    assert_eq!(pair, b.intern(tuple(&[alias, a])).unwrap());
    assert_ne!(a, b.intern(sequence(integer, Some(0))).unwrap());
    assert_ne!(a, b.intern(sequence(P::Float.id(), None)).unwrap());
    let r = b.finish();
    assert!(r.equivalent(a, alias).unwrap());
    assert!(!r.equivalent(a, pair).unwrap());
    assert_eq!(r.depth(pair), Some(3));
}

#[test]
fn nominal_keys_and_generic_binders_do_not_collapse_by_spelling_or_index() {
    let mut b = builder();
    let owner = DefinitionId {
        module: 1,
        declaration: 2,
    };
    let other = DefinitionId {
        module: 2,
        declaration: 2,
    };
    let parameter = b
        .intern(T::Parameter {
            binder: owner,
            index: 0,
        })
        .unwrap();
    assert_eq!(
        parameter,
        b.intern(T::Parameter {
            binder: owner,
            index: 0
        })
        .unwrap()
    );
    assert_ne!(
        parameter,
        b.intern(T::Parameter {
            binder: other,
            index: 0
        })
        .unwrap()
    );
    assert_ne!(
        parameter,
        b.intern(T::Parameter {
            binder: owner,
            index: 1
        })
        .unwrap()
    );
    let a = b
        .intern(T::Applied {
            constructor: owner,
            arguments: vec![parameter].into(),
        })
        .unwrap();
    assert_eq!(
        a,
        b.intern(T::Applied {
            constructor: owner,
            arguments: vec![parameter].into()
        })
        .unwrap()
    );
    assert_ne!(
        a,
        b.intern(T::Applied {
            constructor: other,
            arguments: vec![parameter].into()
        })
        .unwrap()
    );
}

#[test]
fn function_and_tensor_shapes_keep_order_effects_and_dimensions() {
    let mut b = builder();
    let signature = |parameters: &[TypeId], result, effect| T::Function {
        parameters: parameters.into(),
        result,
        effect,
    };
    let pure = b
        .intern(signature(
            &[P::Integer.id(), P::Float.id()],
            P::Boolean.id(),
            FunctionEffect::Pure,
        ))
        .unwrap();
    assert_eq!(
        pure,
        b.intern(signature(
            &[P::Integer.id(), P::Float.id()],
            P::Boolean.id(),
            FunctionEffect::Pure
        ))
        .unwrap()
    );
    for alternative in [
        signature(
            &[P::Float.id(), P::Integer.id()],
            P::Boolean.id(),
            FunctionEffect::Pure,
        ),
        signature(
            &[P::Integer.id(), P::Float.id()],
            P::Unit.id(),
            FunctionEffect::Pure,
        ),
        signature(
            &[P::Integer.id(), P::Float.id()],
            P::Boolean.id(),
            FunctionEffect::Impure,
        ),
    ] {
        assert_ne!(pure, b.intern(alternative).unwrap());
    }
    let tensor = |dimensions: &[Option<u64>]| T::Tensor {
        element: P::Float.id(),
        dimensions: dimensions.into(),
    };
    let id = b.intern(tensor(&[Some(2), None])).unwrap();
    assert_eq!(id, b.intern(tensor(&[Some(2), None])).unwrap());
    assert_ne!(id, b.intern(tensor(&[None, Some(2)])).unwrap());
    assert_ne!(id, b.intern(tensor(&[Some(2), Some(0)])).unwrap());
}

#[test]
fn foreign_handles_are_rejected_even_when_local_indices_match() {
    let (mut left, mut right) = (builder(), builder());
    let a = left.intern(sequence(P::Integer.id(), None)).unwrap();
    let b = right.intern(sequence(P::Integer.id(), None)).unwrap();
    assert_eq!(a.index(), b.index());
    assert_ne!(a, b);
    assert_eq!(left.intern(tuple(&[b])), Err(Error::UnknownType(b)));
    assert_eq!(left.len(), 9);
    assert_eq!(left.component_count(), 1);
    let registry = left.finish();
    assert!(registry.get(b).is_none());
    assert_eq!(registry.depth(b), None);
    assert_eq!(registry.equivalent(a, b), Err(Error::UnknownType(b)));
    assert!(TypeRegistry::default().get(a).is_none());
}

#[test]
fn snapshots_are_immutable_shared_and_safe_for_concurrent_inspection() {
    let mut b = builder();
    let id = b.intern(tuple(&[P::String.id(), P::Integer.id()])).unwrap();
    let r = b.finish();
    let copy = r.clone();
    assert!(std::ptr::eq(r.get(id).unwrap(), copy.get(id).unwrap()));
    let handles: Vec<_> = (0..8)
        .map(|_| {
            let registry = r.clone();
            std::thread::spawn(move || {
                assert_eq!(registry.depth(id), Some(2));
                assert_eq!(registry.iter().count(), 9);
                registry.get(id).unwrap().clone()
            })
        })
        .collect();
    for handle in handles {
        assert_eq!(
            handle.join().unwrap(),
            tuple(&[P::String.id(), P::Integer.id()])
        );
    }
    drop(r);
    assert!(copy.get(id).is_some());
}

#[test]
fn registry_limits_are_exact_and_duplicate_interning_does_not_consume_them() {
    let limits = TypeLimits {
        max_types: 10,
        max_components: 3,
        max_depth: 3,
        max_arity: 2,
    };
    let mut b = Builder::new(limits).unwrap();
    let a = b.intern(tuple(&[P::Integer.id(), P::Float.id()])).unwrap();
    let c = b.intern(sequence(a, None)).unwrap();
    assert_eq!(b.len(), 10);
    assert_eq!(b.component_count(), 3);
    assert_eq!(b.intern(sequence(a, None)).unwrap(), c);
    assert_eq!(
        b.intern(tuple(&[])),
        Err(Error::Limit {
            resource: Resource::Types,
            limit: 10
        })
    );
    assert_eq!(b.len(), 10);
    assert_eq!(b.component_count(), 3);
    let mut b = Builder::new(TypeLimits {
        max_components: 1,
        ..TypeLimits::default()
    })
    .unwrap();
    b.intern(sequence(P::Integer.id(), None)).unwrap();
    assert_eq!(
        b.intern(sequence(P::Float.id(), None)),
        Err(Error::Limit {
            resource: Resource::Components,
            limit: 1
        })
    );
    assert_eq!(b.component_count(), 1);
    assert_eq!(b.len(), 9);
}

#[test]
fn depth_width_and_tensor_component_boundaries_precede_registration() {
    let mut b = Builder::new(TypeLimits {
        max_depth: 2,
        max_arity: 2,
        max_components: 3,
        ..TypeLimits::default()
    })
    .unwrap();
    let a = b
        .intern(T::Tensor {
            element: P::Float.id(),
            dimensions: vec![Some(2), None].into(),
        })
        .unwrap();
    assert_eq!(b.component_count(), 3);
    assert_eq!(
        b.intern(sequence(a, None)),
        Err(Error::Limit {
            resource: Resource::Depth,
            limit: 2
        })
    );
    assert_eq!(
        b.intern(tuple(&[P::Integer.id(); 3])),
        Err(Error::Limit {
            resource: Resource::Arity,
            limit: 2
        })
    );
    assert_eq!(
        b.intern(T::Tensor {
            element: P::Float.id(),
            dimensions: vec![None; 3].into()
        }),
        Err(Error::Limit {
            resource: Resource::Arity,
            limit: 2
        })
    );
    assert_eq!(b.len(), 9);
    assert_eq!(b.component_count(), 3);
}

#[test]
fn invalid_limits_and_zero_component_leaf_types_are_defined() {
    for limits in [
        TypeLimits {
            max_types: 7,
            ..TypeLimits::default()
        },
        TypeLimits {
            max_depth: 0,
            ..TypeLimits::default()
        },
    ] {
        assert!(matches!(Builder::new(limits), Err(Error::InvalidLimits)));
    }
    let mut b = Builder::new(TypeLimits {
        max_types: 9,
        max_components: 0,
        max_depth: 1,
        max_arity: 0,
    })
    .unwrap();
    assert!(b.intern(tuple(&[])).is_ok());
    assert_eq!(
        b.intern(tuple(&[P::Integer.id()])),
        Err(Error::Limit {
            resource: Resource::Arity,
            limit: 0
        })
    );
    let r = b.finish();
    assert_eq!(r.len(), 9);
}

#[test]
fn deep_type_graphs_construct_clone_hash_and_drop_without_rust_recursion() {
    std::thread::Builder::new()
        .stack_size(128 * 1024)
        .spawn(|| {
            let mut b = Builder::new(TypeLimits {
                max_types: 10_008,
                max_components: 10_000,
                max_depth: 10_001,
                max_arity: 1,
            })
            .unwrap();
            let mut current = P::Integer.id();
            for _ in 0..10_000 {
                current = b.intern(sequence(current, None)).unwrap();
            }
            let r = b.finish();
            assert_eq!(r.depth(current), Some(10_001));
            assert_eq!(r.clone().len(), 10_008);
            drop(r);
        })
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn generated_shared_graphs_preserve_topological_handles_and_canonical_shapes() {
    let mut b = builder();
    let mut children = vec![P::Integer.id(), P::Float.id(), P::String.id()];
    let mut shapes = std::collections::BTreeMap::new();
    let mut seed = 17u64;
    for _ in 0..1000 {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        let index = (seed >> 32) as usize % children.len();
        let length = Some(seed % 7);
        let id = b.intern(sequence(children[index], length)).unwrap();
        let key = (children[index].index(), length);
        if let Some(previous) = shapes.insert(key, id) {
            assert_eq!(id, previous);
        }
        assert!(id.index() > children[index].index());
        children.push(id);
    }
    let r = b.finish();
    assert_eq!(r.len(), 8 + shapes.len());
    assert_eq!(r.iter().count(), r.len());
}

#[test]
fn future_descriptors_do_not_expose_unimplemented_source_types() {
    for name in [
        "tuple", "array", "list", "record", "optional", "tensor", "function",
    ] {
        assert_eq!(
            scarp::compile(&format!("bind x: {name}=1;"))
                .unwrap_err()
                .kind,
            scarp::diagnostics::DiagnosticKind::UnexpectedToken
        );
    }
}

#[test]
fn concurrent_registry_creation_never_reuses_composite_identities() {
    let threads: Vec<_> = (0..8)
        .map(|_| {
            std::thread::spawn(|| {
                (0..16)
                    .map(|_| builder().intern(sequence(P::Integer.id(), None)).unwrap())
                    .collect::<Vec<_>>()
            })
        })
        .collect();
    let ids: std::collections::HashSet<_> = threads
        .into_iter()
        .flat_map(|thread| thread.join().unwrap())
        .collect();
    assert_eq!(ids.len(), 128);
    assert!(ids.iter().all(|id| id.primitive().is_none()));
}
