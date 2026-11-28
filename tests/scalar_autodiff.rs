use scarp::{
    diagnostics::DiagnosticKind as Kind,
    numeric::autodiff::{Binary, Graph, GraphLimits, Unary},
    value::MAX_VECTOR_LENGTH,
};

const LIMITS: GraphLimits = GraphLimits {
    max_steps: 2_000_000,
    max_vector_bytes: 2_000_000,
};

fn close(actual: f64, expected: f64) {
    assert!((actual - expected).abs() < 1e-7, "{actual} != {expected}");
}

#[test]
fn shared_subexpressions_accumulate_all_paths() {
    let mut graph = Graph::new(8).unwrap();
    let x = graph.input().unwrap();
    let y = graph.input().unwrap();
    let xy = graph.binary(Binary::Multiply, &x, &y).unwrap();
    let doubled = graph.binary(Binary::Add, &xy, &xy).unwrap();
    let xx = graph.unary(Unary::Square, &x).unwrap();
    let output = graph.binary(Binary::Add, &doubled, &xx).unwrap();
    let tape = graph.evaluate(&output, &[3.0, 4.0], 1.0, LIMITS).unwrap();
    assert_eq!(tape.output(), 33.0);
    assert_eq!(tape.gradient(&x), Some(14.0));
    assert_eq!(tape.gradient(&y), Some(6.0));
    assert_eq!(tape.gradient(&xy), Some(2.0));
    assert_eq!(tape.value(&xx), Some(9.0));
}

#[test]
fn repeated_edges_and_nonunit_seed_are_supported() {
    let mut graph = Graph::new(2).unwrap();
    let x = graph.input().unwrap();
    let output = graph.binary(Binary::Multiply, &x, &x).unwrap();
    let tape = graph.evaluate(&output, &[4.0], -2.0, LIMITS).unwrap();
    assert_eq!(tape.output(), 16.0);
    assert_eq!(tape.gradient(&x), Some(-16.0));
    assert_eq!(tape.gradient(&output), Some(-2.0));
}

#[test]
fn graph_reuse_preserves_old_snapshots() {
    let mut graph = Graph::new(4).unwrap();
    let x = graph.input().unwrap();
    let out = graph.unary(Unary::Square, &x).unwrap();
    let a = graph.evaluate(&out, &[2.0], 1.0, LIMITS).unwrap();
    let b = graph.evaluate(&out, &[5.0], 1.0, LIMITS).unwrap();
    let later = graph.constant(3.0).unwrap();
    assert_eq!(a.output(), 4.0);
    assert_eq!(a.gradient(&x), Some(4.0));
    assert_eq!(b.output(), 25.0);
    assert_eq!(b.gradient(&x), Some(10.0));
    assert_eq!(a.value(&later), None);
    assert_eq!(a.gradient(&later), None);
    drop(graph);
    assert_eq!(b.gradient(&x), Some(10.0));
}

#[test]
fn foreign_nodes_and_snapshots_are_rejected_without_mutation() {
    let mut a = Graph::new(3).unwrap();
    let mut b = Graph::new(3).unwrap();
    let x = a.input().unwrap();
    let y = b.input().unwrap();
    assert_eq!(
        a.binary(Binary::Add, &x, &y).unwrap_err().kind,
        Kind::VectorIndex
    );
    assert_eq!(
        a.unary(Unary::Negate, &y).unwrap_err().kind,
        Kind::VectorIndex
    );
    assert_eq!(
        a.evaluate(&y, &[1.0], 1.0, LIMITS).unwrap_err().kind,
        Kind::VectorIndex
    );
    assert_eq!(a.len(), 1);
    let tape = a.evaluate(&x, &[1.0], 1.0, LIMITS).unwrap();
    assert_eq!(tape.value(&y), None);
    assert_eq!(tape.gradient(&y), None);
    drop(b);
    let mut c = Graph::new(1).unwrap();
    let z = c.input().unwrap();
    assert_eq!(
        c.unary(Unary::Square, &y).unwrap_err().kind,
        Kind::VectorIndex
    );
    assert_eq!(tape.gradient(&z), None);
}

