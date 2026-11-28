use scarp::{
    diagnostics::DiagnosticKind as Kind,
    numeric::{self, Builtin},
    value::{NumericVector, Value, MAX_VECTOR_LENGTH},
    RunError,
};

fn v(values: &[f64]) -> Value {
    Value::Vector(NumericVector::new(values).unwrap())
}

fn f(value: f64) -> Value {
    Value::Float(scarp::value::FiniteFloat::new(value).unwrap())
}

fn call(kernel: Builtin, args: &[Value]) -> Value {
    numeric::evaluate(kernel, args, u64::MAX).unwrap()
}

fn elements(value: &Value) -> Vec<f64> {
    value
        .as_vector()
        .unwrap()
        .as_slice()
        .iter()
        .map(|v| v.get())
        .collect()
}

fn close(actual: f64, expected: f64, tolerance: f64) {
    assert!(
        (actual - expected).abs() <= tolerance * expected.abs().max(1.0),
        "{actual} != {expected}"
    );
}

#[test]
fn relu_backward_selects_positive_inputs_and_zero_subgradient() {
    assert_eq!(
        call(
            Builtin::ReluBackward,
            &[v(&[-2.0, -0.0, 0.0, 2.0]), v(&[3.0, 4.0, 5.0, -6.0])]
        ),
        v(&[0.0, 0.0, 0.0, -6.0])
    );
}

#[test]
fn smooth_gradients_at_zero_are_exact() {
    for (kernel, expected) in [
        (Builtin::SigmoidBackward, 0.5),
        (Builtin::TanhBackward, 2.0),
        (Builtin::SoftplusBackward, 1.0),
    ] {
        close(
            elements(&call(kernel, &[v(&[0.0]), v(&[2.0])]))[0],
            expected,
            2e-15,
        );
    }
}

#[test]
fn positive_saturation_preserves_derivative_tails() {
    for kernel in [Builtin::SigmoidBackward, Builtin::TanhBackward] {
        let result = call(kernel, &[v(&[-40.0, 40.0]), v(&[1.0, 1.0])]);
        let values = elements(&result);
        assert!(values[0] > 0.0);
        assert_eq!(values[0], values[1]);
    }
    let result = call(
        Builtin::TanhBackward,
        &[v(&[f64::MAX, -f64::MAX]), v(&[1.0, 1.0])],
    );
    assert_eq!(result, v(&[0.0, 0.0]));
}

#[test]
fn activation_vjps_match_central_finite_differences() {
    let x = [-3.0, -0.5, 0.75, 3.0];
    let g = [2.0, -1.0, 4.0, -3.0];
    let step = 1e-5;
    for (forward, backward) in [
        (Builtin::Relu, Builtin::ReluBackward),
        (Builtin::Sigmoid, Builtin::SigmoidBackward),
        (Builtin::Tanh, Builtin::TanhBackward),
        (Builtin::Softplus, Builtin::SoftplusBackward),
        (Builtin::Softmax, Builtin::SoftmaxBackward),
    ] {
        let actual = elements(&call(backward, &[v(&x), v(&g)]));
        for (i, actual) in actual.into_iter().enumerate() {
            let mut plus = x;
            let mut minus = x;
            plus[i] += step;
            minus[i] -= step;
            let objective = |input: &[f64]| {
                call(Builtin::Dot, &[call(forward, &[v(input)]), v(&g)])
                    .as_float()
                    .unwrap()
            };
            let expected = (objective(&plus) - objective(&minus)) / (2.0 * step);
            close(actual, expected, 2e-8);
        }
    }
}

#[test]
fn softmax_backward_matches_uniform_case_and_annihilates_constants() {
    assert_eq!(
        call(Builtin::SoftmaxBackward, &[v(&[0.0, 0.0]), v(&[1.0, -1.0])]),
        v(&[0.5, -0.5])
    );
    assert_eq!(
        call(
            Builtin::SoftmaxBackward,
            &[v(&[1.0, 2.0, 3.0]), v(&[f64::MAX; 3])]
        ),
        v(&[0.0; 3])
    );
    assert_eq!(
        call(Builtin::SoftmaxBackward, &[v(&[f64::MAX]), v(&[-f64::MAX])]),
        v(&[0.0])
    );
}

#[test]
fn softmax_backward_handles_extreme_logits_and_gradients() {
    assert_eq!(
        call(
            Builtin::SoftmaxBackward,
            &[v(&[-f64::MAX, f64::MAX]), v(&[f64::MAX, -f64::MAX])]
        ),
        v(&[0.0, 0.0])
    );
    let result = call(
        Builtin::SoftmaxBackward,
        &[v(&[0.0; 4]), v(&[f64::MAX, f64::MAX, f64::MAX, -f64::MAX])],
    );
    let values = elements(&result);
    for value in &values[..3] {
        close(*value / f64::MAX, 0.125, 1e-15);
    }
    close(values[3] / f64::MAX, -0.375, 1e-15);
}

#[test]
fn sgd_step_matches_composed_kernels_and_preserves_inputs() {
    let weights = v(&[2.0, -4.0]);
    let gradients = v(&[1.0, -2.0]);
    let before = weights.clone();
    let rate = f(0.125);
    let result = call(
        Builtin::SgdStep,
        &[weights.clone(), gradients.clone(), rate.clone()],
    );
    let delta = call(Builtin::Scale, &[gradients.clone(), rate]);
    assert_eq!(result, call(Builtin::VectorSub, &[weights.clone(), delta]));
    assert_eq!(result, v(&[1.875, -3.75]));
    assert_eq!(weights, before);
    assert_eq!(
        call(Builtin::SgdStep, &[weights.clone(), gradients, f(0.0)]),
        weights
    );
}

