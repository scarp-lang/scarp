use scarp::{
    diagnostics::DiagnosticKind as Kind,
    numeric::tensor::{binary, sum_axis, sum_to_shape, TensorBinary, TensorLimits},
    value::{NumericTensor, NumericVector, MAX_TENSOR_RANK, MAX_VECTOR_LENGTH},
};

const LIMITS: TensorLimits = TensorLimits {
    max_steps: 4_000_000,
    max_vector_bytes: 524_288,
};

fn tensor(values: &[f64], shape: &[usize]) -> NumericTensor {
    NumericTensor::new(values, shape).unwrap()
}

#[test]
fn sum_axis_preserves_or_removes_dimensions() {
    let input = tensor(&[1.0, 2.0, 3.0, 4.0, 5.0, 6.0], &[2, 3]);
    assert_eq!(
        sum_axis(&input, 0, false, LIMITS).unwrap(),
        tensor(&[5.0, 7.0, 9.0], &[3])
    );
    assert_eq!(
        sum_axis(&input, 0, true, LIMITS).unwrap(),
        tensor(&[5.0, 7.0, 9.0], &[1, 3])
    );
    assert_eq!(
        sum_axis(&input, 1, false, LIMITS).unwrap(),
        tensor(&[6.0, 15.0], &[2])
    );
    assert_eq!(
        sum_axis(&input, 1, true, LIMITS).unwrap(),
        tensor(&[6.0, 15.0], &[2, 1])
    );
    assert_eq!(
        sum_to_shape(&input, &[], LIMITS).unwrap(),
        tensor(&[21.0], &[])
    );
}

#[test]
fn nonadjacent_reduced_axes_preserve_kept_coordinates() {
    let values: Vec<f64> = (1..=12).map(f64::from).collect();
    let input = tensor(&values, &[2, 3, 2]);
    assert_eq!(
        sum_to_shape(&input, &[1, 3, 1], LIMITS).unwrap(),
        tensor(&[18.0, 26.0, 34.0], &[1, 3, 1])
    );
    assert_eq!(
        sum_to_shape(&input, &[3, 1], LIMITS).unwrap(),
        tensor(&[18.0, 26.0, 34.0], &[3, 1])
    );
    assert_eq!(
        sum_to_shape(&input, &[2], LIMITS).unwrap(),
        tensor(&[36.0, 42.0], &[2])
    );
}

#[test]
fn scalar_and_singleton_reductions() {
    let scalar = tensor(&[4.0], &[]);
    assert_eq!(sum_to_shape(&scalar, &[], LIMITS).unwrap(), scalar);
    assert_eq!(
        sum_axis(&scalar, 0, false, LIMITS).unwrap_err().kind,
        Kind::VectorLength
    );
    let input = tensor(&[4.0], &[1, 1, 1]);
    assert_eq!(
        sum_axis(&input, 1, false, LIMITS).unwrap(),
        tensor(&[4.0], &[1, 1])
    );
    assert_eq!(sum_to_shape(&input, &[], LIMITS).unwrap(), scalar);
}

#[test]
fn empty_axes_produce_zeros_or_empty_outputs() {
    let input = tensor(&[], &[2, 0, 3]);
    assert_eq!(
        sum_axis(&input, 1, false, LIMITS).unwrap(),
        tensor(&[0.0; 6], &[2, 3])
    );
    assert_eq!(
        sum_axis(&input, 2, false, LIMITS).unwrap(),
        tensor(&[], &[2, 0])
    );
    assert_eq!(
        sum_to_shape(&input, &[1, 3], LIMITS).unwrap(),
        tensor(&[0.0; 3], &[1, 3])
    );
    assert_eq!(
        sum_to_shape(&input, &[], LIMITS).unwrap(),
        tensor(&[0.0], &[])
    );
    let zero = TensorLimits {
        max_steps: 0,
        max_vector_bytes: 0,
    };
    assert_eq!(
        sum_to_shape(&input, &[0, 3], zero).unwrap(),
        tensor(&[], &[0, 3])
    );
}

#[test]
fn rejecting_oversized_materialization_after_removing_zero_axis() {
    let input = tensor(&[], &[MAX_VECTOR_LENGTH, 0, MAX_VECTOR_LENGTH]);
    let zero = TensorLimits {
        max_steps: 0,
        max_vector_bytes: 0,
    };
    assert_eq!(
        sum_axis(&input, 1, false, zero).unwrap_err().kind,
        Kind::VectorLength
    );
    assert_eq!(
        sum_to_shape(&input, &[1, 0, 1], zero).unwrap(),
        tensor(&[], &[1, 0, 1])
    );
}

#[test]
fn malformed_target_shape_is_rejected_before_budget() {
    let input = tensor(&[1.0; 6], &[2, 3]);
    let zero = TensorLimits {
        max_steps: 0,
        max_vector_bytes: 0,
    };
    for target in [&[2][..], &[3, 3], &[1, 2, 3], &[0, 3], &[usize::MAX]] {
        assert_eq!(
            sum_to_shape(&input, target, zero).unwrap_err().kind,
            Kind::VectorLength
        );
    }
    assert_eq!(
        sum_axis(&input, usize::MAX, true, zero).unwrap_err().kind,
        Kind::VectorLength
    );
}

