//! Source-level regression coverage for nominal and nested binding patterns.
//! These tests deliberately exercise the public compiler/runtime entry points.

#[test]
fn structured_pattern_fixture_executes_with_deterministic_output() {
    let source = include_str!("../examples/structured_pattern_coverage.srp");
    let mut stdout = Vec::new();
    scarp::run(source, &mut stdout).expect("structured patterns must compile and execute");
    assert_eq!(
        stdout,
        include_bytes!("../examples/structured_pattern_coverage.stdout")
    );
}

#[test]
fn duplicate_names_in_nested_tuple_bindings_are_rejected() {
    let source = "bind (same, same) = (1, 2); print(same);";
    assert!(scarp::compile(source).is_err());
}

#[test]
fn incorrect_record_field_in_pattern_is_rejected_before_execution() {
    let source = r#"
        record Point { x: integer, y: integer }
        bind point = new Point { x: 1, y: 2 };
        bind {missing: value} = point;
        print(value);
    "#;
    assert!(scarp::compile(source).is_err());
}

#[test]
fn tuple_pattern_arity_mismatch_is_rejected_before_execution() {
    let source = "bind (first, second) = (1, 2, 3); print(first);";
    assert!(scarp::compile(source).is_err());
}
