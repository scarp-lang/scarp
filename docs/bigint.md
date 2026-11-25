# Exact, bounded BigInt integers

BigInt first shipped in [Scarp v0.0.6-alpha.9](https://github.com/scarp-lang/scarp/releases/tag/v0.0.6-alpha.9).
See the [design](proposals/bounded-bigint.md) and [Hindi guide](tutorials/bigint-hi.md).

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

## Advanced operations in development

| Built-in | Result and domain |
| --- | --- |
| `bigint_pow(x, exponent)` | Exact power. The integer exponent must be nonnegative. `0^0` yields `1`. |
| `bigint_pow_big(x, exponent)` | Exact power with a nonnegative BigInt exponent. Returns a resource diagnostic if expansion exceeds the available budget. |
| `bigint_gcd(a, b)` / `bigint_lcm(a, b)` | Nonnegative results for signed inputs. `gcd(0,0)` and `lcm(0,x)` yield zero. |
| `bigint_sqrt(x)` | Largest integer whose square is at most `x`. Requires a nonnegative input. |
| `bigint_bit_length(x)` | Bit width of the magnitude, with zero returning `0`. |
| `bigint_to_text(x)` | Canonical signed decimal text. |
| `bigint_from_radix(text, base)` / `bigint_to_radix(x, base)` | Exact signed conversion for integer bases 2 through 36. Input accepts ASCII digits and letters in either case. Output uses lowercase. |
| `bigint_and(a,b)`, `bigint_or(a,b)`, `bigint_xor(a,b)`, `bigint_not(x)` | Signed infinite two’s-complement semantics. |
| `bigint_shl(x,n)` / `bigint_shr(x,n)` | Exact left shift and sign-extending right shift. Shift counts are nonnegative integers. |
| `bigint_pow_mod(x, exponent, modulus)` | Nonnegative residue for a nonnegative integer exponent and strictly positive BigInt modulus. Uses variable-time arithmetic. |
| `bigint_pow_mod_big(x, exponent, modulus)` | Nonnegative residue for a nonnegative BigInt exponent and strictly positive BigInt modulus. Uses variable-time arithmetic. |
| `bigint_divmod(a,b)` | `tuple<bigint,bigint>` with truncating quotient and dividend-signed remainder. Divisor must be nonzero. |

Radix parsing checks both the original digit count and the result’s decimal
digit count against `--max-bigint-digits`. Radix formatting pays text allocation
and work quotas. Modular multiplication intermediates pay the ordinary BigInt
limits. Run [`examples/bigint_exact.srp`](../examples/bigint_exact.srp) with the
development compiler.

### Powers with BigInt exponents

```scarp
bind exponent = bigint("340282366920938463463374607431768211456");
print(bigint_pow_big(bigint("-1"), exponent)); // 1
print(bigint_pow_mod_big(bigint("-12345678901234567890987654321"),
                        exponent, bigint("4294967291"))); // 3252763948
```

The `integer` exponent APIs still accept signed 64-bit arguments and reject
negative values. The `_big` APIs take a `bigint` exponent. Exact expansion
treats `0^0` as `1` and handles `0`, `1` and `-1` without expanding the exponent.
Power `1` shares the original limbs. For a nontrivial base with `d` decimal
digits and leading digit `p`, the bounded engine checks
`floor(e * (10 * (d - 1) + floor(log10(p^10))) / 10) + 1` as a lower bound on
output digits, using checked integer arithmetic throughout. It checks the
corresponding minimum limb bytes before squaring and continues to charge
all actual intermediate sizes and work. For powers of ten the digit estimate
is exact and the engine constructs the final decimal limbs directly.

The modular API reduces after every product. It handles exponents beyond
`u128` by scanning the existing decimal limbs from most significant to least
significant, updating the residue by `r^(10^9) * x^limb mod modulus` at each step.
This computes the residue without constructing the expanded power or a second
binary exponent. A modulus of `1` gives `0`, including for `0^0`. The ordinary
digit, allocation and work limits still apply to modular intermediates.
Run [the wide exponent example](../examples/bigint_wide_exponents.srp).

## Extended integer utilities (development)

| Built-in | Signature | Contract |
| --- | --- | --- |
| `bigint_digits(n)` | bigint → integer | Decimal magnitude digits; zero has one digit. |
| `bigint_digit_sum(n)` | bigint → integer | Sum of decimal magnitude digits. |
| `bigint_trailing_zeros(n)` | bigint → integer | Decimal trailing zeros of a nonzero magnitude. |
| `bigint_sign(n)` | bigint → integer | -1, 0 or 1. |
| `bigint_is_even(n)`, `bigint_is_odd(n)` | bigint → boolean | Parity, including negative values and zero. |
| `bigint_is_zero(n)`, `bigint_is_positive(n)`, `bigint_is_negative(n)` | bigint → boolean | Canonical signed predicates. |
| `bigint_log2(n)` | bigint → integer | Floor of log2 of the nonzero magnitude. |
| `bigint_factorial(n)` | integer → bigint | Exact factorial for nonnegative integer n; 0! = 1. |
| `bigint_root(n,k)` | bigint, integer → bigint | Floor of the nonnegative kth root; k must be positive. |
| `bigint_is_prime(n)` | bigint → boolean | Exact on successful return; values less than 2 are false. |
| `bigint_is_probable_prime(n)` | bigint → boolean | Deterministic on unsigned 64-bit values; fixed-base strong probable-prime screen above that range. |

```scarp
bind n = bigint_pow(bigint("250"), 1000);
print(bigint_digits(n)); // 2398
print(bigint_factorial(20)); // 2432902008176640000
print(bigint_root(bigint("1000000"), 3)); // 100
print(bigint_is_prime(bigint("97"))); // true
```

Primality up to 2^64-1 uses deterministic Miller–Rabin with seven
established witnesses. Above this bound, exact primality first uses
the probable-prime screen and then performs budgeted trial division if
necessary. A true probable-prime result above the unsigned 64-bit range
is not a primality proof or a cryptographic assurance; bases are fixed,
not randomly selected. Exact primality may report a resource diagnostic
rather than completing a very large proof. All intermediate products,
scratch and work obey existing quotas. These algorithms are not
constant-time.

Factorial uses a balanced product tree. The nth root uses binary search
with bounded exponentiation comparisons and the existing square-root
kernel for degree two. Zero is outside the domain of log2 and trailing
zeros; negative radicands are rejected. Digit count and sign/parity
predicates do not format a string. Digit sum is linear in decimal digits.

```sh
cargo test --locked --offline --test bigint_utilities
```

## Sequences, combinatorics and additional number theory (development)

| Built-in | Signature | Contract |
| --- | --- | --- |
| `bigint_fibonacci(n)` | integer → bigint | F(0)=0, F(1)=1; nonnegative index. |
| `bigint_binomial(n,k)` | integer, integer → bigint | Exact combinations for 0 <= k <= n. |
| `bigint_permutation(n,k)` | integer, integer → bigint | Ordered selections for 0 <= k <= n. |
| `bigint_next_prime(n)` | bigint → bigint | Smallest proven prime strictly greater than n. |
| `bigint_is_perfect_square(n)` | bigint → boolean | True for zero and nonnegative perfect squares; false for negatives. |
| `bigint_is_power_of_two(n)` | bigint → boolean | True only for positive integer powers of two, including 1. |
| `bigint_reverse(n)` | bigint → bigint | Reverse magnitude digits; preserve sign and normalize leading zeros. |
| `bigint_is_palindrome(n)` | bigint → boolean | Nonnegative canonical decimal palindrome; negatives are false. |
| `bigint_min(a,b)`, `bigint_max(a,b)` | bigint, bigint → bigint | Signed ordering, returning a shared immutable operand. |
| `bigint_mod_inverse(a,m)` | bigint, bigint → bigint | Least nonnegative inverse; m > 1 and gcd(a,m)=1 required. |

```scarp
print(bigint_fibonacci(100)); // 354224848179261915075
print(bigint_binomial(10, 3)); // 120
print(bigint_permutation(10, 3)); // 720
print(bigint_next_prime(bigint("100"))); // 101
print(bigint_mod_inverse(bigint("3"), bigint("11"))); // 4
```

Fibonacci uses fast doubling, combinations cancel common factors before
multiplication, and permutations use a balanced product tree. Prime search
uses exact primality rather than silently accepting probable primes. Decimal
reversal works directly on limbs without materializing a string. Modular
inverse uses extended Euclid; this variable-time routine is not a
constant-time cryptographic primitive. All operations remain subject to
work, digit and cumulative allocation quotas, including intermediates.
For very large prime candidates, a resource diagnostic is possible.

```sh
cargo test --locked --offline --test bigint_sequences_extended
```

## Resource limits

| Resource | Default | Contract |
| --- | --- | --- |
| `--max-bigint-digits` / `ExecutionOptions.max_bigint_digits` | 10,000 | Raw input digits including leading zeros and normalized result digits; sign excluded, zero needs one digit |
| Implementation digit ceiling | 1,000,000,000 on 64-bit; 10,000,000 otherwise | CLI rejects larger requested limits; embedding limits are clamped to the platform ceiling |
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
The algorithms use dependency-free safe Rust with checked buffer-byte
calculations. Large balanced multiplication uses size-decreasing recursion;
individual schoolbook and division kernels do not allocate inside their loops.

| Operation | Algorithm / complexity | Prepaid kernel work (excluding dispatch) |
| --- | --- | --- |
| Parse | Decimal chunks, O(d) | max(d, 1) |
| i64 construction | At most three limbs | 3 |
| Add/subtract | Carry/borrow, O(max(n,m)) | 3*max(n,m)+1 |
| Multiply | Schoolbook for small/unbalanced products; Karatsuba for balanced products with at least 64 limbs per operand; low zero limbs factored out | Schoolbook n*m+n+m+1; larger products also prepay scanning, recursive work and linear recombination |
| General divide/remainder | Binary search per quotient digit, O((n-m+1)*m*log2(10^9)) | max(n,m), then 64*(n-m+1)*m+4*n |
| One-limb divisor | High-to-low long division, O(n) | max(n,m), then n |
| Compare | Signed lexicographic magnitude, O(max(n,m)) worst case | max(n,m,1) |
| Print | Direct decimal chunks, O(d), no conversion buffer | d |

For a one-limb divisor, `/` reserves only the quotient buffer and `%` reserves
one result limb for a nonzero remainder. General division also reserves two
scratch buffers for the current remainder and trial product. Both operators
prepay the same work. Capacity charges can
exceed the normalized result's payload (for example an addition's carry limb).
Actual result digits are checked after normalization. Work counts
are deterministic implementation costs, not CPU instruction counts or timing
guarantees. Large operations may be refused even when values fit the digit cap. Balanced
multiplication uses Karatsuba's three recursive products (approximately
O(n^1.585) for comparable dense operands); low zero limbs are removed before
multiplication and restored in a budgeted buffer. Division remains quadratic.
No claim is made about practical million-digit general multiplication or division.

