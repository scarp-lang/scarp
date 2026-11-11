# Bounded median and type-7 quantiles

> **Development only.** The published compiler baseline is
> [v0.0.6-alpha.6](releases/v0.0.6-alpha.6-validation.md).

These pure built-ins operate on a finite, immutable
numerical vector or visible vector window:

| Built-in | Signature | Result |
| --- | --- | --- |
| `median(values)` | `vector -> float` | Middle value, averaging two central values for even lengths |
| `quantile(values, fraction)` | `vector, float -> float` | Linearly interpolated quantile for fractions in `[0.0, 1.0]` |

## Example

```scarp
bind values = [4.0, 1.0, 7.0, 2.0];
print(median(values));         // 3.0
print(quantile(values, 0.25)); // 1.75
print(quantile(values, 0.75)); // 4.75
```

Runnable example: [`examples/order_statistics.srp`](../examples/order_statistics.srp).

Both functions sort an **independent, bounded copy** of the visible values
in ascending numerical order; they never reorder the user's input. The
definition is the widely used type-7 linear rank interpolation: for
`N` sorted values and a fraction `p`, compute
`rank = (N - 1) * p` and interpolate between the floor/ceiling positions.
In particular, `median(v)` is equivalent to `quantile(v, 0.5)`.
The `0.0` and `1.0` endpoints select the exact minimum and maximum.

All inputs are finite binary64 values. Signed zeros have a deterministic
sort order: **negative zero precedes positive zero**; exact endpoint selection
preserves their source bits. Other interpolated outputs follow binary64
rounding (not arbitrary-precision decimal interpolation). Opposite-sign
extreme values use an overflow-resistant weighted sum. A length-one vector
always yields its only element for any permitted fraction.

## Domain errors and limits

- Empty visible vectors produce numerical-domain diagnostic **E0034** at
  the vector argument. A quantile fraction below `0.0` or above `1.0`
  produces **E0034** at the fraction argument.
- Wrong types and argument counts use existing **E0015** and **E0024**
  diagnostics. Vector inputs cannot exceed **65,536** elements.
- Each successful nonempty evaluation precharges
  `N × (floor(log2(N)) + 5)` work steps, independent of values, ties,
  or fraction. Only **8 × N bytes** of additional numerical payload are
  allocated, for one sortable copy; they are charged to
  `--max-vector-bytes` **before allocation**. Every call uses the same
  cumulative execution quota, including calls on views of one shared vector.
- Work and allocation failures produce **E0017** and **E0036**
  respectively. Errors do not publish partially evaluated output.

## Compiler and embedding behavior

The normal resolver gives an existing user function priority over a built-in
of the same name. The experimental Rust `numeric::Builtin` enum gains
`Median` and `Quantile`. Update exhaustive Rust matches for those cases.
`numeric::evaluate_with_vector_limit` accepts explicit work and copy quotas.
The existing `evaluate` host entry point retains its step-only contract.
The native scalar backends reject these calls with E0049.
