use scarp::{
    diagnostics::DiagnosticKind,
    modules::{self, MemorySourceLoader},
};

fn program(main: &str, sources: &[(&str, &str)]) -> modules::ModuleProgram {
    let mut loader = MemorySourceLoader::new();
    loader.insert("main.srp", main).unwrap();
    for (path, source) in sources {
        loader.insert(*path, *source).unwrap();
    }
    modules::compile_with_loader("main.srp", &loader, Default::default()).unwrap()
}

fn run(main: &str, sources: &[(&str, &str)]) -> String {
    let program = program(main, sources);
    let mut out = Vec::new();
    let mut err = Vec::new();
    program
        .execute(&mut out, &mut err, Default::default())
        .unwrap();
    assert!(err.is_empty());
    String::from_utf8(out).unwrap()
}

#[test]
fn typed_list_of_exported_record_can_cross_modules() {
    let lib = r#"
        pub record Task { title: string, completed: boolean }
        pub pure funcra create(title: string) -> list<record Task> {
            return [list: new Task { title: title, completed: false }];
        }
    "#;
    let main = r#"
        import "lib.srp" as library;
        bind tasks: list<record library.Task> = library.create("Plan");
        print(tasks[0]->title);
        bind expanded = list_push(tasks, new library.Task {
            title: "Review", completed: true
        });
        print(list_len(tasks), list_len(expanded), expanded[1]->title);
    "#;
    assert_eq!(run(main, &[("lib.srp", lib)]), "Plan\n1 2 Review\n");
}

#[test]
fn nested_list_type_identity_preserves_imported_nominal_payloads() {
    let lib = r#"
        pub record Item { count: integer }
        pub pure funcra nested() -> list<list<record Item>> {
            return [list: [list: new Item { count: 42 }]];
        }
    "#;
    let main = r#"
        import "lib.srp" as l;
        bind nested: list<list<record l.Item>> = l.nested();
        print(nested[0][0]->count);
    "#;
    assert_eq!(run(main, &[("lib.srp", lib)]), "42\n");
}

#[test]
fn imported_pure_list_operations_share_the_same_source_type() {
    let lib = r#"
        pub pure funcra extend(items: list<integer>, item: integer) -> list<integer> {
            return list_push(items, item);
        }
    "#;
    let main = r#"
        import "lib.srp" as l;
        bind original: list<integer> = [list: <integer>];
        bind once = l.extend(original, 5);
        bind twice = l.extend(once, 7);
        print(list_len(original), list_len(once), list_len(twice));
        print(twice[1]);
    "#;
    assert_eq!(run(main, &[("lib.srp", lib)]), "0 1 2\n7\n");
}

#[test]
fn a_private_record_cannot_leak_through_exported_list_signature() {
    let lib = r#"
        record Private { count: integer }
        pub pure funcra leak() -> list<record Private> {
            return [list: new Private { count: 1 }];
        }
    "#;
    let mut loader = MemorySourceLoader::new();
    loader
        .insert("main.srp", "import \"lib.srp\" as lib;")
        .unwrap();
    loader.insert("lib.srp", lib).unwrap();
    let error = modules::compile_with_loader("main.srp", &loader, Default::default()).unwrap_err();
    assert_eq!(error.diagnostic.kind, DiagnosticKind::ModuleVisibility);
}

#[test]
fn native_code_generation_rejects_dynamic_list_operations() {
    let compiled = scarp::compile(
        "bind original = [list: 1, 2]; \
         bind changed = list_push(original, 3); \
         print(changed[0]);",
    )
    .unwrap();
    let diagnostic = scarp::c_backend::emit_program(&compiled).unwrap_err();
    assert_eq!(diagnostic.kind, DiagnosticKind::NativeUnsupported);
}

#[test]
fn importing_an_empty_typed_list_does_not_erase_element_identity() {
    let lib = r#"
        pub pure funcra blank() -> list<string> {
            return [list: <string>];
        }
    "#;
    let main = r#"
        import "lib.srp" as l;
        bind blank: list<string> = l.blank();
        bind one = list_push(blank, "first");
        print(list_len(blank), list_len(one), one[0]);
    "#;
    assert_eq!(run(main, &[("lib.srp", lib)]), "0 1 first\n");
}
