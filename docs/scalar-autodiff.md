# Experimental scalar automatic differentiation

The Rust-host API `scarp::numeric::autodiff` records an append-only scalar
computation graph and evaluates a selected output together with its
reverse-mode derivatives. This is experimental, not Scarp-language syntax.

```rust
use scarp::numeric::autodiff::{Binary, Graph, GraphLimits, Unary};

let mut graph = Graph::new(8).unwrap();
let x = graph.input().unwrap();
let y = graph.input().unwrap();
let product = graph.binary(Binary::Multiply, &x, &y).unwrap();
let loss = graph.unary(Unary::Square, &product).unwrap();
let limits = GraphLimits { max_steps: 1000, max_vector_bytes: 1024 };
let tape = graph.evaluate(&loss, &[2.0, 3.0], 1.0, limits).unwrap();
assert_eq!(tape.output(), 36.0);
assert_eq!(tape.gradient(&x), Some(36.0));
assert_eq!(tape.gradient(&y), Some(24.0));
```

## Semantics

Inputs have fixed creation-order slots. An opaque node handle is owned by its
graph; foreign or stale handles are rejected. Graph nodes can only reference
earlier nodes, so cycles cannot be introduced through the public API.
Constants and inputs are finite binary64 values. Forward operations are
negation, square, exponential, natural logarithm, ReLU, sigmoid, tanh,
addition, subtraction, multiplication and division. Log requires a positive
argument and division rejects either signed zero denominator.

The selected output's ancestors execute in topological order. Disconnected
operations, even if invalid, do not execute. All provided inputs and the
seed must nevertheless be finite and the input count must be exact. Reverse
mode visits nodes in reverse topological order, adding contributions for each
edge, including duplicate edges. The seed is the output cotangent (normally
one). Zero cotangents skip local derivatives. ReLU selects the zero
subgradient at zero. Gradients of constants are observable as cotangents,
but constants are not input slots. Unreachable nodes have no value and zero
gradient. Later-added nodes are absent from older tapes.

Every stored result, local derivative contribution and accumulation must be
finite. Division VJPs reuse the forward quotient and compute the left
contribution before the right one; intermediate overflow is a diagnostic
even if another algebraic ordering could avoid it. Smooth derivatives
preserve representable saturation tails, but this is finite binary64
arithmetic, not symbolic or arbitrary-precision differentiation.

## Resource contract

Graph capacity is capped at 65,536 nodes and metadata is reserved during
construction. Evaluation is iterative and uses three bounded buffers:
forward values, gradients, and reachability flags. Every evaluation prepays
16 work units per node plus one per supplied input, and 17 logical payload
bytes per node (two 8-byte finite scalars and one bool byte). These are
logical charges rather than wall-clock or complete process-memory estimates.
No graph edit occurs during evaluation, and no tape is returned if any
validation, reservation, forward computation or reverse computation fails.

Graphs are reusable with different inputs and seeds, and returned tapes are
independent snapshots. This first foundation has no tensor-valued nodes,
higher-order derivatives, graph optimizations, source syntax, optimizer
state or GPU backend.
