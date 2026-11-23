//! Explicit cross-invocation accounting for Rust-embedded Scarp callables.
//!
//! ExecutionOptions limits one invocation. A long-lived host loop can use a
//! HostInvocationLedger to apply one additional cumulative quota, with failed
//! invocations charged by the actual interpreter receipt. No ambient global
//! state, threads, clocks or timers are used.

use std::{error::Error, fmt, io};

use crate::{
    input::InputLimits,
    limits::{ExecutionOptions, ExecutionUsage},
    modules::{ExportedCallableSlot, ExportedSlotInvokeError},
    resolver::{CallableSlot, CallableSlotInvokeError, HostArguments},
    value::Value,
};

/// Application-owned cumulative quotas. The execution options are total
/// allowances across all attempts, except depth, live-slot peaks and BigInt
/// decimal-digit ceiling, which remain *per invocation*. For example,
/// execution.limits.max_steps is the TOTAL work quota for this ledger.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HostInvocationCaps {
    /// Maximum attempted invocations, including preflight failures and
    /// unbound selections that charge no interpreter resources.
    pub max_attempts: u64,
    /// Total work/allocations/calls/prints/capture charges; structural limits
    /// (max_call_depth, max_live_slots and max_bigint_digits) apply per call.
    pub execution: ExecutionOptions,
    /// Total read attempts and raw consumed bytes across reader-enabled
    /// invocations. max_line_bytes remains a per-read safety limit.
    pub input: InputLimits,
}

impl Default for HostInvocationCaps {
    fn default() -> Self {
        Self {
            max_attempts: 1024,
            execution: ExecutionOptions::default(),
            input: InputLimits::default(),
        }
    }
}

/// Usage accounting is owned by one mutable host ledger. This intentionally
/// is not Clone, avoiding accidental reset by copying a live accounting object.
/// The host decides whether the ledger is shared among clients/sessions.
#[derive(Debug)]
pub struct HostInvocationLedger {
    caps: HostInvocationCaps,
    attempts: u64,
    spent: ExecutionUsage,
}

/// A call was denied BEFORE interpreter execution, with no input/output
/// effects. Only initial-call work, call and attempt caps can definitively
/// prevent any valid invocation; zero output or input quotas still permit a
/// pure function that does not perform those operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostQuotaResource {
    Attempts,
    Steps,
    Calls,
}

impl fmt::Display for HostQuotaResource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Attempts => f.write_str("host invocation attempts"),
            Self::Steps => f.write_str("cumulative interpreter steps"),
            Self::Calls => f.write_str("cumulative function calls"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HostQuotaExceeded {
    pub resource: HostQuotaResource,
}
impl fmt::Display for HostQuotaExceeded {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} quota exhausted", self.resource)
    }
}
impl Error for HostQuotaExceeded {}

/// Quota denial is not a runtime diagnostic: a quota-denied invocation was
/// never started. Runtime failures preserve the source-aware underlying type.
#[derive(Debug)]
pub enum HostInvocationError<E> {
    Quota(HostQuotaExceeded),
    Invocation(E),
}
impl<E: fmt::Display> fmt::Display for HostInvocationError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Quota(error) => error.fmt(f),
            Self::Invocation(error) => error.fmt(f),
        }
    }
}
impl<E: Error + 'static> Error for HostInvocationError<E> {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Quota(error) => Some(error),
            Self::Invocation(error) => Some(error),
        }
    }
}

impl HostInvocationLedger {
    pub fn new(caps: HostInvocationCaps) -> Self {
        Self {
            caps,
            attempts: 0,
            spent: ExecutionUsage::default(),
        }
    }

    pub fn caps(&self) -> HostInvocationCaps {
        self.caps
    }

    pub fn attempts(&self) -> u64 {
        self.attempts
    }

    /// Charges include failed executions. An unbound/preflight error normally
    /// charges zero runtime resources but still costs one host attempt.
    pub fn spent(&self) -> ExecutionUsage {
        self.spent
    }