## Larger exact powers

The default limits are unchanged. The opt-in ceiling is one billion decimal digits on 64-bit targets and ten million on smaller address-space targets. To calculate a 23,980-digit result, put
`print(bigint_pow(bigint("250"),10000));` in `large.srp` and run:

```sh
scarp exec --max-bigint-digits 100000 --max-bigint-bytes 67108864 --max-steps 100000000 large.srp
```

Powers of ten use direct decimal-limb construction rather than repeated
multiplication, including negative bases whose magnitude is a power of ten.
For an opt-in ten-million-digit result, use `bigint_pow(bigint("10"),9999999)`
and print the BigInt directly:

```sh
scarp exec --max-bigint-digits 10000000 --max-bigint-bytes 67108864 --max-steps 100000000 large.srp
```

The default capture budget permits a single ten-million-digit output, but
`bigint_to_text` still obeys the separate one-MiB per-string limit. This is not
a guarantee for arbitrary dense ten-million-digit powers. The integration
suite includes a 100,000-digit decimal power, a result beyond the former
one-million-digit ceiling, independent exact-power reference comparisons and
opt-in million- and ten-million-digit output cases:

```sh
cargo test --locked --offline --test bigint_scalable
cargo test --locked --offline --test bigint_scalable million_digit_decimal_power_is_exact -- --ignored
cargo test --locked --offline --test bigint_scalable ten_million_digit_decimal_power_is_exact -- --ignored
```

