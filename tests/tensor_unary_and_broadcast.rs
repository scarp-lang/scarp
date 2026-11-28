use scarp::{
    diagnostics::DiagnosticKind as Kind,
    numeric::tensor::{
        broadcast_to, sum_axis, sum_axis_backward, sum_to_shape, unary, unary_backward,
        TensorLimits, TensorUnary,
    },
    value::{NumericTensor, NumericVector, MAX_TENSOR_RANK, MAX_VECTOR_LENGTH},
};

const LIMITS: TensorLimits = TensorLimits {
    max_steps: 4_000_000,
    max_vector_bytes: 524_288,
};

fn t(values: &[f64], shape: &[usize]) -> NumericTensor {
    NumericTensor::new(values, shape).unwrap()
}

fn close(a: f64, b: f64) {
    assert!((a - b).abs() < 1e-7, "{a} != {b}");
}

#[test]
fn unary_shapes_and_exact_results() {
    let a = t(&[-2.0, 0.0, 3.0, -4.0], &[2, 2]);
    assert_eq!(
        unary(TensorUnary::Relu, &a, LIMITS).unwrap(),
        t(&[0.0, 0.0, 3.0, 0.0], &[2, 2])
    );
    assert_eq!(
        unary(TensorUnary::Square, &a, LIMITS).unwrap(),
        t(&[4.0, 0.0, 9.0, 16.0], &[2, 2])
    );
    assert_eq!(
        unary(TensorUnary::Negate, &a, LIMITS).unwrap(),
        t(&[2.0, -0.0, -3.0, 4.0], &[2, 2])
    );
    assert_eq!(
        unary(TensorUnary::Exp, &t(&[0.0], &[]), LIMITS).unwrap(),
        t(&[1.0], &[])
    );
}

#[test]
fn all_unary_vjps_match_finite_differences() {
    let x = [0.25, 0.5, 1.5, 2.0];
    let g = t(&[1.0, -2.0, 3.0, -1.0], &[2, 2]);
    let h = 1e-5;
    for op in [
        TensorUnary::Negate,
        TensorUnary::Square,
        TensorUnary::Exp,
        TensorUnary::Log,
        TensorUnary::Relu,
        TensorUnary::Sigmoid,
        TensorUnary::Tanh,
        TensorUnary::Softplus,
    ] {
        let gradient = unary_backward(op, &t(&x, &[2, 2]), &g, LIMITS).unwrap();
        let objective = |values: &[f64]| {
            unary(op, &t(values, &[2, 2]), LIMITS)
                .unwrap()
                .cells()
                .iter()
                .zip(g.cells())
                .map(|(a, b)| a.get() * b.get())
                .sum::<f64>()
        };
        for (i, actual) in gradient.cells().iter().enumerate() {
            let mut plus = x;
            let mut minus = x;
            plus[i] += h;
            minus[i] -= h;
            close(
                actual.get(),
                (objective(&plus) - objective(&minus)) / (2.0 * h),
            );
        }
    }
}

#[test]
fn relu_zero_and_smooth_saturated_derivative_tails() {
    let x = t(&[-40.0, -0.0, 40.0], &[3]);
    let g = t(&[1.0; 3], &[3]);
    assert_eq!(
        unary_backward(TensorUnary::Relu, &x, &g, LIMITS).unwrap(),
        t(&[0.0, 0.0, 1.0], &[3])
    );
    for op in [TensorUnary::Sigmoid, TensorUnary::Tanh] {
        let result = unary_backward(op, &x, &g, LIMITS).unwrap();
        assert!(result.cells()[0].get() > 0.0);
        assert_eq!(result.cells()[0], result.cells()[2]);
    }
}

#[test]
fn unary_domain_and_forward_overflow_are_not_hidden_by_zero_gradient() {
    let zero = t(&[0.0], &[]);
    for (op, x, kind) in [
        (TensorUnary::Log, 0.0, Kind::NumericDomain),
        (TensorUnary::Exp, 1000.0, Kind::NonFiniteNumber),
        (TensorUnary::Square, f64::MAX, Kind::NonFiniteNumber),
    ] {
        let input = t(&[x], &[]);
        assert_eq!(unary(op, &input, LIMITS).unwrap_err().kind, kind);
        assert_eq!(
            unary_backward(op, &input, &zero, LIMITS).unwrap_err().kind,
            kind
        );
    }
    let tiny = t(&[f64::from_bits(1)], &[]);
    assert_eq!(
        unary_backward(TensorUnary::Log, &tiny, &zero, LIMITS).unwrap(),
        zero
    );
}

