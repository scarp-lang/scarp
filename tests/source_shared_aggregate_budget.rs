use scarp::ExecutionOptions;

// Regression: the aggregate byte quota is shared across immutable collection
// constructors and projections, rather than reset for each container kind.
fn fails_atomically(source: &str) {
    let mut stdout = Vec::new();
    let result = scarp::run_with_options(
        source,
        &mut stdout,
        ExecutionOptions {
            max_array_bytes: 0,
            ..ExecutionOptions::default()
        },
    );
    assert!(result.is_err(), "expected aggregate allocation to exhaust quota: {source}");
    assert!(stdout.is_empty(), "failed execution must not publish output: {source}");
}

#[test]
fn array_list_and_map_constructors_respect_the_same_zero_byte_quota() {
    fails_atomically("print(\"before\"); bind a = [1, 2]; print(a);");
    fails_atomically("print(\"before\"); bind a = [list: 1, 2]; print(a);");
    fails_atomically("print(\"before\"); bind a = [map: 1: 2]; print(a);");
}

#[test]
fn map_projections_cannot_escape_the_aggregate_quota() {
    fails_atomically(
        "print(\"before\"); bind m = [map: 1: 2]; bind keys = map_keys(m); print(keys);",
    );
    fails_atomically(
        "print(\"before\"); bind m = [map: 1: 2]; bind values = map_values(m); print(values);",
    );
}
