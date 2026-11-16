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
fn persistent_list_updates_match_executable_task_example() {
    assert_eq!(
        run(include_str!("../examples/persistent_list_updates.srp")).as_bytes(),
        include_bytes!("../examples/persistent_list_updates.stdout")
    );
}

#[test]
fn push_set_remove_and_length_preserve_all_prior_snapshots() {
    let source = r#"
        bind a: list<integer> = [list: <integer>];
        bind b = list_push(a, 9);
        bind c = list_push(b, 10);
        bind d = list_set(c, 0, 12);
        bind e = list_remove(d, 1);
        print(list_len(a), list_len(b), list_len(c), list_len(d), list_len(e));
        print(b[0], c[0], c[1], d[0], e[0]);
    "#;
    assert_eq!(run(source), "0 1 2 2 1\n9 9 10 12 12\n");
}

#[test]
fn nested_typed_lists_and_pure_functions_share_snapshots() {
    let source = r#"
        pure funcra add(nested: list<list<string>>) -> list<list<string>> {
            return list_push(nested, [list: "new", "leaf"]);
        }
        bind original: list<list<string>> = [list: <list<string>>];
        bind next = add(original);
        print(list_len(original), list_len(next), next[0][1]);
    "#;
    assert_eq!(run(source), "0 1 leaf\n");
}

#[test]
fn update_intrinsics_check_exact_element_types_and_indices() {
    for source in [
        "bind x = list_push([list: 1], \"wrong\");",
        "bind x = list_set([list: 1], 0, \"wrong\");",
        "bind x = list_set([list: 1], \"0\", 2);",
        "bind x = list_remove([list: 1], true);",
    ] {
        reject(source, DiagnosticKind::TypeMismatch);
    }
}

#[test]
fn update_intrinsics_require_exact_positional_arity() {
    for source in [
        "list_len();",
        "list_push([list: 1]);",
        "list_set([list: 1], 0);",
        "list_remove([list: 1], 0, 5);",
        "list_push(items: [list: 1], item: 2);",
    ] {
        reject(source, DiagnosticKind::CallArguments);
    }
}

#[test]
fn update_indices_are_checked_and_output_is_not_published_on_failure() {
    for expr in [
        "list_set([list: 1], -1, 10)",
        "list_set([list: 1], 1, 10)",
        "list_remove([list: 1], 8)",
        "list_remove([list: <integer>], 0)",
    ] {
        let source = format!("print(\"before\"); print({expr});");
        let mut output = Vec::new();
        let error = scarp::run(&source, &mut output).unwrap_err();
        assert!(format!("{error:?}").contains("ListIndex"), "{error:?}");
        assert!(output.is_empty(), "{expr} published partial output");
    }
}

#[test]
fn persistent_update_allocation_is_metered_before_publication() {
    let source = "print(\"before\"); bind xs = [list: 1]; print(list_push(xs, 2));";
    let mut output = Vec::new();
    let error = scarp::run_with_options(
        source,
        &mut output,
        ExecutionOptions {
            max_array_bytes: 512,
            ..ExecutionOptions::default()
        },
    );
    assert!(error.is_err(), "two list allocations must exceed a tiny quota");
    assert!(output.is_empty());
}

#[test]
fn user_defined_functions_can_shadow_list_intrinsic_names() {
    let source = r#"
        pure funcra list_push(value: integer, extra: integer) -> integer {
            return value + extra;
        }
        print(list_push(5, 7));
    "#;
    assert_eq!(run(source), "12\n");
}

#[test]
fn malformed_list_operations_do_not_create_partial_snapshots() {
    let source = "print(list_set([list: 1], 4, 5));";
    let mut output = Vec::new();
    let error = scarp::run(source, &mut output).unwrap_err();
    assert!(format!("{error:?}").contains("ListIndex"));
    assert!(output.is_empty());
}