#[test]
fn unary_exact_quotas_and_shape_mismatch() {
    let a = t(&[1.0, 2.0], &[2]);
    for (backward, work) in [(false, 16), (true, 32)] {
        let exact = TensorLimits {
            max_steps: work,
            max_vector_bytes: 16,
        };
        let run = |limits| {
            if backward {
                unary_backward(TensorUnary::Square, &a, &a, limits)
            } else {
                unary(TensorUnary::Square, &a, limits)
            }
        };
        run(exact).unwrap();
        assert_eq!(
            run(TensorLimits {
                max_steps: work - 1,
                ..exact
            })
            .unwrap_err()
            .kind,
            Kind::ExecutionLimit
        );
        assert_eq!(
            run(TensorLimits {
                max_vector_bytes: 15,
                ..exact
            })
            .unwrap_err()
            .kind,
            Kind::VectorBudget
        );
    }
    let zero = TensorLimits {
        max_steps: 0,
        max_vector_bytes: 0,
    };
    assert_eq!(
        unary_backward(TensorUnary::Square, &a, &t(&[1.0, 2.0], &[1, 2]), zero)
            .unwrap_err()
            .kind,
        Kind::VectorLength
    );
    let empty = t(&[], &[0, 3]);
    assert_eq!(unary(TensorUnary::Log, &empty, zero).unwrap(), empty);
    assert_eq!(
        unary_backward(TensorUnary::Log, &empty, &empty, zero).unwrap(),
        empty
    );
}

#[test]
fn broadcast_scalar_rows_and_columns() {
    let scalar = t(&[2.0], &[]);
    assert_eq!(
        broadcast_to(&scalar, &[2, 3], LIMITS).unwrap(),
        t(&[2.0; 6], &[2, 3])
    );
    let row = t(&[1.0, 2.0, 3.0], &[3]);
    assert_eq!(
        broadcast_to(&row, &[2, 3], LIMITS).unwrap(),
        t(&[1.0, 2.0, 3.0, 1.0, 2.0, 3.0], &[2, 3])
    );
    let col = t(&[1.0, 2.0], &[2, 1]);
    assert_eq!(
        broadcast_to(&col, &[2, 3], LIMITS).unwrap(),
        t(&[1.0, 1.0, 1.0, 2.0, 2.0, 2.0], &[2, 3])
    );
}

#[test]
fn broadcast_validates_rank_size_and_exact_target() {
    let a = t(&[1.0, 2.0], &[2]);
    let zero = TensorLimits {
        max_steps: 0,
        max_vector_bytes: 0,
    };
    for target in [
        &[][..],
        &[1],
        &[3],
        &[MAX_VECTOR_LENGTH + 1, 0],
        &[1; MAX_TENSOR_RANK + 1],
    ] {
        assert_eq!(
            broadcast_to(&a, target, zero).unwrap_err().kind,
            Kind::VectorLength
        );
    }
    let scalar = t(&[1.0], &[]);
    assert_eq!(
        broadcast_to(&scalar, &[256, 257], zero).unwrap_err().kind,
        Kind::VectorLength
    );
    assert_eq!(
        broadcast_to(&scalar, &[0, MAX_VECTOR_LENGTH], zero).unwrap(),
        t(&[], &[0, MAX_VECTOR_LENGTH])
    );
    assert_eq!(
        broadcast_to(&t(&[], &[0]), &[1], zero).unwrap_err().kind,
        Kind::VectorLength
    );
}

#[test]
fn broadcast_and_sum_to_shape_are_adjoint() {
    let a = t(&[1.0, 2.0, 3.0], &[3]);
    let g = t(&[4.0, 5.0, 6.0, 7.0, 8.0, 9.0], &[2, 3]);
    let expanded = broadcast_to(&a, g.shape(), LIMITS).unwrap();
    let reduced = sum_to_shape(&g, a.shape(), LIMITS).unwrap();
    let dot = |a: &NumericTensor, b: &NumericTensor| {
        a.cells()
            .iter()
            .zip(b.cells())
            .map(|(x, y)| x.get() * y.get())
            .sum::<f64>()
    };
    assert_eq!(dot(&expanded, &g), dot(&a, &reduced));
    assert_eq!(dot(&expanded, &g), 82.0);
}

