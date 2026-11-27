# Scarp Lifetime — automatic memory management

**Status:** Scarp has a compiler-guided scope-retirement implementation and
Rust-managed immutable shared storage. **Lifetime** names this architecture;
it is not yet a complete general-purpose memory manager, a tracing collector,
or a guarantee about process resident memory. This document distinguishes the
current interpreter from planned native, concurrent, and AI runtimes.

Since alpha.4, Scarp includes compiler-planned retirement of frame slots when
their lexical scopes end. Programmers continue to use ordinary `bind`, `var`,
blocks, calls, returns and loop control; no deallocation syntax is needed.

The resolver assigns a unique slot per declaration and emits internal `Retire`
operations at block exits. `break` and `continue` carry the slots of every scope
they exit, preserving outer bindings and the repeat state needed by the next
iteration. On development branches with repeat-exit retirement, a normal
counted-loop exit or a `break` also retires the saved count and index.
Retirement drops
a slot's immutable value by replacing it with the internal unit marker. It is
bookkeeping excluded from source statement counts and existing work charges.

Function completion retires locals even after an early return. Parameters and
the result remain available until all `ensures` checks finish. Default arguments
run in order and may read earlier parameters. Recursive invocations have isolated
frames. Eligible terminal self-calls replace their invocation state only after
outgoing arguments have been evaluated; defaults, preconditions and budgets
apply again to the next invocation. Postconditions prevent this optimization.

The interpreter can also reuse one completed ordinary function frame's
buffers within an execution. It removes values, supplied-argument flags and
expression state before retaining the buffers. Each retained buffer has a
capacity of at most 128 items, and a later call uses it only if its frame
fits. Returned values keep their own references. Calls still spend the same
work and quota and count the same active slots. Retained buffer capacity is
separate from active frame slots and process resident memory.

An eligible terminal self-call reuses the active frame's evaluator buffers
after clearing all temporary values. Those buffers are released with the
frame, or individually discarded if their capacity exceeds the spare limit
when the frame finishes an ordinary invocation.

Clones, returned values, immutable vector windows, tiles and queued output events
retain their own references to shared backing storage. Retiring a source slot
does not change those values. Shared storage is released when its final holder
is dropped. A small window can retain a much larger original buffer.

## Verification and limits

Unit-test builds automatically audit resolver plans using a reachable-control-flow
worklist. Incoming initialized-slot sets join by intersection. Reads, updates,
repeat state, stores, retirement indices and jump targets are checked. Defaults
are checked in order, postconditions have a dedicated initialized result slot,
and parameters must survive all function-completion paths. Negative tests corrupt
valid plans to exercise stale reads, invalid slots/targets, branches, loops,
default/result dependencies and premature parameter retirement.

The checker is test-only development tooling, not a runtime tracing collector or
a proof covering arbitrary future language features. Current reasoning relies on
the absence of addressable references, closures, cycles and asynchronous escapes.
Rust's safe types and immutable `Arc` backing still provide the host storage
implementation. Fixed frame-slot arrays remain allocated for a frame's lifetime.

`max_live_slots` counts allocated frame slots rather than occupied values.
`max_vector_bytes` counts cumulative newly materialized element payload; retirement
does not refund it. `max_capture_bytes` counts logical output snapshots even if
their backing buffers are shared. Work limits count logical execution. None of
these alone or together measures total process RSS or every parser/host allocation.

See [proposal 0019](proposals/0019-scope-retirement-ledger.md),
[architecture](architecture.md), and [the numerical budget contract](budgets.md).


## Shared values and predictable reclamation

Source-level bindings are values, not exposed addresses. `bind` snapshots
and `var` reassignment preserve existing aliases: strings share immutable
`Arc<str>` storage; numerical vectors/tiles share `Arc<Vec<_>>` and
visible ranges; tuples, arrays, lists, maps, records, and variants share
immutable composite allocations. Returning a value or placing it in an
immutable composite can extend its storage lifetime beyond its original
binding's lexical scope. Destructuring produces independent value handles
where the currently implemented type rules allow it.

On normal scope exits, returns, breaks and continues, resolved retirement
operations drop the corresponding frame-slot handles. Rust's reference-counted
payloads are released at their final strong-reference drop. The machine does
not scan arbitrary object graphs; the supported immutable source values do
not expose mutable references or user-constructed cycles.

