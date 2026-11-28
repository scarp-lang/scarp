use scarp::{diagnostics::DiagnosticKind as Kind, ExecutionOptions};

fn output(source: &str) -> String {
    let mut output = Vec::new();
    scarp::run(source, &mut output).unwrap();
    String::from_utf8(output).unwrap()
}

#[test]
fn source_tensor_training_and_scalar_gradient_example() {
    assert_eq!(
        output(include_str!("../examples/tensor_learning.srp")),
        include_str!("../examples/tensor_learning.stdout"),
    );
}

#[test]
fn shapes_are_exact_and_zero_axes_are_preserved() {
    let source = "
        bind data: tensor<float, 2, 0> = [];
        bind scalar = [tensor: <float> 3.0];
        print(data[1], tensor_sum_axis(data, 1), tensor_item(scalar));
        print(tensor_transpose(data), tensor_reshape(data, 0, 7));
        print(tensor_broadcast(scalar, 2, 2));
        print(tensor_sum(data), data == data);
    ";
    assert_eq!(
        output(source),
        "tensor<float, 0> [] tensor<float, 2> [0.0, 0.0] 3.0\n\
         tensor<float, 0, 2> [] tensor<float, 0, 7> []\n\
         tensor<float, 2, 2> [3.0, 3.0, 3.0, 3.0]\n\
         0.0 true\n",
    );
}

#[test]
fn source_shape_and_numeric_errors_are_precise() {
    let compile_cases = [
        (
            "bind x: tensor<float, 2, 2> = [[1.0], [2.0]];",
            Kind::TensorShape,
        ),
        ("bind x: tensor<int, 2> = [];", Kind::TensorShape),
        ("bind x: tensor<float, 300, 300> = [];", Kind::TensorShape),
        ("bind x: tensor<float, 0, 65537> = [];", Kind::TensorShape),
        ("bind x: tensor<float, 1> = [1];", Kind::TypeMismatch),
        ("print([tensor: <float> 1.0][0]);", Kind::TensorIndex),
        (
            "print(matmul([tensor: <float, 1> [1.0]], [tensor: <float, 1> [1.0]]));",
            Kind::TensorShape,
        ),
        (
            "bind n = 1; print(tensor_reshape([tensor: <float> 1.0], n));",
            Kind::TensorShape,
        ),
        (
            "print(tensor_sum_axis([tensor: <float, 1> [1.0]], 1));",
            Kind::TensorShape,
        ),
        (
            "print(tensor_broadcast([tensor: <float, 0> []], 1));",
            Kind::TensorShape,
        ),
        (
            "print(tensor_square_backward([tensor: <float> 1.0], [tensor: <float, 1> [1.0]]));",
            Kind::TensorShape,
        ),
    ];
    for (source, kind) in compile_cases {
        assert_eq!(scarp::compile(source).unwrap_err().kind, kind, "{source}");
    }
    for source in [
        "bind data: tensor<float, 2> = [1.0, 2.0]; print(data[2]);",
        "bind data: tensor<float, 2> = [1.0, 2.0]; print(data[-1]);",
    ] {
        let error = scarp::check(source).unwrap_err();
        assert_eq!(error.kind, Kind::TensorIndex);
        assert!(["2", "-1"].contains(&&source[error.span.start..error.span.end]));
    }
    let source = "print(9); print([tensor: <float> 1.0] / [tensor: <float> 0.0]);";
    let mut captured = Vec::new();
    assert!(scarp::run(source, &mut captured).is_err());
    assert_eq!(scarp::check(source).unwrap_err().kind, Kind::NumericDomain);
    assert!(captured.is_empty());
}

