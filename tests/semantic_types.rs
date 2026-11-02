use scarp::{
    ast::ExpressionKind as E,
    diagnostics::DiagnosticKind as Kind,
    numeric::Builtin,
    resolver::ExpressionContext,
    types::{FunctionEffect, PrimitiveType as P, TypeKind, TypeLimits},
};

#[test]
fn inference_keeps_per_node_types_and_legacy_binding_metadata() {
    let source =
        "bind a: integer=2; bind b=to_float(a)+1.5; bind c=[integer:1,2][0]; print(a,b,c);";
    let program = scarp::compile(source).unwrap();
    assert_eq!(program.binding_types(), &[P::Integer, P::Float, P::Integer]);
    assert_eq!(
        program.binding_type_ids(),
        &[P::Integer.id(), P::Float.id(), P::Integer.id()]
    );
    let mut roots = Vec::new();
    for expression in program.typed_expressions() {
        assert_eq!(expression.context(), ExpressionContext::Entry);
        assert!(source.is_char_boundary(expression.span().start));
        let nodes: Vec<_> = expression.nodes().collect();
        assert!(nodes.iter().any(|node| node.kind == expression.root_type()));
        for node in nodes {
            assert!(program.type_registry().get(node.kind).is_some());
            assert!(
                source.is_char_boundary(node.span.start) && source.is_char_boundary(node.span.end)
            );
            match node.syntax {
                E::Float(_) => assert_eq!(node.kind, P::Float.id()),
                E::Integer(_) => assert_eq!(node.kind, P::Integer.id()),
                E::Integers { .. } => assert_eq!(node.kind, P::Integers.id()),
                _ => {}
            }
        }
        roots.push(expression.root_type());
    }
    assert!(roots.contains(&P::Float.id()));
    let mut output = Vec::new();
    scarp::interpreter::execute(&program, &mut output).unwrap();
    assert_eq!(output, b"2 3.5 1\n");
}

#[test]
fn function_signatures_share_types_without_losing_named_call_targets() {
    let program=scarp::compile("pure funcra a(x: integer)->integer{return x+1;} pure funcra b(y: integer)->integer{return y+2;} print(a(x:1),b(y:1));").unwrap();
    let a = program.function_type_id(0).unwrap();
    assert_eq!(Some(a), program.function_type_id(1));
    assert!(
        matches!(program.type_registry().get(a),Some(TypeKind::Function{parameters,result,effect:FunctionEffect::Pure}) if parameters.as_ref()==[P::Integer.id()] && *result==P::Integer.id())
    );
    assert_eq!(program.function_type_id(2), None);
    assert_eq!(program.function_binding_type_ids(2), None);
    assert_eq!(
        program.function_binding_type_ids(0).unwrap(),
        &[P::Integer.id(), P::Integer.id()]
    );
    let mut output = Vec::new();
    scarp::interpreter::execute(&program, &mut output).unwrap();
    assert_eq!(output, b"2 3\n");
    assert_eq!(
        scarp::compile("pure funcra b(y: integer)->integer{return y;} print(b(x:1));")
            .unwrap_err()
            .kind,
        Kind::CallArguments
    );
}

#[test]
fn defaults_contracts_bodies_and_recursive_calls_have_typed_contexts() {
    let program=scarp::compile("pure funcra f(n: integer=3)->integer needs n>=0; ensures result>=0; {if n==0{return 0;} return f(n-1);} print(f());").unwrap();
    let mut default = false;
    let mut pre = false;
    let mut post = false;
    let mut body = false;
    let mut recursive_call = false;
    for expression in program.typed_expressions() {
        match expression.context() {
            ExpressionContext::ParameterDefault {
                function: 0,
                parameter: 0,
            } => {
                default = true;
                assert_eq!(expression.root_type(), P::Integer.id());
            }
            ExpressionContext::Contract {
                function: 0,
                postcondition,
                ..
            } => {
                if postcondition {
                    post = true;
                } else {
                    pre = true;
                }
                assert_eq!(expression.root_type(), P::Boolean.id());
            }
            ExpressionContext::FunctionBody { function: 0 } => body = true,
            _ => {}
        }
        for node in expression.nodes() {
            if matches!(node.syntax, E::UserCall { .. }) {
                assert_eq!(node.call_signature, program.function_type_id(0));
                recursive_call = true;
            }
        }
    }
    assert!(default && pre && post && body && recursive_call);
}

#[test]
fn builtin_metadata_uses_the_same_exact_signatures_as_runtime_checks() {
    let program = scarp::compile(
        "bind a=[integer:1,2]; print(ints_len(a),ints_sum(a),variance(ints_to_vector(a)));",
    )
    .unwrap();
    assert_eq!(
        program.builtin_type_id(Builtin::IntsLength),
        program.builtin_type_id(Builtin::IntsSum)
    );
    assert_eq!(program.builtin_type_id(Builtin::Tanh), None);
    for expression in program.typed_expressions() {
        for node in expression.nodes() {
            if let Some(signature) = node.call_signature {
                assert!(
                    matches!(program.type_registry().get(signature),Some(TypeKind::Function{result,..}) if *result==node.kind)
                );
            }
        }
    }
    let source = "funcra ints_len(n: integer)->integer{return n+1;} print(ints_len(2));";
    let shadowed = scarp::compile(source).unwrap();
    assert_eq!(shadowed.builtin_type_id(Builtin::IntsLength), None);
    assert!(matches!(
        shadowed
            .type_registry()
            .get(shadowed.function_type_id(0).unwrap()),
        Some(TypeKind::Function {
            effect: FunctionEffect::Impure,
            ..
        })
    ));
}

