//! Experimental compiler-owned ELF64 object emission.
//!
//! A single resolved scalar declaration initializer is compiled into a
//! freestanding, side-effect-free SysV AMD64 `scarp_scalar_kernel() -> i64`
//! symbol. The API returns complete bytes; it never opens or overwrites files.
//! This is NOT Scarp's general program execution/printing semantics, a
//! native executable compiler, or an available CLI command.

use crate::diagnostics::Diagnostic;

/// Emit a deterministic ELF64 relocatable object from one integer initializer.
///
/// Supported operators are only literals, grouping, bitwise NOT, AND, OR and
/// XOR. Loads, runtime-failing arithmetic, calls, modules, loops, output,
/// floats and other source statements return E0049. This API is experimental:
/// the ABI and accepted syntax are not stable.
pub fn emit_elf64_integer_kernel(source: &str) -> Result<Vec<u8>, Diagnostic> {
    crate::scalar_ir::emit_integer_kernel(source)
}
