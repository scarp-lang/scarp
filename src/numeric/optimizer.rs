//! Experimental immutable, budgeted optimizer state for Rust hosts.
//! Optimizer updates never change their input weights or moment buffers.

use super::{buffer, compensated, domain, length_error};
use crate::{
    diagnostics::{Diagnostic, DiagnosticKind, Span},
    limits::Budget,
    value::{finite_number, FiniteFloat, NumericVector},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OptimizerLimits {
    pub max_steps: u64,
    pub max_vector_bytes: usize,
}

impl OptimizerLimits {
    fn budget(self) -> Budget {
        Budget::new(self.max_steps).with_vector_bytes(self.max_vector_bytes)
    }
}

/// Fixed moment decay factors and denominator stabilizer. A state retains
/// these values because changing beta midstream invalidates bias correction.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AdamConfig {
    pub beta1: f64,
    pub beta2: f64,
    pub epsilon: f64,
}

impl Default for AdamConfig {
    fn default() -> Self {
        Self {
            beta1: 0.9,
            beta2: 0.999,
            epsilon: 1e-8,
        }
    }
}

impl AdamConfig {
    fn validate(self) -> Result<(), Diagnostic> {
        if !(0.0..1.0).contains(&self.beta1) || !(0.0..1.0).contains(&self.beta2) {
            return Err(domain(
                Span::default(),
                "Adam beta factors must be finite and in [0, 1)",
            ));
        }
        if !self.epsilon.is_finite() || self.epsilon <= 0.0 {
            return Err(domain(
                Span::default(),
                "Adam epsilon must be finite and positive",
            ));
        }
        Ok(())
    }
}

/// A snapshot of Adam's first and second moments and completed step count.
/// Cloning this state shares immutable numerical storage.
#[derive(Debug, Clone, PartialEq)]
pub struct AdamState {
    config: AdamConfig,
    step: u64,
    first: NumericVector,
    second: NumericVector,
}

/// Both updated weights and state are returned together, or neither is.
#[derive(Debug, Clone, PartialEq)]
pub struct AdamUpdate {
    pub weights: NumericVector,
    pub state: AdamState,
}

fn checked(value: f64) -> Result<FiniteFloat, Diagnostic> {
    finite_number(value, Span::default())
}

fn finish(values: Vec<FiniteFloat>) -> Result<NumericVector, Diagnostic> {
    NumericVector::from_finite(values).ok_or_else(|| length_error(Span::default()))
}

fn bias_correction(beta: f64, step: u64) -> f64 {
    // Avoid cancellation in 1 - beta^step when beta is very close to one.
    // beta=0 is handled without log(0). step is always positive.
    if beta == 0.0 {
        1.0
    } else {
        -((step as f64) * beta.ln()).exp_m1()
    }
}

fn nonnegative(value: f64, message: &str) -> Result<(), Diagnostic> {
    if !value.is_finite() || value < 0.0 {
        return Err(domain(Span::default(), message));
    }
    Ok(())
}

impl AdamState {
    /// Initialize zero moments. Both moments share a single immutable zero
    /// buffer, charging 8*n logical bytes and max(1,n) work.
    pub fn new(
        length: usize,
        config: AdamConfig,
        limits: OptimizerLimits,
    ) -> Result<Self, Diagnostic> {
        config.validate()?;
        if length > crate::value::MAX_VECTOR_LENGTH {
            return Err(length_error(Span::default()));
        }
        let mut budget = limits.budget();
        budget.charge((length as u64).max(1), Span::default())?;
        let mut zeros = buffer(length, &mut budget, Span::default())?;
        zeros.resize(length, checked(0.0)?);
        let zeros = finish(zeros)?;
        Ok(Self {
            config,
            step: 0,
            first: zeros.clone(),
            second: zeros,
        })
    }

    pub fn config(&self) -> AdamConfig {
        self.config
    }

    pub fn step_count(&self) -> u64 {
        self.step
    }

    pub fn first_moment(&self) -> &NumericVector {
        &self.first
    }

    pub fn second_moment(&self) -> &NumericVector {
        &self.second
    }

    /// Restore a checkpoint, sharing the provided moment vectors.
    /// A step-zero checkpoint must contain zero moments; all second
    /// moments must be nonnegative. The caller owns checkpoint provenance.
    pub fn restore(
        config: AdamConfig,
        step: u64,
        first: NumericVector,
        second: NumericVector,
        limits: OptimizerLimits,
    ) -> Result<Self, Diagnostic> {
        config.validate()?;
        if first.len() != second.len() {
            return Err(length_error(Span::default()));
        }
        let mut budget = limits.budget();
        budget.charge((first.len() as u64).max(1), Span::default())?;
        for (m, v) in first.as_slice().iter().zip(second.as_slice()) {
            if v.get() < 0.0 || (step == 0 && (m.get() != 0.0 || v.get() != 0.0)) {
                return Err(domain(Span::default(), "invalid Adam checkpoint moments"));
            }
        }
        Ok(Self {
            config,
            step,
            first,
            second,
        })
    }