#[test]
fn disconnected_invalid_operations_are_not_executed() {
    let mut graph = Graph::new(6).unwrap();
    let x = graph.input().unwrap();
    let y = graph.input().unwrap();
    let invalid = graph.unary(Unary::Log, &y).unwrap();
    let out = graph.unary(Unary::Square, &x).unwrap();
    let later = graph.unary(Unary::Exp, &y).unwrap();
    let tape = graph.evaluate(&out, &[3.0, -2.0], 1.0, LIMITS).unwrap();
    assert_eq!(tape.output(), 9.0);
    assert_eq!(tape.value(&invalid), None);
    assert_eq!(tape.gradient(&invalid), Some(0.0));
    assert_eq!(tape.value(&later), None);
    assert_eq!(tape.gradient(&y), Some(0.0));
    assert_eq!(
        graph.evaluate(&invalid, &[3.0, -2.0], 1.0, LIMITS).unwrap_err().kind,
        Kind::NumericDomain
    );
}

#[test]
fn zero_seed_and_zero_cotangents_do_not_evaluate_singular_derivatives() {
    let mut graph = Graph::new(5).unwrap();
    let x = graph.input().unwrap();
    let log = graph.unary(Unary::Log, &x).unwrap();
    let zero = graph.constant(0.0).unwrap();
    let out = graph.binary(Binary::Multiply, &zero, &log).unwrap();
    let tiny = f64::from_bits(1);
    let tape = graph.evaluate(&out, &[tiny], 1.0, LIMITS).unwrap();
    assert_eq!(tape.gradient(&x), Some(0.0));
    let tape = graph.evaluate(&log, &[tiny], 0.0, LIMITS).unwrap();
    assert_eq!(tape.gradient(&x), Some(0.0));
    assert_eq!(
        graph.evaluate(&log, &[tiny], 1.0, LIMITS).unwrap_err().kind,
        Kind::NonFiniteNumber
    );
}

#[test]
fn all_unary_derivatives_match_finite_differences() {
    for (op, input) in [
        (Unary::Negate, -0.7),
        (Unary::Square, -0.7),
        (Unary::Exp, -0.7),
        (Unary::Log, 1.7),
        (Unary::Relu, -0.7),
        (Unary::Relu, 0.7),
        (Unary::Sigmoid, -0.7),
        (Unary::Tanh, 0.7),
    ] {
        let mut graph = Graph::new(2).unwrap();
        let x = graph.input().unwrap();
        let y = graph.unary(op, &x).unwrap();
        let f = |v| graph.evaluate(&y, &[v], 0.0, LIMITS).unwrap().output();
        let h = 1e-5;
        let tape = graph.evaluate(&y, &[input], 1.0, LIMITS).unwrap();
        close(tape.gradient(&x).unwrap(), (f(input + h) - f(input - h)) / (2.0 * h));
    }
}

#[test]
fn all_binary_derivatives_match_finite_differences() {
    for op in [Binary::Add, Binary::Subtract, Binary::Multiply, Binary::Divide] {
        let mut graph = Graph::new(3).unwrap();
        let x = graph.input().unwrap();
        let y = graph.input().unwrap();
        let out = graph.binary(op, &x, &y).unwrap();
        let f = |a, b| graph.evaluate(&out, &[a, b], 0.0, LIMITS).unwrap().output();
        let a = 1.5;
        let b = -0.75;
        let h = 1e-5;
        let tape = graph.evaluate(&out, &[a, b], 1.0, LIMITS).unwrap();
        close(tape.gradient(&x).unwrap(), (f(a + h, b) - f(a - h, b)) / (2.0 * h));
        close(tape.gradient(&y).unwrap(), (f(a, b + h) - f(a, b - h)) / (2.0 * h));
    }
}

#[test]
fn shape_and_finite_validation_precede_computation() {
    let mut graph = Graph::new(3).unwrap();
    let x = graph.input().unwrap();
    let y = graph.input().unwrap();
    let out = graph.binary(Binary::Divide, &x, &y).unwrap();
    for b in [0.0, -0.0] {
        assert_eq!(
            graph.evaluate(&out, &[1.0, b], 1.0, LIMITS).unwrap_err().kind,
            Kind::NumericDomain
        );
    }
    assert_eq!(
        graph.evaluate(&out, &[1.0], 1.0, LIMITS).unwrap_err().kind,
        Kind::CallArguments
    );
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert_eq!(
            graph.evaluate(&x, &[1.0, value], 1.0, LIMITS).unwrap_err().kind,
            Kind::NonFiniteNumber
        );
        assert_eq!(
            graph.evaluate(&x, &[1.0, 2.0], value, LIMITS).unwrap_err().kind,
            Kind::NonFiniteNumber
        );
    }
}

