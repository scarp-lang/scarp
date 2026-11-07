use scarp::{
    diagnostics::DiagnosticKind as Kind,
    numeric::{self, Builtin},
    value::{FiniteFloat, NumericVector, Value},
    ExecutionOptions, RunError,
};

fn values(scores: &[f64], temperature: f64) -> Vec<Value> {
    vec![
        Value::Vector(NumericVector::new(scores).unwrap()),
        Value::Float(FiniteFloat::new(temperature).unwrap()),
    ]
}

fn numbers(function: Builtin, scores: &[f64], temperature: f64) -> Vec<f64> {
    numeric::evaluate(function, &values(scores, temperature), 1_000_000)
        .unwrap()
        .as_vector()
        .unwrap()
        .as_slice()
        .iter()
        .map(|v| v.get())
        .collect()
}

fn output(source: &str) -> String {
    scarp::check(source).unwrap();
    let mut out = Vec::new();
    scarp::run(source, &mut out).unwrap();
    String::from_utf8(out).unwrap()
}

fn failure(source: &str, expected: Kind) -> scarp::diagnostics::Diagnostic {
    let options = ExecutionOptions::default();
    let checked = scarp::check_with_options(source, options).unwrap_err();
    assert_eq!(checked.kind, expected);
    let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
    let RunError::Diagnostic(actual) =
        scarp::run_with_io_and_options(source, &mut stdout, &mut stderr, options).unwrap_err()
    else {
        panic!("expected diagnostic");
    };
    assert_eq!(checked, actual);
    assert!(stdout.is_empty() && stderr.is_empty());
    checked
}

#[test]
fn typed_temperatures_compose_with_inference_and_user_functions() {
    assert_eq!(
        output("print(softmax_temp([2.0,2.0],0.5)); print(logsoftmax_temp([5.0],2.0));"),
        "[0.5, 0.5]\n[0.0]\n"
    );
    assert_eq!(
        output("pure funcra probs(scores: vector, temp: float) -> vector { return softmax_temp(scores,temp); } print(probs([2.0,2.0],1.0));"),
        "[0.5, 0.5]\n"
    );
    assert_eq!(
        output("print(topk(softmax_temp([1.0,4.0,4.0,2.0],0.5),3));"),
        "[integer: 1, 2, 3]\n"
    );
}

#[test]
fn temperature_one_matches_existing_softmax_and_logsoftmax() {
    let data = [1000.0, 999.0, -1000.0, 1000.0];
    for (old, new) in [
        (Builtin::Softmax, Builtin::SoftmaxTemp),
        (Builtin::LogSoftmax, Builtin::LogSoftmaxTemp),
    ] {
        let old = numeric::evaluate(
            old,
            &[Value::Vector(NumericVector::new(&data).unwrap())],
            1_000,
        )
        .unwrap();
        let selected = numbers(new, &data, 1.0);
        for (actual, expected) in selected.iter().zip(old.as_vector().unwrap().as_slice()) {
            assert!(
                (actual - expected.get()).abs() <= 1.0e-12,
                "{actual} != {}",
                expected.get()
            );
        }
    }
}

#[test]
fn temperature_controls_concentration_and_log_probabilities() {
    let logits = [0.0, 3.0, 2.0, 3.0];
    let cold = numbers(Builtin::SoftmaxTemp, &logits, 0.25);
    let hot = numbers(Builtin::SoftmaxTemp, &logits, 10.0);
    assert!(cold[1] > hot[1] && cold[0] < hot[0]);
    assert_eq!(cold[1], cold[3]);
    assert_eq!(hot[1], hot[3]);
    for temperature in [0.1, 0.25, 1.0, 10.0, 1000.0] {
        let probs = numbers(Builtin::SoftmaxTemp, &logits, temperature);
        let logs = numbers(Builtin::LogSoftmaxTemp, &logits, temperature);
        let sum: f64 = probs.iter().sum();
        assert!((sum - 1.0).abs() < 1.0e-12);
        for (probability, log_probability) in probs.iter().zip(logs) {
            assert!((log_probability.exp() - probability).abs() < 1.0e-12);
        }
    }
}

#[test]
fn overflowed_score_subtraction_is_recovered_after_temperature_scaling() {
    let large = f64::MAX;
    let probabilities = numbers(Builtin::SoftmaxTemp, &[-large, large], large);
    // (-MAX - MAX) overflows, but (-MAX / MAX) - (MAX / MAX)
    // is -2 and must not incorrectly collapse the first class to zero.
    assert!(probabilities[0] > 0.1 && probabilities[0] < 0.2);
    assert!(probabilities[1] > 0.8 && probabilities[1] < 0.9);
    let logs = numbers(Builtin::LogSoftmaxTemp, &[-large, large], large);
    assert!((logs[0] - logs[1] + 2.0).abs() < 1.0e-12);
}

#[test]
fn tiny_temperatures_allow_softmax_underflow_but_reject_nonfinite_log_output() {
    let tiny = f64::from_bits(1);
    let args = values(&[0.0, 1.0], tiny);
    let probabilities = numeric::evaluate(Builtin::SoftmaxTemp, &args, 8).unwrap();
    let values = probabilities.as_vector().unwrap().as_slice();
    assert_eq!((values[0].get(), values[1].get()), (0.0, 1.0));
    assert_eq!(
        numeric::evaluate(Builtin::LogSoftmaxTemp, &args, 8)
            .unwrap_err()
            .kind,
        Kind::NonFinite
    );
}

#[test]
fn empty_input_invalid_temperature_type_and_arity_are_atomic() {
    for name in ["softmax_temp", "logsoftmax_temp"] {
        for value in ["0.0", "-0.0", "-1.0"] {
            let source = format!("print(\"before\"); print({name}([1.0],{value}));");
            let error = failure(&source, Kind::NumericDomain);
            assert_eq!(&source[error.span.start..error.span.end], value);
        }
        failure(&format!("print({name}([],1.0));"), Kind::NumericDomain);
        failure(&format!("print({name}([1.0],1));"), Kind::TypeMismatch);
        failure(&format!("print({name}([1],1.0));"), Kind::TypeMismatch);
        failure(&format!("print({name}([1.0]));"), Kind::CallArguments);
        failure(&format!("print({name}([1.0],1.0,1.0));"), Kind::CallArguments);
    }
}

#[test]
fn scalar_work_and_result_storage_are_bounded() {
    let args = values(&[1.0, 2.0, 3.0], 2.0);
    for function in [Builtin::SoftmaxTemp, Builtin::LogSoftmaxTemp] {
        numeric::evaluate_with_vector_limit(function, &args, 12, 24).unwrap();
        assert_eq!(
            numeric::evaluate_with_vector_limit(function, &args, 11, 24)
                .unwrap_err()
                .kind,
            Kind::ExecutionLimit
        );
        assert_eq!(
            numeric::evaluate_with_vector_limit(function, &args, 12, 23)
                .unwrap_err()
                .kind,
            Kind::VectorBudget
        );
    }
}

#[test]
fn windows_and_equal_scores_keep_independent_immutable_inputs() {
    assert_eq!(
        output("bind source=[20.0,1.0,1.0,5.0]; bind visible=window(source,1,2); print(softmax_temp(visible,2.0)); print(source);"),
        "[0.5, 0.5]\n[20.0, 1.0, 1.0, 5.0]\n"
    );
}
