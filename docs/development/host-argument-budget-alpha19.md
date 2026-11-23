# Alpha.19 — bounded Rust host argument admission

Status: experimental draft-only embedding milestone, stacked after the
cumulative host quotas of PR #347 and the execution receipts of PR #346.

## Runtime protection

Interpreter ExecutionUsage accounts for new work/materialization within
Scarp. A host can construct strings and named argument labels before
execution, so those bytes cannot safely be called "new interpreter memory".
The cumulative ledger now explicitly bounds admission of existing values.

HostInvocationCaps.arguments contains HostArgumentLimits:
- max_values_per_call: 64 by default
- max_total_values: 16384 by default
- max_utf8_bytes_per_call: 1048576 by default
- max_total_utf8_bytes: 8388608 by default

Each positional or named argument counts one value. Logical UTF-8 bytes
include each named parameter label and every Value::String payload,
with no normalization or lossy text conversion. A shared immutable Arc
string passed twice counts twice. Non-string scalar values contribute
one value but zero UTF-8 payload bytes. No composite traversal is
needed because the current checked host callable only permits scalar
integer, boolean, float and string parameters.

Before any interpreter or I/O activity, the ledger checks both per-call
and remaining cumulative limits using checked addition and a bounded
linear scan, without cloning/allocating new strings. A denied admission
does not consume an invocation attempt or its payload quota.

After admission and the existing attempt/work gate, argument payloads
are charged BEFORE Scarp binding. Invalid labels, duplicate binding,
wrong types, unbound slots, runtime errors, input errors and failed
writers cannot undo an admitted charge. Neither work-denied nor
argument-denied calls are charged. Source errors retain the original
diagnostics.

HostInvocationLedger.argument_usage reports admitted host payload
separately from the interpreter charges returned by spent.
remaining_arguments preserves per-call caps while subtracting
cumulative value/UTF-8 usage. The four ledger entry points, including
reader-enabled linked-module entry, enforce the same policy.

### Boundaries

This guard cannot prevent a Rust host from allocating a large value
before calling Scarp. The host must enforce request-body size, memory,
concurrency, scheduling and authorization separately. It is not a
process-memory bound, a wall-clock timeout, or an OS sandbox.

No Scarp source syntax, first-class callable value, closure capture,
native ABI, protected-main merge, release tag or alpha.19 publication
is part of this feature.

Integration tests cover Unicode payload bytes, parameter label costs,
per-call and cumulative quotas, rejected preflight accounting, aliasing
of shared Arc payloads, zero-byte scalar calls, and exported readers
without I/O on admission denial.
