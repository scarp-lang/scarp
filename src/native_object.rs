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
/// zero to sixteen required `int64_t` arguments, in source parameter order.
/// The first six use registers and the rest are passed on the caller stack. The
/// caller must declare the exact arity. It returns a C struct
/// `{ int64_t value; uint32_t status; uint32_t span_id; }`. Success sets both
/// status fields to zero. On arithmetic failure, the result is zero and
/// `span_id` indexes the operator's source span in the returned table.
///
/// The function must return an integer on every path. The backend supports
/// integer and Boolean locals, scalar assignments, integer compound
/// assignments, checked arithmetic, bitwise operations, and forward
/// conditionals in statements and scalar expressions. It rejects calls, loops,
/// contracts, defaults, other scalar types, aggregates, and top-level
/// execution. Interpreter budgets do not
/// apply to the emitted object.
pub fn emit_elf64_checked_integer_function(source: &str) -> Result<CheckedElf64Kernel, Diagnostic> {
    crate::scalar_ir::emit_checked_integer_function(source)
}

/// Native work allowance exhausted before executing the indicated IR event.
pub const CHECKED_KERNEL_WORK_LIMIT: u32 = 4;

/// A counted repeat evaluated a negative iteration count.
pub const CHECKED_KERNEL_REPEAT_COUNT: u32 = 5;

/// Emit one pure integer function with a fixed native work allowance per call.
///
/// The distinct experimental Linux x86-64 SysV symbol
/// `scarp_bounded_integer_function` uses the checked integer-function calling
/// convention and supports while and counted repeat loops, break and
/// continue. It charges one
/// unit per visited non-retirement statement and per expression IR Work
/// event. This is a native IR allowance, not interpreter execution accounting.
/// A zero allowance is valid and rejects the first charged event. On
/// exhaustion, status 4 and the event's source span ID are returned.
/// A negative repeat count returns status 5 and the loop source span ID.
/// Calls, contracts and heap-backed values remain unsupported.
pub fn emit_elf64_bounded_integer_function(
    source: &str,
    max_work: u32,
) -> Result<CheckedElf64Kernel, Diagnostic> {
    crate::scalar_ir::emit_bounded_integer_function(source, max_work)
}

/// Emit a bounded Linux x86-64 ELF64 entry and its private direct-call bodies.
///
/// Select a source function by its exact name. The only exported symbol is
/// `scarp_bounded_integer_entry`. Each function is pure and takes zero to
/// sixteen required integer arguments, with an integer result. The first six
/// use registers and the rest use the stack. The external symbol
/// uses the selected function's arity and the checked integer result layout.
/// Calls are statically resolved, nonrecursive, and confined to this object.
/// Named argument expressions are evaluated in written order.
///
/// One native work allowance is initialized at the external entry and debited
/// by every active function. Callee errors return the original status and
/// source span ID. Compilation rejects over 32 functions, a call chain deeper
/// than 16 active frames, or a chain that needs more than 64 KiB of stack.
pub fn emit_elf64_bounded_integer_entry(
    source: &str,
    entry: &str,
    max_work: u32,
) -> Result<CheckedElf64Kernel, Diagnostic> {
    crate::scalar_ir::emit_bounded_integer_entry(source, entry, max_work)
}
