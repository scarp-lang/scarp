# Alpha.19 — cumulative Rust host invocation budgets

Status: experimental draft work, stacked after PR #346's actual interpreter
ExecutionUsage receipts. This is an embedding API, not a Scarp source feature.

## Problem and semantics

ExecutionOptions and InputLimits are per interpreter entry. A service may
dispatch an arbitrary number of host callable functions and accidentally
reset quotas on every call. HostInvocationLedger fixes this at the explicit
application boundary: a mutable Rust object with a fixed HostInvocationCaps
and cumulative spent ExecutionUsage.

The host owns the ledger and decides which callers share it. There is no
process-wide global counter, cache, network, clock, async request scheduler,
or implicit security principal. A service that deliberately constructs a
fresh ledger for each client request is not protected across requests.

## Interface

HostInvocationCaps has max_attempts, execution: ExecutionOptions, and
input: InputLimits. Its execution allocation, work, function call, output
statement, and output snapshot limits become TOTAL ledger allowances.
max_call_depth, max_live_slots, max_bigint_digits are still per-call
structural/per-value safety limits, intersected with caller options.
InputLimits.max_reads and max_total_bytes are cumulative; max_line_bytes
is a per-read cap.

HostInvocationLedger::invoke_slot / invoke_slot_with_input
HostInvocationLedger::invoke_exported / invoke_exported_with_input
HostInvocationLedger::attempts, spent, caps, remaining_execution,
remaining_input

All invoke functions intersect the caller's limits with the ledger's
remaining quotas before executing the existing interpreter through the
checked host-call API. After execution they charge actual usage even
when a diagnostic, I/O failure or input EOF occurs.

max_attempts counts calls which passed the ledger's preflight, including
unbound selection and invalid host argument errors (their interpreter
resource usage remains zero). Denied calls are rejected *before* any
interpreter activity, input or output, and do not consume an attempt.
A zero total work or function-call allowance is known to forbid any
new host function and returns HostInvocationError::Quota immediately.

Output or input limits reaching zero do not automatically prohibit a
new call: a scalar-only function may legitimately need neither. The
normal interpreter checks a selected read/print, reports the original
source-based runtime diagnostic and retains any already-spent work.
The ledger reports quota denials separately from source runtime errors.

The ledger is intentionally not Clone: clients do not accidentally
fork their counters by copying it. If concurrent access is needed,
the host must serialize it with its own synchronization and apply
trusted service-level authorization/allowlists; a Rust process may still
construct multiple independent ledgers.

## Crucial non-goals

This meters charged interpreter work and payload allocations, NOT actual
RSS, CPU scheduling, externally allocated HostArguments memory, writer
replay I/O volume, process-wide storage, filesystem or network limits.
The per-call ExecutionUsage receipts exclude host I/O writer activity
outside evaluation. This feature is not a remote-execution sandbox.

The ledger retains neither program snapshots nor values: the host owns
CallableSlot/ExportedCallableSlot and selects targets under existing
signature/effect/module pub checks. Program source still cannot create
first-class function values or closures. No native ABI, tag, main merge
or alpha.19 release is included.

## Verification

Integration tests cover attempt caps; measured work exhaustion; call
counts; failed contract work billing; output quotas and atomic buffered
writes; input-read and raw-byte quotas over a shared reader; unbound and
invalid argument attempts; host writer failure accounting; and
source-aware exported module execution/read budgets.
