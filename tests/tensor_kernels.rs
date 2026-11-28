use scarp::{
    diagnostics::DiagnosticKind as Kind,
    numeric::tensor::{binary, matmul, TensorBinary, TensorLimits},
    value::NumericTensor,
};

const LIMITS: TensorLimits = TensorLimits {
    max_steps: 1_000_000,
    max_vector_bytes: 524_288,
};

fn t(values: &[f64], shape: &[usize]) -> NumericTensor {
    NumericTensor::new(values, shape).unwrap()
}

fn values(tensor: &NumericTensor) -> Vec<f64> {
    tensor.cells().iter().map(|v| v.get()).collect()
}

#[test]
fn broadcast_rows_columns_and_scalars() {
    let a = t(&[1.0, 2.0, 3.0], &[1, 3]);
    let b = t(&[10.0, 20.0], &[2, 1]);
    let out = binary(TensorBinary::Add, &a, &b, LIMITS).unwrap();
    assert_eq!(out.shape(), &[2, 3]);
    assert_eq!(values(&out), vec![11.0, 12.0, 13.0, 21.0, 22.0, 23.0]);
    let scalar = t(&[2.0], &[]);
    let scaled = binary(TensorBinary::Multiply, &out, &scalar, LIMITS).unwrap();
    assert_eq!(values(&scaled), vec![22.0, 24.0, 26.0, 42.0, 44.0, 46.0]);
    assert_eq!(
        binary(TensorBinary::Divide, &scaled, &scalar, LIMITS).unwrap(),
        out
    );
    assert_eq!(
        binary(TensorBinary::Subtract, &out, &out, LIMITS).unwrap(),
        t(&[0.0; 6], &[2, 3])
    );
}

#[test]
fn empty_and_incompatible_broadcasts() {
    let empty = t(&[], &[0, 3]);
    let row = t(&[1.0, 2.0, 3.0], &[3]);
    let zero = TensorLimits {
        max_steps: 0,
        max_vector_bytes: 0,
    };
    assert_eq!(
        binary(TensorBinary::Add, &empty, &row, zero).unwrap(),
        empty
    );
    let wrong = t(&[1.0, 2.0], &[2, 1]);
    assert_eq!(
        binary(TensorBinary::Add, &empty, &wrong, LIMITS)
            .unwrap_err()
            .kind,
        Kind::VectorLength
    );
}

#[test]
fn binary_work_and_allocation_fail_before_output() {
    let a = t(&[1.0, 2.0], &[2]);
    let b = t(&[3.0], &[]);
    let exact = TensorLimits {
        max_steps: 6,
        max_vector_bytes: 16,
    };
    assert_eq!(
        values(&binary(TensorBinary::Add, &a, &b, exact).unwrap()),
        vec![4.0, 5.0]
    );
    assert_eq!(
        binary(
            TensorBinary::Add,
            &a,
            &b,
            TensorLimits {
                max_steps: 5,
                ..exact
            },
        )
        .unwrap_err()
        .kind,
        Kind::ExecutionLimit
    );
    assert_eq!(
        binary(
            TensorBinary::Add,
            &a,
            &b,
            TensorLimits {
                max_vector_bytes: 15,
                ..exact
            },
        )
        .unwrap_err()
        .kind,
        Kind::VectorBudget
    );
    assert!(binary(TensorBinary::Divide, &a, &t(&[-0.0], &[]), LIMITS).is_err());
    assert!(binary(
        TensorBinary::Multiply,
        &t(&[f64::MAX], &[]),
        &t(&[2.0], &[]),
        LIMITS,
    )
    .is_err());
    assert_eq!(values(&a), vec![1.0, 2.0]);
}

#[test]
fn batched_matrix_products_broadcast_leading_axes() {
    let a = t(&[1.0, 2.0, 3.0, 4.0, 2.0, 0.0, 0.0, 2.0], &[2, 2, 2]);
    let b = t(&[5.0, 6.0, 7.0, 8.0], &[2, 2]);
    let output = matmul(&a, &b, LIMITS).unwrap();
    assert_eq!(output.shape(), &[2, 2, 2]);
    assert_eq!(
        values(&output),
        vec![19.0, 22.0, 43.0, 50.0, 10.0, 12.0, 14.0, 16.0]
    );
    let left = t(&[2.0, 3.0], &[2, 1, 1, 1]);
    let right = t(&[5.0, 7.0, 11.0], &[1, 3, 1, 1]);
    let out = matmul(&left, &right, LIMITS).unwrap();
    assert_eq!(out.shape(), &[2, 3, 1, 1]);
    assert_eq!(values(&out), vec![10.0, 14.0, 22.0, 15.0, 21.0, 33.0]);
}

#[test]
fn matrix_products_handle_zero_dimensions_and_reject_bad_shapes() {
    let a = t(&[], &[2, 3, 0]);
    let b = t(&[], &[1, 0, 4]);
    let out = matmul(&a, &b, LIMITS).unwrap();
    assert_eq!(out, t(&[0.0; 24], &[2, 3, 4]));
    let zero = TensorLimits {
        max_steps: 0,
        max_vector_bytes: 0,
    };
    let a = t(&[], &[0, 2, 3]);
    let b = t(&[1.0; 6], &[3, 2]);
    assert_eq!(matmul(&a, &b, zero).unwrap(), t(&[], &[0, 2, 2]));
    assert!(matmul(&t(&[1.0], &[1]), &b, LIMITS).is_err());
    assert!(matmul(&t(&[1.0], &[1, 1]), &b, LIMITS).is_err());
    assert!(matmul(&t(&[], &[256, 0]), &t(&[], &[0, 257]), LIMITS).is_err());
}

#[test]
fn matmul_preserves_compensated_accumulation_and_budget() {
    let a = t(&[1e16, 1.0, -1e16], &[1, 3]);
    let b = t(&[1.0; 3], &[3, 1]);
    let limits = TensorLimits {
        max_steps: 7,
        max_vector_bytes: 8,
    };
    assert_eq!(matmul(&a, &b, limits).unwrap(), t(&[1.0], &[1, 1]));
    assert_eq!(
        matmul(
            &a,
            &b,
            TensorLimits {
                max_steps: 6,
                ..limits
            },
        )
        .unwrap_err()
        .kind,
        Kind::ExecutionLimit
    );
    assert_eq!(
        matmul(
            &a,
            &b,
            TensorLimits {
                max_vector_bytes: 7,
                ..limits
            },
        )
        .unwrap_err()
        .kind,
        Kind::VectorBudget
    );
}

#[test]
fn small_rectangular_batched_products_match_integer_reference() {
    for m in 0..4 {
        for k in 0..4 {
            for n in 0..4 {
                let a: Vec<f64> = (0..2 * m * k).map(|i| (i % 7) as f64 - 3.0).collect();
                let b: Vec<f64> = (0..k * n).map(|i| (i % 5) as f64 - 2.0).collect();
                let mut expected = vec![0.0; 2 * m * n];
                for batch in 0..2 {
                    for row in 0..m {
                        for col in 0..n {
                            for h in 0..k {
                                expected[(batch * m + row) * n + col] +=
                                    a[(batch * m + row) * k + h] * b[h * n + col];
                            }
                        }
                    }
                }
                assert_eq!(
                    matmul(&t(&a, &[2, m, k]), &t(&b, &[k, n]), LIMITS).unwrap(),
                    t(&expected, &[2, m, n])
                );
            }
        }
    }
}