    /// Return remaining quotas (not a prediction of how much work a call
    /// will consume). The non-additive structural limits are unchanged.
    pub fn remaining_execution(&self) -> ExecutionOptions {
        let mut result = self.caps.execution;
        result.limits.max_steps = result.limits.max_steps.saturating_sub(self.spent.steps);
        result.limits.max_prints = result.limits.max_prints.saturating_sub(self.spent.prints);
        result.max_calls = result.max_calls.saturating_sub(self.spent.calls);
        result.max_capture_bytes = result.max_capture_bytes.saturating_sub(self.spent.capture_bytes);
        result.max_vector_bytes = result.max_vector_bytes.saturating_sub(self.spent.vector_bytes);
        result.max_text_bytes = result.max_text_bytes.saturating_sub(self.spent.text_bytes);
        result.max_tuple_bytes = result.max_tuple_bytes.saturating_sub(self.spent.tuple_bytes);
        result.max_bigint_bytes = result.max_bigint_bytes.saturating_sub(self.spent.bigint_bytes);
        result.max_array_bytes = result.max_array_bytes.saturating_sub(self.spent.array_bytes);
        result
    }

    pub fn remaining_input(&self) -> InputLimits {
        InputLimits {
            max_line_bytes: self.caps.input.max_line_bytes,
            max_total_bytes: self
                .caps
                .input
                .max_total_bytes
                .saturating_sub(self.spent.input_bytes),
            max_reads: self.caps.input.max_reads.saturating_sub(self.spent.input_reads),
        }
    }

    /// Check the global attempt and unavoidable initial-call requirements,
    /// then debit one attempt. An early quota denial does not spend an attempt
    /// or touch program input/output.
    fn begin(&mut self, options: ExecutionOptions) -> Result<ExecutionOptions, HostQuotaExceeded> {
        let quota = |resource| HostQuotaExceeded { resource };
        if self.attempts >= self.caps.max_attempts {
            return Err(quota(HostQuotaResource::Attempts));
        }
        let remaining = self.remaining_execution();
        if remaining.limits.max_steps == 0 {
            return Err(quota(HostQuotaResource::Steps));
        }
        if remaining.max_calls == 0 {
            return Err(quota(HostQuotaResource::Calls));
        }
        self.attempts += 1;
        let mut effective = options;
        effective.limits.max_steps = effective.limits.max_steps.min(remaining.limits.max_steps);
        effective.limits.max_prints = effective.limits.max_prints.min(remaining.limits.max_prints);
        effective.max_calls = effective.max_calls.min(remaining.max_calls);
        effective.max_capture_bytes = effective.max_capture_bytes.min(remaining.max_capture_bytes);
        effective.max_vector_bytes = effective.max_vector_bytes.min(remaining.max_vector_bytes);
        effective.max_text_bytes = effective.max_text_bytes.min(remaining.max_text_bytes);
        effective.max_tuple_bytes = effective.max_tuple_bytes.min(remaining.max_tuple_bytes);
        effective.max_bigint_bytes = effective.max_bigint_bytes.min(remaining.max_bigint_bytes);
        effective.max_array_bytes = effective.max_array_bytes.min(remaining.max_array_bytes);
        // Depth, live slots and per-value BigInt digit limits are deliberately
        // NOT cumulative. They remain constrained by both caller and ledger.
        effective.max_call_depth = effective.max_call_depth.min(remaining.max_call_depth);
        effective.max_live_slots = effective.max_live_slots.min(remaining.max_live_slots);
        effective.max_bigint_digits = effective.max_bigint_digits.min(remaining.max_bigint_digits);
        Ok(effective)
    }

    fn input_for(&self, requested: InputLimits) -> InputLimits {
        let left = self.remaining_input();
        InputLimits {
            max_line_bytes: requested.max_line_bytes.min(left.max_line_bytes),
            max_total_bytes: requested.max_total_bytes.min(left.max_total_bytes),
            max_reads: requested.max_reads.min(left.max_reads),
        }
    }

