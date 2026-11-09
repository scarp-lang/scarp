use scarp::{
    diagnostics::DiagnosticKind as Kind,
    numeric::{self, Builtin},
    value::{FiniteFloat, NumericVector, Value, MAX_VECTOR_LENGTH},
    ExecutionOptions, RunError,
};

fn arguments(values: &[f64], fraction: Option<f64>) -> Vec<Value> {
    let mut args = vec![Value::Vector(NumericVector::new(values).unwrap())];
    if let Some(fraction) = fraction {
        args.push(Value::Float(FiniteFloat::new(fraction).unwrap()));
    }
    args
}

fn result(values: &[f64], fraction: f64) -> f64 {
    numeric::evaluate(
        Builtin::Quantile,
        &arguments(values, Some(fraction)),
        1_500_000,
    )
    .unwrap()
    .as_float()
    .unwrap()
}

fn output(source: &str) -> String {
    scarp::check(source).unwrap();
    let mut stdout = Vec::new();
    scarp::run(source, &mut stdout).unwrap();
    String::from_utf8(stdout).unwrap()
}

fn fails_atomically(source: &str, kind: Kind, argument: &str) {
    let options = ExecutionOptions::default();
    let checked = scarp::check_with_options(source, options).unwrap_err();
    assert_eq!(checked.kind, kind, "{source}");
    assert_eq!(&source[checked.span.start..checked.span.end], argument);
    let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
    let RunError::Diagnostic(executed) =
        scarp::run_with_io_and_options(source, &mut stdout, &mut stderr, options).unwrap_err()
    else {
        panic!("expected a source diagnostic");
    };
    assert_eq!(checked, executed);
    assert!(stdout.is_empty() && stderr.is_empty());
}

#[test]
fn median_and_quantile_are_typed_language_functions() {
    assert_eq!(
        output("print(median([4.0,1.0,3.0,2.0])); print(quantile([4.0,1.0,3.0,2.0],0.25));"),
        "2.5\n1.75\n"
    );
    assert_eq!(
        output("pure funcra middle(v: vector) -> float { return median(v); } pure funcra percentile(v: vector, q: float) -> float { return quantile(v,q); } print(middle([1.0,3.0,2.0])); print(percentile([1.0,2.0,3.0],0.5));"),
        "2.0\n2.0\n"
    );
    assert_eq!(
        output("print(median([9.0])); print(quantile([9.0],1.0));"),
        "9.0\n9.0\n"
    );
    assert_eq!(output("bind median=7; print(median);"), "7\n");
}

#[test]
fn endpoints_and_signed_zero_bits_are_deterministic() {
    let data = [-0.0, 0.0, -0.0, 0.0];
    assert_eq!(result(&data, 0.0).to_bits(), (-0.0f64).to_bits());
    assert_eq!(result(&data, 1.0).to_bits(), 0.0f64.to_bits());
    assert_eq!(result(&data, 0.5), 0.0);
    assert_eq!(result(&[5.0, 1.0, 4.0, 2.0, 3.0], 0.0), 1.0);
    assert_eq!(result(&[5.0, 1.0, 4.0, 2.0, 3.0], 1.0), 5.0);
}

#[test]
fn extreme_values_interpolate_without_overflow() {
    let largest = f64::MAX;
    assert_eq!(result(&[-largest, largest], 0.5), 0.0);
    assert!((result(&[-largest, largest], 0.25) / largest + 0.5).abs() < 1e-15);
    assert!((result(&[-largest, largest], 0.75) / largest - 0.5).abs() < 1e-15);
    assert_eq!(result(&[largest, largest], 0.5), largest);
    let tiny = f64::from_bits(1);
    assert_eq!(result(&[0.0, tiny], 1.0).to_bits(), tiny.to_bits());
}

#[test]
fn quantiles_respect_visible_windows_and_keep_backing_vectors_unchanged() {
    assert_eq!(
        output("bind all=[100.0,7.0,1.0,3.0,5.0,-100.0]; bind middle=window(all,1,4); print(median(middle)); print(quantile(middle,0.75)); print(all);"),
        "4.0\n5.5\n[100.0, 7.0, 1.0, 3.0, 5.0, -100.0]\n"
    );
    let input = arguments(&[4.0, 1.0, 2.0, 3.0], None);
    let original = input.clone();
    numeric::evaluate(Builtin::Median, &input, 28).unwrap();
    assert_eq!(input, original);
}

