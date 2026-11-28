use scarp::{
    numeric::{self, Builtin},
    value::{NumericTensor, NumericTile, NumericVector, Value, MAX_TENSOR_RANK, MAX_VECTOR_LENGTH},
};

fn values(tensor: &NumericTensor) -> Vec<f64> {
    tensor.cells().iter().map(|v| v.get()).collect()
}

#[test]
fn scalar_vector_and_three_axis_coordinates() {
    let scalar = NumericTensor::new(&[3.0], &[]).unwrap();
    assert_eq!(scalar.rank(), 0);
    assert_eq!(scalar.offset(&[]), Some(0));
    assert_eq!(scalar.get(&[]).unwrap().get(), 3.0);
    assert!(scalar.get(&[0]).is_none());
    assert!(scalar.slice_first(0).is_none());
    let tensor = NumericTensor::new(
        &[0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0],
        &[2, 2, 2],
    )
    .unwrap();
    assert_eq!(tensor.shape(), &[2, 2, 2]);
    assert_eq!(tensor.offset(&[1, 0, 1]), Some(5));
    assert_eq!(tensor.get(&[1, 1, 0]).unwrap().get(), 6.0);
    assert!(tensor.get(&[1, 2, 0]).is_none());
    assert!(tensor.get(&[1, 0]).is_none());
    assert!(tensor.get(&[1, 0, 0, 0]).is_none());
}

#[test]
fn reject_invalid_rank_shape_and_nonfinite_cells() {
    assert!(NumericTensor::new(&[], &[]).is_none());
    assert!(NumericTensor::new(&[0.0], &[2]).is_none());
    assert!(NumericTensor::new(&[], &[1; MAX_TENSOR_RANK + 1]).is_none());
    assert!(NumericTensor::new(&[], &[0, MAX_VECTOR_LENGTH + 1]).is_none());
    assert!(NumericTensor::new(&[], &[MAX_VECTOR_LENGTH, 2]).is_none());
    assert!(NumericTensor::new(&[], &[usize::MAX, 0]).is_none());
    assert!(NumericTensor::new(&[f64::NAN], &[1]).is_none());
    assert!(NumericTensor::new(&[f64::INFINITY], &[1]).is_none());
    assert!(NumericTensor::new(&[f64::NEG_INFINITY], &[1]).is_none());
    let max = NumericTensor::new(&vec![1.0; MAX_VECTOR_LENGTH], &[256, 256]).unwrap();
    assert_eq!(max.len(), MAX_VECTOR_LENGTH);
    let high_rank = NumericTensor::new(&[1.0], &[1; MAX_TENSOR_RANK]).unwrap();
    assert_eq!(high_rank.get(&[0; MAX_TENSOR_RANK]).unwrap().get(), 1.0);
}

#[test]
fn empty_axes_keep_shape_and_do_not_require_large_products() {
    let shape = [MAX_VECTOR_LENGTH, MAX_VECTOR_LENGTH, 0];
    let tensor = NumericTensor::new(&[], &shape).unwrap();
    assert!(tensor.is_empty());
    assert_eq!(tensor.shape(), &shape);
    assert!(tensor.get(&[0, 0, 0]).is_none());
    assert_eq!(tensor.slice_first(0).unwrap().shape(), &[MAX_VECTOR_LENGTH, 0]);
    assert_eq!(tensor.permute(&[2, 0, 1]).unwrap().shape(), &[0, MAX_VECTOR_LENGTH, MAX_VECTOR_LENGTH]);
    assert_ne!(tensor, tensor.reshape(&[0]).unwrap());
    assert!(tensor.reshape(&[]).is_none());
}

