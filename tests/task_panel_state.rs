use scarp::{ExecutionLimits, ExecutionOptions};

const SOURCE: &str = include_str!("../examples/task_panel_state.srp");
const EXPECTED: &[u8] = include_bytes!("../examples/task_panel_state.stdout");

#[test]
fn task_panel_golden_covers_typed_state_and_recoverable_events() {
    let mut out = Vec::new();
    scarp::run(SOURCE, &mut out).unwrap();
    assert_eq!(out, EXPECTED);
}

#[test]
fn task_panel_reexecution_does_not_reuse_mutable_snapshot() {
    let mut first = Vec::new();
    let mut second = Vec::new();
    scarp::run(SOURCE, &mut first).unwrap();
    scarp::run(SOURCE, &mut second).unwrap();
    assert_eq!(first, second);
    assert_eq!(second, EXPECTED);
}

#[test]
fn task_panel_check_validates_without_publishing_output() {
    scarp::check(SOURCE).unwrap();
    let program = scarp::compile(SOURCE).unwrap();
    assert!(!program.type_registry().is_empty());
}

#[test]
fn task_panel_respects_small_step_budget_and_transactional_output() {
    let limits = ExecutionLimits {
        max_steps: 4,
        max_prints: 100,
    };
    let mut output = Vec::new();
    let failure = scarp::run_with_options(
        SOURCE,
        &mut output,
        ExecutionOptions {
            limits,
            ..ExecutionOptions::default()
        },
    );
    assert!(failure.is_err(), "a four-step limit must interrupt execution");
    assert!(output.is_empty(), "failed execution must not publish output");
}
