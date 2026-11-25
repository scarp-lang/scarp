# BigInt milestone: 100 million decimal digits

This is an engineering acceptance gate for 64-bit interpreter targets, not a
claim that the configured one-billion-digit ceiling is proven. Defaults remain
unchanged. Native backends, WASM and dense-by-dense multiplication are not
covered by this gate.

## Reproduce

Run from the repository root on a 64-bit host with sufficient free RAM:

```sh
cargo test --locked --test bigint_capacity_100m
cargo test --locked --release --test bigint_capacity_100m -- --ignored --test-threads=1
```

Each stress case validates every emitted byte against a closed-form decimal
oracle without keeping a second output string. Tests exercise exact sparse
power construction, signed dense addition/subtraction, small-factor
multiplication and one-limb division. Ordinary tests cover decimal output
chunk boundaries and rejection before allocation when quotas are too small.

The magnitude of a 100-million-digit value requires 11,111,112 base-10^9
u32 limbs (44,444,448 bytes). Tests permit 512 MiB of cumulative limb
reservations and two billion deterministic work units; these are not wall
clock time or RSS guarantees. Run stress cases serially and record compiler,
host, elapsed time and peak RSS with results. The decimal output is 100 MB
plus sign/newline; it is validated by a streaming writer.

## Release gate

- All ordinary tests and formatting/lint checks pass.
- All three 100-million-digit tests pass on a documented 64-bit host.
- Record measured elapsed time and peak RSS, not estimates.
- No changes to default resource limits or separate per-string caps.
- Dense-by-dense multiplication, multi-limb division and arbitrary dense
  exponentiation at this scale require separate benchmarks and algorithm work.
- Do not advertise general 100-million-digit arithmetic based solely on
  sparse powers, linear operations or configured digit ceilings.

This document does not record executed stress results.
