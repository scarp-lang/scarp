use scarp::{
    diagnostics::DiagnosticKind as Kind,
    numeric::{self, Builtin},
    value::{NumericVector, Value, MAX_VECTOR_LENGTH},
    ExecutionOptions, RunError,
};

fn pair(left: &[f64], right: &[f64]) -> [Value; 2] {
    [
        Value::Vector(NumericVector::new(left).unwrap()),
        Value::Vector(NumericVector::new(right).unwrap()),
    ]
}

fn pearson(left: &[f64], right: &[f64]) -> f64 {
    numeric::evaluate(Builtin::PearsonCorrelation, &pair(left, right), u64::MAX)
        .unwrap()
        .as_float()
        .unwrap()
}

fn close(actual: f64, expected: f64) {
    assert!(
        (actual - expected).abs() <= 2e-12,
        "expected {expected}, got {actual}"
    );
}

#[test]
fn correlation_identifies_centered_relationships_not_uncentered_cosines() {
    close(pearson(&[1.0, 2.0, 3.0], &[10.0, 20.0, 30.0]), 1.0);
    close(pearson(&[1.0, 2.0, 3.0], &[30.0, 20.0, 10.0]), -1.0);
    close(pearson(&[-1.0, 0.0, 1.0], &[1.0, -2.0, 1.0]), 0.0);
    close(pearson(&[3.0, 4.0], &[9.0, 6.0]), -1.0);
    close(pearson(&[1.0, 2.0, 3.0], &[1.0, 4.0, 9.0]), 0.989743318610787);
}

#[test]
fn shifted_and_scaled_extreme_finite_inputs_remain_well_conditioned() {
    let tiny = f64::from_bits(1);
    close(pearson(&[0.0, tiny], &[tiny, 0.0]), -1.0);
    close(
        pearson(&[-f64::MAX, f64::MAX], &[-f64::MAX, f64::MAX]),
        1.0,
    );
    close(
        pearson(&[-f64::MAX, f64::MAX], &[f64::MAX, -f64::MAX]),
        -1.0,
    );
    close(pearson(&[1.0e308, 1.0e308 + 2.0e292], &[7.0, 8.0]), 1.0);
    close(pearson(&[1.0e308, 1.0e308 + 2.0e292], &[8.0, 7.0]), -1.0);
}

#[test]
fn domains_types_and_source_argument_spans_are_preserved() {
    for (left, right, kind) in [
        (vec![], vec![], Kind::NumericDomain),
        (vec![1.0], vec![1.0], Kind::NumericDomain),
        (vec![1.0, 2.0], vec![1.0], Kind::VectorLength),
        (vec![3.0, 3.0], vec![1.0, 2.0], Kind::NumericDomain),
        (vec![1.0, 2.0], vec![7.0, 7.0], Kind::NumericDomain),
    ] {
        assert_eq!(
            numeric::evaluate(Builtin::PearsonCorrelation, &pair(&left, &right), u64::MAX)
                .unwrap_err()
                .kind,
            kind
        );
    }
    for source in [
        "print(pearson_correlation([1.0]));",
        "print(pearson_correlation([1.0,2.0],1.0));",
        "print(pearson_correlation([1.0],[2.0],[3.0]));",
    ] {
        assert!(matches!(
            scarp::compile(source).unwrap_err().kind,
            Kind::CallArguments | Kind::TypeMismatch
        ));
    }
    let source = "print(\"uncommitted\"); print(pearson_correlation([1.0,2.0],[0.0,0.0]));";
    let options = ExecutionOptions::default();
    let expected = scarp::check_with_options(source, options).unwrap_err();
    assert_eq!(expected.kind, Kind::NumericDomain);
    assert_eq!(
        &source[expected.span.start..expected.span.end],
        "[0.0,0.0]"
    );
    let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
    let RunError::Diagnostic(actual) =
        scarp::run_with_io_and_options(source, &mut stdout, &mut stderr, options).unwrap_err()
    else {
        panic!("expected a numerical diagnostic");
    };
    assert_eq!(actual, expected);
    assert!(stdout.is_empty() && stderr.is_empty());
}

#[test]
fn work_and_allocation_boundaries_are_explicit() {
    let args = pair(&[1.0, 2.0, 3.0], &[10.0, 20.0, 30.0]);
    close(
        numeric::evaluate_with_vector_limit(Builtin::PearsonCorrelation, &args, 36, 0)
            .unwrap()
            .as_float()
            .unwrap(),
        1.0,
    );
    assert_eq!(
        numeric::evaluate_with_vector_limit(Builtin::PearsonCorrelation, &args, 35, 0)
            .unwrap_err()
            .kind,
        Kind::ExecutionLimit
    );
    assert_eq!(args, pair(&[1.0, 2.0, 3.0], &[10.0, 20.0, 30.0]));
}

#[test]
fn visible_windows_and_maximum_source_length_work() {
    let a = NumericVector::new(&[9.0, 1.0, 2.0, 3.0, 9.0]).unwrap();
    let b = NumericVector::new(&[8.0, 10.0, 20.0, 30.0, 8.0]).unwrap();
    let values = [
        Value::Vector(a.window(1, 3).unwrap()),
        Value::Vector(b.window(1, 3).unwrap()),
    ];
    close(
        numeric::evaluate(Builtin::PearsonCorrelation, &values, 36)
            .unwrap()
            .as_float()
            .unwrap(),
        1.0,
    );
    let x: Vec<f64> = (0..MAX_VECTOR_LENGTH).map(|i| i as f64).collect();
    let y: Vec<f64> = x.iter().map(|v| -v).collect();
    close(pearson(&x, &y), -1.0);
}

#[test]
fn deterministic_seeded_integer_oracle_uses_exact_centered_moments() {
    let mut seed = 0xC0FF_EE11_55AA_u64;
    for len in 2usize..=64 {
        for _ in 0..10 {
            let mut x = Vec::new();
            let mut y = Vec::new();
            for _ in 0..len {
                seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
                x.push(((seed >> 32) % 41) as i64 - 20);
                seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
                y.push(((seed >> 32) % 41) as i64 - 20);
            }
            let count = len as i128;
            let sx: i128 = x.iter().map(|v| i128::from(*v)).sum();
            let sy: i128 = y.iter().map(|v| i128::from(*v)).sum();
            let sxx: i128 = x.iter().map(|v| i128::from(*v) * i128::from(*v)).sum();
            let syy: i128 = y.iter().map(|v| i128::from(*v) * i128::from(*v)).sum();
            let sxy: i128 = x
                .iter()
                .zip(&y)
                .map(|(a, b)| i128::from(*a) * i128::from(*b))
                .sum();
            let vx = count * sxx - sx * sx;
            let vy = count * syy - sy * sy;
            if vx == 0 || vy == 0 {
                continue;
            }
            let expected =
                (count * sxy - sx * sy) as f64 / ((vx as f64).sqrt() * (vy as f64).sqrt());
            let xf: Vec<f64> = x.iter().map(|v| *v as f64).collect();
            let yf: Vec<f64> = y.iter().map(|v| *v as f64).collect();
            close(pearson(&xf, &yf), expected);
        }
    }
}

#[test]
fn typed_pure_language_call_has_expected_output() {
    let source = "pure funcra relation(a: vector, b: vector) -> float { return pearson_correlation(a,b); } print(relation([1.0,2.0,3.0],[10.0,20.0,30.0]));";
    scarp::check(source).unwrap();
    let mut stdout = Vec::new();
    scarp::run(source, &mut stdout).unwrap();
    assert_eq!(stdout, b"1.0\n");
}