    /// Sum actual interpreter charges with overflow protection; limits are
    /// monotonically decreasing, including when the underlying call failed.
    fn charge(&mut self, usage: ExecutionUsage) {
        let charged = &mut self.spent;
        charged.steps = charged.steps.saturating_add(usage.steps);
        charged.calls = charged.calls.saturating_add(usage.calls);
        charged.prints = charged.prints.saturating_add(usage.prints);
        charged.capture_bytes = charged.capture_bytes.saturating_add(usage.capture_bytes);
        charged.vector_bytes = charged.vector_bytes.saturating_add(usage.vector_bytes);
        charged.text_bytes = charged.text_bytes.saturating_add(usage.text_bytes);
        charged.tuple_bytes = charged.tuple_bytes.saturating_add(usage.tuple_bytes);
        charged.bigint_bytes = charged.bigint_bytes.saturating_add(usage.bigint_bytes);
        charged.array_bytes = charged.array_bytes.saturating_add(usage.array_bytes);
        charged.input_reads = charged.input_reads.saturating_add(usage.input_reads);
        charged.input_bytes = charged.input_bytes.saturating_add(usage.input_bytes);
    }

    /// Invoke one trusted snapshot-bound host callable slot with the ledger's
    /// global budget. Every attempt counts; failures debit real execution.
    pub fn invoke_slot(
        &mut self,
        slot: &CallableSlot,
        arguments: HostArguments,
        writers: (&mut impl io::Write, &mut impl io::Write),
        options: ExecutionOptions,
    ) -> Result<Value, HostInvocationError<CallableSlotInvokeError>> {
        let effective = self.begin(options).map_err(HostInvocationError::Quota)?;
        let mut usage = ExecutionUsage::default();
        let result = slot.invoke_with_usage_and_options(
            arguments,
            writers.0,
            writers.1,
            effective,
            &mut usage,
        );
        self.charge(usage);
        result.map_err(HostInvocationError::Invocation)
    }

    /// Reader-enabled trusted-host entry; read quotas also accumulate across
    /// calls, including EOF and other failed read attempts.
    pub fn invoke_slot_with_input(
        &mut self,
        slot: &CallableSlot,
        arguments: HostArguments,
        input: &mut impl io::BufRead,
        writers: (&mut impl io::Write, &mut impl io::Write),
        options: ExecutionOptions,
        input_limits: InputLimits,
    ) -> Result<Value, HostInvocationError<CallableSlotInvokeError>> {
        let effective = self.begin(options).map_err(HostInvocationError::Quota)?;
        let effective_input = self.input_for(input_limits);
        let mut usage = ExecutionUsage::default();
        let result = slot.invoke_with_input_and_usage_and_options(
            arguments, input, writers, effective, effective_input, &mut usage,
        );
        self.charge(usage);
        result.map_err(HostInvocationError::Invocation)
    }

    /// Export-checked module entry; selection and source-map policy continue
    /// to be enforced by the ExportedCallableSlot, not this meter.
    pub fn invoke_exported(
        &mut self,
        slot: &ExportedCallableSlot,
        arguments: HostArguments,
        writers: (&mut impl io::Write, &mut impl io::Write),
        options: ExecutionOptions,
    ) -> Result<Value, HostInvocationError<ExportedSlotInvokeError>> {
        let effective = self.begin(options).map_err(HostInvocationError::Quota)?;
        let mut usage = ExecutionUsage::default();
        let result = slot.invoke_with_usage_and_options(
            arguments,
            writers.0,
            writers.1,
            effective,
            &mut usage,
        );
        self.charge(usage);
        result.map_err(HostInvocationError::Invocation)
    }

    pub fn invoke_exported_with_input(
        &mut self,
        slot: &ExportedCallableSlot,
        arguments: HostArguments,
        input: &mut impl io::BufRead,
        writers: (&mut impl io::Write, &mut impl io::Write),
        options: ExecutionOptions,
        input_limits: InputLimits,
    ) -> Result<Value, HostInvocationError<ExportedSlotInvokeError>> {
        let effective = self.begin(options).map_err(HostInvocationError::Quota)?;
        let effective_input = self.input_for(input_limits);
        let mut usage = ExecutionUsage::default();
        let result = slot.invoke_with_input_and_usage_and_options(
            arguments, input, writers, effective, effective_input, &mut usage,
        );
        self.charge(usage);
        result.map_err(HostInvocationError::Invocation)
    }
}
