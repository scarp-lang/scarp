# Exact, bounded BigInt integers (development milestone)

**Unreleased feature branch, after published alpha.5.** The published
v0.0.6-alpha.5 binaries do not contain BigInt.
Use a compiler built from this PR's exact source; a Cargo version printed as
`0.0.6-alpha.5` alone does not identify its feature set. No new release/tag is
created by this work. See the [design](proposals/bounded-bigint.md) and
[Hindi guide](tutorials/bigint-hi.md).

`integer` remains signed 64-bit, and `float` remains finite binary64. The separate
`bigint` type stores exact signed whole numbers. Its capacity is controlled by
explicit digit, allocation and work limits; it is not unlimited computation.

```scarp
bind number: bigint = bigint("999999999999999999999");
bind next = number + bigint_from_integer(1);
print(next); // 1000000000000000000000
```

| API | Exact signature | Behavior |
| --- | --- | --- |
| `bigint` | `(string) -> bigint` | Parse `[+-]?[0-9]+`; optional sign and ASCII decimal digits only |
| `bigint_from_integer` | `(integer) -> bigint` | Exact conversion of every signed i64, including its minimum |
| `bigint_to_integer` | `(bigint) -> integer` | Exact conversion; E0044 outside the signed i64 range |
| `bigint_abs` | `(bigint) -> bigint` | Nonnegative value sharing immutable magnitude storage |

Leading zeros are accepted. `-000` and `+0` become the same zero. Empty strings,
whitespace, underscores, radix prefixes, Unicode digits, decimal points and
exponents produce E0051. Numeric literals are still checked i64/finite float
literals; no BigInt suffix or oversized implicit literal is introduced.

## Operators and types

Two BigInt operands support `+`, `-`, `*`, `/`, `%`, equality and ordering.
Unary `-` is exact. Quotients truncate toward zero. Remainders have the dividend's
sign (or canonical zero), so `a == (a/b)*b + a%b` for nonzero `b`.

```scarp
bind a = bigint("-100000000000000000001");
bind b = bigint("3");
print(a / b); // -33333333333333333333
print(a % b); // -2
print(bigint_abs(a) > b); // true
```

BigInt never implicitly mixes with `integer` or `float`. Use the explicit
constructors/conversion above; `to_float` and `to_integer` retain their existing
signatures. Bitwise operations and shifts remain i64-only. BigInt-to-float,
decimal arithmetic, arbitrary exponentiation and dedicated BigInt vectors are
not implemented.

`bind`, `var`, same-type assignment, `+=`, `-=`, `*=`, `/=`, `%=` work. Copies
are immutable values: updating one binding cannot change another or a captured
output value. Annotations, function parameters/results, defaults, contracts,
branches and loops use existing syntax and resolution.

```scarp
pure funcra twice(n: bigint) -> bigint {
    return n * bigint_from_integer(2);
}
var n: bigint = bigint("12345678901234567890");
n += bigint("1");
print(twice(n));
```

Exported pure functions may construct/return BigInts across modules. Calls in
module constant initializers remain forbidden: `pub bind n=bigint("123");`
is E0037. Put construction in an exported function. BigInt constant folding
requires a separate compiler-budget design; a `bind` in an entry script is a
runtime binding, not a folded library constant.

## Resource limits

| Resource | Default | Contract |
| --- | --- | --- |
| `--max-bigint-digits` / `ExecutionOptions.max_bigint_digits` | 10,000 | Raw input digits including leading zeros and normalized result digits; sign excluded, zero needs one digit |
| Implementation digit ceiling | 1,000,000 | CLI rejects larger requested limits; embedding limits are clamped to this ceiling |
| `--max-bigint-bytes` / `ExecutionOptions.max_bigint_bytes` | 8,388,608 | Cumulative newly reserved u32 limb payload, including scratch; four bytes per reserved limb |
| `--max-steps` | 1,000,000 | Shared deterministic dispatch, arithmetic and formatting work |
| `--max-capture-bytes` | 16,777,216 | Each output snapshot charges 64 plus max(limb bytes, printed digit/sign bytes) |

