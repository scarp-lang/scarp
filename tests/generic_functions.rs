use scarp::{
    ast::{ExpressionKind, Statement},
    diagnostics::DiagnosticKind as Kind,
    modules::{self, MemorySourceLoader, ModuleLimits},
    resolver::{CallableBindError, CallableCatalog, ExpressionContext},
    types::{PrimitiveType as P, TypeKind},
    ExecutionOptions,
};

fn run(source: &str) -> String {
    let mut output = Vec::new();
    scarp::run(source, &mut output).unwrap();
    String::from_utf8(output).unwrap()
}

#[test]
fn inference_handles_primitives_named_arguments_nested_calls_and_exact_collections() {
    let source = r#"
        pure funcra identity<T>(value: T) -> T { return value; }
        pure funcra pair<A, B>(a: A, b: B) -> tuple<A, B> { return (a, b); }
        pure funcra second<T>(items: array<T, 2>) -> T { return items[1]; }
        pure funcra values<T>(items: map<string, T>) -> list<T> {
            return map_values(items);
        }
        print(identity(100), identity("Scarp"), identity(true), identity(3.14));
        print(pair(b: "Scarp", a: identity::<integer>(7)));
        print(second([array: 10, 20]), values([map: "a": 8])[0]);
        print(identity::<list<array<integer, 0>>>([list: <array<integer, 0>>]));
        identity::<boolean>(false);
        print(3 < 4, 8 >> 1);
    "#;
    assert_eq!(
        run(source),
        "100 Scarp true 3.14\n(7, \"Scarp\")\n20 8\n[list: <array<integer, 0>>]\ntrue 4\n"
    );
}

#[test]
fn expected_type_and_defaults_select_a_specialization() {
    let source = r#"
        pure funcra vacant<T>() -> list<T> { return [list: <T>]; }
        pure funcra defaulted<T>(head: T, next: T = head) -> T
            ensures result == next;
        { bind local: T = next; return local; }
        pure funcra forwarded<T>() -> list<T> { return (vacant()); }
        bind from_annotation: list<string> = (vacant());
        bind from_components: tuple<list<boolean>, list<float>> = (vacant(), vacant());
        bind from_branch: list<string> = if true { vacant() } else { vacant() };
        print(from_annotation, forwarded::<integer>(), defaulted(29), list_len(from_branch));
        print(from_components);
        print(vacant::<record Item>());
        record Item { value: integer }
    "#;
    assert_eq!(run(source), "[list: <string>] [list: <integer>] 29 0\n([list: <boolean>], [list: <float>])\n[list: <record Item>]\n");
}

#[test]
fn constraints_keep_existing_precision_ordering_and_runtime_budget_rules() {
    let source = r#"
        pure funcra square<T: Numeric>(x: T) -> T { return x * x; }
        pure funcra maximum<T: Comparable>(x: T, y: T) -> T {
            if x > y { return x; }
            return y;
        }
        pure funcra use_both<T: Numeric>(x: T, y: T) -> T {
            return maximum(square(x), y);
        }
        print(square(5), square(0.1d), square(1.5), square(bigint("10000000000")));
        print(maximum((1, 3), (1, 9)), use_both(3, 7));
    "#;
    assert_eq!(
        run(source),
        "25 0.01 2.25 100000000000000000000\n(1, 9) 9\n"
    );
    let overflow = scarp::run(
        "pure funcra square<T: Numeric>(x:T)->T{return x*x;} print(square(9223372036854775807));",
        &mut Vec::new(),
    )
    .unwrap_err();
    assert!(
        matches!(overflow, scarp::RunError::Diagnostic(d) if d.kind == Kind::ArithmeticOverflow)
    );
}

#[test]
fn recursion_reuses_concrete_frames_and_preserves_returned_shared_values() {
    let source = r#"
        pure funcra left<T>(n: integer, value: T) -> T {
            if n == 0 { return value; }
            return right(n - 1, value);
        }
        pure funcra right<T>(n: integer, value: T) -> T {
            if n == 0 { return value; }
            return left::<T>(n - 1, value);
        }
        pure funcra countdown<T>(n: integer, value: T) -> T {
            if n == 0 { return value; }
            return countdown(n - 1, value);
        }
        funcra emit<T>(value: T) -> unit { print(value); }
        var source = [list: "Scarp"];
        bind saved = left(4, source);
        source = [list: "new"];
        emit(saved[0]);
        print(countdown(1000, "retained"), countdown(1, 13));
    "#;
    assert_eq!(run(source), "Scarp\nretained 13\n");
    let program = scarp::compile(source).unwrap();
    assert!(program.functions()[0..3].iter().all(|f| f.recursive));
    assert_eq!(program.functions()[0].callees, ["right"]);
}

