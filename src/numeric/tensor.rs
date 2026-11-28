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
fn broadcast(left: &[usize], right: &[usize], output: &mut [usize]) -> Result<(), Diagnostic> {
    let rank = output.len();
    for (axis, extent) in output.iter_mut().enumerate() {
        let l = axis.checked_sub(rank - left.len()).map_or(1, |i| left[i]);
        let r = axis.checked_sub(rank - right.len()).map_or(1, |i| right[i]);
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

fn charge(budget: &mut Budget, output_count: usize, per_cell: u64) -> Result<(), Diagnostic> {
    let span = Span::default();
    let work = (output_count as u64).checked_mul(per_cell).ok_or_else(|| {
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
    charge(
        &mut budget,
        count,
        (1 + rank + left.rank() + right.rank()) as u64,
    )?;
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

fn matmul_shape(
    left: &NumericTensor,
    right: &NumericTensor,
    shape: &mut [usize; MAX_TENSOR_RANK],
) -> Result<usize, Diagnostic> {
    if left.rank() < 2 || right.rank() < 2 {
        return Err(shape_error("tensor matmul requires rank at least two"));
    }
    let lrank = left.rank() - 2;
    let rrank = right.rank() - 2;
    let batch_rank = lrank.max(rrank);
    let lshape = left.shape();
    let rshape = right.shape();
    if lshape[lrank + 1] != rshape[rrank] {
        return Err(shape_error("tensor matmul inner dimensions must match"));
    }
    broadcast(&lshape[..lrank], &rshape[..rrank], &mut shape[..batch_rank])?;
    shape[batch_rank] = lshape[lrank];
    shape[batch_rank + 1] = rshape[rrank + 1];
    Ok(batch_rank + 2)
}

/// Rank-two or batched matrix multiplication, broadcasting leading axes.
/// The final two axes are matrix rows and columns. Rank-one promotion is
/// deliberately excluded. Zero inner dimensions produce explicit zeros.
pub fn matmul(
    left: &NumericTensor,
    right: &NumericTensor,
    limits: TensorLimits,
) -> Result<NumericTensor, Diagnostic> {
    let mut shape = [0; MAX_TENSOR_RANK];
    let rank = matmul_shape(left, right, &mut shape)?;
    let shape = &shape[..rank];
    let lrank = left.rank() - 2;
    let rrank = right.rank() - 2;
    let batch_rank = rank - 2;
    let lshape = left.shape();
    let rshape = right.shape();
    let rows = shape[batch_rank];
    let inner = lshape[lrank + 1];
    let columns = shape[batch_rank + 1];
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

/// Sum broadcasted axes back to a compatible, right-aligned target shape.
///
/// This is the reverse of broadcasting a target-shaped value to the input
/// shape. Missing leading axes and singleton target axes are summed, including
/// an empty reduced axis (whose sum is zero). It is useful for accumulating
/// gradients for shared bias and other broadcast parameters.
pub fn sum_to_shape(
    input: &NumericTensor,
    target: &[usize],
    limits: TensorLimits,
) -> Result<NumericTensor, Diagnostic> {
    let rank = input.rank();
    if target.len() > rank || target.iter().any(|&n| n > MAX_VECTOR_LENGTH) {
        return Err(shape_error(
            "tensor reduction target rank or axis exceeds its bound",
        ));
    }
    let source = input.shape();
    let leading = rank - target.len();
    let mut reduced_axes = [0; MAX_TENSOR_RANK];
    let mut reduced_shape = [0; MAX_TENSOR_RANK];
    let mut reduced_rank = 0;
    for (axis, &extent) in source.iter().enumerate() {
        let dest = axis.checked_sub(leading).map_or(1, |i| target[i]);
        if dest != extent && dest != 1 {
            return Err(shape_error(
                "tensor reduction target cannot broadcast to input",
            ));
        }
        if axis < leading || dest != extent {
            reduced_axes[reduced_rank] = axis;
            reduced_shape[reduced_rank] = extent;
            reduced_rank += 1;
        }
    }
    let output_count = count(target)?;
    let span = Span::default();
    let mut budget = limits.budget();
    // Charge even explicit zeros and coordinate decoding. Input and output
    // counts are independently bounded; all work is prepaid before allocation.
    let per_output = (1 + rank + target.len()) as u64;
    let per_term = (1 + rank + reduced_rank) as u64;
    charge(&mut budget, output_count, per_output)?;
    charge(&mut budget, input.len(), per_term)?;
    let mut output = buffer(output_count, &mut budget, span)?;
    if output_count == 0 {
        return finish(output, target);
    }
    if input.is_empty() {
        output.resize(output_count, finite_number(0.0, span)?);
        return finish(output, target);
    }

    // Both counts are nonzero and the target is broadcast-compatible, so
    // every output receives exactly this many input terms.
    let terms_per_output = input.len() / output_count;
    let mut source_coordinates = [0; MAX_TENSOR_RANK];
    let mut reduced_coordinates = [0; MAX_TENSOR_RANK];
    let reduced_shape = &reduced_shape[..reduced_rank];
    let reduced_coordinates = &mut reduced_coordinates[..reduced_rank];
    for flat in 0..output_count {
        source_coordinates.fill(0);
        decode(flat, target, &mut source_coordinates[leading..rank]);
        let terms = (0..terms_per_output).map(|term| {
            decode(term, reduced_shape, reduced_coordinates);
            for (i, &axis) in reduced_axes[..reduced_rank].iter().enumerate() {
                source_coordinates[axis] = reduced_coordinates[i];
            }
            // Every coordinate is checked by shape validation and decoding.
            let offset = source
                .iter()
                .zip(&source_coordinates)
                .fold(0usize, |offset, (&extent, &index)| offset * extent + index);
            Ok(input.cells()[offset].get())
        });
        output.push(finite_number(compensated(terms, span)?, span)?);
    }
    finish(output, target)
}

/// Sum one zero-based axis. Preserve it as a singleton when keep_dims is true.
pub fn sum_axis(
    input: &NumericTensor,
    axis: usize,
    keep_dims: bool,
    limits: TensorLimits,
) -> Result<NumericTensor, Diagnostic> {
    if axis >= input.rank() {
        return Err(shape_error(
            "tensor reduction axis is outside the input rank",
        ));
    }
    let mut shape = [0; MAX_TENSOR_RANK];
    let rank = input.rank();
    shape[..rank].copy_from_slice(input.shape());
    shape[axis] = 1;
    let result = sum_to_shape(input, &shape[..rank], limits)?;
    if keep_dims {
        return Ok(result);
    }
    shape.copy_within(axis + 1..rank, axis);
    result
        .reshape(&shape[..rank - 1])
        .ok_or_else(|| length_error(Span::default()))
}

/// Fixed-size plan for the batch coordinates that broadcast to one input
/// batch. Non-singleton input axes are fixed; missing and singleton axes vary.
struct BatchGradientPlan<'a> {
    batch: &'a [usize],
    own: &'a [usize],
    varying_axes: [usize; MAX_TENSOR_RANK],
    varying_shape: [usize; MAX_TENSOR_RANK],
    varying_rank: usize,
    repeats: usize,
}

impl<'a> BatchGradientPlan<'a> {
    // Called only for nonempty forward output and nonempty input gradients.
    fn new(batch: &'a [usize], own: &'a [usize]) -> Self {
        let mut plan = Self {
            batch,
            own,
            varying_axes: [0; MAX_TENSOR_RANK],
            varying_shape: [0; MAX_TENSOR_RANK],
            varying_rank: 0,
            repeats: 1,
        };
        let leading = batch.len() - own.len();
        for (axis, &extent) in batch.iter().enumerate() {
            let own_extent = axis.checked_sub(leading).map_or(1, |i| own[i]);
            if own_extent == 1 && extent != 1 {
                plan.varying_axes[plan.varying_rank] = axis;
                plan.varying_shape[plan.varying_rank] = extent;
                plan.varying_rank += 1;
                // Nonempty forward output bounds the whole batch product.
                plan.repeats *= extent;
            }
        }
        plan
    }

    fn fixed_coordinates(&self, flat: usize, coordinates: &mut [usize]) {
        coordinates.fill(0);
        let leading = self.batch.len() - self.own.len();
        decode(flat, self.own, &mut coordinates[leading..]);
    }

    fn select_batch(&self, mut index: usize, coordinates: &mut [usize]) -> usize {
        for i in (0..self.varying_rank).rev() {
            let extent = self.varying_shape[i];
            coordinates[self.varying_axes[i]] = index % extent;
            index /= extent;
        }
        self.batch
            .iter()
            .zip(coordinates)
            .fold(0, |flat, (&extent, &coordinate)| flat * extent + coordinate)
    }
}

struct MatmulGradient<'a> {
    left: &'a NumericTensor,
    right: &'a NumericTensor,
    upstream: &'a NumericTensor,
    batch: &'a [usize],
    rows: usize,
    inner: usize,
    columns: usize,
}

impl MatmulGradient<'_> {
    fn accumulate(
        &self,
        for_left: bool,
        output: &mut Vec<crate::value::FiniteFloat>,
    ) -> Result<(), Diagnostic> {
        let span = Span::default();
        let own = if for_left { self.left } else { self.right };
        if own.is_empty() {
            return Ok(());
        }
        if self.upstream.is_empty() {
            output.resize(own.len(), finite_number(0.0, span)?);
            return Ok(());
        }
        let (other, own_matrix, other_matrix, reduction) = if for_left {
            (
                self.right,
                self.rows * self.inner,
                self.inner * self.columns,
                self.columns,
            )
        } else {
            (
                self.left,
                self.inner * self.columns,
                self.rows * self.inner,
                self.rows,
            )
        };

        let own_batch = &own.shape()[..own.rank() - 2];
        let other_batch = &other.shape()[..other.rank() - 2];
        let plan = BatchGradientPlan::new(self.batch, own_batch);
        let mut coordinates = [0; MAX_TENSOR_RANK];
        let coordinates = &mut coordinates[..self.batch.len()];
        let output_matrix = self.rows * self.columns;
        for flat in 0..own.len() {
            plan.fixed_coordinates(flat / own_matrix, coordinates);
            let (row, k, column) = if for_left {
                ((flat % own_matrix) / self.inner, flat % self.inner, 0)
            } else {
                (0, (flat % own_matrix) / self.columns, flat % self.columns)
            };
            // Enumerate only matching broadcast batches, and visit their
            // reduction dimension in increasing order. No transpose or
            // broadcast-sized temporary is materialized.
            let terms = (0..plan.repeats * reduction).map(|term| {
                let batch = plan.select_batch(term / reduction, coordinates);
                let axis = term % reduction;
                let other_base = projected_offset(other_batch, coordinates) * other_matrix;
                let upstream_base = batch * output_matrix;
                let (g, value) = if for_left {
                    (
                        self.upstream.cells()[upstream_base + row * self.columns + axis],
                        other.cells()[other_base + k * self.columns + axis],
                    )
                } else {
                    (
                        self.upstream.cells()[upstream_base + axis * self.columns + column],
                        other.cells()[other_base + axis * self.inner + k],
                    )
                };
                finite_number(g.get() * value.get(), span).map(crate::value::FiniteFloat::get)
            });
            output.push(finite_number(compensated(terms, span)?, span)?);
        }
        Ok(())
    }
}

/// Explicit vector-Jacobian product for batched matrix multiplication.
///
/// Returns gradients matching the original left and right shapes, summing
/// repeated batch contributions directly into each input cell. The upstream
/// shape must exactly match the forward output, not merely broadcast to it.
/// Both returned buffers share one work/payload budget. No partial result
/// escapes if either gradient fails.
///
/// Work is input cell count times (1 + input rank), for each input separately,
/// plus two times the forward multiply-accumulate term count times
/// (4 + 4 * broadcast batch rank). All charges precede computation.
pub fn matmul_backward(
    left: &NumericTensor,
    right: &NumericTensor,
    upstream: &NumericTensor,
    limits: TensorLimits,
) -> Result<(NumericTensor, NumericTensor), Diagnostic> {
    let mut shape = [0; MAX_TENSOR_RANK];
    let rank = matmul_shape(left, right, &mut shape)?;
    let shape = &shape[..rank];
    let forward_count = count(shape)?;
    if upstream.shape() != shape {
        return Err(shape_error(
            "tensor matmul backward requires the exact forward output shape",
        ));
    }
    let batch_rank = rank - 2;
    let inner = left.shape()[left.rank() - 1];
    let span = Span::default();
    let mut budget = limits.budget();
    charge(&mut budget, left.len(), (1 + left.rank()) as u64)?;
    charge(&mut budget, right.len(), (1 + right.rank()) as u64)?;
    let terms = forward_count as u64 * inner as u64;
    budget.charge(2 * terms * (4 + 4 * batch_rank as u64), span)?;
    let mut left_output = buffer(left.len(), &mut budget, span)?;
    let mut right_output = buffer(right.len(), &mut budget, span)?;
    let gradient = MatmulGradient {
        left,
        right,
        upstream,
        batch: &shape[..batch_rank],
        rows: shape[batch_rank],
        inner,
        columns: shape[batch_rank + 1],
    };
    gradient.accumulate(true, &mut left_output)?;
    gradient.accumulate(false, &mut right_output)?;
    Ok((
        finish(left_output, left.shape())?,
        finish(right_output, right.shape())?,
    ))
}
