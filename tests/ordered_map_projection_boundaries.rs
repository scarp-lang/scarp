use scarp::{diagnostics::DiagnosticKind, ExecutionOptions};

fn output(source: &str) -> String {
    let mut bytes = Vec::new();
    scarp::run(source, &mut bytes).unwrap();
    String::from_utf8(bytes).unwrap()
}

#[test]
fn ordered_projections_are_sorted_and_snapshot_isolated() {
    let source = include_str!("../examples/ordered_map_projection_boundaries.srp");
    scarp::check(source).unwrap();
    assert_eq!(output(source), "true\ntrue\ntrue\ntrue\ntrue\n");
}

#[test]
fn empty_projections_retain_distinct_element_types() {
    let source = r#"
        bind empty: map<boolean, string> = [map: <boolean, string>];
        bind keys: list<boolean> = map_keys(empty);
        bind values: list<string> = map_values(empty);
        print(keys == [list: <boolean>]);
        print(values == [list: <string>]);
    "#;
    assert_eq!(output(source), "true\ntrue\n");
}

#[test]
fn map_projection_allocation_respects_shared_aggregate_quota() {
    let source = r#"
        print("not committed");
        bind source = [map: 1: 2];
        print(map_keys(source));
    "#;
    let mut bytes = Vec::new();
    let err = scarp::run_with_options(
        source,
        &mut bytes,
        ExecutionOptions {
            max_array_bytes: 0,
            ..ExecutionOptions::default()
        },
    )
    .unwrap_err();
    assert_eq!(err.kind, DiagnosticKind::MapAllocation);
    assert!(bytes.is_empty());
}
