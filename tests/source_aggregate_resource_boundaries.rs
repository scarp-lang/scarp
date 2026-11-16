//! Reproducible aggregate resource boundaries, independent of host wall-clock time.
//! All programs exercise the source interpreter and must leave output transactional.
use scarp::{diagnostics::DiagnosticKind, ExecutionOptions};

fn execute(source: &str) -> String {
    let mut bytes = Vec::new();
    scarp::run(source, &mut bytes).expect("valid aggregate program");
    String::from_utf8(bytes).expect("UTF-8 output")
}

fn reject_without_output(source: &str, max_array_bytes: usize, expected: &str) {
    let mut bytes = Vec::new();
    let error = scarp::run_with_options(
        source,
        &mut bytes,
        ExecutionOptions {
            max_array_bytes,
            ..ExecutionOptions::default()
        },
    )
    .expect_err("aggregate materialization must exceed the quota");
    assert!(
        format!("{error:?}").contains(expected),
        "expected {expected} in {error:?}"
    );
    assert!(bytes.is_empty(), "failed run committed partial output");
}

#[test]
fn empty_typed_aggregates_still_respect_zero_byte_budget() {
    for (source, kind) in [
        ("print(\"before\"); bind x: list<integer> = [list: <integer>]; print(x);", "ListAllocation"),
        ("print(\"before\"); bind x: map<integer, boolean> = [map: <integer, boolean>]; print(x);", "MapAllocation"),
    ] {
        reject_without_output(source, 0, kind);
    }
}

#[test]
fn map_projections_share_the_same_collection_budget_as_map_construction() {
    let source = r#"
        print("before");
        bind source = [map: 3: 30, 1: 10, 2: 20];
        bind keys = map_keys(source);
        bind values = map_values(source);
        print(keys, values);
    "#;
    reject_without_output(source, 0, "MapAllocation");
    assert_eq!(execute(source), "before\n[list: 1, 2, 3] [list: 10, 20, 30]\n");
}

#[test]
fn persistent_update_and_projection_preserve_previous_snapshots() {
    let source = r#"
        bind original = [map: 2: [list: 20], 1: [list: 10]];
        bind first = map_values(original);
        bind updated = map_set(original, 1, [list: 99]);
        bind second = map_values(updated);
        print(first[0][0], second[0][0]);
        print(map_keys(original), map_keys(updated));
    "#;
    assert_eq!(execute(source), "10 99\n[list: 1, 2] [list: 1, 2]\n");
}

#[test]
fn projection_of_boolean_keys_is_in_canonical_order() {
    let source = r#"
        bind original = [map: true: "yes", false: "no"];
        print(map_keys(original));
        print(map_values(original));
    "#;
    assert_eq!(
        execute(source),
        "[list: false, true]\n[list: \"no\", \"yes\"]\n"
    );
}

#[test]
fn unsupported_native_projection_is_a_diagnostic_not_a_panic() {
    let program =
        scarp::compile("bind original = [map: 1: 2]; print(map_values(original));").unwrap();
    assert_eq!(
        scarp::c_backend::emit_program(&program).unwrap_err().kind,
        DiagnosticKind::NativeUnsupported
    );
}

#[test]
fn executable_resource_example_matches_expected_output() {
    assert_eq!(
        execute(include_str!(
            "../examples/aggregate_resource_boundaries.srp"
        ))
        .as_bytes(),
        include_bytes!("../examples/aggregate_resource_boundaries.stdout")
    );
}
