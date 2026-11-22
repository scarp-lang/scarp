use scarp::diagnostics::DiagnosticKind;
use scarp::types::{
    FunctionEffect as Effect, PrimitiveType as P, TypeKind, TypeLimits, TypeRegistryBuilder,
};

#[test]
fn higher_order_callable_descriptors_preserve_effect_and_identity() {
    let mut builder = TypeRegistryBuilder::new(TypeLimits::default()).unwrap();
    let pure_inner = builder
        .intern(TypeKind::Function {
            parameters: vec![P::Integer.id()].into_boxed_slice(),
            result: P::Integer.id(),
            effect: Effect::Pure,
        })
        .unwrap();
    let impure_inner = builder
        .intern(TypeKind::Function {
            parameters: vec![P::Integer.id()].into_boxed_slice(),
            result: P::Integer.id(),
            effect: Effect::Impure,
        })
        .unwrap();
    let outer_pure = builder
        .intern(TypeKind::Function {
            parameters: vec![pure_inner].into_boxed_slice(),
            result: P::Boolean.id(),
            effect: Effect::Pure,
        })
        .unwrap();
    let outer_impure_parameter = builder
        .intern(TypeKind::Function {
            parameters: vec![impure_inner].into_boxed_slice(),
            result: P::Boolean.id(),
            effect: Effect::Pure,
        })
        .unwrap();
    let registry = builder.finish();
    assert!(registry.accepts_callable(outer_pure, outer_pure).unwrap());
    // Function-valued parameters remain invariant: the inner effect is part of
    // the parameter's identity, even though outer purity is identical.
    assert!(!registry
        .accepts_callable(outer_pure, outer_impure_parameter)
        .unwrap());
    assert!(!registry
        .accepts_callable(outer_impure_parameter, outer_pure)
        .unwrap());
}

#[test]
fn named_pure_and_impure_calls_execute_without_first_class_aliases() {
    let source = include_str!("../examples/callable_effect_boundaries.srp");
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    scarp::run_with_io(source, &mut stdout, &mut stderr).unwrap();
    assert_eq!(stdout, b"12\n13\n");
    assert!(stderr.is_empty());
    scarp::check(source).unwrap();
    let alias = scarp::compile(
        "pure funcra twice(n: integer) -> integer { return n * 2; } bind f = twice;",
    )
    .unwrap_err();
    assert_eq!(alias.kind, DiagnosticKind::FunctionUnsupported);
}
