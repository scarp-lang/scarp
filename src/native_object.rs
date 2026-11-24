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
/// conditionals. It rejects calls, loops, contracts, defaults, other scalar
/// types, aggregates, and top-level execution. Interpreter budgets do not
/// apply to the emitted object.
pub fn emit_elf64_checked_integer_function(source: &str) -> Result<CheckedElf64Kernel, Diagnostic> {
    crate::scalar_ir::emit_checked_integer_function(source)
}

/// Native work allowance exhausted before executing the indicated IR event.
pub const CHECKED_KERNEL_WORK_LIMIT: u32 = 4;

/// Emit one pure integer function with a fixed native work allowance per call.
///
/// The distinct experimental Linux x86-64 SysV symbol
/// `scarp_bounded_integer_function` uses the checked integer-function calling
/// convention and supports while loops, break and continue. It charges one
/// unit per visited non-retirement statement and per expression IR Work
/// event. This is a native IR allowance, not interpreter execution accounting.
/// A zero allowance is valid and rejects the first charged event. On
/// exhaustion, status 4 and the event's source span ID are returned.
/// Calls, counted repeat, contracts and heap-backed values remain unsupported.
pub fn emit_elf64_bounded_integer_function(
    source: &str,
    max_work: u32,
) -> Result<CheckedElf64Kernel, Diagnostic> {
    crate::scalar_ir::emit_bounded_integer_function(source, max_work)
}

/// Maximum callee frame size enforced by the experimental x86-64 encoder.
/// The caller must additionally provide space for the return address,
/// stack-passed arguments, and any surrounding kernel or interrupt frames.
pub const FREESTANDING_X86_64_MAX_FRAME_BYTES: usize = 16 * 1024;

/// Experimental native object target. This is not a complete OS target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FreestandingTarget {
    X86_64None,
}

impl FreestandingTarget {
    /// Exact target identifier for the experimental object API.
    pub const fn name(self) -> &'static str {
        match self {
            Self::X86_64None => "x86_64-none",
        }
    }

    /// Required caller stack alignment immediately before a CALL.
    pub const fn call_stack_alignment(self) -> usize {
        match self {
            Self::X86_64None => 16,
        }
    }

    /// Upper bound on callee-owned stack storage, not total kernel stack use.
    pub const fn max_frame_bytes(self) -> usize {
        match self {
            Self::X86_64None => FREESTANDING_X86_64_MAX_FRAME_BYTES,
        }
    }
}

/// An unrecognized target must never silently select the host architecture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnsupportedFreestandingTarget;

impl std::fmt::Display for UnsupportedFreestandingTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("unsupported experimental target; expected x86_64-none")
    }
}

impl std::error::Error for UnsupportedFreestandingTarget {}

impl std::str::FromStr for FreestandingTarget {
    type Err = UnsupportedFreestandingTarget;

    fn from_str(name: &str) -> Result<Self, Self::Err> {
        match name {
            "x86_64-none" => Ok(Self::X86_64None),
            _ => Err(UnsupportedFreestandingTarget),
        }
    }
}

/// Select the checked scalar function ABI or its bounded-loop counterpart.
/// A finite work allowance is mandatory for the loop-capable variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FreestandingFunctionMode {
    Checked,
    Bounded { max_work: u32 },
}

impl FreestandingFunctionMode {
    /// The global ELF symbol exported by this function mode.
    pub const fn symbol(self) -> &'static str {
        match self {
            Self::Checked => "scarp_checked_integer_function",
            Self::Bounded { .. } => "scarp_bounded_integer_function",
        }
    }
}

/// Target and ABI metadata travel with the existing checked object bytes.
#[derive(Debug, PartialEq, Eq)]
pub struct FreestandingFunction {
    pub target: FreestandingTarget,
    pub mode: FreestandingFunctionMode,
    pub kernel: CheckedElf64Kernel,
}

/// Compile one pure scalar function into an experimental freestanding object.
///
/// Uses the existing verified IR and ELF64 encoder. The caller supplies the
/// boot environment, stack and linker; no executable or boot image is emitted.
/// No filesystem access, host compiler, libc or guest interpreter is involved.
/// The existing checked and bounded APIs retain their exact byte contracts.
pub fn emit_freestanding_function(
    source: &str,
    target: FreestandingTarget,
    mode: FreestandingFunctionMode,
) -> Result<FreestandingFunction, Diagnostic> {
    let kernel = match (target, mode) {
        (FreestandingTarget::X86_64None, FreestandingFunctionMode::Checked) => {
            emit_elf64_checked_integer_function(source)?
        }
        (FreestandingTarget::X86_64None, FreestandingFunctionMode::Bounded { max_work }) => {
            emit_elf64_bounded_integer_function(source, max_work)?
        }
    };
    Ok(FreestandingFunction {
        target,
        mode,
        kernel,
    })
}