### Billion-digit experimental stress case (64-bit only)

For `bigint_pow(bigint("10"),999999999)`, the direct decimal construction
requires 111,111,112 u32 limbs (444,444,448 bytes, approximately 424 MiB),
plus process overhead. Printed output is one billion decimal bytes plus a
newline. Do not render this in an interactive terminal; use a file or a
counting writer. Ensure adequate RAM and disk space first.

```sh
scarp exec --max-bigint-digits 1000000000 --max-bigint-bytes 536870912 --max-steps 2000000000 --max-capture-bytes 1100000000 large.srp > large.txt
```

The default output capture quota is too small for this case. Large zero runs
are emitted in fixed-size chunks without constructing a billion-byte string.
The separate one-MiB string cap still applies to `bigint_to_text`. The opt-in
test checks each output byte with a counting writer, rather than storing a
second billion-byte copy. It is intentionally ignored in ordinary CI:

```sh
cargo test --release --locked --offline --test bigint_scalable billion_digit_decimal_power_streams_exact_output -- --ignored
```

The ceiling is not a performance guarantee for arbitrary dense products,
division, bitwise conversion or powers. Work and cumulative allocation limits
continue to apply. The billion-digit test requires explicit high-memory
validation before advertising actual runtime capacity.

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
to isolate arithmetic-only cost or physical peak memory. Further multiplication optimization, faster division and detailed allocator/RSS
profiling remain later optimization work.

Runnable examples: [arithmetic](../examples/bigint_arithmetic.srp),
[Fibonacci/factorial](../examples/bigint_sequences.srp), and
[module functions](../examples/bigint_module.srp), each with exact stdout fixtures.
