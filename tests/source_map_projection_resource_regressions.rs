//! Deterministic aggregate regression coverage for source map projections.
//! This suite intentionally runs against the interpreter, not the native ABI.
use scarp::ExecutionOptions;

fn execute(source: &str) -> String {
    let mut output = Vec::new();
    scarp::run(source, &mut output).expect("source program should execute");
    String::from_utf8(output).expect("Scarp output must be UTF-8")
}

#[test]
fn projection_order_is_independent_of_insertion_order() {
    let forward = r#"
        bind m = [map: -2: "low", 0: "mid", 9: "high"];
        print(map_keys(m));
        print(map_values(m));
    "#;
    let reverse = r#"
        bind m = [map: 9: "high", 0: "mid", -2: "low"];
        print(map_keys(m));
        print(map_values(m));
    "#;
    assert_eq!(execute(forward), execute(reverse));
    assert_eq!(
        execute(forward),
        "[list: -2, 0, 9]\n[list: \"low\", \"mid\", \"high\"]\n"
    );
}

#[test]
fn repeated_projections_remain_snapshots_after_updates() {
    let source = r#"
        bind first = [map: 2: "two", 1: "one"];
        bind old_keys = map_keys(first);
        bind old_values = map_values(first);
        bind changed = map_set(first, 2, "updated");
        bind extended = map_set(changed, 3, "three");
        bind removed = map_remove(extended, 1);
        print(old_keys, old_values);
        print(map_keys(first), map_values(first));
        print(map_keys(removed), map_values(removed));
    "#;
    assert_eq!(
        execute(source),
        "[list: 1, 2] [list: \"one\", \"two\"]\n\
         [list: 1, 2] [list: \"one\", \"two\"]\n\
         [list: 2, 3] [list: \"updated\", \"three\"]\n"
    );
}

#[test]
fn projections_of_maximum_size_map_have_matching_cardinality() {
    let mut source = String::from("bind m = [map: ");
    for key in (0..1024).rev() {
        if key != 1023 {
            source.push_str(", ");
        }
        source.push_str(&format!("{key}: {key}"));
    }
    source.push_str("]; bind keys = map_keys(m); bind values = map_values(m);");
    source.push_str("print(map_len(m), list_len(keys), list_len(values));");
    // The list length intrinsic is the source-language API, not a host-side count.
    assert_eq!(execute(&source), "1024 1024 1024\n");
}

#[test]
fn zero_collection_budget_rejects_projections_without_partial_output() {
    let source = r#"
        print("before");
        bind m = [map: 1: 2];
        print(map_keys(m));
    "#;
    let mut output = Vec::new();
    let result = scarp::run_with_options(
        source,
        &mut output,
        ExecutionOptions {
            max_array_bytes: 0,
            ..ExecutionOptions::default()
        },
    );
    assert!(result.is_err());
    assert!(output.is_empty(), "failed execution must not commit output");
}

#[test]
fn executable_snapshot_example_matches_golden_output() {
    assert_eq!(
        execute(include_str!("../examples/map_projection_snapshots.srp")).as_bytes(),
        include_bytes!("../examples/map_projection_snapshots.stdout")
    );
}
