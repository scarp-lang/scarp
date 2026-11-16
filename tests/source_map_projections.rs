use scarp::{diagnostics::DiagnosticKind, ExecutionOptions};

fn output(source: &str) -> String {
    let mut bytes = Vec::new();
    scarp::run(source, &mut bytes).unwrap();
    String::from_utf8(bytes).unwrap()
}

#[test]
fn ordered_map_projection_example_is_executable() {
    assert_eq!(
        output(include_str!("../examples/map_projections.srp")).as_bytes(),
        include_bytes!("../examples/map_projections.stdout")
    );
}

#[test]
fn projection_results_keep_canonical_key_value_pairing() {
    let source = r#"
        bind a = [map: -5: "first", 100: "last", 0: "middle"];
        bind keys: list<integer> = map_keys(a);
        bind values: list<string> = map_values(a);
        print(keys, values);
        print(keys[0], values[0], keys[2], values[2]);
    "#;
    assert_eq!(
        output(source),
        "[list: -5, 0, 100] [list: \"first\", \"middle\", \"last\"]\n-5 first 100 last\n"
    );
}

#[test]
fn empty_map_projections_preserve_exact_element_types() {
    let source = r#"
        bind m: map<boolean, list<integer>> = [map: <boolean, list<integer>>];
        bind keys: list<boolean> = map_keys(m);
        bind values: list<list<integer>> = map_values(m);
        print(keys);
        print(values);
    "#;
    assert_eq!(
        output(source),
        "[list: <boolean>]\n[list: <list<integer>>]\n"
    );
}

#[test]
fn projected_lists_do_not_mutate_source_map_or_previous_snapshots() {
    let source = r#"
        bind original = [map: "b": 2, "a": 1];
        bind snapshot = original;
        bind keys = map_keys(original);
        bind values = map_values(original);
        bind changed = map_set(original, "c", 3);
        print(keys, values);
        print(map_len(snapshot), map_len(changed));
        print(map_keys(changed));
    "#;
    assert_eq!(
        output(source),
        "[list: \"a\", \"b\"] [list: 1, 2]\n2 3\n[list: \"a\", \"b\", \"c\"]\n"
    );
}

#[test]
fn maps_of_records_project_values_with_nominal_identity() {
    let source = r#"
        record Task { title: string, done: boolean }
        bind tasks: map<integer, record Task> = [map:
            5: new Task { title: "Review", done: false },
            2: new Task { title: "Draft", done: true }
        ];
        bind values: list<record Task> = map_values(tasks);
        print(values[0]->title, values[1]->title);
        print(map_keys(tasks));
    "#;
    assert_eq!(output(source), "Draft Review\n[list: 2, 5]\n");
}

#[test]
fn map_projection_intrinsics_require_one_positional_map_argument() {
    for source in [
        "print(map_keys(12));",
        "print(map_values([list: 1]));",
    ] {
        let error = scarp::compile(source).unwrap_err();
        assert_eq!(error.kind, DiagnosticKind::TypeMismatch, "{source}");
    }
    for source in [
        "print(map_keys());",
        "print(map_values());",
        "print(map_keys([map: 1: true], 1));",
        "print(map_values(m: [map: 1: true]));",
    ] {
        let error = scarp::compile(source).unwrap_err();
        assert_eq!(error.kind, DiagnosticKind::CallArguments, "{source}");
    }
}

#[test]
fn explicitly_declared_functions_can_shadow_projection_intrinsics() {
    let source = r#"
        pure funcra map_keys(input: integer) -> integer {
            return input + 1;
        }
        print(map_keys(41));
    "#;
    assert_eq!(output(source), "42\n");
}

#[test]
fn map_projection_budget_failure_does_not_publish_partial_output() {
    let source = "print(\"prior\"); bind m = [map: 1: 2]; print(map_values(m));";
    let mut output = Vec::new();
    let failure = scarp::run_with_options(
        source,
        &mut output,
        ExecutionOptions {
            limits: scarp::ExecutionLimits {
                max_steps: 4,
                max_prints: 100,
            },
            ..ExecutionOptions::default()
        },
    );
    assert!(failure.is_err());
    assert!(output.is_empty());
}

#[test]
fn native_backend_still_rejects_map_projections() {
    let program = scarp::compile(
        "bind m = [map: 1: 2]; print(map_keys(m));",
    )
    .unwrap();
    assert_eq!(
        scarp::c_backend::emit_program(&program).unwrap_err().kind,
        DiagnosticKind::NativeUnsupported
    );
}
