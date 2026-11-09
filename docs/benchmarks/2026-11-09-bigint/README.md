# BigInt benchmark snapshot — 9 November 2026

Measured on Linux x86-64 using optimized Rust 1.99.1. Source: the BigInt feature
branch based on `d222996be74ad3150417e084c7f59f67713eec9e`, prior to its commit.
The [CSV](interpreter-stable.csv) contains one local run of the reproducible
`benches/bigint.rs` harness, not a cross-host guarantee. Each compiled plan
constructs an N-digit BigInt and a floor(N/2)-digit BigInt before the named
operation. Timings include constructor and interpreter dispatch costs; `format`
prints to a sink. Every workload has one warmup; repetitions are recorded.

| Operand digits | Parse pair | Multiply | General divide |
| --- | --- | --- | --- |
| 100 | 1.16 µs | 1.66 µs | 5.20 µs |
| 1,000 | 6.48 µs | 20.56 µs | 229.60 µs |
| 10,000 | 21.27 µs | 2.30 ms | 23.42 ms |
| 100,000 | 0.395 ms | Not run | Not run |
| 1,000,000 | 3.84 ms | Not run | Not run |

Add/subtract/compare/output are also measured in the CSV. Independent small
samples can vary enough that an operation appears faster than parse alone;
these figures are whole executions and must not be subtracted to infer kernel
cost. RSS is measured for the whole already-built benchmark process with Linux
`resource.getrusage(RUSAGE_CHILDREN)`: **9,472 KiB peak** in this run, excluding
Rust compilation and Python's own process. This is not a per-operation memory
bound or an allocation-count measurement. [Environment metadata](environment.json)
records process CPU time, limits and workload scope.

Reproduce timing with:

```sh
cargo bench --locked --offline --bench bigint -- --large
```

The harness deliberately skips multiplication/general division at 100k/1m
digits. A separate opt-in Python oracle checks exact linear operations at all
six sizes. Regular CI uses the quick benchmark (19/100 digits) and the bounded
304-pair arithmetic oracle. Algorithm optimization and detailed isolated
allocator/RSS profiling remain future work; no external numerical library
comparison was performed in this snapshot.