#[test]
fn reshape_and_index_share_storage_and_reassignment_keeps_snapshots() {
    let source = "
        var matrix: tensor<float, 2, 2> = [[1.0, 2.0], [3.0, 4.0]];
        bind selected = tensor_reshape(matrix[1], 2, 1);
        print(selected);
        matrix = [tensor: <float, 2, 2> [[9.0, 9.0], [9.0, 9.0]]];
        print(selected, matrix[1]);
    ";
    let mut bytes = Vec::new();
    let mut options = ExecutionOptions {
        max_vector_bytes: 64,
        ..ExecutionOptions::default()
    };
    scarp::run_with_options(source, &mut bytes, options).unwrap();
    assert_eq!(
        String::from_utf8(bytes).unwrap(),
        "tensor<float, 2, 1> [3.0, 4.0]\ntensor<float, 2, 1> [3.0, 4.0] tensor<float, 2> [9.0, 9.0]\n",
    );
    options.max_vector_bytes -= 1;
    assert_eq!(
        scarp::check_with_options(source, options).unwrap_err().kind,
        Kind::VectorBudget
    );
}

#[test]
fn tensor_kernels_debit_one_cumulative_execution_budget() {
    let source = "
        bind x: tensor<float, 2> = [1.0, 2.0];
        bind y = tensor_square(x);
        bind z = x + y;
        print(z);
    ";
    let mut options = ExecutionOptions {
        max_vector_bytes: 48,
        ..ExecutionOptions::default()
    };
    scarp::check_with_options(source, options).unwrap();
    options.max_vector_bytes -= 1;
    let error = scarp::check_with_options(source, options).unwrap_err();
    assert_eq!(error.kind, Kind::VectorBudget);
    assert_eq!(&source[error.span.start..error.span.end], "+");
    options.max_vector_bytes += 1;
    options.max_capture_bytes = 103;
    assert_eq!(
        scarp::check_with_options(source, options).unwrap_err().kind,
        Kind::CaptureLimit
    );
    options.max_capture_bytes = 104;
    scarp::check_with_options(source, options).unwrap();
    options.limits.max_steps = 21;
    assert_eq!(
        scarp::check_with_options(source, options).unwrap_err().kind,
        Kind::ExecutionLimit
    );
}

