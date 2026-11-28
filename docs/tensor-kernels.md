# Experimental host tensor kernels

The Rust host API under `scarp::numeric::tensor` adds checked elementwise
addition, subtraction, multiplication and division and batched matrix
multiplication. These are not yet Scarp-language built-ins.

Shapes use right-aligned broadcasting: equal dimensions or singleton axes can
combine; a singleton and a zero-length dimension produce a zero-length axis.
Matrix multiplication takes rank >= 2 inputs, uses the final two axes as
matrix dimensions, and broadcasts all leading axes. Vector promotion is not
included. Every intermediate product and compensated sum must be finite.

Every call accepts explicit work and numerical payload budgets. The work
charge for binary operations is output count times (1 + output rank +
left rank + right rank). For matrix products it is output count times
(1 + output batch rank + left batch rank + right batch rank + twice inner
dimension). Validation precedes allocation; no partial output is returned
on failure. Empty output requires no per-cell work. These are logical
charges, not wall-clock estimates. Rank and maximum element counts retain the
storage foundation's limits. Input storage remains immutable.

The next separate integration layer must add parser/type support, runtime
value representation, shared interpreter quotas and source conformance.
No release publication or version change is part of this patch.

## Axis sums and broadcast-gradient reduction

`sum_axis(input, axis, keep_dims, limits)` reduces one zero-based axis using
the same finite compensated summation contract as matrix products. Rank-zero
inputs have no axis. The reduced dimension becomes one when retained; otherwise
the output is reshaped without a second cell allocation.

`sum_to_shape(input, target, limits)` sums the axes that would have been
broadcast from the target to the input. The target rank cannot exceed the
input rank; every right-aligned target dimension must equal the corresponding
input dimension or be one. Missing leading target axes are reduced. The
target can be rank zero, in which case all input cells are summed. This is
the adjoint of broadcasting and can be used for explicit bias gradients.

For example, an input shaped [2, 3] with cells [1, 2, 3, 4, 5, 6] reduces to
[5, 7, 9] for target shape [3], or [6, 15] for shape [2, 1]. A reduced zero
axis yields explicit positive zeros, including a scalar zero when all axes
are reduced. Output shape validation precedes allocation; eliminating a zero
axis can produce an oversized output, which is rejected before work.

Let r be input rank, t target rank, k reduced-axis count, N input cell count
and M output cell count. Work is prepaid as M*(1+r+t) + N*(1+r+k); payload
is 8*M cumulative logical bytes within this host call. Axis sums use the
same retained-axis shape internally and dropping the axis shares storage.
The operation scans each input cell once in deterministic row-major order
within its output group and never allocates an intermediate index array
proportional to the number of cells. These remain host-only experimental APIs,
not source-language tensor syntax or automatic differentiation.

## Development: explicit batched matrix gradients

`matmul_backward(left, right, upstream, limits)` returns two tensors with
exactly the original operand shapes. Upstream must have the exact forward
output shape. The implementation directly accumulates the products for each
operand cell, including across batch axes where an input was broadcast.
It allocates no transposed operands or expanded gradients. Contributions are
visited in increasing broadcast-batch and reduction-axis order, using finite
compensated sums. Empty output produces zeros for any nonempty input gradient.
An empty inner dimension gives two empty gradients.

A single host-call budget covers both returned buffers. For input lengths
A and B, ranks rA and rB, forward output length C, inner dimension K, and
broadcast batch rank R, work is `A*(1+rA) + B*(1+rB) + 2*C*K*(4+4*R)`.
Payload is `8*(A+B)` logical bytes. The forward output cell limit still
applies even though no forward result is materialized. Both buffers are
reserved before numerical loops; no partial result escapes on failure.
This is a host-only VJP primitive, not an automatic differentiation engine.

## Development: broadcast elementwise gradients

`binary_backward(operation, left, right, upstream, limits)` computes
both original-shaped operand gradients for addition, subtraction,
multiplication and division. The upstream tensor must match the exact
broadcast result shape. Missing and singleton axes are reduced directly;
no intermediate broadcast-sized gradients are stored. Each input cell's
contributions are accumulated in row-major order with compensated sums.

Division rejects signed-zero denominators for visited cells and checks the
intermediate quotient and gradient contributions for finiteness. The
backward call does not materialize or validate the complete forward result;
a caller must have separately qualified its forward evaluation. Empty
broadcast outputs produce positive zeros in any nonempty operand gradient.

For input lengths A and B, ranks rA and rB, broadcast output length C and
rank R, work is `A*(1+rA) + B*(1+rB) + 2*C*(6+4*R)`. Both gradients share
one payload budget, requiring `8*(A+B)` logical bytes. Shape and size
validation precede work and allocation; no partial gradient is returned.
This is a host API, not a Scarp-language feature yet.
