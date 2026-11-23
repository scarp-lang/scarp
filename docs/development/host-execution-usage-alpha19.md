# Alpha.19 — measured host callable execution receipts

Status: experimental Rust embedding milestone on draft PR #346, stacked after
PR #345. Last published release remains v0.0.6-alpha.18.

## Why receipts matter

ExecutionOptions applies deterministic limits to **one** call. A Rust host
may make many calls using a long-lived CallableSlot or ExportedCallableSlot.
Previously the host could set per-call limits, but could not observe actual
work and allocation charges or whether a failed call consumed input/work.

ExecutionUsage exposes deterministic **charges**, not process memory or wall
time. The interpreter owns the source of truth. No duplicated evaluator,
output formatter, custom call frame or second tracing mechanism is introduced.

### Fields

steps, calls, prints, capture_bytes, vector_bytes, text_bytes, tuple_bytes,
bigint_bytes, array_bytes, input_reads, input_bytes. Calls include the
initial host invocation and selected nested calls; they count tail-replaced
logical calls. Input reads include EOF attempts; bytes include CR/LF.
Allocation fields report already charged materialization and scratch
storage, not Rust host-created argument payloads or physical heap usage.
Capture bytes reflect cumulative output-snapshot charges, not byte-for-byte
serialized stdout/stderr length.

### APIs

NamedCallableRef::invoke_bound_with_usage_and_options and
invoke_bound_with_input_and_usage_and_options
CallableSlot::invoke_with_usage_and_options and
invoke_with_input_and_usage_and_options
ExportedCallableSlot::invoke_with_usage_and_options and
invoke_with_input_and_usage_and_options

Each call takes a mutable ExecutionUsage receipt, overwrites it at the
beginning, and returns the same owned Value/error behavior as before.

The initial argument binder zeroes usage before preflight, so invalid
host labels/types and unbound slots never report work or input. For
accepted calls, the existing machine fills usage BEFORE returning an error
or replaying buffered output. Runtime work and reader consumption on a
failed call remain visible; a writer failure during replay likewise
preserves the completed execution receipt. An execution stopped during
initial frame preflight may have zero usage.

The receipt does NOT include bytes or work performed by user-supplied
writers after replay starts, or time blocked on host input, nor external
allocation of host argument values. Program output is transactionally
replayed under the existing semantics. Host callers remain responsible for
cumulative quotas across invocations and for external I/O boundaries.

No first-class source-language functions, lexical closures, native ABI,
new collector, protected-main merge or alpha.19 prerelease is added.

## Verification

Integration regressions check exact call counts for nested/defaulted
calls; omitted defaults; input CRLF raw bytes, EOF attempts and read
limits; preflight/unbound zeroing; work-budget failures; contract failures;
capture counts; writer errors after execution; and linked-module source
diagnostics. All existing runtime and package CI gates remain required.