#[test]
fn invalid_fraction_empty_input_and_type_errors_never_leak_output() {
    for fraction in ["-0.01", "1.01"] {
        let source = format!("print(\"before\"); print(quantile([1.0,2.0],{fraction}));");
        fails_atomically(&source, Kind::NumericDomain, fraction);
    }
    fails_atomically(
        "print(\"before\"); print(median([]));",
        Kind::NumericDomain,
        "[]",
    );
    fails_atomically(
        "print(\"before\"); print(quantile([],0.5));",
        Kind::NumericDomain,
        "[]",
    );
    for source in [
        "print(median(1.0));",
        "print(quantile([1.0],1));",
        "print(median([1.0],0.5));",
        "print(quantile([1.0]));",
    ] {
        assert!(matches!(
            scarp::compile(source).unwrap_err().kind,
            Kind::TypeMismatch | Kind::CallArguments
        ));
    }
}

#[test]
fn preprocessing_is_charged_for_work_and_one_exact_copy() {
    let values = arguments(&[4.0, 1.0, 3.0, 2.0], Some(0.5));
    // (floor(log2(4)) + 5) * 4 = 28, copy = 8 * 4 = 32 bytes.
    assert_eq!(
        numeric::evaluate_with_vector_limit(Builtin::Quantile, &values, 28, 32)
            .unwrap()
            .as_float(),
        Some(2.5)
    );
    assert_eq!(
        numeric::evaluate_with_vector_limit(Builtin::Quantile, &values, 27, 32)
            .unwrap_err()
            .kind,
        Kind::ExecutionLimit
    );
    assert_eq!(
        numeric::evaluate_with_vector_limit(Builtin::Quantile, &values, 28, 31)
            .unwrap_err()
            .kind,
        Kind::VectorBudget
    );
    let median_args = arguments(&[4.0, 1.0, 3.0, 2.0], None);
    assert_eq!(
        numeric::evaluate_with_vector_limit(Builtin::Median, &median_args, 28, 32)
            .unwrap()
            .as_float(),
        Some(2.5)
    );
}

#[test]
fn deterministic_small_integer_reference_matches_linear_rank_quantiles() {
    let mut state = 0xC0FF_EE11_55AA_u64;
    for len in 1..=64 {
        for _ in 0..16 {
            let mut sample = Vec::new();
            for _ in 0..len {
                state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
                sample.push(((state >> 32) % 21) as f64 - 10.0);
            }
            let mut sorted = sample.clone();
            sorted.sort_unstable_by(f64::total_cmp);
            for (q, rank) in [
                (0.0, 0.0),
                (0.25, 0.25),
                (0.5, 0.5),
                (0.75, 0.75),
                (1.0, 1.0),
            ] {
                let p = (len - 1) as f64 * rank;
                let lo = p.floor() as usize;
                let hi = (lo + 1).min(len - 1);
                let expected = sorted[lo] * (1.0 - (p - lo as f64)) + sorted[hi] * (p - lo as f64);
                let actual = result(&sample, q);
                assert!(
                    (actual - expected).abs() <= 1.0e-12,
                    "q={q}, sample={sample:?}, expected={expected}, actual={actual}"
                );
            }
        }
    }
}

#[test]
fn maximum_visible_vector_is_bounded_and_deterministic() {
    let sample = vec![7.0; MAX_VECTOR_LENGTH];
    let args = arguments(&sample, None);
    let levels = u64::from(usize::BITS - MAX_VECTOR_LENGTH.leading_zeros());
    let work = MAX_VECTOR_LENGTH as u64 * (levels + 4);
    let bytes = MAX_VECTOR_LENGTH * 8;
    assert_eq!(
        numeric::evaluate_with_vector_limit(Builtin::Median, &args, work, bytes)
            .unwrap()
            .as_float(),
        Some(7.0)
    );
    assert_eq!(
        numeric::evaluate_with_vector_limit(Builtin::Median, &args, work, bytes - 1)
            .unwrap_err()
            .kind,
        Kind::VectorBudget
    );
}