#[test]
fn invalid_programs_report_the_right_generic_boundary() {
    let cases = [
        (
            "funcra f<T,T>(a:T)->T{return a;}",
            Kind::GenericDefinition,
            "duplicate type parameter",
        ),
        (
            "funcra f<T>(a:Other)->T{return a;}",
            Kind::GenericDefinition,
            "unknown type or type parameter",
        ),
        (
            "funcra f<T: Arithmetic>(a:T)->T{return a;}",
            Kind::GenericDefinition,
            "unknown constraint",
        ),
        (
            "funcra f<T>(a:T)->T{return a;} f::<integer,string>(1);",
            Kind::GenericArguments,
            "expects 1 type arguments",
        ),
        (
            "funcra f(a:integer)->integer{return a;} f::<integer>(1);",
            Kind::GenericArguments,
            "expects 0 type arguments",
        ),
        (
            "to_float::<integer>(1);",
            Kind::GenericArguments,
            "does not accept explicit",
        ),
        (
            "funcra f<T>(a:T,b:T)->T{return a;} f(1,2.0);",
            Kind::GenericArguments,
            "conflicting types for T",
        ),
        (
            "funcra f<T>(a:T,b:T)->T{return a;} f::<float>(1,2.0);",
            Kind::TypeMismatch,
            "expected float, found integer",
        ),
        (
            "funcra f<T>() -> list<T>{return [list:<T>];} print(f());",
            Kind::GenericArguments,
            "cannot infer type parameter T",
        ),
        (
            "funcra f<T: Numeric>(a:T)->T{return a*a;} f(\"text\");",
            Kind::GenericArguments,
            "requires Numeric, found string",
        ),
        (
            "funcra f<T: Comparable>(a:T)->T{return a;} f(true);",
            Kind::GenericArguments,
            "requires Comparable, found boolean",
        ),
        (
            "funcra f<T>(a:T)->T{return a+1;}",
            Kind::TypeMismatch,
            "found T",
        ),
        (
            "funcra f<T>(a:T)->T{return 1;}",
            Kind::InvalidReturn,
            "produce T",
        ),
        (
            "funcra f<T>(a:T)->T{return a;} pure funcra g()->integer{return f(1);}",
            Kind::ImpureFunction,
            "cannot call impure function",
        ),
    ];
    for (source, kind, message) in cases {
        let error = scarp::compile(source).unwrap_err();
        assert_eq!(error.kind, kind, "{source} {error:?}");
        assert!(error.message.contains(message), "{source} {error:?}");
        assert!(
            error.span.start < error.span.end && error.span.end <= source.len(),
            "{error:?}"
        );
    }
}

#[test]
fn reused_instances_are_hidden_from_source_metadata_and_host_templates_cannot_run() {
    let source = "pure funcra echo<T>(x:T)->T{return x;} pure funcra wrapper(x:integer)->integer{return echo(x);} echo(1); echo(2); echo(true);";
    let program = scarp::compile(source).unwrap();
    assert_eq!(program.functions().len(), 2);
    assert_eq!(program.generic_specialization_count(), 2);
    assert!(program.function_type_id(2).is_none());
    let template = program
        .type_registry()
        .get(program.function_type_id(0).unwrap());
    assert!(
        matches!(template, Some(TypeKind::Function { parameters, .. })
        if matches!(program.type_registry().get(parameters[0]), Some(TypeKind::Parameter { .. })))
    );
    let signatures: Vec<_> = program
        .typed_expressions()
        .filter(|expr| expr.context() == ExpressionContext::Entry)
        .flat_map(|expr| {
            expr.nodes()
                .filter_map(|node| node.call_signature)
                .collect::<Vec<_>>()
        })
        .collect();
    assert_eq!(signatures.len(), 3);
    assert_eq!(signatures[0], signatures[1]);
    assert_ne!(signatures[1], signatures[2]);
    assert_eq!(signatures[0], program.function_type_id(1).unwrap());
    let catalog = CallableCatalog::from_program(program);
    assert_eq!(
        catalog.bind_exact(0).unwrap_err(),
        CallableBindError::GenericFunction(0)
    );
    let wrapper = catalog.bind_exact(1).unwrap();
    assert_eq!(
        wrapper
            .invoke_with_io_and_options(
                vec![scarp::value::Value::Integer(42)],
                &mut Vec::new(),
                &mut Vec::new(),
                ExecutionOptions::default()
            )
            .unwrap(),
        scarp::value::Value::Integer(42)
    );
}

