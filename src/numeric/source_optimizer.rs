//! Typed tensor training calls over the reusable optimizer engines.

use super::{
    optimizer::{AdamConfig, AdamState},
    source_tensor::{self, tensor_shape},
    training,
};
use crate::{
    ast::{CallArgument, Expression},
    diagnostics::{Diagnostic, DiagnosticKind, Span},
    evaluator::make_tuple,
    limits::Budget,
    types::SourceType,
    value::{finite_number, NumericTensor, NumericVector, Value, ValueType},
};

#[derive(Debug, Clone, Copy)]
pub(crate) enum TensorOptimizer {
    Sgd,
    AdamInitial,
    Adam,
    AdamW,
}

impl TensorOptimizer {
    pub(crate) fn named(name: &str) -> Option<Self> {
        Some(match name {
            "tensor_sgd" => Self::Sgd,
            "tensor_adam_init" => Self::AdamInitial,
            "tensor_adam" => Self::Adam,
            "tensor_adamw" => Self::AdamW,
            _ => return None,
        })
    }

    pub(crate) fn infer(
        self,
        syntax: &Expression,
        arguments: &[CallArgument],
        types: &[SourceType],
        span: Span,
    ) -> Result<SourceType, Diagnostic> {
        let (arity, name) = match self {
            Self::Sgd => (3, "tensor_sgd"),
            Self::AdamInitial => (4, "tensor_adam_init"),
            Self::Adam => (4, "tensor_adam"),
            Self::AdamW => (5, "tensor_adamw"),
        };
        if arguments.len() != arity || arguments.iter().any(|arg| arg.label.is_some()) {
            return Err(Diagnostic::new(
                DiagnosticKind::CallArguments,
                span,
                format!("{name} requires {arity} positional arguments"),
            ));
        }
        let kind = |position: usize| &types[arguments[position].expression];
        let at = |position: usize| syntax.nodes()[arguments[position].expression].span;
        let weights = kind(0);
        tensor_shape(weights, at(0))?;
        if !matches!(self, Self::AdamInitial) {
            crate::resolver::require_type(kind(1), weights, at(1))?;
        }
        let scalar = ValueType::Float;
        match self {
            Self::Sgd => {
                crate::resolver::require_type(kind(2), scalar, at(2))?;
                Ok(weights.clone())
            }
            Self::AdamInitial => {
                for position in 1..arity {
                    crate::resolver::require_type(kind(position), scalar, at(position))?;
                }
                Ok(state_type(weights))
            }
            Self::Adam | Self::AdamW => {
                let state = state_type(weights);
                crate::resolver::require_type(kind(2), &state, at(2))?;
                for position in 3..arity {
                    crate::resolver::require_type(kind(position), scalar, at(position))?;
                }
                Ok(SourceType::tuple(vec![weights.clone(), state])
                    .expect("bounded Adam result type"))
            }
        }
    }

    pub(crate) fn evaluate(
        self,
        args: &[Value],
        budget: &mut Budget,
        span: Span,
    ) -> Result<Value, Diagnostic> {
        self.run(args, budget, span).map_err(|mut error| {
            if error.span == Span::default() {
                error.span = span;
            }
            error
        })
    }

    fn run(self, args: &[Value], budget: &mut Budget, span: Span) -> Result<Value, Diagnostic> {
        let weights = args[0].as_tensor().expect("typed optimizer weights");
        let shape = weights.shape();
        let wrap = |vector: NumericVector| {
            source_tensor::wrap(
                NumericTensor::from_vector(vector, shape).expect("optimizer preserves shape"),
            )
        };
        match self {
            Self::Sgd => {
                let gradient = args[1].as_tensor().expect("typed gradient");
                let rate = args[2].require_float(span)?;
                Ok(wrap(training::sgd_with_budget(
                    weights.as_vector(),
                    gradient.as_vector(),
                    rate,
                    budget,
                    span,
                    span,
                )?))
            }
            Self::AdamInitial => {
                let config = config(&args[1..4], span)?;
                let state = AdamState::new_with_budget(weights.len(), config, budget)?;
                encode_state(&state, wrap, budget, span)
            }
            Self::Adam | Self::AdamW => {
                let gradient = args[1].as_tensor().expect("typed gradient");
                let state = args[2]
                    .as_tuple()
                    .expect("typed Adam checkpoint")
                    .elements();
                let step = state[2].require_integer(span)?;
                if !(0..i64::MAX).contains(&step) {
                    return Err(Diagnostic::new(
                        DiagnosticKind::NumericDomain,
                        span,
                        "Adam step must be nonnegative and leave room for the next step",
                    ));
                }
                let moment = |index: usize| {
                    state[index]
                        .as_tensor()
                        .expect("typed Adam moment")
                        .as_vector()
                        .clone()
                };
                let state = AdamState::restore_with_budget(
                    config(&state[3..], span)?,
                    step as u64,
                    moment(0),
                    moment(1),
                    budget,
                )?;
                let rate = args[3].require_float(span)?;
                let decay = if matches!(self, Self::AdamW) {
                    args[4].require_float(span)?
                } else {
                    0.0
                };
                let update = state.adamw_with_budget(
                    weights.as_vector(),
                    gradient.as_vector(),
                    rate,
                    decay,
                    budget,
                )?;
                let state = encode_state(&update.state, wrap, budget, span)?;
                make_tuple([wrap(update.weights), state], budget, span)
            }
        }
    }
}

fn state_type(tensor: &SourceType) -> SourceType {
    let float = ValueType::Float;
    SourceType::tuple(vec![
        tensor.clone(),
        tensor.clone(),
        ValueType::Integer.into(),
        float.into(),
        float.into(),
        float.into(),
    ])
    .expect("bounded Adam state type")
}

fn config(values: &[Value], span: Span) -> Result<AdamConfig, Diagnostic> {
    Ok(AdamConfig {
        beta1: values[0].require_float(span)?,
        beta2: values[1].require_float(span)?,
        epsilon: values[2].require_float(span)?,
    })
}

fn encode_state(
    state: &AdamState,
    wrap: impl Fn(NumericVector) -> Value,
    budget: &mut Budget,
    span: Span,
) -> Result<Value, Diagnostic> {
    let config = state.config();
    let step = i64::try_from(state.step_count()).expect("source optimizer step was bounded");
    make_tuple(
        [
            wrap(state.first_moment().clone()),
            wrap(state.second_moment().clone()),
            Value::Integer(step),
            Value::Float(finite_number(config.beta1, span)?),
            Value::Float(finite_number(config.beta2, span)?),
            Value::Float(finite_number(config.epsilon, span)?),
        ],
        budget,
        span,
    )
}
