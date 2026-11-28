use scarp::{
    diagnostics::DiagnosticKind as Kind,
    numeric::{
        autodiff::{Binary, Graph, GraphLimits, Unary},
        optimizer::{clip_norm, AdamConfig, AdamState, OptimizerLimits},
    },
    value::{NumericVector, MAX_VECTOR_LENGTH},
};

const LIMITS: OptimizerLimits = OptimizerLimits {
    max_steps: 4_000_000,
    max_vector_bytes: 2_000_000,
};

fn v(values: &[f64]) -> NumericVector {
    NumericVector::new(values).unwrap()
}

fn close(actual: f64, expected: f64) {
    assert!(
        (actual - expected).abs() <= 1e-12 * expected.abs().max(1.0),
        "{actual} != {expected}"
    );
}

fn simple_config() -> AdamConfig {
    AdamConfig {
        beta1: 0.5,
        beta2: 0.75,
        epsilon: 0.25,
    }
}

#[test]
fn first_and_second_adam_steps_match_independent_scalar_reference() {
    let config = simple_config();
    let state = AdamState::new(1, config, LIMITS).unwrap();
    let w = v(&[2.0]);
    let first = state.adam(&w, &v(&[3.0]), 0.125, LIMITS).unwrap();
    close(first.state.first_moment().as_slice()[0].get(), 1.5);
    close(first.state.second_moment().as_slice()[0].get(), 2.25);
    let expected_w = 2.0 - 0.125 * 3.0 / 3.25;
    close(first.weights.as_slice()[0].get(), expected_w);

    let second = first.state.adam(&first.weights, &v(&[-1.0]), 0.125, LIMITS).unwrap();
    let m = 0.25;
    let variance: f64 = 1.9375;
    let corrected_m = m / (1.0 - 0.5f64.powi(2));
    let corrected_v = variance / (1.0 - 0.75f64.powi(2));
    let expected_w = expected_w - 0.125 * corrected_m / (corrected_v.sqrt() + 0.25);
    close(second.weights.as_slice()[0].get(), expected_w);
    close(second.state.first_moment().as_slice()[0].get(), m);
    close(second.state.second_moment().as_slice()[0].get(), variance);
    assert_eq!(second.state.step_count(), 2);
    assert_eq!(state.step_count(), 0);
    assert_eq!(state.first_moment(), &v(&[0.0]));
    assert_eq!(first.state.step_count(), 1);
    assert_eq!(w, v(&[2.0]));
}

#[test]
fn adamw_decay_is_decoupled_from_moments() {
    let state = AdamState::new(2, simple_config(), LIMITS).unwrap();
    let w = v(&[2.0, -4.0]);
    let g = v(&[0.0, 0.0]);
    let update = state.adamw(&w, &g, 0.25, 0.5, LIMITS).unwrap();
    assert_eq!(update.weights, v(&[1.75, -3.5]));
    assert_eq!(update.state.first_moment(), &g);
    assert_eq!(update.state.second_moment(), &g);
    assert_eq!(
        state.adam(&w, &g, 0.25, LIMITS).unwrap(),
        state.adamw(&w, &g, 0.25, 0.0, LIMITS).unwrap()
    );
}

#[test]
fn zero_rate_updates_moments_but_not_weights() {
    let state = AdamState::new(1, simple_config(), LIMITS).unwrap();
    let weights = v(&[f64::MAX]);
    let update = state.adamw(&weights, &v(&[2.0]), 0.0, f64::MAX, LIMITS).unwrap();
    assert_eq!(update.weights, weights);
    assert_eq!(update.state.first_moment(), &v(&[1.0]));
    assert_eq!(update.state.second_moment(), &v(&[1.0]));
    assert_eq!(update.state.step_count(), 1);
}

#[test]
fn beta_near_one_and_beta_zero_have_stable_bias_correction() {
    for beta in [0.0, 0.5, 1.0 - f64::EPSILON] {
        let config = AdamConfig {
            beta1: beta,
            beta2: beta,
            epsilon: 0.125,
        };
        let state = AdamState::new(1, config, LIMITS).unwrap();
        let update = state.adam(&v(&[0.0]), &v(&[2.0]), 0.25, LIMITS).unwrap();
        close(update.weights.as_slice()[0].get(), -0.25 * 2.0 / 2.125);
    }
}

#[test]
fn weighted_second_moment_avoids_unnecessary_square_overflow() {
    let config = AdamConfig {
        beta1: 0.5,
        beta2: 0.99,
        epsilon: 1e-8,
    };
    let state = AdamState::new(1, config, LIMITS).unwrap();
    let update = state.adam(&v(&[0.0]), &v(&[1e155]), 0.125, LIMITS).unwrap();
    assert!(update.state.second_moment().as_slice()[0].get().is_finite());
    close(update.weights.as_slice()[0].get(), -0.125);
}

