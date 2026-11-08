use scarp::{
    diagnostics::DiagnosticKind as Kind,
    numeric::{self, Builtin},
    value::{NumericVector, Value, MAX_VECTOR_LENGTH},
    ExecutionOptions, RunError,
};

fn vector(values: &[f64]) -> Value {
    Value::Vector(NumericVector::new(values).unwrap())
}

fn pair(left: &[f64], right: &[f64]) -> [Value; 2] {
    [vector(left), vector(right)]
}

fn cosine(left: &[f64], right: &[f64]) -> f64 {
    numeric::evaluate(Builtin::CosineSimilarity, &pair(left, right), u64::MAX)
        .unwrap()
        .as_float()
        .unwrap()
}

fn close(actual: f64, expected: f64) {
    assert!(
        (actual - expected).abs() <= 4e-14 * expected.abs().max(1.0),
        "{actual} != {expected}"
    );
}

#[test]
fn cosine_similarity_has_defined_geometric_and_signed_zero_behavior() {
    close(cosine(&[3.0, 4.0], &[6.0, 8.0]), 1.0);
    close(cosine(&[3.0, 4.0], &[-6.0, -8.0]), -1.0);
    close(cosine(&[1.0, 0.0], &[0.0, 1.0]), 0.0);
    close(cosine(&[1.0, 1.0], &[1.0, -1.0]), 0.0);
    assert_eq!(cosine(&[-0.0, 1.0], &[0.0, 1.0]), 1.0);
    assert_eq!(cosine(&[3.0], &[-4.0]), -1.0);
}

#[test]
fn independently_scaled_extreme_vectors_remain_finite() {
    let tiny = f64::from_bits(1);
    close(cosine(&[tiny, tiny], &[tiny, tiny]), 1.0);
    close(cosine(&[tiny, tiny], &[-tiny, -tiny]), -1.0);
    close(cosine(&[f64::MAX, f64::MAX], &[f64::MAX, f64::MAX]), 1.0);
    close(
        cosine(&[f64::MAX, f64::MAX], &[-f64::MAX, f64::MAX]),
        0.0,
    );
    close(cosine(&[1e308, 1e-308], &[1e308, 1e-308]), 1.0);
}

#[test]
fn argument_domains_shapes_and_types_are_checked() {
    for (left, right, kind) in [
        (vec![], vec![], Kind::NumericDomain),
        (vec![1.0], vec![], Kind::VectorLength),
        (vec![0.0, 0.0], vec![1.0, 2.0], Kind::NumericDomain),
        (vec![1.0, 2.0], vec![-0.0, 0.0], Kind::NumericDomain),
    ] {
        assert_eq!(
            numeric::evaluate(Builtin::CosineSimilarity, &pair(&left, &right), u64::MAX)
                .unwrap_err()
                .kind,
            kind
        );
    }
    for invalid in [
        "print(cosine_similarity([1.0]));",
        "print(cosine_similarity([1.0],1.0));",
        "print(cosine_similarity([1.0],[1.0],[1.0]));",
    ] {
        assert!(matches!(
            scarp::compile(invalid).unwrap_err().kind,
            Kind::CallArguments | Kind::TypeMismatch
        ));
    }
}

#[test]
fn work_and_allocation_boundaries_are_deterministic() {
    let arguments = pair(&[3.0, 4.0, 0.0], &[6.0, 8.0, 0.0]);
    close(
        numeric::evaluate_with_vector_limit(Builtin::CosineSimilarity, &arguments, 18, 0)
            .unwrap()
            .as_float()
            .unwrap(),
        1.0,
    );
    assert_eq!(
        numeric::evaluate(Builtin::CosineSimilarity, &arguments, 17)
            .unwrap_err()
            .kind,
        Kind::ExecutionLimit
    );
    assert_eq!(arguments, pair(&[3.0, 4.0, 0.0], &[6.0, 8.0, 0.0]));
}

#[test]
fn visible_windows_and_maximum_length_remain_bounded() {
    let a = NumericVector::new(&[9.0, 3.0, 4.0, 9.0]).unwrap();
    let b = NumericVector::new(&[8.0, 6.0, 8.0, 8.0]).unwrap();
    let args = [
        Value::Vector(a.window(1, 2).unwrap()),
        Value::Vector(b.window(1, 2).unwrap()),
    ];
    close(
        numeric::evaluate(Builtin::CosineSimilarity, &args, 12)
            .unwrap()
            .as_float()
            .unwrap(),
        1.0,
    );
    let ones = vec![1.0; MAX_VECTOR_LENGTH];
    let negatives = vec![-1.0; MAX_VECTOR_LENGTH];
    close(cosine(&ones, &negatives), -1.0);
}

#[test]
fn deterministic_integer_oracle_covers_mixed_sign_and_zero_components() {
    let mut seed = 0xC051_5EED_u64;
    for n in 2..=64 {
        for _ in 0..8 {
            let mut left = Vec::new();
            let mut right = Vec::new();
            for _ in 0..n {
                seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
                left.push(((seed >> 32) % 41) as i64 - 20);
                seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
                right.push(((seed >> 32) % 41) as i64 - 20);
            }
            let left_sq: i128 = left.iter().map(|x| i128::from(*x) * i128::from(*x)).sum();
            let right_sq: i128 = right.iter().map(|x| i128::from(*x) * i128::from(*x)).sum();
            if left_sq == 0 || right_sq == 0 {
                continue;
            }
            let dot: i128 = left
                .iter()
                .zip(&right)
                .map(|(x, y)| i128::from(*x) * i128::from(*y))
                .sum();
            let expected = dot as f64 / ((left_sq as f64).sqrt() * (right_sq as f64).sqrt());
            let a: Vec<f64> = left.iter().map(|x| *x as f64).collect();
            let b: Vec<f64> = right.iter().map(|x| *x as f64).collect();
            close(cosine(&a, &b), expected);
        }
    }
}

#[test]
fn pure_language_calls_and_atomic_runtime_errors_match_static_evaluation() {
    let source = "pure funcra similarity(a: vector, b: vector) -> float { return cosine_similarity(a,b); } print(similarity([3.0,4.0],[6.0,8.0]));";
    scarp::compile(source).unwrap();
    scarp::check(source).unwrap();
    let mut output = Vec::new();
    scarp::run(source, &mut output).unwrap();
    assert_eq!(output, b"1.0\n");

    let source = "print(\"not emitted\"); print(cosine_similarity([0.0],[1.0]));";
    let options = ExecutionOptions::default();
    let expected = scarp::check_with_options(source, options).unwrap_err();
    assert_eq!(expected.kind, Kind::NumericDomain);
    let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
    let RunError::Diagnostic(actual) =
        scarp::run_with_io_and_options(source, &mut stdout, &mut stderr, options).unwrap_err()
    else {
        panic!("expected numeric diagnostic");
    };
    assert_eq!(actual, expected);
    assert!(stdout.is_empty() && stderr.is_empty());
}
