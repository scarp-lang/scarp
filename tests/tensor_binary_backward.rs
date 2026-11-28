use scarp::{
    diagnostics::DiagnosticKind as Kind,
    numeric::tensor::{binary, binary_backward, TensorBinary, TensorLimits},
    value::{NumericTensor, NumericVector, MAX_VECTOR_LENGTH},
};

const LIMITS: TensorLimits = TensorLimits {
    max_steps: 10_000_000,
    max_vector_bytes: 1_048_576,
};

fn t(values: &[f64], shape: &[usize]) -> NumericTensor {
    NumericTensor::new(values, shape).unwrap()
}

fn values(tensor: &NumericTensor) -> Vec<f64> {
    tensor.cells().iter().map(|v| v.get()).collect()
}

#[test]
fn add_and_subtract_reduce_both_broadcast_axes() {
    let a = t(&[2.0, 3.0], &[2, 1]);
    let b = t(&[4.0, 5.0, 6.0], &[3]);
    let g = t(&[1.0, 2.0, 3.0, 4.0, 5.0, 6.0], &[2, 3]);
    let (da, db) = binary_backward(TensorBinary::Add, &a, &b, &g, LIMITS).unwrap();
    assert_eq!(da, t(&[6.0, 15.0], &[2, 1]));
    assert_eq!(db, t(&[5.0, 7.0, 9.0], &[3]));
    let (da, db) = binary_backward(TensorBinary::Subtract, &a, &b, &g, LIMITS).unwrap();
    assert_eq!(da, t(&[6.0, 15.0], &[2, 1]));
    assert_eq!(db, t(&[-5.0, -7.0, -9.0], &[3]));
}

#[test]
fn multiplication_reduces_original_shapes() {
    let a = t(&[2.0, 3.0], &[2, 1]);
    let b = t(&[4.0, 5.0, 6.0], &[3]);
    let g = t(&[1.0, 2.0, 3.0, 4.0, 5.0, 6.0], &[2, 3]);
    let (da, db) = binary_backward(TensorBinary::Multiply, &a, &b, &g, LIMITS).unwrap();
    assert_eq!(da, t(&[32.0, 77.0], &[2, 1]));
    assert_eq!(db, t(&[14.0, 19.0, 24.0], &[3]));
}

#[test]
fn division_reduces_scalar_denominator_gradient() {
    let a = t(&[2.0, 4.0], &[2]);
    let b = t(&[2.0], &[]);
    let g = t(&[1.0, 2.0], &[2]);
    let (da, db) = binary_backward(TensorBinary::Divide, &a, &b, &g, LIMITS).unwrap();
    assert_eq!(da, t(&[0.5, 1.0], &[2]));
    assert_eq!(db, t(&[-2.5], &[]));
}

#[test]
fn rank_zero_operations_have_scalar_gradients() {
    let a = t(&[4.0], &[]);
    let b = t(&[2.0], &[]);
    let g = t(&[3.0], &[]);
    for (op, da, db) in [
        (TensorBinary::Add, 3.0, 3.0),
        (TensorBinary::Subtract, 3.0, -3.0),
        (TensorBinary::Multiply, 6.0, 12.0),
        (TensorBinary::Divide, 1.5, -3.0),
    ] {
        assert_eq!(
            binary_backward(op, &a, &b, &g, LIMITS).unwrap(),
            (t(&[da], &[]), t(&[db], &[]))
        );
    }
}

#[test]
fn empty_broadcast_reduces_to_explicit_zeros() {
    let a = t(&[], &[0, 3]);
    let b = t(&[1.0; 3], &[3]);
    let g = t(&[], &[0, 3]);
    for op in [
        TensorBinary::Add,
        TensorBinary::Subtract,
        TensorBinary::Multiply,
        TensorBinary::Divide,
    ] {
        let (da, db) = binary_backward(op, &a, &b, &g, LIMITS).unwrap();
        assert_eq!(da, a);
        assert_eq!(db, t(&[0.0; 3], &[3]));
    }
    let scalar_zero = t(&[0.0], &[]);
    let (da, db) = binary_backward(TensorBinary::Divide, &a, &scalar_zero, &g, LIMITS).unwrap();
    assert_eq!(da, a);
    assert_eq!(db, scalar_zero);
}

