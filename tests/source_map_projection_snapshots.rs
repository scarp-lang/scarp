use scarp::{diagnostics::DiagnosticKind, ExecutionOptions};

fn execute(source: &str) -> String {
    let mut output = Vec::new();
    scarp::run(source, &mut output).expect("source should execute");
    String::from_utf8(output).expect("output should be UTF-8")
}

#[test]
fn persistent_projection_example_matches_golden_output() {
    assert_eq!(
        execute(include_str!("../examples/map_projection_snapshots.srp")).as_bytes(),
        include_bytes!("../examples/map_projection_snapshots.stdout")
    );
}

#[test]
fn boolean_keys_preserve_canonical_order_after_replacement() {
    let source = r#"
        bind first = [map: true: "yes", false: "no"];
        bind second = map_set(first, false, "disabled");
        print(map_keys(first), map_values(first));
        print(map_keys(second), map_values(second));
    "#;
    assert_eq!(
        execute(source),
        "[list: false, true] [list: \"no\", \"yes\"]\n[list: false, true] [list: \"disabled\", \"yes\"]\n"
    );
}

#[test]
fn projection_failure_rolls_back_prior_prints() {
    let source = "print(\"before\"); bind values = map_values([map: 1: 2]); print(values);";
    let mut bytes = Vec::new();
    let result = scarp::run_with_options(
        source,
        &mut bytes,
        ExecutionOptions {
            limits: scarp::ExecutionLimits {
                max_steps: 1,
                max_prints: 100,
            },
            ..ExecutionOptions::default()
        },
    );
    assert!(result.is_err());
    assert!(bytes.is_empty(), "failed execution must not commit output");
}

#[test]
fn native_backend_does_not_silently_lower_map_values() {
    let program = scarp::compile("print(map_values([map: 1: 2]));").unwrap();
    let error = scarp::c_backend::emit_program(&program).unwrap_err();
    assert_eq!(error.kind, DiagnosticKind::NativeUnsupported);
}
