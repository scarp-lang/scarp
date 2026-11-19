use scarp::{diagnostics::DiagnosticKind, ExecutionLimits, ExecutionOptions};

const SOURCE: &str = include_str!("../examples/deterministic_map_snapshots.srp");

fn run_bytes(source: &str) -> Vec<u8> {
    let mut bytes = Vec::new();
    scarp::run(source, &mut bytes).expect("valid source must execute");
    bytes
}

#[test]
fn canonical_map_snapshots_are_reproducible_across_runs() {
    let expected = b"[list: 1, 3, 9]\n[list: \"one\", \"three\", \"nine\"]\n[list: 1, 3, 9]\n[list: 1, 3, 9, 10]\n3 4\n";
    for _ in 0..8 {
        assert_eq!(run_bytes(SOURCE), expected);
    }
}

#[test]
fn persistent_map_projections_do_not_change_prior_snapshots() {
    let source = r#"
        bind original = [map: 9: "nine", 1: "one"];
        bind old_keys = map_keys(original);
        bind changed = map_set(original, 3, "three");
        bind new_keys = map_keys(changed);
        print(old_keys);
        print(new_keys);
        print(map_len(original), map_len(changed));
    "#;
    assert_eq!(run_bytes(source), b"[list: 1, 9]\n[list: 1, 3, 9]\n2 3\n");
}

#[test]
fn resource_exhaustion_never_commits_partial_stdout() {
    let source = r#"
        print("before");
        bind m = [map: 1: 10, 2: 20, 3: 30];
        print(map_values(m));
        print("after");
    "#;
    for steps in [0, 1, 2, 3, 4] {
        let mut output = Vec::new();
        let result = scarp::run_with_options(
            source,
            &mut output,
            ExecutionOptions {
                limits: ExecutionLimits {
                    max_steps: steps,
                    max_prints: 100,
                },
                ..ExecutionOptions::default()
            },
        );
        assert!(
            result.is_err(),
            "step budget {steps} unexpectedly succeeded"
        );
        assert!(output.is_empty(), "step budget {steps} leaked output");
    }
}

#[test]
fn native_backend_rejects_aggregate_projections_before_emission() {
    let program = scarp::compile(SOURCE).expect("interpreter program compiles");
    let error = scarp::c_backend::emit_program(&program).unwrap_err();
    assert_eq!(error.kind, DiagnosticKind::NativeUnsupported);
}
