# BigInt milestone: 100 million decimal digits

This is an engineering acceptance gate for 64-bit interpreter targets, not a
claim that the configured one-billion-digit ceiling is proven. Defaults remain
unchanged. Native backends, WASM and dense-by-dense multiplication are not
covered by this gate.

## Reproduce

Run from the repository root on a 64-bit host with sufficient free RAM:

```sh
cargo test --locked --test bigint_capacity_100m
cargo test --locked --release --test bigint_capacity_100m hundred_million_digit -- --ignored --test-threads=1
```

Each stress case validates every emitted byte against a closed-form decimal
oracle without keeping a second output string. Tests exercise exact sparse
power construction, signed dense addition/subtraction, small-factor
multiplication of both sparse and dense values and one-limb division. Ordinary tests cover decimal output
chunk boundaries and rejection before allocation when quotas are too small.

The magnitude of a 100-million-digit value requires 11,111,112 base-10^9
u32 limbs (44,444,448 bytes). Tests permit 512 MiB of cumulative limb
reservations and two billion deterministic work units; these are not wall
clock time or RSS guarantees. Run stress cases serially and record compiler,
host, elapsed time and peak RSS with results. The decimal output is 100 MB
plus sign/newline; it is validated by a streaming writer.

## Release gate

- All ordinary tests and formatting/lint checks pass.
- All four 100-million-digit tests pass on a documented 64-bit host.
- Record measured elapsed time and peak RSS, not estimates.
- No changes to default resource limits or separate per-string caps.
- Dense-by-dense multiplication, multi-limb division and arbitrary dense
  exponentiation at this scale require separate benchmarks and algorithm work.
- Do not advertise general 100-million-digit arithmetic based solely on
  sparse powers, linear operations or configured digit ceilings.

This document does not record executed stress results.

## Next milestone: one billion digits

Once the four 100-million-digit workloads pass, run the next three stress
cases on a documented 64-bit machine with sufficient free memory:

```sh
cargo test --locked --release --test bigint_capacity_100m one_billion_digit -- --ignored --test-threads=1
```

These reuse the same byte-exact oracle for signed dense addition, multiplication
by one limb and division by one limb. They permit 5 GiB of cumulative limb
allocations, 20 billion work units and a capture quota slightly above one
billion bytes. A billion-digit magnitude needs 444,444,448 bytes of limb
payload; simultaneous operands and outputs increase peak RSS. The parent
branch separately includes a billion-digit sparse power test.

The CI workflow runs the 100-million-digit cases on the milestone PR; its
manual dispatch also runs the billion-digit cases after the smaller cases pass.

## Architecture horizon: one trillion digits

Treat 1,000,000,000,000 decimal digits as an engineering horizon, not a
supported ceiling. At base 10^9, a magnitude at that size requires
111,111,111,112 limbs (444,444,444,448 bytes); decimal output is 1 TB.
Before increasing the ceiling again, audit checked index, offset and byte
arithmetic, cumulative work accounting, capture accounting and all conversions
to narrower types. Introduce storage-independent limb access, bounded scratch
planning and a measured block/transform multiplication path before external
storage. Out-of-core storage needs explicit host opt-in, disk quotas, cleanup
and I/O failure semantics. Keep default and 32-bit limits independently bounded.
Advance through 10 billion and 100 billion only after measured acceptance
at the prior scale. Linear operation success is not evidence of general
dense multiplication or division support.
