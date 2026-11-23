use std::sync::Arc;

use scarp::{
    modules::{self, MemorySourceLoader, ModuleLimits},
    resolver::{CallableBindError as BindError, CallableCatalog},
    types::{
        FunctionEffect as Effect, PrimitiveType as P, TypeKind, TypeRegistryBuilder,
        TypeRegistryError, TypeLimits,
    },
    ExecutionOptions,
};

#[test]
fn same_signature_does_not_collapse_distinct_named_declarations() {
    let program = scarp::compile(
        "pure funcra first(n: integer) -> integer { return n; }\n\
         pure funcra second(n: integer) -> integer { return n + 1; }\n\
         print(first(2), second(2));",
    )
    .unwrap();
    let catalog = CallableCatalog::from_program(program);
    assert_eq!(catalog.len(), 2);
    assert!(!catalog.is_empty());
    let first = catalog.bind_exact(0).unwrap();
    let second = catalog.bind_exact(1).unwrap();
    assert_eq!(first.signature(), second.signature());
    assert_eq!(first.metadata().name, "first");
    assert_eq!(second.metadata().name, "second");
    assert!(!first.same_target(&second));
    assert!(first.same_target(&first.clone()));
    assert!(first.belongs_to(&catalog));
    let other_catalog =
        CallableCatalog::from_program(scarp::compile("pure funcra first(n: integer) -> integer { return n; }").unwrap());
    assert!(!first.belongs_to(&other_catalog));
    let catalog_copy = catalog.clone();
    assert!(first.belongs_to(&catalog_copy));
    assert!(catalog_copy.bind(0, second.signature()).unwrap().same_target(&first));
    assert_eq!(catalog.bind_exact(2).unwrap_err(), BindError::UnknownFunction(2));
}

#[test]
fn binding_respects_effects_shapes_and_foreign_type_arena_ids() {
    let program = scarp::compile(
        "pure funcra pure_one(n: integer) -> integer { return n; }\n\
         funcra impure_one(n: integer) -> integer { print(n); return n; }\n\
         pure funcra zero() -> unit {}",
    )
    .unwrap();
    let registry = program.type_registry().clone();
    let pure = program.function_type_id(0).unwrap();
    let impure = program.function_type_id(1).unwrap();
    let zero = program.function_type_id(2).unwrap();
    let catalog = CallableCatalog::from_program(program);
    assert!(catalog.bind(0, impure).is_ok());
    assert!(catalog.bind(1, impure).is_ok());
    assert!(catalog.bind(0, pure).is_ok());
    assert_eq!(
        catalog.bind(1, pure).unwrap_err(),
        BindError::IncompatibleSignature { expected: pure, actual: impure }
    );
    assert_eq!(
        catalog.bind(0, zero).unwrap_err(),
        BindError::IncompatibleSignature { expected: zero, actual: pure }
    );
    assert_eq!(
        catalog.bind(0, P::Integer.id()).unwrap_err(),
        BindError::IncompatibleSignature { expected: P::Integer.id(), actual: pure }
    );
    let mut foreign = TypeRegistryBuilder::new(TypeLimits::default()).unwrap();
    let foreign_id = foreign
        .intern(TypeKind::Function {
            parameters: vec![P::Integer.id()].into(),
            result: P::Integer.id(),
            effect: Effect::Pure,
        })
        .unwrap();
    assert_eq!(
        catalog.bind(0, foreign_id).unwrap_err(),
        BindError::Registry(TypeRegistryError::UnknownType(foreign_id))
    );
    assert_eq!(
        catalog.bind(usize::MAX, foreign_id).unwrap_err(),
        BindError::UnknownFunction(usize::MAX)
    );
    assert!(registry.get(pure).is_some());
}

#[test]
fn snapshot_survives_catalog_and_original_drop_then_reclaims_after_last_handle() {
    let original = Arc::new(
        scarp::compile("pure funcra keep(x: string) -> string { return x; }").unwrap(),
    );
    let weak = Arc::downgrade(&original);
    let catalog = CallableCatalog::new(Arc::clone(&original));
    let handle = catalog.bind_exact(0).unwrap();
    let duplicate = handle.clone();
    drop(original);
    drop(catalog);
    assert!(weak.upgrade().is_some());
    assert_eq!(handle.metadata().name, "keep");
    assert!(handle.same_target(&duplicate));
    drop(handle);
    assert!(weak.upgrade().is_some());
    drop(duplicate);
    assert!(weak.upgrade().is_none());
}

#[test]
fn identical_recompilations_have_distinct_targets_and_foreign_signatures() {
    let source = "pure funcra ident(n: integer) -> integer { return n; }";
    let old = CallableCatalog::from_program(scarp::compile(source).unwrap());
    let new = CallableCatalog::from_program(scarp::compile(source).unwrap());
    let old_ref = old.bind_exact(0).unwrap();
    let new_ref = new.bind_exact(0).unwrap();
    assert_eq!(old_ref.metadata(), new_ref.metadata());
    assert_ne!(old_ref.signature(), new_ref.signature());
    assert!(!old_ref.same_target(&new_ref));
    assert!(!old_ref.belongs_to(&new));
    assert_eq!(
        new.bind(0, old_ref.signature()).unwrap_err(),
        BindError::Registry(TypeRegistryError::UnknownType(old_ref.signature()))
    );
}

#[test]
fn linked_same_signature_exports_bind_distinctly_without_changing_named_execution() {
    let mut loader = MemorySourceLoader::new();
    loader
        .insert(
            "main.srp",
            "// café 🌍\r\nimport \"lib.srp\" as lib; print(lib.left(3), lib.right(3));",
        )
        .unwrap();
    loader
        .insert(
            "lib.srp",
            "pub pure funcra left(n: integer) -> integer { return n; }\r\n\
             pub pure funcra right(n: integer) -> integer { return n + 1; }",
        )
        .unwrap();
    let module =
        modules::compile_with_loader("main.srp", &loader, ModuleLimits::default()).unwrap();
    let catalog = CallableCatalog::from_program(module.program().clone());
    assert_eq!(catalog.len(), 2);
    let left = catalog.bind_exact(0).unwrap();
    let right = catalog.bind_exact(1).unwrap();
    assert_eq!(left.signature(), right.signature());
    assert!(!left.same_target(&right));
    assert!(left.metadata().span.end > left.metadata().span.start);
    assert!(right.metadata().span.end > right.metadata().span.start);
    let (mut out, mut err) = (Vec::new(), Vec::new());
    module
        .execute(&mut out, &mut err, ExecutionOptions::default())
        .unwrap();
    assert_eq!(out, b"3 4\n");
    assert!(err.is_empty());
}

#[test]
fn empty_catalog_preserves_existing_type_budget_limits() {
    let program = scarp::compile_with_type_limits(
        "print(1);",
        TypeLimits { max_types: P::ALL.len(), ..TypeLimits::default() },
    )
    .unwrap();
    let catalog = CallableCatalog::from_program(program);
    assert!(catalog.is_empty());
    assert_eq!(catalog.len(), 0);
    assert_eq!(catalog.bind_exact(0).unwrap_err(), BindError::UnknownFunction(0));
}