#[test]
fn shape_validation_precedes_allocation_and_work() {
    let a = t(&[1.0; 2], &[2, 1]);
    let b = t(&[1.0; 3], &[3]);
    let zero = TensorLimits {
        max_steps: 0,
        max_vector_bytes: 0,
    };
    let bad = t(&[1.0; 6], &[6]);
    assert_eq!(
        binary_backward(TensorBinary::Add, &a, &b, &bad, zero)
            .unwrap_err()
            .kind,
        Kind::VectorLength
    );
    let a = t(&[1.0; 2], &[2]);
    assert_eq!(
        binary_backward(TensorBinary::Add, &a, &b, &bad, zero)
            .unwrap_err()
            .kind,
        Kind::VectorLength
    );
    let a = t(&[1.0; 256], &[256, 1]);
    let b = t(&[1.0; 257], &[257]);
    assert_eq!(
        binary_backward(TensorBinary::Add, &a, &b, &bad, zero)
            .unwrap_err()
            .kind,
        Kind::VectorLength
    );
}

#[test]
fn both_output_buffers_use_one_exact_budget() {
    let a = t(&[2.0, 3.0], &[2]);
    let b = t(&[4.0], &[]);
    let g = t(&[1.0, 2.0], &[2]);
    // A=2, rA=1, B=1, rB=0, C=2, R=1: 45 work and 24 bytes.
    let exact = TensorLimits {
        max_steps: 45,
        max_vector_bytes: 24,
    };
    binary_backward(TensorBinary::Multiply, &a, &b, &g, exact).unwrap();
    assert_eq!(
        binary_backward(
            TensorBinary::Multiply,
            &a,
            &b,
            &g,
            TensorLimits {
                max_steps: 44,
                ..exact
            },
        )
        .unwrap_err()
        .kind,
        Kind::ExecutionLimit
    );
    assert_eq!(
        binary_backward(
            TensorBinary::Multiply,
            &a,
            &b,
            &g,
            TensorLimits {
                max_vector_bytes: 23,
                ..exact
            },
        )
        .unwrap_err()
        .kind,
        Kind::VectorBudget
    );
}

#[test]
fn signed_zero_division_and_nonfinite_contributions_are_atomic() {
    let a = t(&[1.0], &[]);
    let g = t(&[2.0], &[]);
    for zero in [0.0, -0.0] {
        assert_eq!(
            binary_backward(TensorBinary::Divide, &a, &t(&[zero], &[]), &g, LIMITS)
                .unwrap_err()
                .kind,
            Kind::NumericDomain
        );
    }
    let huge = t(&[f64::MAX], &[]);
    assert_eq!(
        binary_backward(TensorBinary::Multiply, &a, &huge, &g, LIMITS)
            .unwrap_err()
            .kind,
        Kind::NonFiniteNumber
    );
    assert_eq!(
        binary_backward(TensorBinary::Multiply, &huge, &a, &g, LIMITS)
            .unwrap_err()
            .kind,
        Kind::NonFiniteNumber
    );
    assert_eq!(huge, t(&[f64::MAX], &[]));
}

#[test]
fn compensated_broadcast_gradients_retain_small_terms() {
    let a = t(&[1.0], &[]);
    let b = t(&[1.0; 3], &[3]);
    let g = t(&[1e16, 1.0, -1e16], &[3]);
    let (da, db) = binary_backward(TensorBinary::Add, &a, &b, &g, LIMITS).unwrap();
    assert_eq!(da, t(&[1.0], &[]));
    assert_eq!(db, g);
}