#[test]
fn reshape_and_slices_share_existing_immutable_storage() {
    let vector = NumericVector::new(&[9.0, 0.0, 1.0, 2.0, 3.0, 8.0]).unwrap();
    let window = vector.window(1, 4).unwrap();
    let tensor = NumericTensor::from_vector(window.clone(), &[2, 2]).unwrap();
    let flattened = tensor.reshape(&[4]).unwrap();
    assert_eq!(values(&flattened), vec![0.0, 1.0, 2.0, 3.0]);
    assert!(std::ptr::eq(flattened.cells().as_ptr(), window.as_slice().as_ptr()));
    assert!(tensor.reshape(&[3, 2]).is_none());
    let slice = tensor.slice_first(1).unwrap();
    assert_eq!(slice.shape(), &[2]);
    assert_eq!(values(&slice), vec![2.0, 3.0]);
    assert!(std::ptr::eq(slice.cells().as_ptr(), &tensor.cells()[2]));
    assert!(tensor.slice_first(2).is_none());
    let scalar = slice.slice_first(1).unwrap();
    assert_eq!(scalar.shape(), &[]);
    assert_eq!(values(&scalar), vec![3.0]);
    assert!(std::ptr::eq(tensor.permute(&[0, 1]).unwrap().cells().as_ptr(), tensor.cells().as_ptr()));
}

#[test]
fn permutation_reorders_cells_and_validates_bijection() {
    let input: Vec<f64> = (0..12).map(f64::from).collect();
    let tensor = NumericTensor::new(&input, &[2, 2, 3]).unwrap();
    let result = tensor.permute(&[2, 0, 1]).unwrap();
    assert_eq!(result.shape(), &[3, 2, 2]);
    assert_eq!(values(&result), vec![0.0, 3.0, 6.0, 9.0, 1.0, 4.0, 7.0, 10.0, 2.0, 5.0, 8.0, 11.0]);
    assert_eq!(result.permute(&[1, 2, 0]).unwrap(), tensor);
    for invalid in [&[0, 0, 1][..], &[0, 1, 3], &[0, 1], &[0, 1, 2, 3]] {
        assert!(tensor.permute(invalid).is_none());
    }
    let scalar = NumericTensor::new(&[-0.0], &[]).unwrap();
    assert_eq!(scalar.permute(&[]).unwrap().cells()[0].get().to_bits(), (-0.0f64).to_bits());
}

#[test]
fn rank_two_bridge_reuses_the_existing_checked_matmul() {
    let a = NumericTensor::new(&[1.0, 2.0, 3.0, 4.0], &[2, 2]).unwrap();
    let b = NumericTensor::new(&[5.0, 6.0, 7.0, 8.0], &[2, 2]).unwrap();
    let tile = a.as_tile().unwrap();
    assert!(std::ptr::eq(tile.cells().as_ptr(), a.cells().as_ptr()));
    assert_eq!(NumericTensor::from_tile(&tile), a);
    let output = numeric::evaluate(
        Builtin::MatMul,
        &[Value::Tile(tile), Value::Tile(b.as_tile().unwrap())],
        20,
    )
    .unwrap();
    let tensor = NumericTensor::from_tile(output.as_tile().unwrap());
    assert_eq!(tensor.shape(), &[2, 2]);
    assert_eq!(values(&tensor), vec![19.0, 22.0, 43.0, 50.0]);
    assert!(a.reshape(&[4]).unwrap().as_tile().is_none());
    let empty = NumericTile::new(&[], 3, 0).unwrap();
    assert_eq!(NumericTensor::from_tile(&empty).as_tile().unwrap(), empty);
}

#[test]
fn all_small_three_axis_permutations_match_coordinate_reference() {
    let permutations = [[0, 1, 2], [0, 2, 1], [1, 0, 2], [1, 2, 0], [2, 0, 1], [2, 1, 0]];
    for x in 0..4 {
        for y in 0..4 {
            for z in 0..4 {
                let shape = [x, y, z];
                let input: Vec<f64> = (0..x * y * z).map(|n| n as f64).collect();
                let tensor = NumericTensor::new(&input, &shape).unwrap();
                for axes in permutations {
                    let output = tensor.permute(&axes).unwrap();
                    let expected = [shape[axes[0]], shape[axes[1]], shape[axes[2]]];
                    assert_eq!(output.shape(), &expected);
                    for i in 0..x {
                        for j in 0..y {
                            for k in 0..z {
                                let source = [i, j, k];
                                let dest = [source[axes[0]], source[axes[1]], source[axes[2]]];
                                let expected = (i * y * z + j * z + k) as f64;
                                assert_eq!(output.get(&dest).unwrap().get(), expected);
                            }
                        }
                    }
                }
            }
        }
    }
}
