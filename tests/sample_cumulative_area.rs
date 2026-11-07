use scarp::{
    diagnostics::DiagnosticKind as Kind,
    numeric::{self, Builtin},
    value::{NumericVector, Value, MAX_VECTOR_LENGTH},
    ExecutionOptions, RunError,
};

fn vector(values: &[f64]) -> Value {
    Value::Vector(NumericVector::new(values).unwrap())
}

fn args(x: &[f64], y: &[f64]) -> [Value; 2] {
    [vector(x), vector(y)]
}

fn prefix(x: &[f64], y: &[f64]) -> Result<Value, scarp::diagnostics::Diagnostic> {
    numeric::evaluate(Builtin::SampleCumulativeArea, &args(x, y), u64::MAX)
}

#[test]
fn nonuniform_samples_accumulate_physical_displacement() {
    assert_eq!(
        prefix(&[0.0, 1.0, 3.0], &[0.0, 2.0, 6.0]).unwrap(),
        vector(&[0.0, 1.0, 9.0])
    );
    assert_eq!(
        prefix(&[0.0, 1.0, 2.0, 3.0], &[1.0, 1.0, -1.0, -1.0]).unwrap(),
        vector(&[0.0, 1.0, 1.0, 0.0])
    );
    assert_eq!(
        prefix(&[3.0, 4.0], &[0.0, 0.0]).unwrap(),
        vector(&[0.0, 0.0])
    );
}

#[test]
fn every_prefix_matches_affine_exact_integration() {
    for count in 2..=257 {
        let x: Vec<f64> = (0..count).map(|i| i as f64).collect();
        let y: Vec<f64> = x.iter().map(|v| 2.0 * v + 1.0).collect();
        let answer = prefix(&x, &y).unwrap();
        let v = answer.as_vector().unwrap().as_slice();
        assert_eq!(v.len(), count);
        for (i, actual) in v.iter().enumerate() {
            let at = i as f64;
            assert_eq!(actual.get(), at * at + at);
        }
        let total = numeric::evaluate(Builtin::SampleArea, &args(&x, &y), u64::MAX)
            .unwrap()
            .as_float()
            .unwrap();
        assert_eq!(v.last().unwrap().get(), total);
    }
}

#[test]
fn compensated_cancellation_and_subnormal_areas_are_retained() {
    let x = [0.0, 1.0, 2.0, 3.0];
    let y = [2.0e16, 0.0, 2.0, -2.0e16];
    let output = prefix(&x, &y).unwrap();
    let values = output.as_vector().unwrap().as_slice();
    assert_eq!(values[1].get(), 1.0e16);
    assert_eq!(values[3].get(), 1.0);
    let tiny = f64::from_bits(1);
    let extreme = prefix(&[0.0, 1.0e308], &[tiny, 0.0]).unwrap();
    let expected = (1.0e308 * 0.5) * tiny;
    assert_eq!(extreme.as_vector().unwrap().as_slice()[1].get(), expected);
}

#[test]
fn grid_length_domain_and_nonfinite_arithmetic_are_checked() {
    for (x, y, kind) in [
        (vec![], vec![], Kind::NumericDomain),
        (vec![1.0], vec![2.0], Kind::NumericDomain),
        (vec![0.0], vec![1.0, 2.0], Kind::VectorLength),
        (vec![0.0, 0.0], vec![1.0, 2.0], Kind::NumericDomain),
        (vec![2.0, 1.0], vec![1.0, 2.0], Kind::NumericDomain),
        (
            vec![-f64::MAX, f64::MAX],
            vec![1.0, 2.0],
            Kind::NonFiniteNumber,
        ),
        (
            vec![0.0, 2.0],
            vec![f64::MAX, f64::MAX],
            Kind::NonFiniteNumber,
        ),
    ] {
        assert_eq!(prefix(&x, &y).unwrap_err().kind, kind);
    }
}

#[test]
fn exact_work_and_payload_budget_boundaries() {
    let values = args(&[0.0, 1.0, 3.0], &[0.0, 2.0, 6.0]);
    assert_eq!(
        numeric::evaluate_with_vector_limit(Builtin::SampleCumulativeArea, &values, 8, 24)
        .unwrap(),
        vector(&[0.0, 1.0, 9.0])
    );
    assert_eq!(
        numeric::evaluate_with_vector_limit(Builtin::SampleCumulativeArea, &values, 7, 24)
        .unwrap_err()
        .kind,
        Kind::ExecutionLimit
    );
    assert_eq!(
        numeric::evaluate_with_vector_limit(Builtin::SampleCumulativeArea, &values, 8, 23)
        .unwrap_err()
        .kind,
        Kind::VectorBudget
    );
}

#[test]
fn shared_visible_windows_and_max_length_work_without_mutation() {
    let full_x = NumericVector::new(&[-10.0, 0.0, 1.0, 3.0, -10.0]).unwrap();
    let full_y = NumericVector::new(&[-10.0, 0.0, 2.0, 6.0, -10.0]).unwrap();
    let values = [
        Value::Vector(full_x.window(1, 3).unwrap()),
        Value::Vector(full_y.window(1, 3).unwrap()),
    ];
    assert_eq!(
        numeric::evaluate(Builtin::SampleCumulativeArea, &values, 8).unwrap(),
        vector(&[0.0, 1.0, 9.0])
    );
    let xs: Vec<f64> = (0..MAX_VECTOR_LENGTH).map(|i| i as f64).collect();
    let ys = vec![2.0; MAX_VECTOR_LENGTH];
    let output = prefix(&xs, &ys).unwrap();
    let result = output.as_vector().unwrap().as_slice();
    assert_eq!(result.len(), MAX_VECTOR_LENGTH);
    assert_eq!(result[0].get(), 0.0);
    assert_eq!(
        result[MAX_VECTOR_LENGTH - 1].get(),
        2.0 * (MAX_VECTOR_LENGTH - 1) as f64
    );
    assert_eq!(values[0], Value::Vector(full_x.window(1, 3).unwrap()));
}

#[test]
fn scarp_syntax_types_purity_and_atomic_error_output_work() {
    let source = "pure funcra trajectory(x: vector, y: vector) -> vector { return sample_cumulative_area(x, y); } print(trajectory([0.0,1.0,3.0],[0.0,2.0,6.0]));";
    scarp::compile(source).unwrap();
    scarp::check(source).unwrap();
    let mut output = Vec::new();
    scarp::run(source, &mut output).unwrap();
    assert_eq!(output, b"[0.0, 1.0, 9.0]\n");

    for invalid in [
        "print(sample_cumulative_area([0.0,1.0]));",
        "print(sample_cumulative_area(1.0,[2.0,3.0]));",
    ] {
        assert!(matches!(
            scarp::compile(invalid).unwrap_err().kind,
            Kind::CallArguments | Kind::TypeMismatch
        ));
    }
    let source = "print(\"not emitted\"); print(sample_cumulative_area([0.0,0.0],[1.0,2.0]));";
    let opts = ExecutionOptions::default();
    let checked = scarp::check_with_options(source, opts).unwrap_err();
    assert_eq!(checked.kind, Kind::NumericDomain);
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let RunError::Diagnostic(actual) =
        scarp::run_with_io_and_options(source, &mut out, &mut err, opts).unwrap_err()
    else {
        panic!("expected diagnostic");
    };
    assert_eq!(actual, checked);
    assert!(out.is_empty() && err.is_empty());
}