#[test]
fn linked_generic_calls_resolve_type_arguments_and_byte_spans() {
    let mut loader = MemorySourceLoader::new();
    let entry = r#"import "lib.srp" as lib;
        bind value = lib.echo::<record lib.Item>(new lib.Item { value: "Scarp" });
        print(value);"#;
    loader.insert("main.srp", entry).unwrap();
    loader.insert("lib.srp", "// 🌍\r\npub record Item { value: string }\r\npub pure funcra echo<T>(x: T) -> T { return x; }").unwrap();
    let linked =
        modules::compile_with_loader("main.srp", &loader, ModuleLimits::default()).unwrap();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    linked
        .execute(&mut out, &mut err, ExecutionOptions::default())
        .unwrap();
    assert!(String::from_utf8(out).unwrap().contains("Scarp"));
    assert!(err.is_empty());
    let parsed =
        scarp::parse("// 🌍\r\nbind value = echo::<list<list<integer>>>([list: <list<integer>>]);")
            .unwrap();
    let Statement::Declaration {
        initializer: expression,
        ..
    } = &parsed.statements[0]
    else {
        panic!("expected binding");
    };
    let ExpressionKind::UserCall {
        type_arguments: Some(type_arguments),
        ..
    } = &expression.nodes()[expression.root()].kind
    else {
        panic!("expected call");
    };
    assert_eq!(type_arguments[0].kind.to_string(), "list<list<integer>>");
    let source = "// 🌍\r\nbind value = echo::<list<list<integer>>>([list: <list<integer>>]);";
    let span = type_arguments[0].span;
    assert_eq!(&source[span.start..span.end], "list<list<integer>>");
}

#[test]
fn repeated_specializations_and_type_growth_have_bounded_outcomes() {
    let mut repeated = String::from("pure funcra f<T>(x:T)->T{return x;}\n");
    for _ in 0..1500 {
        repeated.push_str("f(1);\n");
    }
    assert_eq!(
        scarp::compile(&repeated)
            .unwrap()
            .generic_specialization_count(),
        1
    );

    let mut varied = String::from("pure funcra f<T>(x:T)->T{return x;}\n");
    for length in 0..257 {
        varied.push_str(&format!(
            "pure funcra use_{length}(x:array<integer,{length}>)->integer {{ f(x); return 0; }}\n"
        ));
    }
    assert_eq!(
        scarp::compile(&varied).unwrap_err().kind,
        Kind::GenericLimit
    );

    let error =
        scarp::compile("pure funcra grow<T>(x:T)->unit { grow((x,)); } grow(1);").unwrap_err();
    assert!(matches!(error.kind, Kind::GenericLimit | Kind::TupleLimit));
    assert!(error
        .related
        .iter()
        .any(|r| r.message.contains("specialization")));

    let result =
        scarp::compile("pure funcra f<T>(x:T)->tuple<T,T>{return (x,x);} bind y=f(false);")
            .unwrap();
    assert_eq!(
        result.binding_types()[0].tuple_elements().unwrap(),
        &[P::Boolean, P::Boolean]
    );
}

#[test]
fn abstract_signature_ids_remain_lexical_and_call_sites_show_the_substitution() {
    let source = "pure funcra first<T>(x:T)->T{return second(x);} pure funcra second<T>(x:T)->T{return x;} print(first(1));";
    let program = scarp::compile(source).unwrap();
    let other = scarp::compile(source).unwrap();
    assert_ne!(program.functions()[0].result, program.functions()[1].result);
    assert_ne!(program.functions()[0].result, other.functions()[0].result);
    let typed = program
        .typed_expressions()
        .find(|e| e.context() == ExpressionContext::FunctionBody { function: 0 })
        .unwrap();
    let node = typed.nodes().find(|n| n.call_signature.is_some()).unwrap();
    assert!(matches!(
        program.type_registry().get(node.call_signature.unwrap()),
        Some(TypeKind::Function { result, .. }) if *result == node.kind
    ));
    assert_eq!(program.generic_specialization_count(), 2);
}

#[test]
fn specialized_collection_shape_failure_precedes_execution() {
    let source = r#"
        pure funcra grow<T>(part: array<T, 8>) -> integer {
            return array_len(array_concat(part, part));
        }
        pure funcra wrapper(part: array<array<integer, 4096>, 8>) -> integer {
            return grow(part);
        }
    "#;
    let error = scarp::compile(source).unwrap_err();
    assert_eq!(error.kind, Kind::ArrayLimit);
    assert!(error
        .related
        .iter()
        .any(|r| r.message.contains("specialization")));
}

#[test]
fn native_entry_requires_a_concrete_integer_wrapper() {
    let source = r#"
        pure funcra generic<T>(x:T)->T {return x;}
        pure funcra entry(x:integer)->integer {return generic(x);}
    "#;
    let error =
        scarp::native_object::emit_elf64_bounded_integer_entry(source, "generic", 100).unwrap_err();
    assert_eq!(error.kind, Kind::NativeUnsupported);
    assert!(error.message.contains("integer wrapper"));
}

