use scarp::{
    diagnostics::DiagnosticKind as Kind,
    numeric::{self, Builtin},
    value::{NumericVector, Value, MAX_VECTOR_LENGTH},
    ExecutionOptions, RunError,
};

fn vector(values: &[f64]) -> Value {
    Value::Vector(NumericVector::new(values).unwrap())
}

fn call(kernel: Builtin, left: &[f64], right: &[f64]) -> Value {
    numeric::evaluate(kernel, &[vector(left), vector(right)], u64::MAX).unwrap()
}

#[test]
fn source_operations_compose_with_dot_and_preserve_inputs() {
    let source = r#"
        pure funcra residual(a: vector, b: vector) -> vector
            needs len(a) == len(b);
            ensures len(result) == len(a);
        { return vector_sub(a, b); }
        bind a = [1.0, 2.0, 3.0];
        bind b = [4.0, 5.0, 6.0];
        print(hadamard(a, b));
        print(sum(hadamard(a, b)), dot(a, b));
        print(residual(a, b));
        print(a, b);
    "#;
    let mut out = Vec::new();
    scarp::check(source).unwrap();
    scarp::run(source, &mut out).unwrap();
    assert_eq!(
        String::from_utf8(out).unwrap(),
        "[4.0, 10.0, 18.0]\n32.0 32.0\n[-3.0, -3.0, -3.0]\n[1.0, 2.0, 3.0] [4.0, 5.0, 6.0]\n"
    );
}

#[test]
fn empty_and_signed_values_are_supported() {
    for kernel in [Builtin::VectorSub, Builtin::Hadamard] {
        assert_eq!(call(kernel, &[], &[]), vector(&[]));
    }
    assert_eq!(
        call(Builtin::VectorSub, &[-2.0, 5.0], &[3.0, -1.0]),
        vector(&[-5.0, 6.0])
    );
    assert_eq!(
        call(Builtin::Hadamard, &[-2.0, 5.0], &[3.0, -1.0]),
        vector(&[-6.0, -5.0])
    );
    let result = call(Builtin::Hadamard, &[-0.0], &[2.0]);
    assert_eq!(
        result.as_vector().unwrap().as_slice()[0].get().to_bits(),
        (-0.0f64).to_bits()
    );
}

#[test]
fn incompatible_lengths_precede_work_and_allocation() {
    for kernel in [Builtin::VectorSub, Builtin::Hadamard] {
        for args in [
            [vector(&[]), vector(&[1.0])],
            [vector(&[1.0]), vector(&[])],
            [vector(&[1.0, 2.0]), vector(&[1.0])],
        ] {
            assert_eq!(
                numeric::evaluate_with_vector_limit(kernel, &args, 0, 0)
                    .unwrap_err()
                    .kind,
                Kind::VectorLength
            );
        }
    }
}

#[test]
fn work_and_payload_limits_are_exact() {
    let args = [vector(&[1.0, 2.0, 3.0]), vector(&[4.0, 5.0, 6.0])];
    let before = args.clone();
    for kernel in [Builtin::VectorSub, Builtin::Hadamard] {
        numeric::evaluate_with_vector_limit(kernel, &args, 3, 24).unwrap();
        assert_eq!(
            numeric::evaluate_with_vector_limit(kernel, &args, 2, 24)
                .unwrap_err()
                .kind,
            Kind::ExecutionLimit
        );
        assert_eq!(
            numeric::evaluate_with_vector_limit(kernel, &args, 3, 23)
                .unwrap_err()
                .kind,
            Kind::VectorBudget
        );
        assert_eq!(args, before);
    }
}

#[test]
fn windows_never_expose_hidden_elements() {
    let a = NumericVector::new(&[999.0, 2.0, 3.0, 999.0]).unwrap();
    let b = NumericVector::new(&[999.0, 999.0, 4.0, 5.0]).unwrap();
    let args = [
        Value::Vector(a.window(1, 2).unwrap()),
        Value::Vector(b.window(2, 2).unwrap()),
    ];
    assert_eq!(
        numeric::evaluate(Builtin::VectorSub, &args, 2).unwrap(),
        vector(&[-2.0, -2.0])
    );
    assert_eq!(
        numeric::evaluate(Builtin::Hadamard, &args, 2).unwrap(),
        vector(&[8.0, 15.0])
    );
}

#[test]
fn maximum_length_preserves_every_cell() {
    let a = vec![2.0; MAX_VECTOR_LENGTH];
    let b = vec![3.0; MAX_VECTOR_LENGTH];
    assert_eq!(
        call(Builtin::VectorSub, &a, &b),
        vector(&vec![-1.0; MAX_VECTOR_LENGTH])
    );
    assert_eq!(
        call(Builtin::Hadamard, &a, &b),
        vector(&vec![6.0; MAX_VECTOR_LENGTH])
    );
}

#[test]
fn generated_integer_products_match_exact_reference() {
    let mut seed = 91u64;
    for len in 0..65 {
        let mut next = || {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            ((seed >> 32) % 201) as i64 - 100
        };
        let a: Vec<i64> = (0..len).map(|_| next()).collect();
        let b: Vec<i64> = (0..len).map(|_| next()).collect();
        let left: Vec<f64> = a.iter().map(|&v| v as f64).collect();
        let right: Vec<f64> = b.iter().map(|&v| v as f64).collect();
        let sub: Vec<f64> = a.iter().zip(&b).map(|(x, y)| (x - y) as f64).collect();
        let mul: Vec<f64> = a.iter().zip(&b).map(|(x, y)| (x * y) as f64).collect();
        assert_eq!(call(Builtin::VectorSub, &left, &right), vector(&sub));
        assert_eq!(call(Builtin::Hadamard, &left, &right), vector(&mul));
        let exact: i64 = a.iter().zip(&b).map(|(x, y)| x * y).sum();
        assert_eq!(
            call(Builtin::Dot, &left, &right).as_float().unwrap(),
            exact as f64
        );
    }
}

#[test]
fn nonfinite_results_abort_pending_output() {
    for source in [
        "print(1); print(vector_sub([1e308], [-1e308]));",
        "print(1); print(hadamard([1e308], [2.0]));",
    ] {
        let expected = scarp::check(source).unwrap_err();
        assert_eq!(expected.kind, Kind::NonFiniteNumber);
        let mut out = Vec::new();
        let RunError::Diagnostic(actual) = scarp::run(source, &mut out).unwrap_err() else {
            panic!("expected diagnostic");
        };
        assert_eq!(actual, expected);
        assert!(out.is_empty());
    }
}

#[test]
fn cumulative_payload_counts_each_new_result() {
    let source = "bind a = [1.0, 2.0]; bind b = hadamard(a, a); print(vector_sub(b, a));";
    let exact = ExecutionOptions {
        max_vector_bytes: 48,
        ..ExecutionOptions::default()
    };
    scarp::check_with_options(source, exact).unwrap();
    assert_eq!(
        scarp::check_with_options(
            source,
            ExecutionOptions {
                max_vector_bytes: 47,
                ..exact
            }
        )
        .unwrap_err()
        .kind,
        Kind::VectorBudget
    );
}