    /// Perform Adam with no weight decay.
    pub fn adam(
        &self,
        weights: &NumericVector,
        gradient: &NumericVector,
        rate: f64,
        limits: OptimizerLimits,
    ) -> Result<AdamUpdate, Diagnostic> {
        self.adamw(weights, gradient, rate, 0.0, limits)
    }

    /// Perform decoupled weight decay and an Adam gradient update.
    /// A zero learning rate still updates moments and the step counter.
    /// Work is max(1, 24*n+8); all three output buffers share 24*n bytes.
    pub fn adamw(
        &self,
        weights: &NumericVector,
        gradient: &NumericVector,
        rate: f64,
        decay: f64,
        limits: OptimizerLimits,
    ) -> Result<AdamUpdate, Diagnostic> {
        let n = weights.len();
        if n != gradient.len() || n != self.first.len() {
            return Err(length_error(Span::default()));
        }
        nonnegative(rate, "Adam learning rate must be finite and nonnegative")?;
        nonnegative(decay, "AdamW decay must be finite and nonnegative")?;
        let step = self.step.checked_add(1).ok_or_else(|| {
            Diagnostic::new(
                DiagnosticKind::ExecutionLimit,
                Span::default(),
                "Adam step counter exhausted",
            )
        })?;
        let span = Span::default();
        let mut budget = limits.budget();
        budget.charge(24 * n as u64 + 8, span)?;
        let mut next_weights = buffer(n, &mut budget, span)?;
        let mut next_first = buffer(n, &mut budget, span)?;
        let mut next_second = buffer(n, &mut budget, span)?;
        let AdamConfig {
            beta1,
            beta2,
            epsilon,
        } = self.config;
        let correction1 = bias_correction(beta1, step);
        let correction2 = bias_correction(beta2, step);
        let decay_factor = checked(rate * decay)?.get();
        let retain = checked(1.0 - decay_factor)?.get();
        let gradient_scale = (1.0 - beta2).sqrt();
        for (((w, g), m), v) in weights
            .as_slice()
            .iter()
            .zip(gradient.as_slice())
            .zip(self.first.as_slice())
            .zip(self.second.as_slice())
        {
            let g = g.get();
            let m = checked(beta1 * m.get() + (1.0 - beta1) * g)?.get();
            // Scale before squaring: a weighted square can be finite when
            // the unweighted gradient square would overflow.
            let scaled = checked(gradient_scale * g)?.get();
            let square = checked(scaled * scaled)?.get();
            let v = checked(beta2 * v.get() + square)?.get();
            let next = if rate == 0.0 {
                w.get()
            } else {
                let corrected_m = checked(m / correction1)?.get();
                // sqrt(v) / sqrt(correction2) avoids overflowing v/correction2
                // when its square root would still be representable.
                let rms = checked(v.sqrt() / correction2.sqrt())?.get();
                let denom = checked(rms + epsilon)?.get();
                let direction = checked(corrected_m / denom)?.get();
                let delta = checked(rate * direction)?.get();
                let retained = checked(w.get() * retain)?.get();
                checked(retained - delta)?.get()
            };
            next_weights.push(checked(next)?);
            next_first.push(checked(m)?);
            next_second.push(checked(v)?);
        }
        Ok(AdamUpdate {
            weights: finish(next_weights)?,
            state: Self {
                config: self.config,
                step,
                first: finish(next_first)?,
                second: finish(next_second)?,
            },
        })
    }
}

/// Clip one gradient vector's L2 norm without computing an overflowing
/// unscaled norm. Unchanged values share storage; a clipped vector uses 8*n
/// logical bytes. All branches prepay max(1, 5*n) work.
pub fn clip_norm(
    gradient: &NumericVector,
    maximum: f64,
    limits: OptimizerLimits,
) -> Result<NumericVector, Diagnostic> {
    nonnegative(
        maximum,
        "gradient norm bound must be finite and nonnegative",
    )?;
    let span = Span::default();
    let mut budget = limits.budget();
    let n = gradient.len();
    budget.charge((5 * n as u64).max(1), span)?;
    let scale = gradient
        .as_slice()
        .iter()
        .map(|value| value.get().abs())
        .fold(0.0, f64::max);
    if scale == 0.0 {
        return Ok(gradient.clone());
    }
    let normalized_squares = gradient.as_slice().iter().map(|value| {
        let normalized = value.get() / scale;
        Ok(normalized * normalized)
    });
    let norm = compensated(normalized_squares, span)?.sqrt();
    // Compare in normalized space. The ratio may overflow to infinity,
    // which simply means the original vector already fits the limit.
    if maximum / scale >= norm {
        return Ok(gradient.clone());
    }
    let target_scale = maximum / norm;
    let mut result = buffer(n, &mut budget, span)?;
    for value in gradient.as_slice() {
        result.push(checked((value.get() / scale) * target_scale)?);
    }
    finish(result)
}
