//! Deterministic per-call execution budgets, independent of wall-clock timing.

use crate::diagnostics::{Diagnostic, DiagnosticKind, Span};

const DEFAULT_ARRAY_BYTES: usize = 8 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExecutionLimits {
    /// Program instructions and expression tasks, including backward jumps.
    pub max_steps: u64,
    /// Number of selected output statements (including empty writes), also enforced during validation.
    pub max_prints: usize,
}

impl Default for ExecutionLimits {
    fn default() -> Self {
        Self {
            max_steps: 1_000_000,
            max_prints: 100_000,
        }
    }
}

/// Additive interpreter options. Existing two-field ExecutionLimits literals
/// remain valid and old APIs use these defaults for the additional safeguards.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExecutionOptions {
    /// Existing work and selected-output limits, shared by all invocations.
    pub limits: ExecutionLimits,
    /// Active user frames, excluding the file frame.
    pub max_call_depth: usize,
    /// Cumulative selected calls, including defaults and contracts.
    pub max_calls: u64,
    /// Slots in all live file/function frames, including saved loop state.
    pub max_live_slots: usize,
    /// Conservative cumulative storage of output snapshots, including overhead.
    pub max_capture_bytes: usize,
    /// Cumulative newly reserved vector/tile element payload, in bytes.
    /// Sharing an existing immutable vector or tile does not charge again.
    pub max_vector_bytes: usize,
    /// Cumulative newly materialized UTF-8 string payload; sharing does not recharge.
    pub max_text_bytes: usize,
    /// Cumulative immutable tuple member/structural payload, independent of text/vectors.
    pub max_tuple_bytes: usize,
    /// Cumulative immutable fixed-array backing and type payload, independent of other values.
    pub max_array_bytes: usize,
}

impl Default for ExecutionOptions {
    fn default() -> Self {
        Self {
            limits: ExecutionLimits::default(),
            max_call_depth: 256,
            max_calls: 100_000,
            max_live_slots: 1_000_000,
            max_capture_bytes: 16 * 1024 * 1024,
            max_vector_bytes: 8 * 1024 * 1024,
            max_text_bytes: 8 * 1024 * 1024,
            max_tuple_bytes: 8 * 1024 * 1024,
            max_array_bytes: DEFAULT_ARRAY_BYTES,
        }
    }
}

impl From<ExecutionLimits> for ExecutionOptions {
    fn from(limits: ExecutionLimits) -> Self {
        Self {
            limits,
            ..Self::default()
        }
    }
}

pub(crate) struct Budget {
    remaining: u64,
    limit: u64,
    vector_remaining: usize,
    vector_limit: usize,
    text_remaining: usize,
    text_limit: usize,
    tuple_remaining: usize,
    tuple_limit: usize,
    array_remaining: usize,
    array_limit: usize,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct BudgetAmounts {
    steps: u64,
    vector_bytes: usize,
    text_bytes: usize,
    tuple_bytes: usize,
    array_bytes: usize,
}

impl BudgetAmounts {
    pub fn spent(self, after: Self) -> Self {
        Self {
            steps: self.steps - after.steps,
            vector_bytes: self.vector_bytes - after.vector_bytes,
            text_bytes: self.text_bytes - after.text_bytes,
            tuple_bytes: self.tuple_bytes - after.tuple_bytes,
            array_bytes: self.array_bytes - after.array_bytes,
        }
    }
}

impl Budget {
    pub fn new(steps: u64) -> Self {
        Self {
            remaining: steps,
            limit: steps,
            vector_remaining: usize::MAX,
            vector_limit: usize::MAX,
            text_remaining: usize::MAX,
            text_limit: usize::MAX,
            tuple_remaining: 8 * 1024 * 1024,
            tuple_limit: 8 * 1024 * 1024,
            array_remaining: DEFAULT_ARRAY_BYTES,
            array_limit: DEFAULT_ARRAY_BYTES,
        }
    }

    /// Configure a separate cumulative payload-allocation cap. This is not a
    /// physical process-memory cap or a live-heap byte count.
    pub fn with_vector_bytes(mut self, bytes: usize) -> Self {
        self.vector_remaining = bytes;
        self.vector_limit = bytes;
        self
    }

    pub fn charge_vector_allocation(&mut self, bytes: usize, span: Span) -> Result<(), Diagnostic> {
        if bytes > self.vector_remaining {
            return Err(Diagnostic::new(
                DiagnosticKind::VectorBudget,
                span,
                format!(
                    "cumulative vector allocation limit of {} bytes exceeded",
                    self.vector_limit
                ),
            )
            .with_help("reduce vector materialization or raise --max-vector-bytes / ExecutionOptions.max_vector_bytes"));
        }
        self.vector_remaining -= bytes;
        Ok(())
    }

