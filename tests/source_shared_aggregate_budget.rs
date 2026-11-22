//! Regression tests for cumulative collection quotas and atomic output.
//! Check the exact error kind so unrelated parse or type errors cannot pass.
use scarp::{diagnostics::DiagnosticKind, ExecutionOptions, RunError};

fn execute_with_budget(
    source: &str,
    max_array_bytes: usize,
) -> (Result<(), RunError>, Vec<u8>) {
    let mut stdout = Vec::new();
    let result = scarp::run_with_options(
        source,
        &mut stdout,
        ExecutionOptions {
            max_array_bytes,
            ..ExecutionOptions::default()
        },
    );
    (result, stdout)
}

fn fails_atomically(source: &str, budget: usize, expected: DiagnosticKind) {
    // First prove the exact same source can run without the restricted quota.
    let mut ordinary_output = Vec::new();
    scarp::run(source, &mut ordinary_output).expect("valid source control");
    assert!(
        ordinary_output.starts_with(b"before\n"),
        "the control program did not execute its first print: {source}"
    );

    let (result, stdout) = execute_with_budget(source, budget);
    let diagnostic = match result {
        Err(RunError::Diagnostic(diagnostic)) => diagnostic,
        other => panic!("expected a source diagnostic, got {other:?}: {source}"),
    };
    assert_eq!(
        diagnostic.kind, expected,
        "unexpected failure reason for source: {source}"
    );
    assert!(
        stdout.is_empty(),
        "failed execution must not publish buffered output: {source}"
    );
}

#[test]
fn array_list_and_map_constructors_respect_the_same_zero_byte_quota() {
    for (source, kind) in [
        (
            "print(\"before\"); bind a = [array: 1, 2]; print(a);",
            DiagnosticKind::ArrayAllocation,
        ),
        (
            "print(\"before\"); bind a = [list: 1, 2]; print(a);",
            DiagnosticKind::ListAllocation,
        ),
        (
            "print(\"before\"); bind a = [map: 1: 2]; print(a);",
            DiagnosticKind::MapAllocation,
        ),
    ] {
        fails_atomically(source, 0, kind);
    }
}

#[test]
fn map_projections_charge_after_a_successfully_allocated_map() {
    // Find the exact budget needed by the map constructor; no assumptions
    // about the host's Value size or 32-bit versus 64-bit layout.
    let map_only = "bind m = [map: 1: 2]; print(map_len(m));";
    let mut low = 0_usize;
    let mut high = 8192_usize;
    assert!(
        execute_with_budget(map_only, high).0.is_ok(),
        "the control map unexpectedly exceeds the generous test quota"
    );
    while low < high {
        let middle = low + (high - low) / 2;
        if execute_with_budget(map_only, middle).0.is_ok() {
            high = middle;
        } else {
            low = middle + 1;
        }
    }
    assert!(low > 0, "nonempty map must consume a positive quota");
    assert!(
        execute_with_budget(map_only, low).0.is_ok(),
        "the control map must fit within its exact quota"
    );

    for projection in ["map_keys", "map_values"] {
        let source = format!(
            "print(\"before\"); bind m = [map: 1: 2]; bind projected = {projection}(m); print(projected);"
        );
        // Map construction fits at this quota. The projection must fail
        // specifically while charging its new typed list allocation.
        fails_atomically(&source, low, DiagnosticKind::ListAllocation);
    }
}
