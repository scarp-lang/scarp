use scarp::{
    ast::ExpressionKind as E,
    modules::{self, MemorySourceLoader, ModuleLimits},
    types::{
        FunctionEffect as Effect, PrimitiveType as P, TypeId, TypeKind as T, TypeLimits,
        TypeRegistryBuilder as Builder, TypeRegistryError as Error, TypeResource as Resource,
    },
    ExecutionOptions,
};

fn signature(parameters: &[TypeId], result: TypeId, effect: Effect) -> T {
    T::Function {
        parameters: parameters.into(),
        result,
        effect,
    }
}

#[test]
fn callable_compatibility_matches_the_complete_small_signature_matrix() {
    let mut builder = Builder::new(TypeLimits::default()).unwrap();
    let parameters: &[&[TypeId]] = &[&[], &[P::Integer.id()], &[P::Boolean.id()]];
    let mut cases = Vec::new();
    for &arguments in parameters {
        for result in [P::Unit.id(), P::Integer.id()] {
            for effect in [Effect::Pure, Effect::Impure] {
                let id = builder.intern(signature(arguments, result, effect)).unwrap();
                cases.push((arguments.to_vec(), result, effect, id));
            }
        }
    }
    let registry = builder.finish();
    for (expected_params, expected_result, expected_effect, expected) in &cases {
        for (candidate_params, candidate_result, candidate_effect, candidate) in &cases {
            let compatible = expected_params == candidate_params
                && expected_result == candidate_result
                && (*expected_effect == *candidate_effect
                    || (*expected_effect == Effect::Impure
                        && *candidate_effect == Effect::Pure));
            assert_eq!(
                registry.accepts_callable(*expected, *candidate).unwrap(),
                compatible,
                "expected {expected:?}, candidate {candidate:?}"
            );
        }
    }
    assert!(!registry
        .accepts_callable(P::Unit.id(), cases[0].3)
        .unwrap());
    assert!(!registry
        .accepts_callable(cases[0].3, P::Integer.id())
        .unwrap());
    let mut foreign_builder = Builder::new(TypeLimits::default()).unwrap();
    let foreign = foreign_builder
        .intern(signature(&[], P::Unit.id(), Effect::Pure))
        .unwrap();
    assert_eq!(
        registry.accepts_callable(cases[0].3, foreign),
        Err(Error::UnknownType(foreign))
    );
    assert_eq!(
        registry.accepts_callable(foreign, P::Unit.id()),
        Err(Error::UnknownType(foreign))
    );
}

#[test]
fn zero_argument_callable_limits_do_not_partially_register_types() {
    let limits = TypeLimits {
        max_types: P::ALL.len() + 1,
        max_components: 1,
        max_depth: 2,
        max_arity: 0,
    };
    let mut builder = Builder::new(limits).unwrap();
    let unit_pure = signature(&[], P::Unit.id(), Effect::Pure);
    let id = builder.intern(unit_pure.clone()).unwrap();
    assert_eq!(builder.intern(unit_pure).unwrap(), id);
    assert_eq!(
        builder.intern(signature(&[P::Integer.id()], P::Unit.id(), Effect::Pure)),
        Err(Error::Limit {
            resource: Resource::Arity,
            limit: 0
        })
    );
    assert_eq!(
        builder.intern(signature(&[], P::Unit.id(), Effect::Impure)),
        Err(Error::Limit {
            resource: Resource::Types,
            limit: P::ALL.len() + 1
        })
    );
    assert_eq!(builder.len(), P::ALL.len() + 1);
    assert_eq!(builder.component_count(), 1);
    assert_eq!(builder.finish().depth(id), Some(2));
}

#[test]
fn named_callable_signature_snapshot_outlives_the_compiled_program() {
    let program =
        scarp::compile("pure funcra keep(n: integer) -> integer { return n; }").unwrap();
    let id = program.function_type_id(0).unwrap();
    let registry = program.type_registry().clone();
    drop(program);
    assert!(matches!(
        registry.get(id),
        Some(T::Function {
            effect: Effect::Pure,
            ..
        })
    ));
    assert!(registry.accepts_callable(id, id).unwrap());
    let replacement =
        scarp::compile("pure funcra keep(n: integer) -> integer { return n; }").unwrap();
    assert!(replacement.type_registry().get(id).is_none());
    assert_ne!(replacement.function_type_id(0), Some(id));
}

#[test]
fn imported_named_calls_preserve_effect_types_and_unicode_source_spans() {
    let mut loader = MemorySourceLoader::new();
    loader
        .insert(
            "main.srp",
            "// café 🌍\r\nimport \"lib.srp\" as lib; print(lib.twice(3), lib.loud(3));",
        )
        .unwrap();
    loader
        .insert(
            "lib.srp",
            "// λ\r\npub pure funcra twice(n: integer) -> integer { return n * 2; }\r\n\
             pub funcra loud(n: integer) -> integer { print(n); return n + 1; }\r\n",
        )
        .unwrap();
    let module =
        modules::compile_with_loader("main.srp", &loader, ModuleLimits::default()).unwrap();
    let program = module.program();
    let mut pure = None;
    let mut impure = None;
    for index in 0..program.functions().len() {
        let id = program.function_type_id(index).unwrap();
        match program.type_registry().get(id).unwrap() {
            T::Function {
                parameters,
                result,
                effect,
            } => {
                assert_eq!(parameters.as_ref(), &[P::Integer.id()]);
                assert_eq!(*result, P::Integer.id());
                match effect {
                    Effect::Pure => pure = Some(id),
                    Effect::Impure => impure = Some(id),
                }
            }
            _ => panic!("resolved named declaration lacks a function type"),
        }
    }
    let pure = pure.expect("pure declaration");
    let impure = impure.expect("impure declaration");
    assert!(program.type_registry().accepts_callable(impure, pure).unwrap());
    assert!(!program.type_registry().accepts_callable(pure, impure).unwrap());
    let mut calls = 0;
    for expression in program.typed_expressions() {
        for node in expression.nodes() {
            if matches!(node.syntax, E::UserCall { .. }) {
                assert!(node.call_signature == Some(pure) || node.call_signature == Some(impure));
                let location = module.sources().locate_span(node.span).unwrap();
                let original = module.sources().source(location.source).unwrap();
                assert!(original.is_char_boundary(location.span.start));
                assert!(original.is_char_boundary(location.span.end));
                calls += 1;
            }
        }
    }
    assert_eq!(calls, 2);
    module.check(ExecutionOptions::default()).unwrap();
    let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
    module
        .execute(&mut stdout, &mut stderr, ExecutionOptions::default())
        .unwrap();
    assert_eq!(stdout, b"3\n6 4\n");
    assert!(stderr.is_empty());
}