#[test]
fn exact_work_and_payload_boundaries() {
    let input = tensor(&[1.0; 6], &[2, 3]);
    // rank=2, target rank=1, reduced rank=1: 3*4 + 6*4 = 36.
    let exact = TensorLimits {
        max_steps: 36,
        max_vector_bytes: 24,
    };
    assert_eq!(
        sum_to_shape(&input, &[3], exact).unwrap(),
        tensor(&[2.0; 3], &[3])
    );
    assert_eq!(
        sum_to_shape(
            &input,
            &[3],
            TensorLimits {
                max_steps: 35,
                ..exact
            }
        )
        .unwrap_err()
        .kind,
        Kind::ExecutionLimit
    );
    assert_eq!(
        sum_to_shape(
            &input,
            &[3],
            TensorLimits {
                max_vector_bytes: 23,
                ..exact
            }
        )
        .unwrap_err()
        .kind,
        Kind::VectorBudget
    );
    let empty = tensor(&[], &[0, 3]);
    assert_eq!(
        sum_to_shape(
            &empty,
            &[3],
            TensorLimits {
                max_steps: 12,
                max_vector_bytes: 24,
            }
        )
        .unwrap(),
        tensor(&[0.0; 3], &[3])
    );
}

#[test]
fn compensated_sums_and_nonfinite_failure() {
    let input = tensor(&[1e16, 1.0, -1e16, 1e16, 3.0, -1e16], &[2, 3]);
    assert_eq!(
        sum_axis(&input, 1, false, LIMITS).unwrap(),
        tensor(&[1.0, 3.0], &[2])
    );
    let huge = tensor(&[f64::MAX; 2], &[2]);
    assert_eq!(
        sum_to_shape(&huge, &[], LIMITS).unwrap_err().kind,
        Kind::NonFiniteNumber
    );
    assert_eq!(huge, tensor(&[f64::MAX; 2], &[2]));
}

#[test]
fn maximum_rank_and_window_storage_are_supported() {
    let backing = NumericVector::new(&[99.0, 2.0, 3.0, 99.0]).unwrap();
    let mut shape = [1; MAX_TENSOR_RANK];
    shape[3] = 2;
    let input = NumericTensor::from_vector(backing.window(1, 2).unwrap(), &shape).unwrap();
    assert_eq!(
        sum_axis(&input, 3, true, LIMITS).unwrap(),
        tensor(&[5.0], &[1; MAX_TENSOR_RANK])
    );
    assert_eq!(
        sum_to_shape(&input, &[], LIMITS).unwrap(),
        tensor(&[5.0], &[])
    );
}

#[test]
fn maximum_cell_count_reduces_without_hidden_intermediates() {
    let input = tensor(&vec![1.0; MAX_VECTOR_LENGTH], &[256, 256]);
    assert_eq!(
        sum_to_shape(&input, &[256], LIMITS).unwrap(),
        tensor(&[256.0; 256], &[256])
    );
    assert_eq!(
        sum_to_shape(&input, &[], LIMITS).unwrap(),
        tensor(&[MAX_VECTOR_LENGTH as f64], &[])
    );
}

#[test]
fn generated_reductions_match_independent_index_reference() {
    for x in 0..5 {
        for y in 0..5 {
            for z in 0..5 {
                let cells: Vec<f64> =
                    (0..x * y * z).map(|i| (i % 17) as f64 - 8.0).collect();
                let input = tensor(&cells, &[x, y, z]);
                let mut middle = vec![0.0; y];
                let mut last = vec![0.0; z];
                let mut first = vec![0.0; x];
                for i in 0..x {
                    for j in 0..y {
                        for k in 0..z {
                            let value = cells[(i * y + j) * z + k];
                            first[i] += value;
                            middle[j] += value;
                            last[k] += value;
                        }
                    }
                }
                assert_eq!(
                    sum_to_shape(&input, &[x, 1, 1], LIMITS).unwrap(),
                    tensor(&first, &[x, 1, 1])
                );
                assert_eq!(
                    sum_to_shape(&input, &[y, 1], LIMITS).unwrap(),
                    tensor(&middle, &[y, 1])
                );
                assert_eq!(
                    sum_to_shape(&input, &[z], LIMITS).unwrap(),
                    tensor(&last, &[z])
                );
            }
        }
    }
}

#[test]
fn broadcast_and_reduction_satisfy_adjoint_identity() {
    let a = tensor(&[1.0, 2.0, 3.0], &[3]);
    let b = tensor(&[4.0, 5.0, 6.0, 7.0, 8.0, 9.0], &[2, 3]);
    let broadcasted = binary(TensorBinary::Multiply, &a, &b, LIMITS).unwrap();
    let left = sum_to_shape(&broadcasted, &[], LIMITS).unwrap();
    let reduced = sum_to_shape(&b, &[3], LIMITS).unwrap();
    let products = binary(TensorBinary::Multiply, &a, &reduced, LIMITS).unwrap();
    let right = sum_to_shape(&products, &[], LIMITS).unwrap();
    assert_eq!(left, right);
    assert_eq!(left, tensor(&[82.0], &[]));
}
