use scarp::{diagnostics::DiagnosticKind, ExecutionOptions, RunError};

fn execute(source: &str) -> String {
    let mut output = Vec::new();
    scarp::run(source, &mut output).expect("source should execute");
    String::from_utf8(output).expect("output should be UTF-8")
}

#[test]
fn persistent_projection_example_matches_golden_output() {
    assert_eq!(
        execute(include_str!(
            "../examples/map_string_projection_snapshots.srp"
        ))
        .as_bytes(),
        include_bytes!("../examples/map_string_projection_snapshots.stdout")
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
    let source =
        "print(\"before\"); bind m = [map: 1: 2]; bind values = map_values(m); print(values);";
    let mut bytes = Vec::new();
    let result = scarp::run_with_options(
        source,
        &mut bytes,
        ExecutionOptions {
            limits: scarp::ExecutionLimits {
                max_steps: 14,
                max_prints: 100,
            },
            ..ExecutionOptions::default()
        },
    )
    .unwrap_err();
    let RunError::Diagnostic(error) = result else {
        panic!("expected a runtime diagnostic");
    };
    assert_eq!(error.kind, DiagnosticKind::ExecutionLimit);
    assert_eq!(&source[error.span.start..error.span.end], "map_values(m)");
    assert!(bytes.is_empty(), "failed execution must not commit output");
}

#[test]
fn native_backend_does_not_silently_lower_map_values() {
    let program = scarp::compile("print(map_values([map: 1: 2]));").unwrap();
    let error = scarp::c_backend::emit_program(&program).unwrap_err();
    assert_eq!(error.kind, DiagnosticKind::NativeUnsupported);
}

#[test]
fn integer_map_snapshots_and_both_streams_survive_exact_work_boundaries() {
    let source = r#"
        bind original = [map: 9: "nine", 1: "one"];
        bind old_keys = map_keys(original);
        bind changed = map_set(original, 3, "three");
        eprint("before");
        print(old_keys);
        print(map_values(changed));
        print(map_len(original), map_len(changed));
    "#;
    let run = |max_steps| {
        let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
        let result = scarp::run_with_io_and_options(
            source,
            &mut stdout,
            &mut stderr,
            ExecutionOptions {
                limits: scarp::ExecutionLimits {
                    max_steps,
                    max_prints: 4,
                },
                ..ExecutionOptions::default()
            },
        );
        (result, stdout, stderr)
    };
    const EXACT_STEPS: u64 = 232;
    for steps in [0, 1, EXACT_STEPS - 1] {
        let (result, stdout, stderr) = run(steps);
        assert!(matches!(result, Err(RunError::Diagnostic(ref diagnostic))
            if diagnostic.kind == DiagnosticKind::ExecutionLimit));
        assert!(stdout.is_empty(), "stdout escaped at {steps} steps");
        assert!(stderr.is_empty(), "stderr escaped at {steps} steps");
    }
    for steps in [EXACT_STEPS, EXACT_STEPS, EXACT_STEPS + 1] {
        let (result, stdout, stderr) = run(steps);
        result.unwrap();
        assert_eq!(
            stdout,
            b"[list: 1, 9]\n[list: \"one\", \"three\", \"nine\"]\n2 3\n"
        );
        assert_eq!(stderr, b"before\n");
    }
}
