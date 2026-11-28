//! Bounded, immutable row-major tensor storage for Rust hosts.
//!
//! This is a storage foundation, not a source-language tensor type. All
//! numerical cells reuse the finite vector representation.

use super::{FiniteFloat, NumericTile, NumericVector, MAX_VECTOR_LENGTH};

/// Maximum number of axes in an experimental numerical tensor.
pub const MAX_TENSOR_RANK: usize = 8;

/// An immutable finite tensor with an explicit, bounded shape.
///
/// Rank zero denotes one scalar. An axis of length zero denotes an empty
/// tensor; its other dimensions remain observable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NumericTensor {
    cells: NumericVector,
    dimensions: [usize; MAX_TENSOR_RANK],
    rank: usize,
}

/// Validate every axis, even for empty tensors, before computing the count.
fn element_count(shape: &[usize]) -> Option<usize> {
    if shape.len() > MAX_TENSOR_RANK || shape.iter().any(|&n| n > MAX_VECTOR_LENGTH) {
        return None;
    }
    if shape.contains(&0) {
        return Some(0);
    }
    shape.iter().try_fold(1usize, |count, &axis| {
        count.checked_mul(axis).filter(|&n| n <= MAX_VECTOR_LENGTH)
    })
}

impl NumericTensor {
    /// Construct from finite binary64 values in row-major order.
    pub fn new(values: &[f64], shape: &[usize]) -> Option<Self> {
        if element_count(shape)? != values.len() {
            return None;
        }
        Self::from_vector(NumericVector::new(values)?, shape)
    }

    /// Reuse an existing vector without copying its elements.
    pub fn from_vector(cells: NumericVector, shape: &[usize]) -> Option<Self> {
        if element_count(shape)? != cells.len() {
            return None;
        }
        let mut dimensions = [0; MAX_TENSOR_RANK];
        dimensions[..shape.len()].copy_from_slice(shape);
        Some(Self {
            cells,
            dimensions,
            rank: shape.len(),
        })
    }

    pub fn shape(&self) -> &[usize] {
        &self.dimensions[..self.rank]
    }

    pub fn rank(&self) -> usize {
        self.rank
    }

    pub fn len(&self) -> usize {
        self.cells.len()
    }

    pub fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }

    pub fn as_vector(&self) -> &NumericVector {
        &self.cells
    }

    pub fn cells(&self) -> &[FiniteFloat] {
        self.cells.as_slice()
    }

    /// Return a row-major offset, rejecting incorrect rank and coordinates.
    pub fn offset(&self, coordinates: &[usize]) -> Option<usize> {
        if coordinates.len() != self.rank || self.is_empty() {
            return None;
        }
        coordinates
            .iter()
            .zip(self.shape())
            .try_fold(0usize, |offset, (&index, &axis)| {
                if index >= axis {
                    return None;
                }
                offset.checked_mul(axis)?.checked_add(index)
            })
    }

    pub fn get(&self, coordinates: &[usize]) -> Option<FiniteFloat> {
        self.cells().get(self.offset(coordinates)?).copied()
    }

    /// Change shape without copying; the total number of cells must agree.
    pub fn reshape(&self, shape: &[usize]) -> Option<Self> {
        Self::from_vector(self.cells.clone(), shape)
    }

    /// Select one leading-axis slice. Nonempty slices share vector storage;
    /// empty slices use the vector's canonical empty allocation.
    pub fn slice_first(&self, index: usize) -> Option<Self> {
        let (&first, remaining) = self.shape().split_first()?;
        if index >= first {
            return None;
        }
        let count = element_count(remaining)?;
        let start = index.checked_mul(count)?;
        Self::from_vector(self.cells.window(start, count)?, remaining)
    }

    /// Convert a matrix to a rank-two tensor without copying its elements.
    pub fn from_tile(tile: &NumericTile) -> Self {
        Self::from_vector(tile.as_vector().clone(), &[tile.height(), tile.width()])
            .expect("validated tile shape")
    }

    /// Convert only rank-two tensors to the existing checked matrix type.
    pub fn as_tile(&self) -> Option<NumericTile> {
        if self.rank != 2 {
            return None;
        }
        NumericTile::from_vector(self.cells.clone(), self.dimensions[0], self.dimensions[1])
    }

    /// Rearrange axes; output axis `i` corresponds to input axis `axes[i]`.
    ///
    /// The work is bounded by rank * element count. Identity and empty
    /// permutations share existing storage; other permutations allocate at
    /// most MAX_VECTOR_LENGTH cells. This host API does not meter interpreter
    /// execution and must not be exposed as an unmetered source built-in.
    pub fn permute(&self, axes: &[usize]) -> Option<Self> {
        if axes.len() != self.rank {
            return None;
        }
        let mut seen = [false; MAX_TENSOR_RANK];
        let mut shape = [0; MAX_TENSOR_RANK];
        for (output, &input) in axes.iter().enumerate() {
            if input >= self.rank || seen[input] {
                return None;
            }
            seen[input] = true;
            shape[output] = self.dimensions[input];
        }
        let shape = &shape[..self.rank];
        if self.is_empty() || axes.iter().copied().eq(0..self.rank) {
            return self.reshape(shape);
        }

        let mut cells = Vec::new();
        cells.try_reserve_exact(self.len()).ok()?;
        let mut coordinates = [0; MAX_TENSOR_RANK];
        for flat in 0..self.len() {
            let mut remainder = flat;
            for (output, &input) in axes.iter().enumerate().rev() {
                let axis = shape[output];
                coordinates[input] = remainder % axis;
                remainder /= axis;
            }
            cells.push(self.get(&coordinates[..self.rank])?);
        }
        Self::from_vector(NumericVector::from_finite(cells)?, shape)
    }
}