#[test]
fn caller_type_limits_have_exact_canonical_boundaries_and_precise_failures() {
    let limits = TypeLimits {
        max_types: 8,
        ..TypeLimits::default()
    };
    scarp::compile_with_type_limits("bind x=1; print(x+2);", limits).unwrap();
    let source = "print(variance([1.0]));";
    let error = scarp::compile_with_type_limits(source, limits).unwrap_err();
    assert_eq!(error.kind, Kind::TypeComplexity);
    assert_eq!(&source[error.span.start..error.span.end], "variance([1.0])");
    let source="pure funcra a(x: integer)->integer{return x;} pure funcra b(y: integer)->integer{return y;} print(a(1));";
    let limits = TypeLimits {
        max_types: 9,
        max_components: 2,
        max_depth: 2,
        max_arity: 1,
    };
    assert_eq!(
        scarp::compile_with_type_limits(source, limits)
            .unwrap()
            .type_registry()
            .len(),
        9
    );
    for limits in [
        TypeLimits {
            max_depth: 1,
            ..TypeLimits::default()
        },
        TypeLimits {
            max_arity: 0,
            ..TypeLimits::default()
        },
        TypeLimits {
            max_components: 1,
            ..TypeLimits::default()
        },
        TypeLimits {
            max_types: 7,
            ..TypeLimits::default()
        },
    ] {
        assert_eq!(
            scarp::compile_with_type_limits(source, limits)
                .unwrap_err()
                .kind,
            Kind::TypeComplexity
        );
    }
}

#[test]
fn linked_module_nodes_and_type_limit_errors_keep_owning_source_locations() {
    use scarp::modules::{self, ConstantLimits, MemorySourceLoader, ModuleLimits};
    let mut loader = MemorySourceLoader::new();
    loader
        .insert(
            "main.srp",
            "// café 🌍\r\nimport \"lib.srp\" as lib; print(lib.f(3));",
        )
        .unwrap();
    loader
        .insert(
            "lib.srp",
            "// λ\r\npub pure funcra f(n: integer)->float {return to_float(n);}",
        )
        .unwrap();
    let program =
        modules::compile_with_loader("main.srp", &loader, ModuleLimits::default()).unwrap();
    let mut owners = std::collections::BTreeSet::new();
    for expression in program.program().typed_expressions() {
        for node in expression.nodes() {
            let location = program.sources().locate_span(node.span).unwrap();
            owners.insert(location.source);
            assert!(program.program().type_registry().get(node.kind).is_some());
        }
    }
    assert_eq!(owners.len(), 2);
    let graph = modules::load("main.srp", &loader, ModuleLimits::default()).unwrap();
    let error = modules::resolve_with_compiler_limits(
        graph,
        ConstantLimits::default(),
        TypeLimits {
            max_types: 8,
            ..TypeLimits::default()
        },
    )
    .unwrap_err();
    assert_eq!(error.diagnostic.kind, Kind::TypeComplexity);
    let location = error.sources.locate_span(error.diagnostic.span).unwrap();
    assert_eq!(error.sources.files()[location.source].name, "lib.srp");
}

#[test]
fn snapshots_share_semantic_storage_but_recompilations_have_distinct_arenas() {
    let source = "pure funcra f()->integer{return 1;} print(f());";
    let a = scarp::compile(source).unwrap();
    let copy = a.clone();
    let b = scarp::compile(source).unwrap();
    let id = a.function_type_id(0).unwrap();
    assert_eq!(Some(id), copy.function_type_id(0));
    assert!(std::ptr::eq(
        a.type_registry().get(id).unwrap(),
        copy.type_registry().get(id).unwrap()
    ));
    assert_ne!(Some(id), b.function_type_id(0));
    assert_eq!(format!("{a:?}"), format!("{b:?}"));
    assert!(b.type_registry().get(id).is_none());
    assert_eq!(
        a.typed_expressions()
            .map(|expression| expression.root_type())
            .collect::<Vec<_>>(),
        b.typed_expressions()
            .map(|expression| expression.root_type())
            .collect::<Vec<_>>()
    );
}

#[test]
fn debug_inspection_is_deterministic_with_multiple_interned_builtin_signatures() {
    let source = "print(ints_sum([integer:1]),to_float(1),stddev([1.0]),text_len(\"a\"));";
    let first = format!("{:?}", scarp::compile(source).unwrap());
    for _ in 0..5 {
        assert_eq!(first, format!("{:?}", scarp::compile(source).unwrap()));
    }
}

#[test]
fn typed_inspection_does_not_execute_invalid_runtime_paths() {
    let program = scarp::compile(
        "pure funcra f(n: integer=1/0)->integer needs false; {return n;} print(f());",
    )
    .unwrap();
    assert!(program
        .typed_expressions()
        .flat_map(|expression| expression.nodes().map(|node| node.kind).collect::<Vec<_>>())
        .all(|kind| kind.primitive().is_some()));
    assert!(scarp::check(
        "pure funcra f(n: integer=1/0)->integer needs false; {return n;} print(f());"
    )
    .is_err());
}

#[test]
fn default_type_limits_preserve_the_existing_function_parameter_boundary() {
    let parameters = (0..1024)
        .map(|i| format!("p{i}: integer"))
        .collect::<Vec<_>>()
        .join(",");
    let source = format!("pure funcra wide({parameters})->integer {{return p0;}} print(1);");
    let program = scarp::compile(&source).unwrap();
    assert!(
        matches!(program.type_registry().get(program.function_type_id(0).unwrap()),Some(TypeKind::Function{parameters,..}) if parameters.len()==1024)
    );
    assert_eq!(program.function_binding_type_ids(0).unwrap().len(), 1025);
}
