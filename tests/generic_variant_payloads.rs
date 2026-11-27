use scarp::{
    diagnostics::DiagnosticKind as Kind,
    modules::{self, MemorySourceLoader, ModuleLimits},
    types::{PrimitiveType, TypeKind},
    ExecutionOptions,
};

fn run(source: &str) -> Vec<u8> {
    let mut output = Vec::new();
    scarp::run(source, &mut output).unwrap();
    output
}

#[test]
fn option_construction_matching_and_context_inference_work_with_generic_payloads() {
    let source = r#"
        pure funcra some<T>(value: T) -> option<T> {
            return new option<T>::Some(value);
        }
        pure funcra none<T>() -> option<T> { return new option<T>::None(); }
        pure funcra unwrap<T>(input: option<T>, fallback: T) -> T {
            match on input {
                case Some(value) { return value; }
                case None { return fallback; }
            }
        }
        pure funcra detect<T>(input: option<T>) -> boolean {
            match on input {
                case Some(_) { return true; }
                case None { return false; }
            }
        }
        bind absent: option<integer> = none();
        assert(some(17) == new option<integer>::Some(17));
        print(unwrap(some("Scarp"), "missing"), unwrap(absent, 9), detect(some(true)));
    "#;
    assert_eq!(run(source), b"Scarp 9 true\n");
}

#[test]
fn result_propagation_keeps_the_original_error_identity_and_retypes_success() {
    let source = r#"
        pure funcra wrap<T, E>(input: result<T, E>) -> result<tuple<T>, E> {
            bind value = input?;
            return new result<tuple<T>, E>::Ok((value,));
        }
        pure funcra format<T, E>(input: result<tuple<T>, E>) -> unit {
            match on input {
                case Ok((value,)) { assert(value == value); }
                case Err(error) { assert(error == error); }
            }
        }
        funcra display(input: result<tuple<integer>, string>) -> unit {
            match on input {
                case Ok((value,)) { print(value); }
                case Err(error) { print(error); }
            }
        }
        bind ok = wrap(new result<integer, string>::Ok(12));
        bind failed = wrap(new result<integer, string>::Err("failed"));
        format(ok);
        format(failed);
        display(ok);
        display(failed);
    "#;
    assert_eq!(run(source), b"12\nfailed\n");
}

#[test]
fn nested_products_and_cross_module_values_keep_canonical_case_types() {
    let mut loader = MemorySourceLoader::new();
    loader
        .insert(
            "main.srp",
            r#"
        import "lib.srp" as lib;
        bind a: option<result<integer, string>> = lib.nested(73);
        bind b = new option<result<integer, string>>::Some(new result<integer, string>::Ok(73));
        assert(a == b);
        match on a { case Some(payload) { print(payload); } case None { print("absent"); } }
    "#,
        )
        .unwrap();
    loader
        .insert(
            "lib.srp",
            r#"
        pub pure funcra nested<T>(number: T) -> option<result<T, string>> {
            bind result_value: result<T, string> = new result<T, string>::Ok(number);
            return new option<result<T, string>>::Some(result_value);
        }
    "#,
        )
        .unwrap();
    let program =
        modules::compile_with_loader("main.srp", &loader, ModuleLimits::default()).unwrap();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    program
        .execute(&mut out, &mut err, ExecutionOptions::default())
        .unwrap();
    assert!(String::from_utf8(out).unwrap().contains("73"));
    assert!(err.is_empty());
}

#[test]
fn abstract_and_specialized_variants_reuse_the_applied_type_registry() {
    let program = scarp::compile(
        r#"
        pure funcra make<T>(value: T) -> option<T> { return new option<T>::Some(value); }
        bind inferred = make(7);
        bind declared: option<integer> = inferred;
    "#,
    )
    .unwrap();
    let registry = program.type_registry();
    let Some(TypeKind::Function {
        result, parameters, ..
    }) = registry.get(program.function_type_id(0).unwrap())
    else {
        panic!("generic signature");
    };
    assert!(
        matches!(registry.get(*result), Some(TypeKind::Applied { arguments, .. })
        if arguments.as_ref() == parameters.as_ref())
    );
    let [a, b] = program.binding_type_ids() else {
        panic!("binding types");
    };
    assert_eq!(a, b);
    assert!(
        matches!(registry.get(*a), Some(TypeKind::Applied { arguments, .. })
        if arguments.as_ref() == [PrimitiveType::Integer.id()])
    );
}

