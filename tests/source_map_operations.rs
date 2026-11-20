use scarp::{diagnostics::DiagnosticKind, ExecutionOptions};

fn run(source: &str) -> String {
    let mut output = Vec::new();
    scarp::run(source, &mut output).unwrap();
    String::from_utf8(output).unwrap()
}

fn reject(source: &str, kind: DiagnosticKind) {
    let error = scarp::compile(source).unwrap_err();
    assert_eq!(error.kind, kind, "{source}: {error:?}");
}

#[test]
fn pure_persistent_map_example_is_executable() {
    assert_eq!(
        run(include_str!("../examples/persistent_maps.srp")).as_bytes(),
        include_bytes!("../examples/persistent_maps.stdout")
    );
}

#[test]
fn map_snapshots_preserve_exact_key_and_value_identity() {
    let source = r#"
        bind a: map<integer, string> = [map: <integer, string>];
        bind b = map_set(a, 2, "two");
        bind c = map_set(b, 1, "one");
        bind d = map_set(c, 2, "changed");
        bind e = map_remove(d, 1);
        print(map_len(a), map_len(b), map_len(c), map_len(d), map_len(e));
        print(map_get_or(b, 2, "fallback"));
        print(map_get_or(c, 2, "fallback"));
        print(map_get_or(d, 2, "fallback"));
        print(map_contains(e, 1), map_contains(d, 1));
        print(e);
    "#;
    assert_eq!(
        run(source),
        "0 1 2 2 1\ntwo\ntwo\nchanged\nfalse true\n[map: 2: \"changed\"]\n"
    );
}

#[test]
fn remove_missing_key_does_not_discard_any_entry() {
    let source = r#"
        bind first = [map: 5: true];
        bind second = map_remove(first, -99);
        print(first == second, map_len(second));
    "#;
    assert_eq!(run(source), "true 1\n");
}

#[test]
fn map_get_or_returns_typed_fallback_for_empty_map() {
    let source = r#"
        bind settings: map<string, list<integer>> = [map: <string, list<integer>>];
        bind fallback = [list: 9, 10];
        bind selected = map_get_or(settings, "absent", fallback);
        print(selected[1], map_contains(settings, "absent"));
    "#;
    assert_eq!(run(source), "10 false\n");
}

#[test]
fn map_intrinsics_are_checked_for_type_and_arity() {
    for source in [
        "print(map_set([map: 1: true], \"1\", false));",
        "print(map_set([map: 1: true], 1, 0));",
        "print(map_contains([map: 1: true], \"1\"));",
        "print(map_get_or([map: 1: true], 1, 0));",
        "print(map_remove([map: 1: true], false));",
        "print(map_len(42));",
    ] {
        reject(source, DiagnosticKind::TypeMismatch);
    }
    for source in [
        "print(map_set([map: 1: 2], 1));",
        "print(map_get_or([map: 1: 2], 1));",
        "print(map_len());",
        "print(map_remove([map: 1: 2], 1, 4));",
        "print(map_contains(m: [map: 1: 2], k: 1));",
    ] {
        reject(source, DiagnosticKind::CallArguments);
    }
}

#[test]
fn map_intrinsics_are_shadowed_by_explicit_functions() {
    let source = r#"
        pure funcra map_len(value: integer) -> integer { return value + 1; }
        print(map_len(41));
    "#;
    assert_eq!(run(source), "42\n");
}

#[test]
fn pure_map_updates_accept_nested_record_and_list_values() {
    let source = r#"
        record Task { title: string, flags: list<boolean> }
        pure funcra add(original: map<string, record Task>) -> map<string, record Task> {
            return map_set(original, "next", new Task {
                title: "Ready", flags: [list: true, false]
            });
        }
        bind empty: map<string, record Task> = [map: <string, record Task>];
        bind next = add(empty);
        bind current = map_get_or(next, "next", new Task {
            title: "Fallback", flags: [list: false]
        });
        print(current->title, current->flags[0]);
    "#;
    assert_eq!(run(source), "Ready true\n");
}

#[test]
fn inserting_a_new_key_above_the_limit_fails() {
    let mut source = String::from("bind m = [map: ");
    for i in 0..1024 {
        if i != 0 {
            source.push_str(", ");
        }
        source.push_str(&format!("{i}: {i}"));
    }
    source.push_str("]; print(map_set(m, 9999, 0));");
    let mut output = Vec::new();
    let error = scarp::run(&source, &mut output).unwrap_err();
    assert!(format!("{error:?}").contains("MapLimit"), "{error:?}");
    assert!(output.is_empty());
}

#[test]
fn cumulative_collection_quota_applies_to_map_updates() {
    let source = "print(\"earlier\"); bind m = [map: 1: 2]; print(map_set(m, 2, 3));";
    let mut output = Vec::new();
    let error = scarp::run_with_options(
        source,
        &mut output,
        ExecutionOptions {
            max_array_bytes: 0,
            ..ExecutionOptions::default()
        },
    )
    .unwrap_err();
    assert!(format!("{error:?}").contains("MapAllocation"));
    assert!(output.is_empty());
}

#[test]
fn lookup_work_is_metered_for_long_utf8_keys() {
    let source = r#"
        bind m = [map: "क": 1, "ख": 2, "ग": 3];
        print("earlier");
        print(map_get_or(m, "ग", 9));
    "#;
    let mut output = Vec::new();
    let error = scarp::run_with_options(
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
    assert!(error.is_err());
    assert!(output.is_empty());
}

#[test]
fn native_backend_rejects_persistent_map_operations() {
    let program = scarp::compile("bind m = [map: 1: true]; print(map_contains(m, 1));").unwrap();
    assert_eq!(
        scarp::c_backend::emit_program(&program).unwrap_err().kind,
        DiagnosticKind::NativeUnsupported
    );
}

#[test]
fn removing_boundary_keys_keeps_canonical_order_and_prior_snapshot() {
    let source = r#"
        bind original = [map: "z": [list: 3], "a": [list: 1], "m": [list: 2]];
        bind without_first = map_remove(original, "a");
        bind without_last = map_remove(without_first, "z");
        bind empty = map_remove(without_last, "m");
        assert(map_keys(original) == [list: "a", "m", "z"]);
        assert(map_keys(without_first) == [list: "m", "z"]);
        assert(map_values(without_last) == [list: [list: 2]]);
        assert(map_len(empty) == 0);
    "#;
    scarp::run(source, &mut Vec::new()).unwrap();
}