#[test]
fn invalid_shapes_domains_and_argument_types_precede_work() {
    for kernel in [
        Builtin::ReluBackward,
        Builtin::SigmoidBackward,
        Builtin::TanhBackward,
        Builtin::SoftplusBackward,
        Builtin::SoftmaxBackward,
    ] {
        assert_eq!(
            numeric::evaluate(kernel, &[v(&[1.0]), v(&[])], 0)
                .unwrap_err()
                .kind,
            Kind::VectorLength
        );
        assert_eq!(
            numeric::evaluate(kernel, &[v(&[]), f(0.0)], 0)
                .unwrap_err()
                .kind,
            Kind::TypeMismatch
        );
    }
    assert_eq!(
        numeric::evaluate(Builtin::SoftmaxBackward, &[v(&[]), v(&[])], 0)
            .unwrap_err()
            .kind,
        Kind::NumericDomain
    );
    assert_eq!(
        numeric::evaluate(Builtin::SgdStep, &[v(&[]), v(&[]), f(-1.0)], 0)
            .unwrap_err()
            .kind,
        Kind::NumericDomain
    );
    assert_eq!(
        numeric::evaluate(Builtin::SgdStep, &[v(&[1.0]), v(&[]), f(0.5)], 0)
            .unwrap_err()
            .kind,
        Kind::VectorLength
    );
}

#[test]
fn work_and_payload_boundaries_are_exact_for_all_primitives() {
    let a = v(&[1.0, -2.0]);
    let b = v(&[3.0, 4.0]);
    for (kernel, args, work) in [
        (Builtin::ReluBackward, vec![a.clone(), b.clone()], 2),
        (Builtin::SigmoidBackward, vec![a.clone(), b.clone()], 12),
        (Builtin::TanhBackward, vec![a.clone(), b.clone()], 12),
        (Builtin::SoftplusBackward, vec![a.clone(), b.clone()], 12),
        (Builtin::SoftmaxBackward, vec![a.clone(), b.clone()], 24),
        (Builtin::SgdStep, vec![a, b, f(0.5)], 4),
    ] {
        let before = args.clone();
        numeric::evaluate_with_vector_limit(kernel, &args, work, 16).unwrap();
        assert_eq!(
            numeric::evaluate_with_vector_limit(kernel, &args, work - 1, 16)
                .unwrap_err()
                .kind,
            Kind::ExecutionLimit
        );
        assert_eq!(
            numeric::evaluate_with_vector_limit(kernel, &args, work, 15)
                .unwrap_err()
                .kind,
            Kind::VectorBudget
        );
        assert_eq!(args, before);
    }
}

#[test]
fn empty_elementwise_gradients_charge_one_step_and_no_payload() {
    for kernel in [
        Builtin::ReluBackward,
        Builtin::SigmoidBackward,
        Builtin::TanhBackward,
        Builtin::SoftplusBackward,
        Builtin::SgdStep,
    ] {
        let mut args = vec![v(&[]), v(&[])];
        if kernel == Builtin::SgdStep {
            args.push(f(1.0));
        }
        assert_eq!(
            numeric::evaluate_with_vector_limit(kernel, &args, 1, 0).unwrap(),
            v(&[])
        );
        assert_eq!(
            numeric::evaluate(kernel, &args, 0).unwrap_err().kind,
            Kind::ExecutionLimit
        );
    }
}

#[test]
fn windows_and_maximum_length_remain_bounded() {
    let a = NumericVector::new(&[999.0, -1.0, 2.0, 999.0]).unwrap();
    let b = NumericVector::new(&[999.0, 3.0, 4.0, 999.0]).unwrap();
    assert_eq!(
        call(
            Builtin::ReluBackward,
            &[
                Value::Vector(a.window(1, 2).unwrap()),
                Value::Vector(b.window(1, 2).unwrap())
            ]
        ),
        v(&[0.0, 4.0])
    );
    let a = v(&vec![1.0; MAX_VECTOR_LENGTH]);
    let b = v(&vec![2.0; MAX_VECTOR_LENGTH]);
    assert_eq!(
        call(Builtin::SgdStep, &[a, b, f(0.5)]),
        v(&vec![0.0; MAX_VECTOR_LENGTH])
    );
}

#[test]
fn nonfinite_sgd_update_discards_pending_source_output() {
    for source in [
        "print(1); print(sgd_step([0.0], [1e308], 2.0));",
        "print(1); print(sgd_step([-1e308], [1e308], 1.0));",
    ] {
        let expected = scarp::check(source).unwrap_err();
        assert_eq!(expected.kind, Kind::NonFiniteNumber);
        let mut out = Vec::new();
        let RunError::Diagnostic(actual) = scarp::run(source, &mut out).unwrap_err() else {
            panic!("expected diagnostic");
        };
        assert_eq!(expected, actual);
        assert!(out.is_empty());
    }
}

#[test]
fn source_signatures_and_training_example_execute() {
    let source = include_str!("../examples/two_layer_training.srp");
    let expected = include_str!("../examples/two_layer_training.stdout");
    let mut out = Vec::new();
    scarp::check(source).unwrap();
    scarp::run(source, &mut out).unwrap();
    assert_eq!(String::from_utf8(out).unwrap(), expected);
}
