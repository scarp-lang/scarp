//! Experimental compiler-owned ELF64 object emission.
//!
//! A bounded sequence of pure integer binding initializers is compiled into a
//! freestanding, side-effect-free SysV AMD64 `scarp_scalar_kernel() -> i64`
//! symbol. The API returns complete bytes; it never opens or overwrites files.
//! This is NOT Scarp's general program execution/printing semantics, a
//! native executable compiler, or an available CLI command.

use crate::diagnostics::Diagnostic;

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