#[test]
fn reduction_backward_restores_dropped_and_retained_axes() {
    let original = [2, 3];
    let g = t(&[2.0, 4.0], &[2]);
    let expected = t(&[2.0, 2.0, 2.0, 4.0, 4.0, 4.0], &original);
    assert_eq!(
        sum_axis_backward(&original, 1, false, &g, LIMITS).unwrap(),
        expected
    );
    assert_eq!(
        sum_axis_backward(&original, 1, true, &g.reshape(&[2, 1]).unwrap(), LIMITS).unwrap(),
        expected
    );
    let g = t(&[1.0, 2.0, 3.0], &[3]);
    assert_eq!(
        sum_axis_backward(&original, 0, false, &g, LIMITS).unwrap(),
        t(&[1.0, 2.0, 3.0, 1.0, 2.0, 3.0], &original)
    );
    let scalar = t(&[5.0], &[]);
    assert_eq!(
        sum_axis_backward(&[3], 0, false, &scalar, LIMITS).unwrap(),
        t(&[5.0; 3], &[3])
    );
}

#[test]
fn reduction_backward_checks_exact_shapes_even_when_empty() {
    let zero = TensorLimits {
        max_steps: 0,
        max_vector_bytes: 0,
    };
    let g = t(&[1.0; 3], &[3]);
    assert_eq!(
        sum_axis_backward(&[0, 3], 0, false, &g, zero).unwrap(),
        t(&[], &[0, 3])
    );
    assert_eq!(
        sum_axis_backward(&[0, 3], 1, false, &t(&[], &[0]), zero).unwrap(),
        t(&[], &[0, 3])
    );
    assert_eq!(
        sum_axis_backward(&[0, 3], 0, true, &g, zero)
            .unwrap_err()
            .kind,
        Kind::VectorLength
    );
    assert_eq!(
        sum_axis_backward(&[0, 3], 2, false, &g, zero)
            .unwrap_err()
            .kind,
        Kind::VectorLength
    );
    assert_eq!(
        sum_axis_backward(&[MAX_VECTOR_LENGTH + 1, 0], 0, false, &t(&[], &[0]), zero)
            .unwrap_err()
            .kind,
        Kind::VectorLength
    );
}

#[test]
fn reduction_adjoint_identity_across_all_small_shapes_and_axes() {
    for x in 0..4 {
        for y in 0..4 {
            for z in 0..4 {
                let shape = [x, y, z];
                let a = t(
                    &(0..x * y * z)
                        .map(|i| (i % 7) as f64 - 3.0)
                        .collect::<Vec<_>>(),
                    &shape,
                );
                for axis in 0..3 {
                    for keep in [false, true] {
                        let reduced = sum_axis(&a, axis, keep, LIMITS).unwrap();
                        let g = t(
                            &(0..reduced.len())
                                .map(|i| (i % 5) as f64 - 2.0)
                                .collect::<Vec<_>>(),
                            reduced.shape(),
                        );
                        let expanded = sum_axis_backward(&shape, axis, keep, &g, LIMITS).unwrap();
                        let dot = |a: &NumericTensor, b: &NumericTensor| {
                            a.cells()
                                .iter()
                                .zip(b.cells())
                                .map(|(x, y)| x.get() * y.get())
                                .sum::<f64>()
                        };
                        assert_eq!(dot(&reduced, &g), dot(&a, &expanded));
                    }
                }
            }
        }
    }
}

#[test]
fn windows_and_maximum_size_preserve_bounds() {
    let storage = NumericVector::new(&[99.0, 1.0, 2.0, 99.0]).unwrap();
    let input = NumericTensor::from_vector(storage.window(1, 2).unwrap(), &[2]).unwrap();
    assert_eq!(
        broadcast_to(&input, &[2, 2], LIMITS).unwrap(),
        t(&[1.0, 2.0, 1.0, 2.0], &[2, 2])
    );
    let maximum = broadcast_to(&t(&[1.0], &[]), &[256, 256], LIMITS).unwrap();
    assert_eq!(maximum.len(), MAX_VECTOR_LENGTH);
    assert_eq!(
        unary(TensorUnary::Square, &maximum, LIMITS).unwrap(),
        maximum
    );
}
