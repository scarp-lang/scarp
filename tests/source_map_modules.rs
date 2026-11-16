use scarp::{
    diagnostics::DiagnosticKind,
    modules::{self, MemorySourceLoader},
};

fn compile(main: &str, files: &[(&str, &str)]) -> modules::ModuleProgram {
    let mut loader = MemorySourceLoader::new();
    loader.insert("main.srp", main).unwrap();
    for (name, source) in files {
        loader.insert(*name, *source).unwrap();
    }
    modules::compile_with_loader("main.srp", &loader, Default::default()).unwrap()
}

fn execute(main: &str, files: &[(&str, &str)]) -> String {
    let compiled = compile(main, files);
    let mut output = Vec::new();
    let mut errors = Vec::new();
    compiled
        .execute(&mut output, &mut errors, Default::default())
        .unwrap();
    assert!(errors.is_empty());
    String::from_utf8(output).unwrap()
}

#[test]
fn map_of_imported_record_retains_exact_nominal_type_and_visibility() {
    let library = r#"
        pub record Item { label: string, done: boolean }
        pub pure funcra create() -> map<string, record Item> {
            return [map: "a": new Item { label: "Draft", done: false }];
        }
    "#;
    let main = r#"
        import "lib.srp" as l;
        bind before: map<string, record l.Item> = l.create();
        bind after = map_set(before, "b", new l.Item {
            label: "Review", done: true
        });
        bind selected = map_get_or(after, "b", new l.Item {
            label: "Fallback", done: false
        });
        print(map_len(before), map_len(after), selected->label, selected->done);
    "#;
    assert_eq!(execute(main, &[("lib.srp", library)]), "1 2 Review true\n");
    let program = compile(main, &[("lib.srp", library)]);
    assert_eq!(program.inspection_schema_version(), 4);
}

#[test]
fn pure_cross_module_map_operations_do_not_change_prior_snapshots() {
    let library = r#"
        pub pure funcra advance(
            items: map<integer, string>,
            key: integer,
            name: string
        ) -> map<integer, string> {
            return map_set(items, key, name);
        }
        pub pure funcra remove(
            items: map<integer, string>,
            key: integer
        ) -> map<integer, string> {
            return map_remove(items, key);
        }
    "#;
    let main = r#"
        import "lib.srp" as l;
        bind original = [map: 10: "ten"];
        bind added = l.advance(original, -1, "minus");
        bind removed = l.remove(added, 10);
        print(original, added, removed);
        print(map_get_or(added, 10, "missing"));
    "#;
    assert_eq!(
        execute(main, &[("lib.srp", library)]),
        "[map: 10: \"ten\"]\n[map: -1: \"minus\", 10: \"ten\"]\n[map: -1: \"minus\"]\nten\n"
    );
}

#[test]
fn nested_map_values_preserve_imported_record_type_and_order() {
    let library = r#"
        pub record Entry { count: integer }
        pub pure funcra nested() -> map<boolean, list<record Entry>> {
            return [map:
                true: [list: new Entry { count: 2 }],
                false: [list: new Entry { count: 1 }],
            ];
        }
    "#;
    let main = r#"
        import "lib.srp" as l;
        bind nested: map<boolean, list<record l.Entry>> = l.nested();
        bind fallback: list<record l.Entry> = [list: <record l.Entry>];
        bind values = map_get_or(nested, false, fallback);
        print(values[0]->count);
        print(nested);
    "#;
    let actual = execute(main, &[("lib.srp", library)]);
    assert!(actual.starts_with("1\n[map: false:"), "{actual}");
}

#[test]
fn private_record_cannot_be_exposed_as_a_public_map_value() {
    let library = r#"
        record Private { count: integer }
        pub pure funcra leak() -> map<string, record Private> {
            return [map: "secret": new Private { count: 10 }];
        }
    "#;
    let mut loader = MemorySourceLoader::new();
    loader
        .insert("main.srp", "import \"lib.srp\" as l;")
        .unwrap();
    loader.insert("lib.srp", library).unwrap();
    let failure =
        modules::compile_with_loader("main.srp", &loader, Default::default()).unwrap_err();
    assert_eq!(failure.diagnostic.kind, DiagnosticKind::ModuleVisibility);
}

#[test]
fn runtime_duplicate_map_keys_abort_cross_module_program_output() {
    let library = r#"
        pub pure funcra bad() -> map<integer, integer> {
            return [map: 7: 1, 3+4: 2];
        }
    "#;
    let main = r#"
        import "lib.srp" as l;
        print("should not publish");
        print(l.bad());
    "#;
    let program = compile(main, &[("lib.srp", library)]);
    let mut output = Vec::new();
    let mut stderr = Vec::new();
    let failure = program
        .execute(&mut output, &mut stderr, Default::default())
        .unwrap_err();
    assert!(format!("{failure:?}").contains("MapKey"), "{failure:?}");
    assert!(output.is_empty());
}

#[test]
fn native_code_generation_explicitly_rejects_map_operations() {
    let compiled = scarp::compile("bind m = [map: 1: 2]; print(map_set(m, 2, 3));").unwrap();
    assert_eq!(
        scarp::c_backend::emit_program(&compiled).unwrap_err().kind,
        DiagnosticKind::NativeUnsupported
    );
}