    pub fn with_tuple_bytes(mut self, bytes: usize) -> Self {
        self.tuple_remaining = bytes;
        self.tuple_limit = bytes;
        self
    }
    pub fn charge_tuple_allocation(&mut self, bytes: usize, span: Span) -> Result<(), Diagnostic> {
        if bytes > self.tuple_remaining {
            return Err(Diagnostic::new(DiagnosticKind::TupleAllocation, span, format!("cumulative tuple allocation limit of {} bytes exceeded", self.tuple_limit)).with_help("reduce tuple construction or raise --max-tuple-bytes / ExecutionOptions.max_tuple_bytes"));
        }
        self.tuple_remaining -= bytes;
        Ok(())
    }

    pub fn with_array_bytes(mut self, bytes: usize) -> Self {
        self.array_remaining = bytes;
        self.array_limit = bytes;
        self
    }

    pub fn charge_array_allocation(&mut self, bytes: usize, span: Span) -> Result<(), Diagnostic> {
        self.array_remaining = self.array_remaining.checked_sub(bytes).ok_or_else(|| {
            Diagnostic::new(
                DiagnosticKind::ArrayAllocation,
                span,
                format!(
                    "cumulative fixed array allocation limit of {} bytes exceeded",
                    self.array_limit
                ),
            )
            .with_help("reuse immutable arrays or raise --max-array-bytes / ExecutionOptions.max_array_bytes")
        })?;
        Ok(())
    }

    pub fn with_text_bytes(mut self, bytes: usize) -> Self {
        self.text_remaining = bytes;
        self.text_limit = bytes;
        self
    }

    pub fn charge_text_allocation(&mut self, bytes: usize, span: Span) -> Result<(), Diagnostic> {
        if bytes > crate::value::MAX_TEXT_BYTES {
            return Err(text_construction(
                span,
                "string result exceeds the 1048576-byte per-value limit",
            ));
        }
        if bytes > self.text_remaining {
            return Err(text_construction(
                span,
                format!(
                    "cumulative text allocation limit of {} bytes exceeded",
                    self.text_limit
                ),
            ));
        }
        self.text_remaining -= bytes;
        Ok(())
    }

    pub fn charge(&mut self, amount: u64, span: Span) -> Result<(), Diagnostic> {
        if self.remaining < amount {
            return Err(Diagnostic::new(
                DiagnosticKind::ExecutionLimit,
                span,
                format!("execution step limit of {} exceeded", self.limit),
            )
            .with_help("check loop progress or raise --max-steps / ExecutionLimits.max_steps"));
        }
        self.remaining -= amount;
        Ok(())
    }

    pub fn remaining_amounts(&self) -> BudgetAmounts {
        BudgetAmounts {
            steps: self.remaining,
            vector_bytes: self.vector_remaining,
            text_bytes: self.text_remaining,
            tuple_bytes: self.tuple_remaining,
            array_bytes: self.array_remaining,
        }
    }

    /// Replay work and allocations from a successful evaluation of the same
    /// input. If any allowance is insufficient, leave all budgets untouched so
    /// the caller can run the evaluator and report its first failing span.
    pub fn try_replay(&mut self, spent: BudgetAmounts) -> bool {
        if spent.steps > self.remaining
            || spent.vector_bytes > self.vector_remaining
            || spent.text_bytes > self.text_remaining
            || spent.tuple_bytes > self.tuple_remaining
            || spent.array_bytes > self.array_remaining
        {
            return false;
        }
        self.remaining -= spent.steps;
        self.vector_remaining -= spent.vector_bytes;
        self.text_remaining -= spent.text_bytes;
        self.tuple_remaining -= spent.tuple_bytes;
        self.array_remaining -= spent.array_bytes;
        true
    }
}

pub(crate) fn text_construction(span: Span, message: impl Into<String>) -> Diagnostic {
    Diagnostic::new(DiagnosticKind::TextConstruction, span, message)
        .with_help("use nonnegative repetition counts and smaller strings; raise --max-text-bytes / ExecutionOptions.max_text_bytes for the cumulative payload quota")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replay_charges_array_storage_and_keeps_all_ledgers_on_failure() {
        let span = Span::default();
        let mut evaluated = Budget::new(6).with_array_bytes(8);
        let before = evaluated.remaining_amounts();
        evaluated.charge(3, span).unwrap();
        evaluated.charge_array_allocation(8, span).unwrap();
        let spent = before.spent(evaluated.remaining_amounts());

        let mut exact = Budget::new(3).with_array_bytes(8);
        assert!(exact.try_replay(spent));
        assert_eq!(exact.remaining, 0);
        assert_eq!(exact.array_remaining, 0);

        let mut too_small = Budget::new(3).with_array_bytes(7);
        assert!(!too_small.try_replay(spent));
        assert_eq!(too_small.remaining, 3);
        assert_eq!(too_small.array_remaining, 7);
    }
}