#[test]
fn windows_and_maximum_count_are_supported() {
    let storage = NumericVector::new(&[99.0, 2.0, 3.0, 99.0]).unwrap();
    let a = NumericTensor::from_vector(storage.window(1, 2).unwrap(), &[2]).unwrap();
    let b = t(&[2.0], &[]);
    let g = t(&[1.0, 1.0], &[2]);
    assert_eq!(
        binary_backward(TensorBinary::Multiply, &a, &b, &g, LIMITS).unwrap(),
        (t(&[2.0, 2.0], &[2]), t(&[5.0], &[]))
    );
    let a = t(&vec![1.0; MAX_VECTOR_LENGTH], &[MAX_VECTOR_LENGTH]);
    let g = a.clone();
    let (da, db) = binary_backward(TensorBinary::Add, &a, &b, &g, LIMITS).unwrap();
    assert_eq!(da, a);
    assert_eq!(db, t(&[MAX_VECTOR_LENGTH as f64], &[]));
}

#[test]
fn finite_differences_match_all_broadcast_operations() {
    let a = t(&[0.25, -1.0], &[2, 1]);
    let b = t(&[0.5, 2.0, -0.75], &[3]);
    let g = t(&[1.0, -2.0, 3.0, -1.0, 0.5, 2.0], &[2, 3]);
    for op in [
        TensorBinary::Add,
        TensorBinary::Subtract,
        TensorBinary::Multiply,
        TensorBinary::Divide,
    ] {
        let (da, db) = binary_backward(op, &a, &b, &g, LIMITS).unwrap();
        let objective = |a: &NumericTensor, b: &NumericTensor| {
            binary(op, a, b, LIMITS)
                .unwrap()
                .cells()
                .iter()
                .zip(g.cells())
                .map(|(x, y)| x.get() * y.get())
                .sum::<f64>()
        };
        let h = 1e-5;
        for (operand, actual, left) in [(&a, da, true), (&b, db, false)] {
            for (i, derivative) in values(&actual).into_iter().enumerate() {
                let mut plus = values(operand);
                let mut minus = plus.clone();
                plus[i] += h;
                minus[i] -= h;
                let plus = t(&plus, operand.shape());
                let minus = t(&minus, operand.shape());
                let (hi, lo) = if left {
                    (objective(&plus, &b), objective(&minus, &b))
                } else {
                    (objective(&a, &plus), objective(&a, &minus))
                };
                assert!((derivative - (hi - lo) / (2.0 * h)).abs() < 1e-7);
            }
        }
    }
}

#[test]
fn generated_rank_three_products_match_scatter_reference() {
    for x in 0..4 {
        for y in 0..4 {
            for z in 0..4 {
                let a: Vec<f64> = (0..x * z).map(|i| i as f64 - 3.0).collect();
                let b: Vec<f64> = (0..y).map(|i| i as f64 + 1.0).collect();
                let g: Vec<f64> = (0..x * y * z).map(|i| (i % 7) as f64 - 3.0).collect();
                let mut da = vec![0.0; a.len()];
                let mut db = vec![0.0; b.len()];
                for i in 0..x {
                    for j in 0..y {
                        for k in 0..z {
                            let ai = i * z + k;
                            let gi = (i * y + j) * z + k;
                            da[ai] += b[j] * g[gi];
                            db[j] += a[ai] * g[gi];
                        }
                    }
                }
                let a = t(&a, &[x, 1, z]);
                let b = t(&b, &[y, 1]);
                let g = t(&g, &[x, y, z]);
                let (actual_a, actual_b) =
                    binary_backward(TensorBinary::Multiply, &a, &b, &g, LIMITS).unwrap();
                assert_eq!(actual_a, t(&da, a.shape()));
                assert_eq!(actual_b, t(&db, b.shape()));
            }
        }
    }
}
