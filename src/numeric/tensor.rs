//! Explicitly budgeted host tensor arithmetic.
//!
//! Source syntax and interpreter dispatch are intentionally not exposed.

use super::{buffer, compensated, domain, length_error};
use crate::{
    diagnostics::{Diagnostic, DiagnosticKind, Span},
    limits::Budget,
    value::{finite_number, NumericTensor, NumericVector, MAX_TENSOR_RANK, MAX_VECTOR_LENGTH},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TensorBinary {
    Add,
    Subtract,
    Multiply,
    Divide,
}

/// Work and logical output-buffer limits for a single host call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TensorLimits {
    pub max_steps: u64,
    pub max_vector_bytes: usize,
}

impl TensorLimits {
    fn budget(self) -> Budget {
        Budget::new(self.max_steps).with_vector_bytes(self.max_vector_bytes)
    }
}

fn shape_error(message: &str) -> Diagnostic {
    Diagnostic::new(DiagnosticKind::VectorLength, Span::default(), message)
}

fn count(shape: &[usize]) -> Result<usize, Diagnostic> {
    if shape.contains(&0) {
        return Ok(0);
    }
    shape
        .iter()
        .try_fold(1usize, |n, &d| {
            n.checked_mul(d).filter(|&v| v <= MAX_VECTOR_LENGTH)
        })
        .ok_or_else(|| length_error(Span::default()))
}

/// Right-aligned broadcasting: equal axes or a singleton can combine.
/// In particular, zero and one combine to zero, not one.
fn broadcast(
    left: &[usize],
    right: &[usize],
    output: &mut [usize],
) -> Result<(), Diagnostic> {
    let rank = output.len();
    for (axis, extent) in output.iter_mut().enumerate() {
        let l = axis
            .checked_sub(rank - left.len())
            .map_or(1, |i| left[i]);
        let r = axis
            .checked_sub(rank - right.len())
            .map_or(1, |i| right[i]);
        *extent = match (l, r) {
            (a, b) if a == b => a,
            (1, b) => b,
            (a, 1) => a,
            _ => return Err(shape_error("incompatible tensor broadcast dimensions")),
        };
    }
    Ok(())
}

fn decode(mut flat: usize, shape: &[usize], coordinates: &mut [usize]) {
    for (axis, &extent) in shape.iter().enumerate().rev() {
        coordinates[axis] = flat % extent;
        flat /= extent;
    }
}

/// The input shape is a suffix of the output rank; singleton axes select zero.
fn projected_offset(shape: &[usize], coordinates: &[usize]) -> usize {
    let suffix = &coordinates[coordinates.len() - shape.len()..];
    shape.iter().zip(suffix).fold(0, |offset, (&axis, &index)| {
        offset * axis + if axis == 1 { 0 } else { index }
    })
}

fn finish(
    values: Vec<crate::value::FiniteFloat>,
    shape: &[usize],
) -> Result<NumericTensor, Diagnostic> {
    NumericVector::from_finite(values)
        .and_then(|v| NumericTensor::from_vector(v, shape))
        .ok_or_else(|| length_error(Span::default()))
}

fn charge(
    budget: &mut Budget,
    output_count: usize,
    per_cell: u64,
) -> Result<(), Diagnostic> {
    let span = Span::default();
    let work = (output_count as u64)
        .checked_mul(per_cell)
        .ok_or_else(|| {
            Diagnostic::new(DiagnosticKind::ExecutionLimit, span, "tensor work overflow")
        })?;
    budget.charge(work, span)
}

/// Apply finite elementwise arithmetic with right-aligned broadcasting.
/// Work charges one unit per output plus coordinate projection for both inputs.
pub fn binary(
    operation: TensorBinary,
    left: &NumericTensor,
    right: &NumericTensor,
    limits: TensorLimits,
) -> Result<NumericTensor, Diagnostic> {
    let rank = left.rank().max(right.rank());
    let mut shape = [0; MAX_TENSOR_RANK];
    let shape = &mut shape[..rank];
    broadcast(left.shape(), right.shape(), shape)?;
    let count = count(shape)?;
    let span = Span::default();
    let mut budget = limits.budget();
    charge(&mut budget, count, (1 + rank + left.rank() + right.rank()) as u64)?;
    let mut output = buffer(count, &mut budget, span)?;
    let mut coordinates = [0; MAX_TENSOR_RANK];
    let coordinates = &mut coordinates[..rank];
    for flat in 0..count {
        decode(flat, shape, coordinates);
        let a = left.cells()[projected_offset(left.shape(), coordinates)].get();
        let b = right.cells()[projected_offset(right.shape(), coordinates)].get();
        let value = match operation {
            TensorBinary::Add => a + b,
            TensorBinary::Subtract => a - b,
            TensorBinary::Multiply => a * b,
            TensorBinary::Divide => {
                if b == 0.0 {
                    return Err(domain(span, "tensor division by zero"));
                }
                a / b
            }
        };
        output.push(finite_number(value, span)?);
    }
    finish(output, shape)
}

/// Rank-two or batched matrix multiplication, broadcasting leading axes.
/// The final two axes are matrix rows and columns. Rank-one promotion is
/// deliberately excluded. Zero inner dimensions produce explicit zeros.
pub fn matmul(
    left: &NumericTensor,
    right: &NumericTensor,
    limits: TensorLimits,
) -> Result<NumericTensor, Diagnostic> {
    if left.rank() < 2 || right.rank() < 2 {
        return Err(shape_error("tensor matmul requires rank at least two"));
    }
    let lrank = left.rank() - 2;
    let rrank = right.rank() - 2;
    let batch_rank = lrank.max(rrank);
    let lshape = left.shape();
    let rshape = right.shape();
    let rows = lshape[lrank];
    let inner = lshape[lrank + 1];
    if inner != rshape[rrank] {
        return Err(shape_error("tensor matmul inner dimensions must match"));
    }
    let columns = rshape[rrank + 1];
    let mut shape = [0; MAX_TENSOR_RANK];
    broadcast(
        &lshape[..lrank],
        &rshape[..rrank],
        &mut shape[..batch_rank],
    )?;
    shape[batch_rank] = rows;
    shape[batch_rank + 1] = columns;
    let shape = &shape[..batch_rank + 2];
    let count = count(shape)?;
    let span = Span::default();
    let mut budget = limits.budget();
    let per_cell = (1 + batch_rank + lrank + rrank) as u64 + 2 * inner as u64;
    charge(&mut budget, count, per_cell)?;
    let mut output = buffer(count, &mut budget, span)?;
    if count == 0 {
        return finish(output, shape);
    }
    let mut coordinates = [0; MAX_TENSOR_RANK];
    let coordinates = &mut coordinates[..batch_rank];
    let matrix_cells = rows * columns;
    for flat in 0..count {
        let batch = flat / matrix_cells;
        let row = (flat % matrix_cells) / columns;
        let column = flat % columns;
        if inner == 0 {
            output.push(finite_number(0.0, span)?);
            continue;
        }
        decode(batch, &shape[..batch_rank], coordinates);
        let lbase = projected_offset(&lshape[..lrank], coordinates) * rows * inner;
        let rbase = projected_offset(&rshape[..rrank], coordinates) * inner * columns;
        let terms = (0..inner).map(|k| {
            let a = left.cells()[lbase + row * inner + k].get();
            let b = right.cells()[rbase + k * columns + column].get();
            finite_number(a * b, span).map(crate::value::FiniteFloat::get)
        });
        output.push(finite_number(compensated(terms, span)?, span)?);
    }
    finish(output, shape)
}
