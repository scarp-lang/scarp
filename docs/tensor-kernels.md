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
