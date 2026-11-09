# Bounded scalar/text foundations — draft implementation contract

These six pure intrinsics are implemented only on the core-text feature branch.
They are **absent from published v0.0.6-alpha.5**. The Cargo version remains
alpha.5 during development; identify the exact PR/source before using them.
This engineering specification and regression example stay with the compiler.
Public Docs/By Example adoption follows an integrated, compatibility-tested
compiler revision; this PR does not publish learner pages or a release.

| Intrinsic | Exact signature | Result |
| --- | --- | --- |
| `integer_to_text` | `(integer) -> string` | Canonical signed decimal, no leading plus/zeros |
| `float_to_text` | `(float) -> string` | Existing finite-float display form; float marker and signed zero retained |
| `text_to_integer` | `(string) -> integer` | Exact decimal parse with signed i64 range checking |
| `text_to_float` | `(string) -> float` | Explicit finite binary64 parse, rounded like a float literal |
| `text_trim` | `(string) -> string` | Trim leading/trailing Unicode White_Space, preserve interior text |
| `text_replace` | `(string, string, string) -> string` | Replace non-overlapping literal UTF-8 substring matches |

## Decimal and conversion policy

Integer grammar is `[+-]?[0-9]+`. Float grammar is
`[+-]?[0-9]+(\.[0-9]+)?([eE][+-]?[0-9]+)?`; integer-shaped input is accepted
because conversion to float is explicit. Leading zeros and `+` are allowed.
Whitespace, digit separators, radix prefixes, Unicode digits, incomplete
fractions/exponents, NaN and infinities are rejected with E0044 at the argument.
Use `text_trim` explicitly when whitespace is intended to be removed.

Integer parse is exact or fails. Float parse uses the same binary64 rounding
and finite-result policy as existing decimal literals: `0.1` is rounded,
subnormals are preserved when representable, finite underflow can become signed
zero, and overflow is E0044. This is not exact decimal arithmetic. Formatting
then reparsing any existing finite float preserves its bit pattern, including
`-0.0`. Existing `to_float(integer)` still requires exact representability;
`to_integer(float)` still requires an integral in-range value. No implicit
numeric/string conversion or truthiness is introduced.

```scarp
bind count: integer = text_to_integer(text_trim(" 42 "));
bind value: float = text_to_float("1.25e2");
print(integer_to_text(count), float_to_text(value));
```

The exact output is `42 125.0` followed by LF. The larger
[regression example](../examples/text_foundations.srp) has a byte-exact
[stdout fixture](../examples/text_foundations.stdout).

## Unicode string policy

Trimming uses Rust's Unicode scalar whitespace classification, without locale,
case folding or normalization. U+2003 and U+3000 whitespace is trimmed; BOM
U+FEFF and zero-width space U+200B are retained. Combining marks and grapheme
clusters are not split or normalized. An unchanged trim shares the original
immutable value; a changed trim materializes its selected UTF-8 substring.

Replacement uses exact, case-sensitive substrings at valid UTF-8 boundaries,
left to right without overlapping matches. Thus replacing `aa` with `b` in
`aaaaa` produces `bba`. The replacement may be empty. An empty search pattern
is E0034 at the pattern argument; no insertion-at-every-boundary convention is
implied. No match, or an identical pattern/replacement, shares the input value.
Other successful replacements materialize one new logical text payload.
Neither operation mutates input/captured values. No regex or string ordering
operator is introduced.

## Deterministic resource contract

Let `N` be the input text's UTF-8 byte length, `P` the nonempty pattern byte
length, and `R` the materialized result's byte length. Source string creation
and ordinary expression/statement dispatch also retain their existing charges.

| Intrinsic | Native work units, excluding language dispatch | New text payload |
| --- | --- | --- |
| Parse integer/float | `max(N,1)` before scanning | None |
| Format integer/float | 64 before bounded stack formatting, then `max(R,1)` | `R` |
| Trim | `max(N,1)`; changed result additionally `max(R,1)` | 0 if unchanged, else `R` |
| Replace | `max(2*(N*P+N+P),1)` before counting/matching; materialized result adds `max(R,1)` | 0 on shared-result cases, else `R` |

The existing per-value 1,048,576-byte limit and cumulative
`--max-text-bytes`/`ExecutionOptions.max_text_bytes` apply to every new result.
Retirement gives no refund; sharing charges no new payload. Output snapshots
still consume capture allowance even when payload is shared. Size calculations
are checked before fallible reservation/copying; replacement counts matches
before reserving its result and performs no allocation inside the match loop.
Parsing reserves no Scarp text result. Formatting uses a bounded 64-byte stack
buffer before charging/reserving the exact result payload. These are logical
work/payload quotas, not elapsed-time, total allocator-byte or process-RSS caps.

E0017 reports work exhaustion, E0048 excessive/failed text materialization,
E0044 malformed/out-of-range numeric text, E0034 empty replacement patterns,
and existing E0015/E0024 report type/arity errors. All source/runtime/resource
failures preserve empty stdout and stderr. I/O failures retain the existing
writer contract and cannot undo bytes already written.

## Compiler-stage and embedding compatibility

| Stage | Support |
| --- | --- |
| Parser/resolver, `verify` | Existing call syntax and exact fixed signatures; no runtime decimal validation |
| Interpreter, `exec`/`run` and evaluating `check` | All six intrinsics, including functions/defaults/contracts/loops |
| Source modules | Runtime exported functions supported with owning-source diagnostic spans |
| Constant evaluation | Calls remain forbidden, E0037; no accidental compile-time intrinsic execution |
| Rust embedding | Existing `Builtin` and `numeric::evaluate`/`evaluate_with_text_limit` boundaries |
| Experimental C11/ELF64 | Explicit E0049 rejection; no interpreter fallback |

No primitive, AST shape, keyword, runtime option field or diagnostic number is
added. New Builtin variants affect exhaustive host matches, as with earlier
intrinsic additions. Existing primitive identities and numeric rules remain
unchanged. Built-ins keep existing user-function shadowing rules.

## Hindi notes

यह feature अभी draft branch में है; published alpha.5 binary में उपलब्ध नहीं है।
`text_to_integer("42")` string से integer बनाता है और `integer_to_text(42)`
वापस string देता है। Spaces हटाने के लिए `text_trim` अलग से लिखें। हिन्दी अंक,
underscore और hexadecimal text इस decimal conversion में स्वीकार नहीं हैं।
`text_to_float` binary64 literal जैसा rounding करता है; यह exact decimal type
नहीं है। गलत input या limit पार होने पर program का stdout/stderr खाली रहता है।

## Verification and next boundary

Tests cover all i64 boundaries, finite signed-zero/subnormal/extreme floats,
10,000 seeded scalar round trips, malformed text, UTF-8/combining marks/NUL,
replacement overlap, exact work/payload boundaries, excessive expansion,
immutability, functions/contracts, modules, CLI modes/statuses and native rejection.
The repeatable independent Python oracle checks 1,010 scalar/text results and
verify/check parity: `python3 scripts/test_text_foundations.py --scarp target/debug/scarp`.
Character/byte values, general collections, split/join, regex, locale formatting,
arbitrary precision decimal arithmetic and constant intrinsics remain distinct
future milestones, not promises supplied by these six functions.
