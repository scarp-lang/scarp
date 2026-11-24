//! Experimental compiler-owned ELF64 object emission.
//!
//! A bounded sequence of pure integer binding initializers is compiled into a
//! freestanding, side-effect-free SysV AMD64 `scarp_scalar_kernel() -> i64`
//! symbol. The API returns complete bytes; it never opens or overwrites files.
//! The parameterized checked ABI also supports pure scalar functions from
//! resolved Scarp source. All entry points return relocatable object bytes.

use crate::diagnostics::{Diagnostic, Span};

/// Emit a deterministic ELF64 relocatable object from integer bindings.
///
/// Every source operation must be a scalar integer binding initialized by
/// literals, grouping, earlier resolved integer names, bitwise NOT, AND, OR
/// or XOR. The returned i64 is the last binding's value, NOT Scarp's program
/// exit status or printed output. Runtime-failing arithmetic, calls, modules,
/// loops, updates, output and floats reject with E0049. This API remains
/// experimental and has no stable ABI or user-facing native CLI command.
pub fn emit_elf64_integer_kernel(source: &str) -> Result<Vec<u8>, Diagnostic> {
    crate::scalar_ir::emit_integer_kernel(source)
}

/// A checked integer kernel and its file-local UTF-8 failure spans.
#[derive(Debug, PartialEq, Eq)]
pub struct CheckedElf64Kernel {
    pub object: Vec<u8>,
    /// On failure, the returned `span_id` indexes this table from zero.
    pub failure_spans: Vec<Span>,
}

/// Runtime ABI status for signed integer arithmetic overflow.
pub const CHECKED_KERNEL_INTEGER_OVERFLOW: u32 = 1;

/// Runtime ABI status for a zero divisor in signed division or remainder.
pub const CHECKED_KERNEL_DIVISION_BY_ZERO: u32 = 2;

/// Runtime ABI status for an integer shift count outside 0 through 63.
pub const CHECKED_KERNEL_INVALID_SHIFT: u32 = 3;

/// Emit a deterministic, freestanding x86-64 ELF64 checked integer kernel.
///
/// The distinct SysV AMD64 symbol `scarp_checked_scalar_kernel` takes no
/// arguments and returns a C struct `{ int64_t value; uint32_t status;
/// uint32_t span_id; }`. Success returns the last binding value, status zero
/// and span ID zero. Failure returns zero as the value, a nonzero status and
/// the index of the operator in `failure_spans`.
///
/// The linear binding subset adds signed addition, subtraction, multiplication,
/// negation, division, remainder and fixed-width shifts to the bitwise kernel.
/// Configurable interpreter execution budgets are outside this experimental ABI.
pub fn emit_elf64_checked_integer_kernel(source: &str) -> Result<CheckedElf64Kernel, Diagnostic> {
    crate::scalar_ir::emit_checked_integer_kernel(source)
}

/// Emit a freestanding Linux x86-64 ELF64 object for one pure Scarp function.
///
/// The experimental SysV AMD64 symbol `scarp_checked_integer_function` takes
/// zero to six required `int64_t` arguments, in source parameter order. The
/// caller must declare the exact arity. It returns a C struct
/// `{ int64_t value; uint32_t status; uint32_t span_id; }`. Success sets both
/// status fields to zero. On arithmetic failure, the result is zero and
/// `span_id` indexes the operator's source span in the returned table.
///
/// The function must return an integer on every path. The backend supports
/// integer and Boolean locals, scalar assignments, integer compound
/// assignments, checked arithmetic, bitwise operations, and forward
/// conditionals. It rejects calls, loops, contracts, defaults, other scalar
/// types, aggregates, and top-level execution. Interpreter budgets do not
/// apply to the emitted object.
pub fn emit_elf64_checked_integer_function(source: &str) -> Result<CheckedElf64Kernel, Diagnostic> {
    crate::scalar_ir::emit_checked_integer_function(source)
}