**Lifetime alpha.18 source candidate:** an empty numerical window is detached
from its source buffer and uses a canonical immutable empty allocation.
This applies to finite and integer vectors, nested windows, and zero-cell
tiles formed from those empty views. No numerical elements are copied.
A **nonempty** window remains a zero-copy shared view and may keep a much
larger source allocation live after the original binding is retired. Window
offset and length validation is unchanged. Empty constructors also reuse
their type-specific empty allocation.

There is no guarantee that the host allocator immediately returns released
physical pages to the operating system. String formatting, output capture,
expression stacks, compilation data, and native host allocations can retain
other memory independently. Cumulative payload budgets charge new numerical
materializations; empty views and clones do not refund or recharge those
budgets. `max_live_slots` measures reserved frame slots, not occupied bytes.

## How Lifetime compares

- **Rust ownership:** Rust exposes ownership and borrowing rules in its source
  language; ordinary Scarp programs use no borrow annotations, lifetime
  parameters, or explicit release commands. Rust still implements the
  interpreter's internal storage safety.
- **Python garbage collection:** Python implementations can use reference
  counting and/or cycle collection; Scarp's current supported values use
  lexical slot retirement plus immutable reference-counted storage. Scarp
  does **not** currently implement collection of cycles.
- **Manual allocation:** Scarp source offers no `free` command for ordinary
  values. Dropping a binding handle cannot invalidate another retained alias.

## Architecture constraints before future features

Closures and escaping references require an explicit capture/escape policy
before introduction. Cyclic user graphs must either be rejected or use a
reviewed collector or cycle-breaking representation. Concurrency and
asynchronous execution will need thread-safe atomic retention semantics,
task-cancellation cleanup, and verified cross-task handoff. Native code and
FFI require explicit ownership contracts across ABI boundaries; a safe
interpreter does not automatically make external pointers safe. Large AI
tensors will need checked view retention, buffer lifetime coordination,
and separate live/peak allocation diagnostics. **These are design
requirements, not currently shipped features.**

## Verification

The `value::lifetime_storage_tests` unit tests use `Arc::downgrade` to
show that empty views do not keep an original buffer alive, while nonempty
views and composite aliases do until their final holder is dropped.
`tests/lifetime_windows.rs` checks Scarp-level behavior and cumulative
numerical quotas. `cargo bench --bench lifetime -- --quick` prints median
time per operation for empty and nonempty views, constructors, and clones.
Run `sh scripts/benchmarks/compare-lifetime.sh BASE_COMMIT` with a
complete Git history to check out the pinned baseline in a temporary worktree,
copy the **same** benchmark source to both revisions, and print a comparison
on the same machine and toolchain. Lifetime feature pull requests named
`feature/lifetime-*` also run this comparison in Linux CI. Comparisons are
indicative short samples, not a promise about RSS or a speedup. The weak
reference checks demonstrate buffer release, **not** total RSS measurement.

### Alpha.20 scalar object functions

The experimental ELF64 function subset gives each activation one bounded
stack frame. Argument slots start initialized. The IR checks both sides of
conditionals and rejects a local read after retirement or before its first
write. Locals and temporaries contain only immediate scalar values. Successful
returns and arithmetic failures share the stack-restoring epilogue. Native
heap values and ownership transfer are future extensions.

### Alpha.2 native direct calls

The native call verifier computes the maximum number of active frames and
their combined stack footprint before emitting an object. Each integer
call receives initialized argument slots. Values live across a call keep
their stack slots, and both success and failure restore the active frames.
The [bounded-entry command](native-object-output.md) checks the caller's
stack budget against that footprint.

### Interpreter terminal self-calls

On a terminal self-call the binding and supplied-argument buffers keep their
fixed frame shape. The evaluator retires its value stack and task stack
individually. A stack with capacity above 128 entries drops its backing
allocation at the transfer, so a wide expression early in the recursion cannot
hold that allocation through later, smaller calls.

In the bounded ELF64 backend, aggregate locals containing integer or Boolean
values live in verified stack lanes. Their components cannot escape through
the integer-only call signature. Replacing a mutable binding copies the
completed value into its own frame lanes, preserving earlier immutable
aliases. Expression temporaries use the existing liveness-based stack and
register planner. Scope retirement ends reads of the local's lanes.
