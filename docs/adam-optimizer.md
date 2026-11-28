# Experimental Adam and AdamW optimizers

The Rust host module `scarp::numeric::optimizer` provides immutable Adam
state, decoupled weight decay (AdamW), and bounded vector L2 clipping.
These APIs do not yet have Scarp source-language bindings.

```rust
use scarp::numeric::optimizer::{AdamConfig, AdamState, OptimizerLimits};
use scarp::value::NumericVector;

let limits = OptimizerLimits { max_steps: 1000, max_vector_bytes: 1024 };
let state = AdamState::new(2, AdamConfig::default(), limits).unwrap();
let weights = NumericVector::new(&[1.0, 2.0]).unwrap();
let gradient = NumericVector::new(&[0.1, -0.2]).unwrap();
let update = state.adamw(&weights, &gradient, 0.01, 0.001, limits).unwrap();
assert_eq!(update.state.step_count(), 1);
```

## Contract

A state fixes beta1, beta2 in [0,1) and finite positive epsilon for its
lifetime. Every step accepts finite nonnegative learning rate and weight
decay. The first and second moments and bias correction are conventional
Adam; AdamW multiplies weights by (1-rate*decay) before subtracting the
Adam direction times the rate. A zero learning rate still advances
moments and the step count but preserves the weight values.

All stored and checked intermediate values must be finite. The second
moment is formed by scaling the gradient by sqrt(1-beta2) before squaring,
reducing unnecessary overflow; the corrected RMS is computed without
materializing the corrected second moment. Bias corrections use expm1
to avoid cancellation near beta=1. Binary64 underflow is allowed and
intermediate overflow that cannot be handled by the stated ordering
produces a diagnostic, not a partial update. The optimizer does not claim
bitwise equivalence to third-party implementations.

The state and input vectors are immutable; an update returns a new weight
vector and state. Restore validates moment lengths, nonnegative second
moments and zero moments at step zero, then shares the provided buffers.
Restore is not a provenance or checkpoint authenticity check. A u64
step counter overflow is rejected.

## Resource bounds

Initialization reserves one shared zero vector: max(1,n) logical work and
8*n bytes. Restore charges max(1,n) work with no numerical payload copy.
Every update prepays 24*n+8 work and 24*n bytes across the three new
buffers. These logical bounds exclude Arc metadata and process overhead.
The existing 65,536-element vector limit applies.

The clip_norm function computes a scaled compensated L2 norm, compares
in normalized space and returns shared storage if no change is needed.
Clipping allocates 8*n bytes and every branch prepays max(1,5*n) work.
The norm bound is finite and nonnegative; rounding can affect the final
norm by binary64 rounding error. This is per-vector clipping rather than
a global norm across parameter groups.

The integration test trains a reusable scalar autodiff graph with Adam,
and checks independent scalar references, state restoration, resource
boundaries, extreme values, windows and failure atomicity.
