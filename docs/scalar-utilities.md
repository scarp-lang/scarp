# Unicode and fixed-width integer utilities

These functions are available in the development interpreter. They use the
existing `string` and signed 64-bit `integer` types.

| Function | Arguments → result | Behavior |
| --- | --- | --- |
| `text_lower`, `text_upper` | `string → string` | Map each Unicode scalar using Rust's Unicode case tables. A mapping may expand to several scalars. |
| `text_reverse` | `string → string` | Reverse Unicode scalars while retaining valid UTF-8. |
| `text_from_codepoint` | `integer → string` | Create one scalar from 0 through 0x10FFFF, excluding 0xD800 through 0xDFFF. |
| `text_rfind` | `string, string → integer` | Return the scalar index of the last exact substring match, or -1. An empty needle returns `text_len(text)`. |
| `text_count` | `string, string → integer` | Count nonoverlapping exact substring matches from the left. An empty needle counts every scalar boundary, including both ends. |

Case mappings are locale-independent, with no normalization or neighboring
character context. For example, `text_upper("Straße")` returns `"STRASSE"`.
Lowercasing U+0130 produces U+0069 followed by U+0307. Unicode table versions
can vary with the Rust toolchain. Reversal may separate a combining mark
from its base because the unit of reversal is the scalar.

New strings charge their UTF-8 result bytes against the 1 MiB per-value
limit and the cumulative `--max-text-bytes` quota. Text transformations
and searches charge execution work before traversal. The codepoint
constructor reports an invalid value at the argument span.

| Function | Arguments → result | Behavior |
| --- | --- | --- |
| `bits_count_ones`, `bits_leading_zeros`, `bits_trailing_zeros` | `integer → integer` | Inspect all 64 bits, including the sign bit. |
| `bits_reverse`, `bits_swap_bytes` | `integer → integer` | Reverse the order of bits or bytes in the 64-bit representation. |
| `bits_rotate_left`, `bits_rotate_right` | `integer, integer → integer` | Rotate the full bit pattern by 0 through 63 positions. Invalid counts report E0046. |
| `integer_abs`, `integer_sign` | `integer → integer` | Return the checked magnitude or the sign (-1, 0, or 1). |
| `integer_gcd`, `integer_lcm` | `integer, integer → integer` | Use operand magnitudes and return the exact nonnegative result if it fits. Both return zero for `(0, 0)`, and LCM returns zero if either input is zero. |

`integer_abs(-9223372036854775808)` and any GCD or LCM above the signed
maximum raise arithmetic overflow. Bit-pattern results are interpreted as
signed two's-complement integers, so rotating 1 left by 63 yields
`-9223372036854775808`. These operations perform no implicit conversions.

The functions are pure and work in typed function bodies and defaults.
The experimental C11 and ELF64 paths reject these calls. Rust hosts use
`numeric::Builtin` with the typed evaluation API. Exhaustive Rust matches
over `Builtin` must handle the new variants.

Run [the module example](../examples/core_collections.srp) with
`cargo run -- exec examples/core_collections.srp`. Its
[expected output](../examples/core_collections.stdout) is checked by both
the CLI and the Rust embedding regression.