#[test]
fn tensor_shape_survives_module_signatures_and_generic_specialization() {
    let directory =
        std::env::temp_dir().join(format!("scarp-source-tensors-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    struct Cleanup(std::path::PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let _cleanup = Cleanup(directory.clone());
    std::fs::write(
        directory.join("net.srp"),
        "
        pub pure funcra bias(x: tensor<float, 2>) -> tensor<float, 2> {
            return x + [tensor: <float> 1.0];
        }
    ",
    )
    .unwrap();
    let main = directory.join("main.srp");
    std::fs::write(
        &main,
        "
        import \"net.srp\" as net;
        pure funcra identity<T>(x: T) -> T { return x; }
        print(identity::<tensor<float, 2>>(net.bias([tensor: <float, 2> [2.0, 4.0]])));
    ",
    )
    .unwrap();
    let result = std::process::Command::new(env!("CARGO_BIN_EXE_scarp"))
        .arg("exec")
        .arg(main)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{:?}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(result.stdout, b"tensor<float, 2> [3.0, 5.0]\n");
}

#[test]
fn scalar_source_graph_reuses_plan_with_new_inputs_and_respects_limits() {
    let source = "
        var x = 2.0;
        bind b = 3.0;
        repeat 3 as i {
            print(value_grad(x, x * x + x * b));
            x = x + 1.0;
        }
        print(value_grad(x, b * b));
    ";
    assert_eq!(
        output(source),
        "(10.0, 7.0)\n(18.0, 9.0)\n(28.0, 11.0)\n(9.0, 0.0)\n"
    );
    let options = ExecutionOptions {
        max_vector_bytes: 50,
        ..ExecutionOptions::default()
    };
    assert_eq!(
        scarp::check_with_options(source, options).unwrap_err().kind,
        Kind::VectorBudget
    );
    for rejected in [
        "bind x = 2.0; print(value_grad(x + x, x * x));",
        "bind x = 2.0; print(value_grad(x, if true { x } else { 0.0 }));",
        "pure funcra f(x: float) -> float { return x; } bind x = 2.0; print(value_grad(x, f(x)));",
    ] {
        assert_eq!(
            scarp::compile(rejected).unwrap_err().kind,
            Kind::AutoDiffExpression
        );
    }
}

#[test]
fn source_gradient_matches_independent_centered_differences() {
    let reference = |x: f64| x.exp() / (1.0 + x * x);
    for x in [-1.5_f64, -0.25, 0.0, 0.5, 2.0] {
        let source =
            format!("bind x = {x:.9}; print(value_grad(x, scalar_exp(x) / (1.0 + x * x))[1]);");
        let actual: f64 = output(&source).trim().parse().unwrap();
        let h = 1e-5;
        let expected = (reference(x + h) - reference(x - h)) / (2.0 * h);
        assert!(
            (actual - expected).abs() < 2e-9,
            "{x}: {actual} vs {expected}"
        );
    }
}

#[test]
fn tensor_graph_accumulates_shared_edges_and_handles_disconnected_parameters() {
    let source = "
        bind x: tensor<float, 2> = [1.5, -2.0];
        bind other: tensor<float, 2> = [1.0, 2.0];
        print(tensor_value_grad(x, tensor_sum(x * x + x)));
        print(tensor_value_grad(x, tensor_sum(other)));
        print(tensor_value_grad(x, tensor_sum(x)));
    ";
    assert_eq!(
        output(source),
        "(5.75, tensor<float, 2> [4.0, -3.0])\n\
         (3.0, tensor<float, 2> [0.0, 0.0])\n\
         (-0.5, tensor<float, 2> [1.0, 1.0])\n",
    );
}

#[test]
fn tensor_graph_matmul_broadcast_and_reduction_match_finite_differences() {
    let program = "
        bind row: tensor<float, 1, 2> = [[1.0, -2.0]];
        var w: tensor<float, 2, 1> = [[0.25], [0.75]];
        bind scalar = [tensor: <float> 0.5];
        bind result = tensor_value_grad(w, tensor_sum(tensor_sum_axis(
            tensor_square(tensor_transpose(matmul(row, w)) + tensor_broadcast(scalar, 2, 1)), 0)));
        print(result[0], result[1][0][0], result[1][1][0]);
    ";
    let values: Vec<f64> = output(program)
        .split_whitespace()
        .map(|v| v.parse().unwrap())
        .collect();
    let reference = |w: [f64; 2]| 2.0 * (w[0] - 2.0 * w[1] + 0.5).powi(2);
    let point = [0.25, 0.75];
    assert_eq!(values[0], reference(point));
    for axis in 0..2 {
        let mut plus = point;
        let mut minus = point;
        let h = 1e-5;
        plus[axis] += h;
        minus[axis] -= h;
        let expected = (reference(plus) - reference(minus)) / (2.0 * h);
        assert!((values[axis + 1] - expected).abs() < 1e-9);
    }
    let options = ExecutionOptions {
        max_vector_bytes: 100,
        ..ExecutionOptions::default()
    };
    let error = scarp::check_with_options(program, options).unwrap_err();
    assert_eq!(error.kind, Kind::VectorBudget);
    let unsupported = "
        bind x: tensor<float, 1> = [1.0];
        print(tensor_value_grad(x, tensor_sum(x) + 1.0));
    ";
    assert_eq!(
        scarp::compile(unsupported).unwrap_err().kind,
        Kind::AutoDiffExpression
    );
}

#[test]
fn graph_adjoint_of_a_broadcast_reduces_to_the_original_shape() {
    let source = "
        bind x: tensor<float, 2, 1> = [[1.0], [3.0]];
        bind row: tensor<float, 3> = [10.0, 20.0, 30.0];
        print(tensor_value_grad(x, tensor_sum(tensor_reshape(x + row, 6))));
        bind empty: tensor<float, 2, 0> = [];
        print(tensor_value_grad(empty, tensor_sum(tensor_square(empty))));
    ";
    assert_eq!(
        output(source),
        "(132.0, tensor<float, 2, 1> [3.0, 3.0])\n(0.0, tensor<float, 2, 0> [])\n",
    );
}
