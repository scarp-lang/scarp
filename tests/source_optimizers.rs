use scarp::{diagnostics::DiagnosticKind as Kind, ExecutionOptions};

fn output(source: &str) -> Vec<f64> {
    let mut output = Vec::new();
    scarp::run(source, &mut output).unwrap();
    String::from_utf8(output)
        .unwrap()
        .split_whitespace()
        .map(|v| v.parse().unwrap())
        .collect()
}

#[test]
fn source_adam_and_adamw_match_independent_multistep_reference() {
    for decay in [0.0, 0.15] {
        let operation = if decay == 0.0 {
            "tensor_adam(w,g,state,0.05)".to_owned()
        } else {
            format!("tensor_adamw(w,g,state,0.05,{decay})")
        };
        let program = format!(
            "
            var w: tensor<float, 2> = [2.0, -3.0];
            bind g: tensor<float, 2> = [4.0, -8.0];
            var state = tensor_adam_init(w, 0.5, 0.75, 0.001);
            repeat 4 as i {{
                bind next = {operation};
                w = next[0];
                state = next[1];
                print(w[0], w[1], state[0][0], state[1][1], state[2]);
            }}
        "
        );
        let actual = output(&program);
        let mut w = [2.0, -3.0];
        let mut m = [0.0; 2];
        let mut v = [0.0; 2];
        for step in 1..=4 {
            for (i, g) in [4.0, -8.0].into_iter().enumerate() {
                m[i] = 0.5 * m[i] + 0.5 * g;
                v[i] = 0.75 * v[i] + 0.25 * g * g;
                let mh = m[i] / (1.0 - 0.5_f64.powi(step));
                let vh = v[i] / (1.0 - 0.75_f64.powi(step));
                w[i] = w[i] * (1.0 - 0.05 * decay) - 0.05 * mh / (vh.sqrt() + 0.001);
            }
            let actual = &actual[(step as usize - 1) * 5..step as usize * 5];
            for (actual, expected) in actual.iter().zip([w[0], w[1], m[0], v[1], f64::from(step)]) {
                assert!((actual - expected).abs() < 1e-12, "{actual} != {expected}");
            }
        }
    }
}

#[test]
fn optimizer_enforces_shape_and_runtime_domain_before_publishing_a_result() {
    let bad_shape = "
        bind w: tensor<float, 1, 2> = [[0.1, 0.2]];
        bind g: tensor<float, 2> = [0.1, 0.2];
        print(tensor_sgd(w, g, 0.1));
    ";
    assert_eq!(
        scarp::compile(bad_shape).unwrap_err().kind,
        Kind::TypeMismatch
    );
    let setup = "
        bind w: tensor<float, 2> = [1.0, -2.0];
        bind g: tensor<float, 2> = [2.0, 3.0];
        bind state = tensor_adam_init(w, 0.5, 0.75, 0.001);
        print(1);
    ";
    for bad in [
        "tensor_sgd(w, g, -0.1)",
        "tensor_adam_init(w, 1.0, 0.75, 0.001)",
        "tensor_adam(w, g, (w, w, 0, 0.5, 0.75, 0.001), 0.05)",
        "tensor_adam(w, g, (w, w, 9223372036854775807, 0.5, 0.75, 0.001), 0.05)",
        "tensor_adamw(w, g, state, 0.05, -0.1)",
    ] {
        let source = format!("{setup}print({bad});");
        let mut output = Vec::new();
        assert!(scarp::run(&source, &mut output).is_err(), "{bad}");
        assert!(output.is_empty(), "{bad}");
        assert_eq!(scarp::check(&source).unwrap_err().kind, Kind::NumericDomain);
    }
}

#[test]
fn shared_state_is_immutable_and_kernel_allocations_have_an_exact_boundary() {
    let source = "
        bind w: tensor<float, 2> = [1.0, 2.0];
        bind g: tensor<float, 2> = [1.0, 2.0];
        bind state = tensor_adam_init(w, 0.5, 0.5, 0.01);
        bind update = tensor_adam(w, g, state, 0.1);
        print(w[0], state[0][0], state[1][0], state[2], update[1][2]);
    ";
    assert_eq!(output(source), vec![1.0, 0.0, 0.0, 0.0, 1.0]);
    // Two input tensors, one shared pair of zero moments, three result buffers.
    let mut opts = ExecutionOptions {
        max_vector_bytes: 6 * 16,
        ..ExecutionOptions::default()
    };
    scarp::check_with_options(source, opts).unwrap();
    opts.max_vector_bytes -= 1;
    assert_eq!(
        scarp::check_with_options(source, opts).unwrap_err().kind,
        Kind::VectorBudget
    );
}