#[test]
fn failed_forward_or_reverse_evaluation_does_not_mutate_graph() {
    let mut graph = Graph::new(3).unwrap();
    let x = graph.input().unwrap();
    let square = graph.unary(Unary::Square, &x).unwrap();
    assert_eq!(
        graph.evaluate(&square, &[1e308], 1.0, LIMITS).unwrap_err().kind,
        Kind::NonFiniteNumber
    );
    let half_max = f64::MAX / 2.0;
    let out = graph.binary(Binary::Add, &x, &x).unwrap();
    assert_eq!(
        graph.evaluate(&out, &[1.0], f64::MAX, LIMITS).unwrap_err().kind,
        Kind::NonFiniteNumber
    );
    assert_eq!(
        graph.evaluate(&out, &[1.0], half_max, LIMITS).unwrap().gradient(&x),
        Some(f64::MAX)
    );
}

#[test]
fn graph_capacity_and_failed_edits_are_atomic() {
    assert_eq!(Graph::new(MAX_VECTOR_LENGTH + 1).unwrap_err().kind, Kind::VectorLength);
    let mut empty = Graph::new(0).unwrap();
    assert_eq!(empty.input().unwrap_err().kind, Kind::VectorLength);
    assert_eq!(empty.input_count(), 0);
    let mut graph = Graph::new(1).unwrap();
    assert_eq!(graph.constant(f64::NAN).unwrap_err().kind, Kind::NonFiniteNumber);
    assert!(graph.is_empty());
    let x = graph.input().unwrap();
    assert_eq!(graph.input().unwrap_err().kind, Kind::VectorLength);
    assert_eq!(graph.input_count(), 1);
    assert_eq!(graph.unary(Unary::Negate, &x).unwrap_err().kind, Kind::VectorLength);
}

#[test]
fn exact_tape_work_and_payload_boundaries() {
    let mut graph = Graph::new(3).unwrap();
    let x = graph.input().unwrap();
    let y = graph.input().unwrap();
    let out = graph.binary(Binary::Multiply, &x, &y).unwrap();
    let exact = GraphLimits {
        max_steps: 50,
        max_vector_bytes: 51,
    };
    graph.evaluate(&out, &[1.0, 2.0], 1.0, exact).unwrap();
    assert_eq!(
        graph.evaluate(&out, &[1.0, 2.0], 1.0, GraphLimits { max_steps: 49, ..exact })
            .unwrap_err().kind,
        Kind::ExecutionLimit
    );
    assert_eq!(
        graph.evaluate(&out, &[1.0, 2.0], 1.0, GraphLimits { max_vector_bytes: 50, ..exact })
            .unwrap_err().kind,
        Kind::VectorBudget
    );
}

#[test]
fn maximum_depth_is_iterative_and_bounded() {
    let mut graph = Graph::new(MAX_VECTOR_LENGTH).unwrap();
    let x = graph.input().unwrap();
    let mut out = x.clone();
    for _ in 1..MAX_VECTOR_LENGTH {
        out = graph.unary(Unary::Negate, &out).unwrap();
    }
    let tape = graph.evaluate(&out, &[3.0], 1.0, LIMITS).unwrap();
    assert_eq!(tape.output(), -3.0);
    assert_eq!(tape.gradient(&x), Some(-1.0));
}

#[test]
fn smooth_derivatives_retain_saturated_tails_and_relu_zero() {
    let mut graph = Graph::new(4).unwrap();
    let x = graph.input().unwrap();
    let sigmoid = graph.unary(Unary::Sigmoid, &x).unwrap();
    let tanh = graph.unary(Unary::Tanh, &x).unwrap();
    let relu = graph.unary(Unary::Relu, &x).unwrap();
    for out in [&sigmoid, &tanh] {
        let tape = graph.evaluate(out, &[40.0], 1.0, LIMITS).unwrap();
        assert!(tape.gradient(&x).unwrap() > 0.0);
    }
    let tape = graph.evaluate(&relu, &[-0.0], 1.0, LIMITS).unwrap();
    assert_eq!(tape.gradient(&x), Some(0.0));
}

#[test]
fn reusable_graph_trains_scalar_linear_regression() {
    let mut graph = Graph::new(8).unwrap();
    let weight = graph.input().unwrap();
    let feature = graph.input().unwrap();
    let target = graph.input().unwrap();
    let prediction = graph.binary(Binary::Multiply, &weight, &feature).unwrap();
    let error = graph.binary(Binary::Subtract, &prediction, &target).unwrap();
    let loss = graph.unary(Unary::Square, &error).unwrap();
    let mut w = 0.0;
    for _ in 0..32 {
        let tape = graph.evaluate(&loss, &[w, 2.0, 6.0], 1.0, LIMITS).unwrap();
        w -= 0.0625 * tape.gradient(&weight).unwrap();
    }
    close(w, 3.0);
}
