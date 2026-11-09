# Bounded arbitrary-precision integers

Status: implemented first runtime milestone on a separate post-alpha.5 feature
branch; not a released language guarantee. See the [reference](../bigint.md)
for the exact implemented APIs, resource cost table and unsupported forms.

## Motivation and compatibility

Keep `integer` as checked signed i64 and `float` as finite binary64. Exact large
whole-number computation needs a distinct `bigint`; silently widening ordinary
integer arithmetic would change overflow behavior and conceal cost. A typed
string constructor fits the existing built-in call grammar without introducing
keywords, lexer tokens, literal ambiguity or platform-dependent precision.

## Representation and arithmetic

Use an explicit sign and `Arc<Vec<u32>>` magnitude in little-endian base 10^9.
The empty magnitude is canonical nonnegative zero; the highest stored limb is
nonzero. Every limb is below the base. Decimal 32-bit limbs were chosen over
binary 2^32 limbs so parsing and printing are linear chunk operations with no
quadratic base-conversion pass or temporary decimal-magnitude buffer.
Multiplication intermediates are at most `BASE^2-1`, safely inside u64.

Addition/subtraction use iterative carry/borrow. Multiplication initially uses
schoolbook accumulation. Division uses one-limb long division when possible,
otherwise signed-magnitude long division with a bounded binary search for each
quotient limb. Quotients truncate toward zero, remainder signs follow the
dividend, and canonicalization removes negative zero. Equality and ordering are
mathematical, not pointer/storage identity. No unsafe code or numerical library
dependency is needed for this initial implementation.

The binary-search division is deliberately easy to audit and has a larger
constant factor than normalized quotient estimation. Karatsuba, Knuth-style
division, binary limbs with faster conversion, and external libraries require
measurement and separate review; their absence is not hidden by the digit cap.

## Language and module boundary

`bigint(string)`, `bigint_from_integer(integer)`, `bigint_to_integer(bigint)` and
`bigint_abs(bigint)` are pure typed built-ins. Inference, annotations, bindings,
mutation, arithmetic, comparisons, control flow, defaults/contracts and typed
function results use existing resolver/evaluator machinery. BigInt is appended
to primitive IDs to preserve the original eight identities. Values are shared
immutably; sign changes and copies are cheap, while arithmetic creates new
quota-accounted storage. There is no implicit float/integer conversion.

Exported runtime functions may construct/return BigInts. Constant initializers
still prohibit calls, so BigInt folding/exported BigInt constants remain
unsupported. A future constant milestone must use the compiler's own cumulative
constant work/allocation caps and source ownership, rather than executing an
unbudgeted constructor during resolution. Native backends reject the new type;
the bounded interpreter remains the execution path.

## Resource and failure contract

Prepay work before arithmetic; account for every newly reserved limb buffer,
including temporary trial/quotient/remainder capacity. Use fallible Vec
reservation. Clamp host digit limits to the one-million-digit implementation
ceiling; defaults remain 10,000 digits and 8 MiB cumulative limb payload. Input
leading zeros count toward the input limit, which is checked before scanning.
Arithmetic checks normalized result digits, and multiplication rejects a
provably oversized nonzero product before reserving its result. Per-operation
work uses saturating cost calculations, so overflow cannot make work free.

The per-run Budget owns mutable counters; compiled plans and BigInt values do
not. Repeated/concurrent execution starts with fresh counters. Comparisons and
output formatting are charged as well as arithmetic. Output capture counts
each selected logical payload, even when Arc shares storage. Validation and
execution traverse the same selected path and charge the same budgets before
any program output is replayed. Existing I/O failure semantics are unchanged.

Use dedicated E0051 (syntax), E0052 (digit/payload quota), E0053 (reservation)
diagnostics, existing E0008 (zero divisor), E0044 (lossy conversion) and E0017
(work). Diagnostics retain argument/operator spans and cross-module call sites.
The quotas are not a physical heap cap: small Arc metadata allocations retain
the project's normal allocator behavior, as do source/compiler allocations.

## Verification and measured scope

Rust tests cover base boundaries, canonical zero, signs, carry/borrow, uneven
multiplication, signed quotient/remainder identities, conversion edges, type
rejection, mutation, loops, pure functions, modules, selected-path errors,
Unicode locations, isolated runs, CLI exit codes and budget boundaries.
Deterministic i128 tests and Python big integers provide independent arithmetic
oracles. Controlled large tests exercise linear operations at 19/100/1k/10k/
100k/1m digits; no million-digit quadratic stress belongs in normal CI.

The benchmark times compiled interpreter plans with realistic constructor and
output costs. Results depend on toolchain/host; no isolated arithmetic,
cryptographic constant-time, allocator-count or peak-RSS guarantee is implied.
All supported-platform CI and the Rust 1.74 minimum must pass before integration.
This PR does not change the frozen release candidate, publish tags, or authorize
release publication. Ecosystem sites/extensions should adopt the feature only
after a specific compiler revision is integrated and compatibility-tested.

## Future decimal design (proposal only)

Keep decimal computation separate: a signed BigInt coefficient plus an explicit
base-10 exponent, canonical zero, bounded coefficient/exponent/scale alignment,
caller-selected precision and rounding policy. Finite exact decimal addition
and multiplication differ from division, which needs a documented rounding
context for recurring expansions. Reject silent BigInt/float coercion and
specify exact parsing/conversion first. Define resource charges for alignment,
normalization and output before any runtime implementation. This milestone
adds no decimal keyword, value, syntax, arithmetic or financial guarantees.