#[test]
fn generated_tuple_substitution_preserves_types_and_values() {
    for depth in 0..=10 {
        let mut arguments = ["integer", "string"].map(str::to_owned);
        let mut values = ["7", "\"text\""].map(str::to_owned);
        for _ in 0..depth {
            for (kind, value) in arguments.iter_mut().zip(&mut values) {
                *kind = format!("tuple<{kind}>");
                *value = format!("({value},)");
            }
        }
        let [left, right] = &arguments;
        let [a, b] = &values;
        let source = format!(
            "pure funcra pack<A,B>(a:A,b:B)->tuple<B,A>{{return (b,a);}}\n\
             bind actual:tuple<{right},{left}> = pack({a},{b});\n\
             assert(actual == ({b},{a}));"
        );
        assert!(run(&source).is_empty());
    }
}

#[test]
fn nested_return_only_inference_uses_concrete_callee_and_assignment_types() {
    let source = r#"
        pure funcra empty<T>() -> list<T> { return [list: <T>]; }
        pure funcra sort_key(values: list<integer>, tag: string) -> integer {
            return list_len(values) + text_len(tag);
        }
        pure funcra unwrap(values: tuple<list<boolean>>) -> integer {
            return list_len(values[0]);
        }
        var words = [list: "before"];
        words = empty();
        var group = ([list: 1], true);
        group[0] = empty();
        print(sort_key((empty()), "aa"), sort_key(tag: "b", values: empty()), unwrap((empty(),)));
        print(list_len(words), list_len(group[0]));
    "#;
    assert_eq!(run(source), "2 1 0\n0 0\n");
}

#[test]
fn a_fixed_parameter_of_a_generic_function_guides_a_named_argument() {
    let source = r#"
        pure funcra empty<T>() -> list<T> { return [list: <T>]; }
        pure funcra take<T>(value: T, ids: list<integer>) -> T {
            return value;
        }
        print(take(ids: empty(), value: "inferred"), take::<boolean>(true, empty()));
    "#;
    assert_eq!(run(source), "inferred true\n");
}

#[test]
fn nested_calls_use_explicit_and_result_inferred_parameter_types() {
    let source = r#"
        pure funcra blank<T>() -> list<T> { return [list: <T>]; }
        pure funcra identity<T>(value: T) -> T { return value; }
        pure funcra size<T>(items: list<T>) -> integer { return list_len(items); }
        pure funcra nested<T>() -> list<T> { return identity(identity(blank())); }
        bind result: list<integer> = identity(identity(blank()));
        assert(list_len(result) == 0);
        assert(size::<string>(items: (identity(blank()))) == 0);
        assert(size::<integer>(nested()) == 0);
    "#;
    let mut output = Vec::new();
    scarp::run(source, &mut output).unwrap();
    assert!(output.is_empty());
}

#[test]
fn partial_result_context_guides_nested_calls_across_modules() {
    let entry = r#"import "lib.srp" as lib;
        bind ids: list<integer> = lib.keep(tag: "ids", items: lib.blank());
        bind flags: list<boolean> = lib.keep(items: (lib.blank()), tag: 7);
        bind more: list<integer> = lib.keep(lib.blank(), "more");
        print(ids, flags, more);"#;
    let mut loader = MemorySourceLoader::new();
    loader.insert("main.srp", entry).unwrap();
    loader
        .insert(
            "lib.srp",
            "pub pure funcra blank<T>() -> list<T> { return [list: <T>]; }
             pub pure funcra keep<T, U>(items: list<T>, tag: U) -> list<T> { return items; }",
        )
        .unwrap();
    let linked =
        modules::compile_with_loader("main.srp", &loader, ModuleLimits::default()).unwrap();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    linked
        .execute(&mut out, &mut err, ExecutionOptions::default())
        .unwrap();
    assert_eq!(
        out,
        b"[list: <integer>] [list: <boolean>] [list: <integer>]\n"
    );
    assert!(err.is_empty());
}

#[test]
fn partial_context_preserves_constraints_and_does_not_guess_missing_types() {
    let declarations = "pure funcra blank<T>() -> list<T> { return [list: <T>]; }
        pure funcra keep<T, U: Numeric>(items: list<T>, tag: U) -> list<T> { return items; }";
    let check = |script| scarp::compile(&format!("{declarations}\n{script}")).unwrap_err();
    let missing = check("bind items = keep(blank(), 2);");
    assert_eq!(missing.kind, Kind::GenericArguments);
    assert!(missing.message.contains("cannot infer type parameter T"));
    let constraint = check("bind items: list<integer> = keep(blank(), true);");
    assert_eq!(constraint.kind, Kind::GenericArguments);
    assert!(constraint.message.contains("U requires Numeric"));
    let mismatch = check("bind items: list<integer> = keep(blank::<boolean>(), 2);");
    assert_eq!(mismatch.kind, Kind::TypeMismatch);
}
