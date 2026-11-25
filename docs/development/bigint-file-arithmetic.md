# File-backed BigInt addition and subtraction

This phase adds an experimental file backend to
[`scarp bigint`](../reference/cli.md#experimental-file-backed-bigint) and
`scarp::bigint::file`. A calculation can exceed the interpreter's digit and
string caps because operands and results remain in files. The normal Scarp
`bigint` value continues to use its in-memory representation and execution
budgets. The compiler's [BigInt milestone](https://github.com/scarp-lang/scarp/milestone/3)
tracks the rest of the work.

## Run it

```sh
printf '%s' '999999999999999999999' > left.txt
printf '%s' '2' > right.txt
scarp bigint add --stats left.txt right.txt sum.txt
scarp bigint sub left.txt right.txt difference.txt
```

`sum.txt` contains `1000000000000000000001`. `difference.txt` contains
`999999999999999999997`. Both contain just the number, with no newline.
The equivalent source operations use the existing syntax.

```scarp
bind a = bigint("999999999999999999999");
bind b = bigint("2");
print(a + b, a - b);
```

File inputs follow `[+-]?[0-9]+`. `-000` and `+0` normalize to `0`. Signs
and leading zeros count toward input bytes, and a minus sign counts toward
output bytes. Reusing one file as both operands is supported and counts its
length twice toward the input budget.

| Option | Default | What it bounds |
| --- | ---: | --- |
| `--max-input-bytes` | 67,108,864 | The exact sum of the two input file lengths, checked before the content scan |
| `--max-output-bytes` | 33,554,432 | Final decimal size. A lower bound is checked before arithmetic and the exact size before staging decimal bytes |
| `--max-temp-bytes` | 67,108,864 | Peak logical byte length of scratch plus staged output. The scratch length is computed before arithmetic |
| `--max-buffer-bytes` | 262,144 | Payload of three reused buffers. At least 30 bytes is required and the kernel uses at most 1 MiB to keep each I/O and cancellation interval short |

CLI errors go to stderr. Exit status 2 indicates invalid arguments and 1
indicates an input, resource or filesystem failure. `--stats` writes JSON after
the output has been published. Host applications can use `FileError` to inspect
quota denials, invalid decimal byte offsets and underlying OS errors.

## Arithmetic and file lifecycle

1. Open and validate both regular files in forward chunks. Record the sign
   and count of significant digits. A subtraction of magnitudes with equal
   digit counts compares forward chunks until one differs.
2. Seek backward through the significant digits in aligned groups of nine.
   Convert each group to a `u32` limb and propagate carry or borrow from least
   to most significant. Write the result limbs sequentially to scratch.
3. Verify and consume the scratch blocks backward. Format directly into one
   reused byte buffer and append it to a staged decimal file. Track only the
   most significant nonzero limb and its index during computation, so leading
   zero limbs never reach the decimal output.
4. Sync the staged file, check the input lengths and modification times, and
   publish with an atomic create-if-absent hard link. Close and remove both
   private staging files. A competing destination, including a symlink, is
   preserved. Filesystems that cannot create a hard link report an I/O error.

An addition reserves at most one extra carry limb. Scratch starts with the
eight bytes `SCLIMB01` and two little-endian `u64` geometry fields for chunk
length and total limb count. Records contain little-endian `u32` limbs and
an eight-byte FNV-1a checksum that includes the record index. A checksum
mismatch, invalid limb, short file or invalid header prevents publication.
This format is private scratch and detects accidental corruption. A persistent
format will need a separately reviewed integrity and compatibility contract.

Scratch and staged decimal output share the output directory's filesystem.
Checked `u64` arithmetic bounds file sizes. Local buffer lengths must fit
`usize` and use fallible reservation. The byte limits cover file lengths and
buffer payload. Filesystem metadata, allocator overhead, OS cache and total
RSS have separate costs. A full filesystem or an OS file-size limit produces
an I/O error. The kernel leaves space admission against filesystem quotas to
the host, since a free-space query cannot reserve space against other writers.

Addition and subtraction take linear passes over the inputs and result.
With `k` limbs per reusable block, the three buffers hold at most `22*k + 8`
bytes. Memory for the arithmetic is independent of the operand digit count
once the configured block size is reached. The scratch file holds four bytes
per limb, eight bytes per block and a 24-byte header. Equal magnitudes can
cancel directly and require no scratch file.

Rust hosts can call `calculate_with_progress` and return `ControlFlow::Break`
at a block boundary or before publication. Keep input files and parent paths
stable throughout a calculation. The final metadata check detects common
concurrent changes. Use a trusted directory and choose input snapshots for
shared or mutable data. A hard kill can leave a private scratch directory.
The destination is installed only after the staged file is synced. Power-loss
durability of the new directory entry, orphan recovery and resumption need
separate platform-specific verification.

## Repository baseline and design choices

The audit started at `638dc4c227f34f1c660be66834bcc0cbdb22dec8` on `main`.
The published baseline is [v0.0.7-alpha.1](https://github.com/scarp-lang/scarp/releases/tag/v0.0.7-alpha.1),
tagged at `996bd5b83c8cb26e98637563729f67bdacc61cd4`. `Cargo.toml` on `main`
identifies the later alpha.2 candidate.

| Layer | Baseline behavior and constraint |
| --- | --- |
| Types and Lifetime | `integer` is signed i64. `bigint` is an explicit signed type with canonical base-10^9 `u32` limbs in `Arc<Vec<u32>>`. Copies and sign changes share immutable payload. Limb reservations use `try_reserve_exact` and cumulative allocation charges. The runtime keeps Arc metadata on the normal Rust allocator |
| Arithmetic | Linear carry/borrow and comparison, schoolbook multiplication, one-limb long division and a binary-searched quotient limb for larger divisors. `bigint_pow` and `bigint_pow_mod` use squaring with a nonnegative i64 exponent. `0^0` is `1`. Modular exponentiation also charges reduced intermediates and has variable timing |
| Runtime and compiler | Typed pure built-ins, operators and runtime input reuse the interpreter's budget. GCD, LCM, square root, signed bitwise operations, radix conversions and divmod already exist. Module constant initializers and native code generation provide no BigInt evaluation |
| Input and output | The default per-source-file cap and the fixed individual string limit are both 1 MiB. Typed console input defaults to a 64 KiB line and 1 MiB total. Direct decimal formatting is linear. `bigint_to_text` materializes an interpreter string and pays its independent cap. Output capture also has a separate budget |
| Quotas and addressing | Interpreter default is 10,000 digits, with a 1,000,000-digit clamp and an independent 8 MiB cumulative limb budget. Work, capture and text are checked separately. Decimal-to-binary conversions and general division can exhaust work at values far smaller than the digit clamp |
| Parallel work in review | [#389](https://github.com/scarp-lang/scarp/pull/389) proposes adaptive in-memory multiplication, faster powers of ten, more built-ins and a larger clamp. [#390](https://github.com/scarp-lang/scarp/pull/390) adds dense decimal output batching and 100-million/billion-digit interpreter tests. Both use an in-memory representation. The file backend can be reviewed directly against `main` |

The decimal limb format makes this first external-memory kernel a small,
auditable change with linear decimal I/O. Keep it as an independent backend
until native binary conversion, immutable file handles and operation-level
resource estimates have measured crossovers. A common future storage contract
should expose sized read/write blocks and ownership of a temporary workspace.
It should preserve `bigint` equality and signed remainder across backends.
Per-operation lower bounds should reject jobs that cannot fit. Upper bounds
should drive reservations when they are tight enough. A heuristic estimate
should carry its own confidence and units and have no authority to admit a job
past a checked quota.

## Research directions

- GMP selects among schoolbook, Karatsuba, Toom variants and exact FFT-based
  multiplication using tuned thresholds. Rust `num-bigint` provides a safe-Rust
  comparison with schoolbook, unbalanced splitting, Karatsuba and Toom-3. Compare
  both as independent benchmarks around proposed thresholds for balanced and
  uneven operands. GMP's default fatal allocation behavior also needs a process
  or allocation boundary before any host integration. [GMP multiplication](https://gmplib.org/manual/Multiplication-Algorithms) · [GMP allocation](https://gmplib.org/manual/Custom-Allocation) · [`num-bigint` source](https://docs.rs/num-bigint/latest/src/num_bigint/biguint/multiplication.rs.html)
- Reevaluate binary limbs when bitwise operations, division and arithmetic
  throughput dominate decimal I/O. Power-of-two radices permit linear bit
  packing. For decimal, compare grouped conversions and divide-and-conquer
  conversion, measuring computation, workspace and output separately.
  [GMP binary-to-radix](https://gmplib.org/manual/Binary-to-Radix) · [GMP radix-to-binary](https://gmplib.org/manual/Radix-to-Binary) · [Modern Computer Arithmetic](https://members.loria.fr/PZimmermann/mca/mca-cup-0.5.9.pdf)
- For unsigned radix `B` and length `n` and `m` inputs, each convolution
  coefficient is at most `min(n,m) * (B-1)^2`. An exact NTT design needs length
  at least `n+m-1`, a supported root of unity for each distinct modulus, and
  a CRT modulus product strictly above the checked coefficient bound. Carry
  propagation follows reconstruction. A floating transform would need a proved
  rounding bound or exact verification with a fallback. Asymptotic complexity
  alone cannot establish a practical threshold. [GMP's exact FFT](https://gmplib.org/manual/FFT-Multiplication) · [Harvey and van der Hoeven](https://annals.math.princeton.edu/2021/193-2/p04)
- Prototype blocked multiplication after measuring block size, full-file
  passes and scratch amplification. Use an immutable staged generation, a manifest
  with input identity and version, and verified block digests for checkpoint
  work. Test interruption at every commit boundary. For distributed transforms,
  measure full transposes and network traffic before choosing the partition and
  recovery plan. [Aggarwal and Vitter](https://research.ibm.com/publications/the-inputoutput-complexity-of-sorting-and-related-problems) · [SQLite atomic commit](https://sqlite.org/atomiccommit.html) · [FFTW transposed layouts](https://www.fftw.org/fftw3_doc/Transposed-distributions.html)
- For enormous powers, retain exact signed base and exponent in a separate
  symbolic type. Distinguish a checked expansion, an exact modular residue,
  proved bounds and approximate leading digits. Keep probabilistic primality
  distinct from proof. Schedule parallel/GPU work after a serial reference
  exists and check determinism across worker counts and exact carry boundaries.

## Verification

```sh
cargo test --locked --offline --test bigint_files --test native_object_cli
cargo build --locked --offline --release --bin scarp
python3 scripts/benchmarks/bigint-files.py target/release/scarp \
  --case add --digits 100000000 --work-dir /path/to/existing-workspace
python3 scripts/benchmarks/bigint-files.py target/release/scarp \
  --case sub --digits 100000000 --work-dir /path/to/existing-workspace
```

The Python 3.9+ benchmark supplies deterministic dense digits, verifies every
output byte with a separate decimal identity, and reports a SHA-256 digest. Its
phase timings separate input scan, arithmetic plus scratch writes, and output
conversion plus publication. Linux reports the program's post-exec peak RSS
from `/proc/self/status`. The launcher reports its OS child measurement
separately where available. The two-file cases require about 345 MB of working
filesystem space at 100 million digits. `--case double` reads one dense file
as both operands and needs about 2.45 GB at one billion digits. Use `scarp.exe`
on Windows. Rust tests cover signed i128 differential cases, equivalence with
the interpreter, exact quota boundaries, cancellation, corruption and concurrent
publication.

File-backed multiplication, division, modular/exact exponentiation, binary/hex
I/O, automatic spilling and the source-language file value are follow-up work.
Establish those capabilities separately before using dense linear results to
describe the capacity of other operations.
