# Alpha.19 — unwind-safe host ledger accounting

Status: experimental draft Rust embedding milestone stacked on PR #349.

## Problem

An existing Scarp host may provide any type implementing std::io::Write.
The interpreter evaluates the function and fills ExecutionUsage BEFORE
replaying buffered output. A caller-supplied writer can still panic during
write() or flush(), causing a Rust unwind past the ledger's normal
post-call charge. If the application catches the panic and reuses the
ledger, it could repeat completed Scarp execution without paying quota.

## Implementation

The ledger now uses a small private PendingUsage guard for each
invocation. It owns a receipt and a temporary mutable reference to the
application ledger; the interpreter fills that receipt. The guard's
Drop implementation always adds the currently measured receipt, whether
the call returns successfully, returns an ordinary error, or unwinds
through a panicking writer. No unsafe code, catch_unwind inside the
runtime, global storage, alternative evaluator, or writer-side accounting
is introduced.

Guard construction happens only AFTER host argument admission, the
attempt/work quota gate, and host argument debiting. Reader-enabled
calls calculate the effective remaining InputLimits before borrowing
the ledger for the guard.

A successful or returned-error invocation is debited exactly once.
If a writer panics after the interpreter has finished, the same
completed charges survive the unwind. The host may catch the panic
with its own unwind policy and inspect updated counters. The ledger
keeps no partially initialized function Value or source frame.

### Precise boundary

An input reader panicking DURING evaluation can unwind before the
interpreter publishes its usage receipt. This change cannot infer
the partial work already done in that circumstance and does not claim
panic-safety for arbitrary host or interpreter code, process termination,
panic=abort builds, hostile threads, or asynchronous cancellation.
Hosts must enforce their own input-reader reliability, request limits
and isolation policy. Program output already written to the host writer
before a panic is not reversible.

No Scarp source syntax, first-class functions or closure capture, new
GC, native ABI, main merge or alpha.19 release is included.

Regression tests exercise panicking write() and flush() implementations,
normal/returned-error single debits, reader-byte charges that survive
a replay panic, and trusted/export-constrained host entries.
