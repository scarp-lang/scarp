use scarp::{
    diagnostics::DiagnosticKind,
    modules::{self, MemorySourceLoader},
    types::TypeKind,
};

fn compile(entry: &str, sources: &[(&str, &str)]) -> modules::ModuleProgram {
    let mut loader = MemorySourceLoader::new();
    loader.insert("main.srp", entry).unwrap();
    for (name, source) in sources {
        loader.insert(name, source).unwrap();
    }
    modules::compile_with_loader("main.srp", &loader, Default::default()).unwrap()
}

fn run(entry: &str, sources: &[(&str, &str)]) -> String {
    let program = compile(entry, sources);
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    program
        .execute(&mut stdout, &mut stderr, Default::default())
        .unwrap();
    assert!(stderr.is_empty());
    String::from_utf8(stdout).unwrap()
}

#[test]
fn imported_and_locally_constructed_options_share_identity() {
    let lib = r#"
        pub pure funcra read(n: integer) -> option<integer> {
            return new option<integer>::Some(n);
        }
    "#;
    let main = r#"
        import "lib.srp" as lib;
        bind imported: option<integer> = lib.read(12);
        bind local: option<integer> = new option<integer>::Some(5);
        print(imported == local);
        match on imported {
            case None { print("no value"); }
            case Some(value) { print(value); }
        }
    "#;
    assert_eq!(run(main, &[("lib.srp", lib)]), "false\n12\n");
}

#[test]
fn result_propagation_crosses_module_boundaries_with_exact_error_type() {
    let lib = r#"
        pub pure funcra parse(n: integer) -> result<integer, string> {
            if n < 0 {
                return new result<integer, string>::Err("negative");
            }
            return new result<integer, string>::Ok(n);
        }
    "#;
    let main = r#"
        import "lib.srp" as lib;
        pure funcra forward(n: integer) -> result<boolean, string> {
            bind number: integer = lib.parse(n)?;
            return new result<boolean, string>::Ok(number > 0);
        }
        print(forward(-1));
        print(forward(6));
    "#;
    assert_eq!(
        run(main, &[("lib.srp", lib)]),
        "new result<boolean, string>::Err(\"negative\")\nnew result<boolean, string>::Ok(true)\n"
    );
}

#[test]
fn imported_record_in_option_payload_canonicalizes_aliases() {
    let lib = r#"
        pub record Item { count: integer }
        pub pure funcra wrap(n: integer) -> option<record Item> {
            return new option<record Item>::Some(new Item { count: n });
        }
    "#;
    let main = r#"
        import "lib.srp" as lib;
        bind wrapped: option<record lib.Item> = lib.wrap(33);
        match on wrapped {
            case None {}
            case Some(item) { print(item->count); }
        }
    "#;
    assert_eq!(run(main, &[("lib.srp", lib)]), "33\n");
}

#[test]
fn identically_named_private_record_types_do_not_share_option_identity() {
    let a = r#"
        record Item { count: integer }
        pub pure funcra get() -> option<record Item> {
            return new option<record Item>::Some(new Item { count: 1 });
        }
    "#;
    let b = r#"
        record Item { count: integer }
        pub pure funcra get() -> option<record Item> {
            return new option<record Item>::Some(new Item { count: 2 });
        }
    "#;
    let entry = r#"
        import "a.srp" as a;
        import "b.srp" as b;
        bind first = a.get();
        bind second = b.get();
    "#;
    let mut loader = MemorySourceLoader::new();
    loader.insert("main.srp", entry).unwrap();
    loader.insert("a.srp", a).unwrap();
    loader.insert("b.srp", b).unwrap();
    let result = modules::compile_with_loader("main.srp", &loader, Default::default());
    // Exporting an optional private record still violates visibility.
    let err = result.unwrap_err();
    assert_eq!(err.diagnostic.kind, DiagnosticKind::ModuleVisibility);
}

#[test]
fn nested_plain_specializations_share_type_identity_across_modules() {
    let lib = r#"
        pub pure funcra nested() -> option<result<integer, string>> {
            return new option<result<integer, string>>::Some(
                new result<integer, string>::Ok(41)
            );
        }
    "#;
    let main = r#"
        import "lib.srp" as lib;
        bind value: option<result<integer, string>> = lib.nested();
        match on value {
            case None {}
            case Some(inner) {
                match on inner {
                    case Ok(number) { print(number); }
                    case Err(message) { print(message); }
                }
            }
        }
    "#;
    let program = compile(main, &[("lib.srp", lib)]);
    let mut out = Vec::new();
    program
        .execute(&mut out, &mut Vec::new(), Default::default())
        .unwrap();
    assert_eq!(out, b"41\n");
    let ty = program.program().binding_types()[0].as_variant().unwrap();
    assert!(matches!(
        program.program().type_registry().get(ty.type_id()),
        Some(TypeKind::Applied { constructor, arguments })
            if constructor.module == u32::MAX && arguments.len() == 1
    ));
}
