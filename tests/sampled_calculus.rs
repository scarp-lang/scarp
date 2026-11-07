use scarp::{
    diagnostics::DiagnosticKind as Kind,
    numeric::{self, Builtin},
    value::{NumericVector, Value, MAX_VECTOR_LENGTH},
    ExecutionOptions, RunError,
};

fn vector(values: &[f64]) -> Value {
    Value::Vector(NumericVector::new(values).unwrap())
}

fn arguments(x: &[f64], y: &[f64]) -> [Value; 2] {
    [vector(x), vector(y)]
}

fn call(function: Builtin, x: &[f64], y: &[f64]) -> Result<Value, scarp::diagnostics::Diagnostic> {
    numeric::evaluate(function, &arguments(x, y), u64::MAX)
}

fn close(actual: f64, expected: f64) {
    let scale = expected.abs().max(1.0);
    assert!((actual - expected).abs() <= 2.0e-14 * scale, "{actual} != {expected}");
}

#[test]
fn sampled_area_and_slopes_integrate_linear_data_on_nonuniform_grid() {
    let x = [0.0, 1.0, 3.0, 4.0];
    let y = [0.0, 2.0, 6.0, 8.0];
    assert_eq!(call(Builtin::SampleArea, &x, &y).unwrap().as_float(), Some(16.0));
    assert_eq!(
        call(Builtin::SampleSlopes, &x, &y).unwrap(),
        vector(&[2.0, 2.0, 2.0])
    );
    assert_eq!(
        call(Builtin::SampleArea, &[0.0, 1.0, 2.0], &[1.0, 0.0, -1.0])
            .unwrap()
            .as_float(),
        Some(0.0)
    );
    assert_eq!(
        call(Builtin::SampleSlopes, &[0.0, 1.0, 3.0], &[0.0, 1.0, 9.0]).unwrap(),
        vector(&[1.0, 4.0])
    );
}

#[test]
fn generated_linear_samples_match_exact_analytic_integrals() {
    for count in 2..=257 {
        let coordinates: Vec<f64> = (0..count).map(f64::from).collect();
        let readings: Vec<f64> = coordinates.iter().map(|x| 2.0 * x + 1.0).collect();
        let n = (count - 1) as f64;
        close(
            call(Builtin::SampleArea, &coordinates, &readings)
                .unwrap()
                .as_float()
                .unwrap(),
            n * n + n,
        );
        let slopes = call(Builtin::SampleSlopes, &coordinates, &readings).unwrap();
        assert!(slopes
            .as_vector()
            .unwrap()
            .as_slice()
            .iter()
            .all(|slope| slope.get() == 2.0));
    }
}

#[test]
fn extreme_finite_data_preserves_subnormal_areas_and_finite_large_slopes() {
    let tiny = f64::from_bits(1);
    assert_eq!(
        call(Builtin::SampleArea, &[0.0, 1.0], &[tiny, tiny])
            .unwrap()
            .as_float()
            .unwrap()
            .to_bits(),
        tiny.to_bits()
    );
    assert_eq!(
        call(Builtin::SampleArea, &[0.0, 1.0], &[f64::MAX, f64::MAX])
            .unwrap()
            .as_float(),
        Some(f64::MAX)
    );
    let slope = call(
        Builtin::SampleSlopes,
        &[0.0, 1.0e308],
        &[-f64::MAX, f64::MAX],
    )
    .unwrap();
    close(
        slope.as_vector().unwrap().as_slice()[0].get(),
        (f64::MAX / 1.0e308) * 2.0,
    );
    assert_eq!(
        call(
            Builtin::SampleArea,
            &[0.0, 1.0e308],
            &[-f64::MAX, f64::MAX]
        )
        .unwrap()
        .as_float(),
        Some(0.0)
    );
}

#[test]
fn no_empty_singleton_mismatch_duplicate_or_reversed_grid() {
    for (x, y, kind) in [
        (vec![], vec![], Kind::NumericDomain),
        (vec![1.0], vec![2.0], Kind::NumericDomain),
        (vec![0.0], vec![0.0, 1.0], Kind::VectorLength),
        (vec![0.0, 0.0], vec![1.0, 2.0], Kind::NumericDomain),
        (vec![2.0, 1.0], vec![1.0, 2.0], Kind::NumericDomain),
    ] {
        for function in [Builtin::SampleArea, Builtin::SampleSlopes] {
            assert_eq!(call(function, &x, &y).unwrap_err().kind, kind);
        }
    }
}

