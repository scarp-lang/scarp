# Source tensors and differentiation

A tensor is an immutable row-major array of finite binary64 values. Its type
includes the exact shape. Each extent is a decimal literal from 0 through
65536, rank is at most eight, and the shape product is at most 65536 cells.
Rank zero has one scalar cell. A shape with a zero axis has zero cells.

```scarp
bind a: tensor<float, 2, 2> = [[1.0, 2.0], [3.0, 4.0]];
bind b = [tensor: <float, 2, 1> [[5.0], [6.0]]];
bind product = matmul(a, b);
bind row = product[1];
print(product, row[0]); // tensor<float, 2, 1> [17.0, 39.0] 39.0
```

An annotated binding can use a bare nested literal as its complete
initializer. The tagged form works in other expression positions, including
returns, assignments and call arguments. Use `[tensor: <float> 2.0]` for a
rank-zero tensor or `[tensor: <float, 2, 0> []]` for empty storage. Array
levels must have the specified lengths, and each cell expression has type
`float`. Indexing checks one leading axis at a time. Rank-one indexing gives
a float. Use `tensor_item` to extract a rank-zero cell.

Tensors work in function signatures, module exports, record fields,
composites and unconstrained generic functions. Module constant initializers
remain scalar. Tensor equality requires the same exact shape and compares
the visible cells. Reassigning a `var` preserves existing snapshots.

| Operation | Source |
| --- | --- |
| Elementwise arithmetic | `a + b`, `a - b`, `a * b`, `a / b` |
| Matrix or batched matrix product | `matmul(a, b)` |
| Immutable zero-copy reshape | `tensor_reshape(a, dim0, dim1, ...)` |
| Explicit broadcast | `tensor_broadcast(a, dim0, dim1, ...)` |
| Swap the final two axes | `tensor_transpose(a)` |
| Sum all cells to a float | `tensor_sum(a)` |
| Sum and drop one axis | `tensor_sum_axis(a, axis)` |
| Elementwise activations | `tensor_neg`, `tensor_square`, `tensor_exp`, `tensor_log`, `tensor_relu`, `tensor_sigmoid`, `tensor_tanh`, `tensor_softplus` |

Broadcasting aligns trailing axes. Equal axes combine directly and a singleton
can expand, including to length zero. Matrix multiplication uses the final two
axes and broadcasts the leading axes. Shape arguments and reduction axes are
nonnegative integer literals. Division by zero, an invalid logarithm and
nonfinite intermediates fail at the source operation.

## Scalar and tensor gradients

```scarp
bind x = 3.0;
bind b = 2.0;
print(value_grad(x, scalar_square(x) + x * b)); // (15.0, 8.0)

bind w: tensor<float, 2> = [2.0, 3.0];
bind result = tensor_value_grad(w, tensor_sum(tensor_square(w)));
bind next = tensor_sgd(w, result[1], 0.1);
print(result[0], next); // 13.0 tensor<float, 2> [1.6, 2.4]
```

Both forms select one named binding and return a tuple of the loss and its
derivative. Other named bindings are captured from the current frame each
time the immutable graph is evaluated. The scalar graph accepts float
literals, arithmetic, negation, and the `scalar_square`, `scalar_exp`,
`scalar_log`, `scalar_relu`, `scalar_sigmoid` and `scalar_tanh` calls.
Its expression limit is 4095 syntax nodes plus the parameter. The tensor
graph accepts named tensors, arithmetic, matmul, activation, reshape,
broadcast, transpose and axis-sum nodes under `tensor_sum`. Bind a tensor
literal before using it in the graph. Its limit is 1023 syntax nodes plus
the parameter. Both forms reject custom calls and control flow within the
differentiated expression. The tensor reverse pass uses the checked
adjoints, accumulating repeated edges in deterministic order. For a binary
operation it checks both local adjoints, even when one operand is inactive.

Explicit tensor adjoints are also available. Use `tensor_add_backward`,
`tensor_sub_backward`, `tensor_mul_backward`, `tensor_div_backward` or
`tensor_matmul_backward` with `(left, right, upstream)` to get the two
input-shaped gradients. Unary builtins accept the suffix `_backward` and
take `(input, upstream)`. `tensor_sum_axis_backward(input, axis, upstream)`
restores the reduced shape.

## Optimizers and resource limits

`tensor_sgd(weights, gradient, rate)` returns updated weights with the
same shape. `tensor_adam_init(weights, beta1, beta2, epsilon)` returns a
checkpoint tuple of two moment tensors, the completed step count and the
three configuration values. `tensor_adam(weights, gradient, state, rate)`
and `tensor_adamw(weights, gradient, state, rate, decay)` return
`(updated_weights, updated_state)`. Checkpoints are validated on each use.
Resume a run with the original configuration to preserve its bias correction.

Numerical operations, tape scratch and output snapshots use the ordinary
Scarp execution budgets. Tensor reshapes and slices share immutable cell
storage. A view's storage stays alive through its last reference. New
materializations charge the cumulative numerical byte budget, and optimizer
checkpoints and gradients use the cumulative tuple budget for their tuple
storage. Native code generation currently supports a separate scalar subset.

Run the checked end-to-end example with
`scarp exec examples/tensor_learning.srp`.
