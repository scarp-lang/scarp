# Fixed-width integers

Scarp defines signed `i8`, `i16`, `i32`, `i64`, `i128` and unsigned
`u8`, `u16`, `u32`, `u64`, `u128`. The word-sized types `isize` and
`usize` use the 64-bit Scarp target width. Types are distinct even when
they have the same width. Plain integer literals and `integer` are signed
64-bit. Use explicit conversions to initialize width-qualified values.

```scarp
var flags: u8 = to_u8(129);
flags <<= 1;
bind counter: u64 = to_u64(bigint("18446744073709551615"));
print(flags, counter); // 2 18446744073709551615
```

`to_i8` through `to_u128`, `to_isize` and `to_usize` accept an `integer`,
a `bigint`, or a fixed-width value and require exact representability. `to_integer`
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

## Explicit arithmetic and bit operations

Use `wrapping_add`, `wrapping_sub` and `wrapping_mul` for a result modulo
the declared width. `saturating_add`, `saturating_sub` and
`saturating_mul` clamp a mathematical result to the type's endpoints.
`overflowing_add`, `overflowing_sub` and `overflowing_mul` return a tuple
of the wrapped value and a Boolean indicating overflow. Both operands must
have the same width-qualified type.

The `bits_count_ones`, `bits_leading_zeros` and `bits_trailing_zeros`
overloads return ordinary integers. `bits_reverse`, `bits_swap_bytes`,
`bits_rotate_left` and `bits_rotate_right` preserve the input type.
Rotation counts have the same validity range as shift counts. Each bit
operation uses the declared width, including for negative operands and zero.

## Native x86-64 subset

Bounded native functions can use signed and unsigned 8-, 16-, 32- and
64-bit locals and aggregate lanes, including machine-word types. Explicit
casts between these types and `integer` are checked. Checked operators,
shifts, comparisons and compound updates use native instructions with the
same range and signedness rules as the interpreter. The external function
ABI continues to use ordinary `integer` parameters and results.

Native lowering rejects 128-bit values, BigInt conversions, and calls to
the explicit arithmetic and bit-operation helpers. Those operations use
the interpreter. Native conversion failure is status 14 in the checked
machine ABI. Arithmetic, division and shift failures retain statuses 1 to 3.

Widths 8, 16, 32, 64 and 128 use 1, 2, 4, 8 and 16 bytes, respectively.
The x86-64 layout query reports natural alignment. The type registry
predefines 22 primitives. Embedders should include those entries when
setting `TypeLimits.max_types`.