#[test]
fn unrepresentable_coordinate_width_or_output_is_a_numeric_error() {
    for (function, x, y) in [
        (
            Builtin::SampleArea,
            vec![-f64::MAX, f64::MAX],
            vec![1.0, 1.0],
        ),
        (
            Builtin::SampleSlopes,
            vec![-f64::MAX, f64::MAX],
            vec![1.0, 1.0],
        ),
        (
            Builtin::SampleArea,
            vec![0.0, 2.0],
            vec![f64::MAX, f64::MAX],
        ),
        (
            Builtin::SampleSlopes,
            vec![0.0, 1.0],
            vec![-f64::MAX, f64::MAX],
        ),
    ] {
        assert_eq!(
            call(function, &x, &y).unwrap_err().kind,
            Kind::NonFiniteNumber
        );
    }
}

#[test]
fn host_budget_and_allocation_boundaries_are_exact() {
    let args = arguments(&[0.0, 1.0, 3.0], &[0.0, 2.0, 6.0]);
    assert_eq!(
        numeric::evaluate_with_vector_limit(Builtin::SampleArea, &args, 8, 0)
            .unwrap()
            .as_float(),
        Some(9.0)
    );
    assert_eq!(
        numeric::evaluate_with_vector_limit(Builtin::SampleSlopes, &args, 8, 16).unwrap(),
        vector(&[2.0, 2.0])
    );
    for function in [Builtin::SampleArea, Builtin::SampleSlopes] {
        assert_eq!(
            numeric::evaluate(function, &args, 7).unwrap_err().kind,
            Kind::ExecutionLimit
        );
    }
    assert_eq!(
        numeric::evaluate_with_vector_limit(Builtin::SampleSlopes, &args, 8, 15)
            .unwrap_err()
            .kind,
        Kind::VectorBudget
    );
    assert_eq!(args, arguments(&[0.0, 1.0, 3.0], &[0.0, 2.0, 6.0]));
}

#[test]
fn only_visible_shared_windows_are_used() {
    let full_x = NumericVector::new(&[999.0, 0.0, 1.0, 3.0, -999.0]).unwrap();
    let full_y = NumericVector::new(&[999.0, 0.0, 2.0, 6.0, -999.0]).unwrap();
    let args = [
        Value::Vector(full_x.window(1, 3).unwrap()),
        Value::Vector(full_y.window(1, 3).unwrap()),
    ];
    assert_eq!(
        numeric::evaluate(Builtin::SampleArea, &args, 8)
            .unwrap()
            .as_float(),
        Some(9.0)
    );
    assert_eq!(
        numeric::evaluate_with_vector_limit(Builtin::SampleSlopes, &args, 8, 16).unwrap(),
        vector(&[2.0, 2.0])
    );
}

#[test]
fn maximum_vector_length_is_bounded_and_linear() {
    let coordinates: Vec<f64> = (0..MAX_VECTOR_LENGTH).map(|i| i as f64).collect();
    let readings = vec![1.0; MAX_VECTOR_LENGTH];
    assert_eq!(
        call(Builtin::SampleArea, &coordinates, &readings)
            .unwrap()
            .as_float(),
        Some((MAX_VECTOR_LENGTH - 1) as f64)
    );
    let slopes = call(Builtin::SampleSlopes, &coordinates, &readings).unwrap();
    assert_eq!(slopes.as_vector().unwrap().len(), MAX_VECTOR_LENGTH - 1);
    assert!(slopes
        .as_vector()
        .unwrap()
        .as_slice()
        .iter()
        .all(|slope| slope.get() == 0.0));
}

#[test]
fn compiler_functions_and_execution_match_native_results() {
    let source = "pure funcra integral(x: vector, y: vector) -> float { return sample_area(x,y); } bind x=[0.0,1.0,3.0]; bind y=[0.0,2.0,6.0]; print(integral(x,y)); print(sample_slopes(x,y));";
    scarp::compile(source).unwrap();
    scarp::check(source).unwrap();
    let mut output = Vec::new();
    scarp::run(source, &mut output).unwrap();
    assert_eq!(output, b"9.0\n[2.0, 2.0]\n");
}

#[test]
fn argument_type_arity_and_atomic_error_output_are_preserved() {
    for name in ["sample_area", "sample_slopes"] {
        for source in [
            format!("print({name}([1.0]));"),
            format!("print({name}([1.0],[1]));"),
            format!("print({name}(1.0,[1.0]));"),
        ] {
            assert!(matches!(
                scarp::compile(&source).unwrap_err().kind,
                Kind::CallArguments | Kind::TypeMismatch
            ));
        }
    }
    let source = "print(\"not emitted\"); print(sample_area([0.0,0.0],[1.0,2.0]));";
    let options = ExecutionOptions::default();
    let checked = scarp::check_with_options(source, options).unwrap_err();
    assert_eq!(checked.kind, Kind::NumericDomain);
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let RunError::Diagnostic(executed) =
        scarp::run_with_io_and_options(source, &mut out, &mut err, options).unwrap_err()
    else {
        panic!("expected runtime diagnostic");
    };
    assert_eq!(checked, executed);
    assert!(out.is_empty() && err.is_empty());
}