#[test]
fn invalid_hyperparameters_and_shapes_precede_budget() {
    let zero = OptimizerLimits {
        max_steps: 0,
        max_vector_bytes: 0,
    };
    for beta in [f64::NAN, f64::INFINITY, -0.1, 1.0] {
        for config in [
            AdamConfig { beta1: beta, ..simple_config() },
            AdamConfig { beta2: beta, ..simple_config() },
        ] {
            assert_eq!(AdamState::new(1, config, zero).unwrap_err().kind, Kind::NumericDomain);
        }
    }
    for epsilon in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        let config = AdamConfig { epsilon, ..simple_config() };
        assert_eq!(AdamState::new(1, config, zero).unwrap_err().kind, Kind::NumericDomain);
    }
    let state = AdamState::new(1, simple_config(), LIMITS).unwrap();
    let w = v(&[1.0]);
    for bad in [-1.0, f64::NAN, f64::INFINITY] {
        assert_eq!(state.adam(&w, &w, bad, zero).unwrap_err().kind, Kind::NumericDomain);
        assert_eq!(state.adamw(&w, &w, 0.1, bad, zero).unwrap_err().kind, Kind::NumericDomain);
    }
    assert_eq!(state.adam(&w, &v(&[]), 0.1, zero).unwrap_err().kind, Kind::VectorLength);
    assert_eq!(AdamState::new(MAX_VECTOR_LENGTH + 1, simple_config(), zero).unwrap_err().kind, Kind::VectorLength);
}

#[test]
fn restore_shares_buffers_and_continues_identically() {
    let initial = AdamState::new(2, simple_config(), LIMITS).unwrap();
    let update = initial.adam(&v(&[1.0, 2.0]), &v(&[3.0, 4.0]), 0.125, LIMITS).unwrap();
    let restored = AdamState::restore(
        update.state.config(),
        update.state.step_count(),
        update.state.first_moment().clone(),
        update.state.second_moment().clone(),
        OptimizerLimits { max_steps: 2, max_vector_bytes: 0 },
    ).unwrap();
    assert_eq!(restored, update.state);
    assert!(std::ptr::eq(restored.first_moment().as_slice().as_ptr(), update.state.first_moment().as_slice().as_ptr()));
    let g = v(&[-1.0, 0.25]);
    assert_eq!(
        restored.adam(&update.weights, &g, 0.125, LIMITS).unwrap(),
        update.state.adam(&update.weights, &g, 0.125, LIMITS).unwrap()
    );
}

#[test]
fn invalid_checkpoint_and_exhausted_counter_are_rejected() {
    for (step, m, variance) in [(0, 1.0, 0.0), (0, 0.0, 1.0), (1, 0.0, -1.0)] {
        assert_eq!(AdamState::restore(simple_config(), step, v(&[m]), v(&[variance]), LIMITS).unwrap_err().kind, Kind::NumericDomain);
    }
    let state = AdamState::restore(simple_config(), u64::MAX, v(&[0.0]), v(&[0.0]), LIMITS).unwrap();
    let w = v(&[1.0]);
    assert_eq!(state.adam(&w, &w, 0.1, LIMITS).unwrap_err().kind, Kind::ExecutionLimit);
    assert_eq!(state.step_count(), u64::MAX);
}

#[test]
fn all_output_buffers_share_exact_resource_budget() {
    let state = AdamState::new(2, simple_config(), OptimizerLimits { max_steps: 2, max_vector_bytes: 16 }).unwrap();
    assert!(std::ptr::eq(state.first_moment().as_slice().as_ptr(), state.second_moment().as_slice().as_ptr()));
    let w = v(&[1.0, 2.0]);
    let exact = OptimizerLimits { max_steps: 56, max_vector_bytes: 48 };
    state.adam(&w, &w, 0.1, exact).unwrap();
    assert_eq!(state.adam(&w, &w, 0.1, OptimizerLimits { max_steps: 55, ..exact }).unwrap_err().kind, Kind::ExecutionLimit);
    assert_eq!(state.adam(&w, &w, 0.1, OptimizerLimits { max_vector_bytes: 47, ..exact }).unwrap_err().kind, Kind::VectorBudget);
    assert_eq!(AdamState::new(2, simple_config(), OptimizerLimits { max_steps: 2, max_vector_bytes: 15 }).unwrap_err().kind, Kind::VectorBudget);
}

#[test]
fn failed_update_preserves_all_previous_snapshots() {
    let state = AdamState::new(2, simple_config(), LIMITS).unwrap();
    let before = state.clone();
    let w = v(&[1.0, 2.0]);
    let g = v(&[1.0, f64::MAX]);
    assert_eq!(state.adam(&w, &g, 0.1, LIMITS).unwrap_err().kind, Kind::NonFiniteNumber);
    assert_eq!(state.adamw(&w, &w, f64::MAX, 2.0, LIMITS).unwrap_err().kind, Kind::NonFiniteNumber);
    assert_eq!(state, before);
    assert_eq!(w, v(&[1.0, 2.0]));
}