#[test]
fn invalid_payloads_and_families_are_rejected_at_compile_time() {
    let cases = [
        ("pure funcra wrap<T>(x:T)->option<T>{return new option<T>::Some(1);}", Kind::TypeMismatch),
        ("pure funcra get<T>(x:option<T>)->option<T>{return x;} get(new result<integer,string>::Ok(1));", Kind::GenericArguments),
        ("pure funcra get<T>(x:option<T>)->boolean{match on x{case Some(v){return true;}}}", Kind::MatchCoverage),
        ("pure funcra f<T>(x:result<integer,T>)->result<integer,string>{bind v=x?;return new result<integer,string>::Ok(v);}", Kind::InvalidReturn),
    ];
    for (source, kind) in cases {
        let error = scarp::compile(source).unwrap_err();
        assert_eq!(error.kind, kind, "{source}: {error:?}");
    }
}

#[test]
fn a_public_generic_signature_cannot_expose_a_private_nominal_payload() {
    let mut loader = MemorySourceLoader::new();
    loader
        .insert("main.srp", "import \"lib.srp\" as lib;")
        .unwrap();
    loader
        .insert(
            "lib.srp",
            r#"
        record Private { value: integer }
        pub pure funcra hidden<T>(x: T) -> result<T, record Private> {
            return new result<T, record Private>::Ok(x);
        }
    "#,
        )
        .unwrap();
    let error =
        modules::compile_with_loader("main.srp", &loader, ModuleLimits::default()).unwrap_err();
    assert_eq!(error.diagnostic.kind, Kind::ModuleVisibility);
}

#[test]
fn declared_record_fields_and_variant_payloads_guide_inner_calls() {
    let source = r#"
        pure funcra empty<T>() -> list<T> { return [list: <T>]; }
        record Rows { words: list<string>, flags: tuple<list<boolean>> }
        variant Event { Rows(list<integer>), Idle }
        bind rows = new Rows { flags: (empty(),), words: (empty()) };
        assert(rows->words == [list: <string>]);
        assert(rows->flags == ([list: <boolean>],));
        assert(new Event::Rows(empty()) == new Event::Rows([list: <integer>]));
        assert(new option<list<boolean>>::Some(empty()) == new option<list<boolean>>::Some([list: <boolean>]));
        pure funcra outcome<A, B>() -> result<list<A>, list<B>> {
            return new result<list<A>, list<B>>::Err(empty());
        }
        assert(outcome::<integer,string>() == new result<list<integer>, list<string>>::Err([list: <string>]));
    "#;
    assert!(run(source).is_empty());
}

#[test]
fn imported_constructor_context_preserves_nominal_identity() {
    let mut loader = MemorySourceLoader::new();
    loader
        .insert(
            "lib.srp",
            r#"
        pub record Item { value: integer }
        pub record Boxed { items: list<record Item> }
        pub variant Event { Box(list<record Item>) }
    "#,
        )
        .unwrap();
    loader
        .insert(
            "main.srp",
            r#"
        import "lib.srp" as lib;
        record Item { value: string }
        pure funcra empty<T>() -> list<T> { return [list: <T>]; }
        bind boxed = new lib.Boxed { items: empty() };
        bind typed: list<record lib.Item> = boxed->items;
        assert(new lib.Event::Box(empty()) == new lib.Event::Box(typed));
    "#,
        )
        .unwrap();
    let program =
        modules::compile_with_loader("main.srp", &loader, ModuleLimits::default()).unwrap();
    program
        .execute(
            &mut Vec::new(),
            &mut Vec::new(),
            ExecutionOptions::default(),
        )
        .unwrap();
}

#[test]
fn constructor_context_keeps_explicit_types_and_constraints() {
    let empty = "pure funcra empty<T: Numeric>() -> list<T> { return [list: <T>]; }";
    let cases = [
        (
            "record R { xs: list<string> } bind r = new R { xs: empty() };",
            Kind::GenericArguments,
        ),
        (
            "variant V { X(list<boolean>) } bind v = new V::X(empty::<integer>());",
            Kind::TypeMismatch,
        ),
    ];
    for (program, kind) in cases {
        let error = scarp::compile(&format!("{empty} {program}")).unwrap_err();
        assert_eq!(error.kind, kind);
        if kind == Kind::GenericArguments {
            assert!(error.message.contains("Numeric"), "{error:?}");
        }
    }
}
