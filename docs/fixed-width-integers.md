# Fixed-width integers

Scarp defines signed `i8`, `i16`, `i32`, `i64` and unsigned `u8`,
`u16`, `u32`, `u64`. Types are distinct even when they have the same
width. Plain integer literals and the existing `integer` type are signed
64-bit. Use explicit conversions to initialize width-qualified values.

```scarp
var flags: u8 = to_u8(129);
flags <<= 1;
bind counter: u64 = to_u64(bigint("18446744073709551615"));
print(flags, counter); // 2 18446744073709551615
```

`to_i8` through `to_u64` accept an `integer`, a `bigint`, or a
fixed-width value and require exact representability. `to_integer`
converts a width-qualified value back to the legacy type when it fits.
Existing float-to-integer conversion continues to require an exact integral
value. Mixed-width arithmetic and implicit coercions are rejected.

Addition, subtraction, multiplication, division, remainder and unary
negation check the result range. Signed division truncates toward zero.
Division by zero is an error. The minimum signed value divided by or
remaindered with minus one signals overflow. Bitwise operations operate
within the declared width. The right shift count is a legacy `integer`
from zero through width minus one. `<<` discards the shifted-out bits,
`>>` preserves the sign for signed operands, and `>>>` shifts in zeroes.

Layout is target independent for these scalar values. Widths 8, 16, 32
and 64 use 1, 2, 4 and 8 bytes, respectively. The x86-64 layout query
reports natural alignment. The native function emitter diagnoses
width-qualified operations outside its current lowering subset. The type
registry now predefines eighteen primitives. Embedders that set
`TypeLimits.max_types` explicitly should include those entries in the limit.