Input strings also obey the existing per-string/cumulative text limits and source
module caps. Parsing one million digits is permitted only with suitable limits;
default work/digit limits are intentionally smaller. Zero limits are allowed;
a zero digit limit prevents BigInt construction. Reusing a shared value or
changing its sign does not reserve another magnitude. Retirement does not refund
cumulative allocations. Quotas cover logical payload reservations, not Arc/Vec
metadata, allocator rounding, compiler memory or total RSS. `try_reserve_exact`
handles limb reservation failure, but process-wide allocator failure for small
metadata allocations is not recoverable, consistent with other Scarp values.

```sh
scarp exec --max-bigint-digits 1000000 --max-bigint-bytes 8388608 --max-steps 10000000 large.srp
scarp check --max-bigint-digits 1000000 --max-steps 10000000 large.srp
```

`exec`/`run` execute and print. `check` executes selected paths silently with the
same resource/error semantics. `verify` checks syntax/names/types statically;
it neither validates decimal constructor contents nor accepts runtime budget
flags. Arithmetic, conversion, malformed input and quota failures leave both
program output streams empty. Later I/O failures retain the existing writer
contract and cannot roll back completed writes.

## Algorithms and costs

Let `n`, `m` be base-10^9 limb counts and `d` raw input/printed decimal digits.
The magnitude uses canonical little-endian u32 limbs with a separate sign.
All algorithms are iterative, dependency-free safe Rust with checked buffer-byte
calculations. No allocation occurs inside multiplication or division loops.

| Operation | Algorithm / complexity | Prepaid kernel work (excluding dispatch) |
| --- | --- | --- |
| Parse | Decimal chunks, O(d) | max(d, 1) |
| i64 construction | At most three limbs | 3 |
| Add/subtract | Carry/borrow, O(max(n,m)) | 3*max(n,m)+1 |
| Multiply | Schoolbook, O(n*m) | n*m+n+m+1 |
| General divide/remainder | Binary search per quotient digit, O((n-m+1)*m*log2(10^9)) | max(n,m), then 64*(n-m+1)*m+4*n |
| One-limb divisor | High-to-low long division, O(n) | max(n,m), then n |
| Compare | Signed lexicographic magnitude, O(max(n,m)) worst case | max(n,m,1) |
| Print | Direct decimal chunks, O(d), no conversion buffer | d |

Division computes both quotient and remainder, even when selecting one. The
allocation ledger charges both results and its reusable trial buffer. Capacity
charges can exceed the normalized result's payload (for example an addition's
carry limb); actual result digits are checked after normalization. Work counts
are deterministic implementation costs, not CPU instruction counts or timing
guarantees. Large quadratic operations may be refused even when values fit the
digit cap. No claim is made about practical million-digit general multiplication
or division.

## Rust embedding and validation

Use `numeric::evaluate_with_options` for standalone typed built-ins with the
same work, text, vector and BigInt quotas. Existing `numeric::evaluate` keeps its
historical work-only policy for older kernels; new BigInt calls use the default
BigInt digit/byte limits. `Value::as_bigint()` exposes an immutable `BigInt`
with exact `to_integer`, digit/sign/zero and logical payload inspection.

Existing primitive type IDs are preserved by appending the new type. Rust hosts
with exhaustive `Value`/`PrimitiveType` matches need a BigInt arm, and exhaustive
`ExecutionOptions` literals need the two new fields or `..Default::default()`.
`ExecutionLimits { max_steps, max_prints }` stays unchanged. Native C11 and ELF64
backends reject BigInt with E0049 instead of narrowing it.

```sh
cargo test --locked --offline --test bigints --test bigint_modules_cli
cargo build --locked --offline
python3 scripts/test_bigint.py target/debug/scarp
python3 scripts/test_bigint.py target/debug/scarp --large
cargo bench --locked --offline --bench bigint -- --quick
cargo bench --locked --offline --bench bigint -- --large
```

The seeded Python oracle independently checks signed arithmetic/comparisons.
`--large` runs controlled linear-operation fixtures through one million digits;
it is opt-in and explicitly adjusts Python's own conversion safeguard. Ordinary
CI does not perform million-digit stress tests. Benchmarks compile once and
measure real interpreter execution including constructors; they do not pretend
to isolate arithmetic-only cost or physical peak memory. Faster multiplication,
division and detailed allocator/RSS profiling remain later optimization work.

Runnable examples: [arithmetic](../examples/bigint_arithmetic.srp),
[Fibonacci/factorial](../examples/bigint_sequences.srp), and
[module functions](../examples/bigint_module.srp), each with exact stdout fixtures.