#[test]
fn empty_and_maximum_vectors_are_bounded() {
    let empty = AdamState::new(0, simple_config(), OptimizerLimits { max_steps: 1, max_vector_bytes: 0 }).unwrap();
    let update = empty.adam(&v(&[]), &v(&[]), 0.1, OptimizerLimits { max_steps: 8, max_vector_bytes: 0 }).unwrap();
    assert!(update.weights.is_empty());
    assert_eq!(update.state.step_count(), 1);
    let state = AdamState::new(MAX_VECTOR_LENGTH, simple_config(), LIMITS).unwrap();
    let w = v(&vec![1.0; MAX_VECTOR_LENGTH]);
    let update = state.adam(&w, &w, 0.1, LIMITS).unwrap();
    assert_eq!(update.weights.len(), MAX_VECTOR_LENGTH);
    close(update.weights.as_slice()[MAX_VECTOR_LENGTH - 1].get(), 0.92);
}

#[test]
fn optimizer_respects_visible_windows() {
    let storage = v(&[999.0, 2.0, -3.0, 999.0]);
    let weights = storage.window(1, 2).unwrap();
    let grad = v(&[999.0, 1.0, -2.0, 999.0]).window(1, 2).unwrap();
    let state = AdamState::new(2, simple_config(), LIMITS).unwrap();
    let update = state.adam(&weights, &grad, 0.25, LIMITS).unwrap();
    assert_eq!(update, state.adam(&v(&[2.0, -3.0]), &v(&[1.0, -2.0]), 0.25, LIMITS).unwrap());
    assert_eq!(storage, v(&[999.0, 2.0, -3.0, 999.0]));
}

#[test]
fn clipping_handles_extreme_and_subnormal_norms() {
    let clipped = clip_norm(&v(&[3.0, 4.0]), 2.5, LIMITS).unwrap();
    assert_eq!(clipped, v(&[1.5, 2.0]));
    let extreme = clip_norm(&v(&[f64::MAX, f64::MAX]), 1.0, LIMITS).unwrap();
    for cell in extreme.as_slice() {
        close(cell.get(), 1.0 / 2.0f64.sqrt());
    }
    let tiny = f64::from_bits(1);
    assert_eq!(clip_norm(&v(&[tiny]), tiny, LIMITS).unwrap(), v(&[tiny]));
    assert_eq!(clip_norm(&v(&[3.0, -4.0]), 0.0, LIMITS).unwrap(), v(&[0.0, -0.0]));
}

#[test]
fn clipping_shares_unchanged_storage_and_precharges_all_work() {
    let gradient = v(&[3.0, 4.0]);
    let exact = OptimizerLimits { max_steps: 10, max_vector_bytes: 0 };
    let unchanged = clip_norm(&gradient, 5.0, exact).unwrap();
    assert!(std::ptr::eq(gradient.as_slice().as_ptr(), unchanged.as_slice().as_ptr()));
    assert_eq!(clip_norm(&gradient, 4.0, exact).unwrap_err().kind, Kind::VectorBudget);
    assert_eq!(clip_norm(&gradient, 5.0, OptimizerLimits { max_steps: 9, ..exact }).unwrap_err().kind, Kind::ExecutionLimit);
    assert_eq!(clip_norm(&gradient, f64::NAN, exact).unwrap_err().kind, Kind::NumericDomain);
}

#[test]
fn adam_trains_a_reusable_autodiff_graph() {
    let mut graph = Graph::new(8).unwrap();
    let weight = graph.input().unwrap();
    let x = graph.input().unwrap();
    let target = graph.input().unwrap();
    let prediction = graph.binary(Binary::Multiply, &weight, &x).unwrap();
    let error = graph.binary(Binary::Subtract, &prediction, &target).unwrap();
    let loss = graph.unary(Unary::Square, &error).unwrap();
    let graph_limits = GraphLimits { max_steps: 1000, max_vector_bytes: 1024 };
    let mut state = AdamState::new(1, AdamConfig::default(), LIMITS).unwrap();
    let mut weights = v(&[0.0]);
    for _ in 0..400 {
        let tape = graph.evaluate(&loss, &[weights.as_slice()[0].get(), 2.0, 6.0], 1.0, graph_limits).unwrap();
        let g = v(&[tape.gradient(&weight).unwrap()]);
        let update = state.adam(&weights, &g, 0.05, LIMITS).unwrap();
        weights = update.weights;
        state = update.state;
    }
    assert!((weights.as_slice()[0].get() - 3.0).abs() < 1e-6);
    assert_eq!(state.step_count(), 400);
}
