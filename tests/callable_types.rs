use scarp::types::{
    FunctionEffect as Effect, PrimitiveType as P, TypeId, TypeKind as T, TypeLimits,
    TypeRegistryBuilder as Builder, TypeRegistryError as Error,
};

fn signature(parameters: &[TypeId], result: TypeId, effect: Effect) -> T {
    T::Function {
        parameters: parameters.into(),
        result,
        effect,
    }
}

#[test]
fn callable_signatures_are_invariant_in_parameters_and_results() {
    let mut builder = Builder::new(TypeLimits::default()).unwrap();
    let expected = builder
        .intern(signature(
            &[P::Integer.id(), P::String.id()],
            P::Boolean.id(),
            Effect::Pure,
        ))
        .unwrap();
    let reordered = builder
        .intern(signature(
            &[P::String.id(), P::Integer.id()],
            P::Boolean.id(),
            Effect::Pure,
        ))
        .unwrap();
    let result_changed = builder
        .intern(signature(
            &[P::Integer.id(), P::String.id()],
            P::Integer.id(),
            Effect::Pure,
        ))
        .unwrap();
    let shorter = builder
        .intern(signature(&[P::Integer.id()], P::Boolean.id(), Effect::Pure))
        .unwrap();
    let registry = builder.finish();
    assert!(registry.accepts_callable(expected, expected).unwrap());
    for candidate in [reordered, result_changed, shorter, P::Integer.id()] {
        assert!(!registry.accepts_callable(expected, candidate).unwrap());
    }
    assert!(!registry
        .accepts_callable(P::Boolean.id(), expected)
        .unwrap());
}

#[test]
fn pure_functions_can_fill_impure_slots_but_impure_functions_cannot_fill_pure_slots() {
    let mut builder = Builder::new(TypeLimits::default()).unwrap();
    let pure = builder
        .intern(signature(&[P::Integer.id()], P::Unit.id(), Effect::Pure))
        .unwrap();
    let impure = builder
        .intern(signature(&[P::Integer.id()], P::Unit.id(), Effect::Impure))
        .unwrap();
    let registry = builder.finish();
    assert!(registry.accepts_callable(pure, pure).unwrap());
    assert!(registry.accepts_callable(impure, impure).unwrap());
    assert!(registry.accepts_callable(impure, pure).unwrap());
    assert!(!registry.accepts_callable(pure, impure).unwrap());
}

#[test]
fn callable_handles_from_different_registries_are_rejected_even_when_shapes_match() {
    let mut left = Builder::new(TypeLimits::default()).unwrap();
    let mut right = Builder::new(TypeLimits::default()).unwrap();
    let left_id = left
        .intern(signature(&[P::Integer.id()], P::Integer.id(), Effect::Pure))
        .unwrap();
    let right_id = right
        .intern(signature(&[P::Integer.id()], P::Integer.id(), Effect::Pure))
        .unwrap();
    assert_eq!(left_id.index(), right_id.index());
    assert_ne!(left_id, right_id);
    let registry = left.finish();
    assert_eq!(
        registry.accepts_callable(left_id, right_id),
        Err(Error::UnknownType(right_id))
    );
    assert_eq!(
        registry.accepts_callable(right_id, left_id),
        Err(Error::UnknownType(right_id))
    );
    assert!(registry.accepts_callable(left_id, left_id).unwrap());
}

#[test]
fn resolved_named_functions_expose_checked_semantic_callable_signatures() {
    let program = scarp::compile(
        "pure funcra identity(n: integer) -> integer { return n; }\n\
         funcra echo(n: integer) -> integer { print(n); return n; }",
    )
    .unwrap();
    let pure = program.function_type_id(0).unwrap();
    let impure = program.function_type_id(1).unwrap();
    let registry = program.type_registry();
    assert!(matches!(
        registry.get(pure),
        Some(T::Function {
            effect: Effect::Pure,
            ..
        })
    ));
    assert!(matches!(
        registry.get(impure),
        Some(T::Function {
            effect: Effect::Impure,
            ..
        })
    ));
    assert!(registry.accepts_callable(impure, pure).unwrap());
    assert!(!registry.accepts_callable(pure, impure).unwrap());
    // This is still a semantic-type API, not an executable function value.
    let unsupported = scarp::compile(
        "funcra identity(n: integer) -> integer { return n; } bind alias = identity;",
    )
    .unwrap_err();
    assert_eq!(
        unsupported.kind,
        scarp::diagnostics::DiagnosticKind::FunctionUnsupported
    );
}

#[test]
fn callable_arguments_obey_registry_arity_and_depth_limits() {
    let mut builder = Builder::new(TypeLimits {
        max_arity: 1,
        max_depth: 2,
        ..TypeLimits::default()
    })
    .unwrap();
    let leaf = builder
        .intern(signature(&[P::Integer.id()], P::Boolean.id(), Effect::Pure))
        .unwrap();
    assert!(builder
        .intern(signature(&[leaf], P::Boolean.id(), Effect::Pure))
        .is_err());
    assert!(builder
        .intern(signature(
            &[P::Integer.id(), P::String.id()],
            P::Boolean.id(),
            Effect::Pure
        ))
        .is_err());
    assert_eq!(builder.len(), P::ALL.len() + 1);
    let registry = builder.finish();
    assert_eq!(registry.depth(leaf), Some(2));
}
