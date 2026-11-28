use scarp::{
    diagnostics::DiagnosticKind as Kind,
    numeric::tensor::{matmul, matmul_backward, TensorLimits},
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
fn rank_two_matrix_gradients_match_known_products() {
    let a = t(&[1.0, 2.0, 3.0, 4.0], &[2, 2]);
    let b = t(&[5.0, 6.0, 7.0, 8.0], &[2, 2]);
    let g = t(&[1.0, 2.0, 3.0, 4.0], &[2, 2]);
    let (da, db) = matmul_backward(&a, &b, &g, LIMITS).unwrap();
    assert_eq!(da, t(&[17.0, 23.0, 39.0, 53.0], &[2, 2]));
    assert_eq!(db, t(&[10.0, 14.0, 14.0, 20.0], &[2, 2]));
}

#[test]
fn both_operands_broadcast_over_distinct_axes() {
    let a = t(&[2.0, 3.0], &[2, 1, 1, 1]);
    let b = t(&[5.0, 7.0, 11.0], &[1, 3, 1, 1]);
    let g = t(&[1.0; 6], &[2, 3, 1, 1]);
    let (da, db) = matmul_backward(&a, &b, &g, LIMITS).unwrap();
    assert_eq!(da, t(&[23.0; 2], &[2, 1, 1, 1]));
    assert_eq!(db, t(&[5.0; 3], &[1, 3, 1, 1]));
}

#[test]
fn missing_batch_axes_accumulate_into_original_rank() {
    let a = t(&[2.0, 3.0], &[2, 1, 1]);
    let b = t(&[4.0], &[1, 1]);
    let g = t(&[5.0, 7.0], &[2, 1, 1]);
    let (da, db) = matmul_backward(&a, &b, &g, LIMITS).unwrap();
    assert_eq!(da, t(&[20.0, 28.0], &[2, 1, 1]));
    assert_eq!(db, t(&[31.0], &[1, 1]));
    let (db, da) = matmul_backward(&b, &a, &g, LIMITS).unwrap();
    assert_eq!(da, t(&[20.0, 28.0], &[2, 1, 1]));
    assert_eq!(db, t(&[31.0], &[1, 1]));
}

#[test]
fn upstream_must_have_exact_shape_and_forward_must_fit() {
    let a = t(&[1.0; 4], &[2, 2]);
    let b = t(&[1.0; 4], &[2, 2]);
    let zero = TensorLimits {
        max_steps: 0,
        max_vector_bytes: 0,
    };
    for g in [t(&[1.0; 4], &[4]), t(&[1.0; 2], &[1, 2])] {
        assert_eq!(
            matmul_backward(&a, &b, &g, zero).unwrap_err().kind,
            Kind::VectorLength
        );
    }
    let a = t(&[], &[256, 0]);
    let b = t(&[], &[0, 257]);
    assert_eq!(
        matmul_backward(&a, &b, &t(&[], &[0]), zero)
            .unwrap_err()
            .kind,
        Kind::VectorLength
    );
    assert_eq!(
        matmul_backward(&t(&[1.0], &[1]), &b, &t(&[], &[0]), zero)
            .unwrap_err()
            .kind,
        Kind::VectorLength
    );
}

#[test]
fn empty_forward_output_materializes_only_original_gradient_cells() {
    let a = t(&[1.0; 6], &[2, 3]);
    let b = t(&[], &[3, 0]);
    let g = t(&[], &[2, 0]);
    let (da, db) = matmul_backward(&a, &b, &g, LIMITS).unwrap();
    assert_eq!(da, t(&[0.0; 6], &[2, 3]));
    assert_eq!(db, b);

    let a = t(&[], &[0, 2, 3]);
    let b = t(&[1.0; 6], &[3, 2]);
    let g = t(&[], &[0, 2, 2]);
    let (da, db) = matmul_backward(&a, &b, &g, LIMITS).unwrap();
    assert_eq!(da, a);
    assert_eq!(db, t(&[0.0; 6], &[3, 2]));
}

#[test]
fn zero_inner_dimension_and_huge_empty_metadata_are_safe() {
    let a = t(&[], &[2, 0]);
    let b = t(&[], &[0, 3]);
    let g = t(&[1.0; 6], &[2, 3]);
    let zero = TensorLimits {
        max_steps: 0,
        max_vector_bytes: 0,
    };
    assert_eq!(
        matmul_backward(&a, &b, &g, zero).unwrap(),
        (a, b)
    );
    let a = t(&[], &[0, MAX_VECTOR_LENGTH, MAX_VECTOR_LENGTH]);
    let b = t(&[], &[0, MAX_VECTOR_LENGTH, MAX_VECTOR_LENGTH]);
    let g = t(&[], &[0, MAX_VECTOR_LENGTH, MAX_VECTOR_LENGTH]);
    assert_eq!(
        matmul_backward(&a, &b, &g, zero).unwrap(),
        (a, b)
    );
}

#[test]
fn aggregate_work_and_payload_boundaries_cover_both_gradients() {
    let a = t(&[2.0, 3.0], &[1, 2]);
    let b = t(&[4.0, 5.0], &[2, 1]);
    let g = t(&[1.0], &[1, 1]);
    // Four input cells at three units each; two terms per gradient
    // at four units each. Combined payload is four f64 cells.
    let exact = TensorLimits {
        max_steps: 28,
        max_vector_bytes: 32,
    };
    let before = (a.clone(), b.clone(), g.clone());
    assert_eq!(
        matmul_backward(&a, &b, &g, exact).unwrap(),
        (t(&[4.0, 5.0], &[1, 2]), t(&[2.0, 3.0], &[2, 1]))
    );
    assert_eq!(
        matmul_backward(
            &a,
            &b,
            &g,
            TensorLimits {
                max_steps: 27,
                ..exact
            }
        )
        .unwrap_err()
        .kind,
        Kind::ExecutionLimit
    );
    assert_eq!(
        matmul_backward(
            &a,
            &b,
            &g,
            TensorLimits {
                max_vector_bytes: 31,
                ..exact
            }
        )
        .unwrap_err()
        .kind,
        Kind::VectorBudget
    );
    assert_eq!((a, b, g), before);
}

#[test]
fn compensated_batch_reduction_preserves_small_cancellation_terms() {
    let a = t(&[1.0], &[1, 1]);
    let b = t(&[1e16, 1.0, -1e16], &[3, 1, 1]);
    let g = t(&[1.0; 3], &[3, 1, 1]);
    let (da, db) = matmul_backward(&a, &b, &g, LIMITS).unwrap();
    assert_eq!(da, t(&[1.0], &[1, 1]));
    assert_eq!(db, t(&[1.0; 3], &[3, 1, 1]));
}

#[test]
fn nonfinite_product_or_sum_rejects_whole_result() {
    let a = t(&[1.0], &[1, 1]);
    let b = t(&[f64::MAX], &[1, 1]);
    let g = t(&[2.0], &[1, 1]);
    assert_eq!(
        matmul_backward(&a, &b, &g, LIMITS).unwrap_err().kind,
        Kind::NonFiniteNumber
    );
    // Left gradient is finite; right gradient overflows.
    assert_eq!(
        matmul_backward(&b, &a, &g, LIMITS).unwrap_err().kind,
        Kind::NonFiniteNumber
    );
    let b = t(&[f64::MAX; 2], &[1, 2]);
    let g = t(&[1.0; 2], &[1, 2]);
    assert_eq!(
        matmul_backward(&a, &b, &g, LIMITS).unwrap_err().kind,
        Kind::NonFiniteNumber
    );
}

#[test]
fn window_backing_does_not_leak_hidden_cells() {
    let storage = NumericVector::new(&[99.0, 2.0, 3.0, 99.0]).unwrap();
    let a = NumericTensor::from_vector(storage.window(1, 2).unwrap(), &[1, 2]).unwrap();
    let b = t(&[4.0, 5.0], &[2, 1]);
    let g = t(&[2.0], &[1, 1]);
    let (da, db) = matmul_backward(&a, &b, &g, LIMITS).unwrap();
    assert_eq!(da, t(&[8.0, 10.0], &[1, 2]));
    assert_eq!(db, t(&[4.0, 6.0], &[2, 1]));
}

#[test]
fn generated_rectangular_gradients_match_forward_scatter_reference() {
    for m in 0..4 {
        for k in 0..4 {
            for n in 0..4 {
                let a: Vec<f64> = (0..2 * m * k).map(|i| (i % 7) as f64 - 3.0).collect();
                let b: Vec<f64> = (0..3 * k * n).map(|i| (i % 5) as f64 - 2.0).collect();
                let g: Vec<f64> = (0..6 * m * n).map(|i| (i % 11) as f64 - 5.0).collect();
                let mut da = vec![0.0; a.len()];
                let mut db = vec![0.0; b.len()];
                for x in 0..2 {
                    for y in 0..3 {
                        for row in 0..m {
                            for col in 0..n {
                                for h in 0..k {
                                    let ai = (x * m + row) * k + h;
                                    let bi = (y * k + h) * n + col;
                                    let gi = ((x * 3 + y) * m + row) * n + col;
                                    da[ai] += g[gi] * b[bi];
                                    db[bi] += g[gi] * a[ai];
                                }
                            }
                        }
                    }
                }
                let a = t(&a, &[2, 1, m, k]);
                let b = t(&b, &[1, 3, k, n]);
                let g = t(&g, &[2, 3, m, n]);
                let (actual_a, actual_b) = matmul_backward(&a, &b, &g, LIMITS).unwrap();
                assert_eq!(actual_a, t(&da, a.shape()));
                assert_eq!(actual_b, t(&db, b.shape()));
            }
        }
    }
}

#[test]
fn finite_differences_verify_both_broadcasted_operand_gradients() {
    let a = t(&[0.25, -1.0, 2.0, 0.5], &[2, 1, 1, 2]);
    let b = t(&[0.5, 2.0, -0.75, 3.0, 1.0, -2.0], &[1, 3, 2, 1]);
    let g = t(&[1.0, -2.0, 3.0, -1.0, 0.5, 2.0], &[2, 3, 1, 1]);
    let (da, db) = matmul_backward(&a, &b, &g, LIMITS).unwrap();
    let objective = |a: &NumericTensor, b: &NumericTensor| {
        let output = matmul(a, b, LIMITS).unwrap();
        output
            .cells()
            .iter()
            .zip(g.cells())
            .map(|(x, y)| x.get() * y.get())
            .sum::<f64>()
    };
    let step = 1e-5;
    for (operand, actual, left) in [(&a, da, true), (&b, db, false)] {
        for (i, derivative) in values(&actual).into_iter().enumerate() {
            let mut plus = values(operand);
            let mut minus = plus.clone();
            plus[i] += step;
            minus[i] -= step;
            let plus = t(&plus, operand.shape());
            let minus = t(&minus, operand.shape());
            let (hi, lo) = if left {
                (objective(&plus, &b), objective(&minus, &b))
            } else {
                (objective(&a, &plus), objective(&a, &minus))
            };
            let expected = (hi - lo) / (2.0 * step);
            assert!((derivative - expected).abs() < 1e-8);
        }
    }
}
