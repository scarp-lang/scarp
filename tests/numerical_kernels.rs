use scarp::{
    diagnostics::DiagnosticKind as Kind,
    numeric::{self, Builtin},
    value::{NumericVector, Value, MAX_VECTOR_LENGTH},
    ExecutionOptions,
};
fn vector(values: &[f64]) -> Value {
    Value::Vector(NumericVector::new(values).unwrap())
}
fn call(function: Builtin, values: &[f64]) -> Value {
    numeric::evaluate(function, &[vector(values)], u64::MAX).unwrap()
}
fn scalar(function: Builtin, values: &[f64]) -> f64 {
    call(function, values).as_float().unwrap()
}
fn close(actual: f64, expected: f64, tolerance: f64) {
    assert!(
        (actual - expected).abs() <= tolerance * expected.abs().max(1.0),
        "{actual} != {expected}"
    );
}

#[test]
fn statistics_are_population_reductions_and_accept_singletons() {
    close(
        scalar(Builtin::Variance, &[1.0, 2.0, 3.0, 4.0]),
        1.25,
        1e-15,
    );
    close(
        scalar(Builtin::StdDev, &[1.0, 2.0, 3.0, 4.0]),
        1.25f64.sqrt(),
        1e-15,
    );
    for values in [
        vec![f64::MAX],
        vec![f64::MAX; 4],
        vec![-0.0; 4],
        vec![1e-300; 4],
    ] {
        assert_eq!(scalar(Builtin::Variance, &values).to_bits(), 0);
        assert_eq!(scalar(Builtin::StdDev, &values).to_bits(), 0);
    }
}

#[test]
fn large_offsets_keep_small_spreads_and_extreme_differences_stay_finite() {
    close(scalar(Builtin::Variance, &[1e16, 1e16 + 2.0]), 1.0, 1e-15);
    close(
        scalar(Builtin::Variance, &[1e16, 1e16, 1e16 + 2.0]),
        8.0 / 9.0,
        1e-15,
    );
    close(
        scalar(Builtin::Variance, &[1e16, 1e16 + 2.0, 1e16 + 4.0]),
        8.0 / 3.0,
        1e-15,
    );
    close(scalar(Builtin::StdDev, &[1e308, -1e308]), 1e308, 1e-15);
    close(
        scalar(Builtin::StdDev, &[f64::MAX, -f64::MAX, f64::MAX]),
        f64::MAX * (8.0f64 / 9.0).sqrt(),
        1e-15,
    );
    close(
        scalar(Builtin::StdDev, &[f64::MAX, -f64::MAX]),
        f64::MAX,
        1e-15,
    );
    assert_eq!(
        numeric::evaluate(Builtin::Variance, &[vector(&[1e308, -1e308])], 16)
            .unwrap_err()
            .kind,
        Kind::NonFiniteNumber
    );
}

#[test]
fn tiny_spreads_do_not_disappear_before_standard_deviation_rescaling() {
    let tiny = f64::from_bits(1);
    assert_eq!(scalar(Builtin::StdDev, &[tiny, -tiny]), tiny);
    assert_eq!(scalar(Builtin::StdDev, &[1e-200, -1e-200]), 1e-200);
    assert_eq!(scalar(Builtin::Variance, &[1e-200, -1e-200]), 0.0);
}

#[test]
fn activations_saturate_safely_and_define_signed_zero_behavior() {
    assert_eq!(
        call(Builtin::Sigmoid, &[-f64::MAX, -0.0, 0.0, f64::MAX]),
        vector(&[0.0, 0.5, 0.5, 1.0])
    );
    assert_eq!(
        call(Builtin::Relu, &[-f64::MAX, -0.0, 0.0, f64::MAX]),
        vector(&[0.0, 0.0, 0.0, f64::MAX])
    );
    assert_eq!(
        call(Builtin::Relu, &[-0.0]).as_vector().unwrap().as_slice()[0]
            .get()
            .to_bits(),
        0
    );
    assert_eq!(
        call(Builtin::Tanh, &[-0.0]).as_vector().unwrap().as_slice()[0]
            .get()
            .to_bits(),
        (-0.0f64).to_bits()
    );
    assert_eq!(
        call(Builtin::Tanh, &[-f64::MAX, 0.0, f64::MAX]),
        vector(&[-1.0, 0.0, 1.0])
    );
    let result = call(Builtin::Sigmoid, &[-1.0, 1.0]);
    for (value, expected) in result
        .as_vector()
        .unwrap()
        .as_slice()
        .iter()
        .zip([0.2689414213699951, 0.7310585786300049])
    {
        close(value.get(), expected, 1e-15);
    }
}

#[test]
fn empty_activation_values_and_statistical_domains_are_explicit() {
    for function in [Builtin::Sigmoid, Builtin::Relu, Builtin::Tanh] {
        assert_eq!(
            numeric::evaluate_with_vector_limit(function, &[vector(&[])], 1, 0).unwrap(),
            vector(&[])
        );
        assert_eq!(
            numeric::evaluate(function, &[vector(&[])], 0)
                .unwrap_err()
                .kind,
            Kind::ExecutionLimit
        );
    }
    for function in [Builtin::Variance, Builtin::StdDev] {
        assert_eq!(
            numeric::evaluate(function, &[vector(&[])], 0)
                .unwrap_err()
                .kind,
            Kind::NumericDomain
        );
    }
}

#[test]
fn all_kernel_work_and_payload_limits_are_exact_and_inputs_stay_immutable() {
    for (function, work, bytes) in [
        (Builtin::Variance, 12, 0),
        (Builtin::StdDev, 12, 0),
        (Builtin::Relu, 3, 24),
        (Builtin::Sigmoid, 12, 24),
        (Builtin::Tanh, 12, 24),
    ] {
        let args = [vector(&[-1.0, 0.0, 1.0])];
        let before = args.clone();
        numeric::evaluate_with_vector_limit(function, &args, work, bytes).unwrap();
        assert_eq!(
            numeric::evaluate(function, &args, work - 1)
                .unwrap_err()
                .kind,
            Kind::ExecutionLimit
        );
        if bytes > 0 {
            assert_eq!(
                numeric::evaluate_with_vector_limit(function, &args, work, bytes - 1)
                    .unwrap_err()
                    .kind,
                Kind::VectorBudget
            );
        }
        assert_eq!(args, before);
    }
    let options = ExecutionOptions {
        max_vector_bytes: 24,
        ..ExecutionOptions::default()
    };
    scarp::check_with_options(
        "bind a=[1.0,2.0,3.0]; print(variance(a),stddev(a));",
        options,
    )
    .unwrap();
    assert_eq!(
        scarp::check_with_options("bind a=[1.0,2.0,3.0]; print(relu(a));", options)
            .unwrap_err()
            .kind,
        Kind::VectorBudget
    );
}

#[test]
fn statistics_and_activations_read_only_visible_windows_at_maximum_length() {
    let values = NumericVector::new(&[999.0, 1.0, 2.0, 3.0, 999.0]).unwrap();
    let args = [Value::Vector(values.window(1, 3).unwrap())];
    close(
        numeric::evaluate_with_vector_limit(Builtin::Variance, &args, 24, 0)
            .unwrap()
            .as_float()
            .unwrap(),
        2.0 / 3.0,
        1e-15,
    );
    assert_eq!(
        numeric::evaluate_with_vector_limit(Builtin::Relu, &args, 3, 24).unwrap(),
        vector(&[1.0, 2.0, 3.0])
    );
    let values = vec![2.0; MAX_VECTOR_LENGTH];
    assert_eq!(scalar(Builtin::Variance, &values), 0.0);
    assert_eq!(
        call(Builtin::Sigmoid, &values).as_vector().unwrap().len(),
        MAX_VECTOR_LENGTH
    );
}

#[test]
fn generated_populations_match_exact_integer_moments() {
    let mut seed = 7u64;
    for count in 1..65 {
        let values: Vec<i64> = (0..count)
            .map(|_| {
                seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
                ((seed >> 32) % 81) as i64 - 40
            })
            .collect();
        let sum: i128 = values.iter().map(|v| i128::from(*v)).sum();
        let squares: i128 = values.iter().map(|v| i128::from(*v) * i128::from(*v)).sum();
        let expected = (squares * count as i128 - sum * sum) as f64 / (count * count) as f64;
        let values: Vec<f64> = values.iter().map(|v| *v as f64).collect();
        close(scalar(Builtin::Variance, &values), expected, 2e-15);
        close(scalar(Builtin::StdDev, &values), expected.sqrt(), 2e-15);
        let shifted: Vec<f64> = values.iter().map(|value| 1e16 + value * 2.0).collect();
        close(scalar(Builtin::Variance, &shifted), expected * 4.0, 4e-15);
    }
}

#[test]
fn activation_symmetry_monotonicity_and_bounds_hold_for_generated_inputs() {
    let values: Vec<f64> = (-200..=200).map(|n| f64::from(n) / 8.0).collect();
    for function in [Builtin::Sigmoid, Builtin::Tanh] {
        let result = call(function, &values);
        let output = result.as_vector().unwrap().as_slice();
        assert!(output.windows(2).all(|pair| pair[0].get() <= pair[1].get()));
        for (value, opposite) in output.iter().zip(output.iter().rev()) {
            if function == Builtin::Sigmoid {
                assert!((0.0..=1.0).contains(&value.get()));
                close(value.get() + opposite.get(), 1.0, 2e-15);
            } else {
                assert!((-1.0..=1.0).contains(&value.get()));
                close(value.get(), -opposite.get(), 2e-15);
            }
        }
    }
}

#[test]
fn function_signatures_and_builtin_arguments_use_existing_static_rules() {
    for name in ["variance", "stddev", "sigmoid", "relu", "tanh"] {
        assert_eq!(
            scarp::compile(&format!("print({name}(1.0));"))
                .unwrap_err()
                .kind,
            Kind::TypeMismatch
        );
        assert_eq!(
            scarp::compile(&format!("print({name}([integer:1]));"))
                .unwrap_err()
                .kind,
            Kind::TypeMismatch
        );
        assert_eq!(
            scarp::compile(&format!("print({name}());"))
                .unwrap_err()
                .kind,
            Kind::CallArguments
        );
    }
    let mut out = Vec::new();
    scarp::run("pure funcra spread(v: vector=[1.0,3.0])->float ensures result>=0.0; {return stddev(v);} print(spread(),false and variance([])>0.0);",&mut out).unwrap();
    assert_eq!(out, b"1.0 false\n");
    assert_eq!(
        numeric::evaluate(Builtin::Variance, &[Value::Integer(1)], 0)
            .unwrap_err()
            .kind,
        Kind::TypeMismatch
    );
}

#[test]
fn numerical_errors_discard_both_program_streams() {
    for (expression, kind) in [
        ("variance([])", Kind::NumericDomain),
        ("stddev([])", Kind::NumericDomain),
        ("variance([1e308,-1e308])", Kind::NonFiniteNumber),
    ] {
        let source = format!("print(\"hidden\"); eprint(\"hidden\"); print({expression});");
        let (mut out, mut err) = (Vec::new(), Vec::new());
        assert!(scarp::run_with_io(&source, &mut out, &mut err).is_err());
        assert!(out.is_empty() && err.is_empty());
        assert_eq!(scarp::check(&source).unwrap_err().kind, kind);
    }
}

#[test]
fn imported_functions_keep_vector_and_float_metadata() {
    use scarp::modules::{self, MemorySourceLoader, ModuleLimits};
    let mut loader = MemorySourceLoader::new();
    loader
        .insert(
            "main.srp",
            "import \"model.srp\" as model; print(model.spread([1.0,-1.0]));",
        )
        .unwrap();
    loader
        .insert(
            "model.srp",
            "pub pure funcra spread(v: vector)->float {return variance(relu(v));}",
        )
        .unwrap();
    let program =
        modules::compile_with_loader("main.srp", &loader, ModuleLimits::default()).unwrap();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    program
        .execute(&mut out, &mut err, ExecutionOptions::default())
        .unwrap();
    assert_eq!(out, b"0.25\n");
    assert!(err.is_empty());
    let mut json = Vec::new();
    program.write_inspection_json(&mut json).unwrap();
    let json = String::from_utf8(json).unwrap();
    assert!(json.contains("\"result\":\"float\"") && json.contains("\"type\":\"vector\""));
}

#[test]
fn exact_statistical_diagnostics_agree_between_cli_and_embedding() {
    let folder = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/kernels");
    for name in ["empty", "overflow"] {
        let filename = format!("{name}.srp");
        let source = std::fs::read_to_string(folder.join(&filename)).unwrap();
        let expected = std::fs::read_to_string(folder.join(format!("{name}.stderr"))).unwrap();
        assert_eq!(
            format!(
                "{}\n",
                scarp::check(&source)
                    .unwrap_err()
                    .render(&filename, &source)
            ),
            expected
        );
        for mode in ["run", "check"] {
            let result = std::process::Command::new(env!("CARGO_BIN_EXE_scarp"))
                .current_dir(&folder)
                .args([mode, &filename])
                .output()
                .unwrap();
            assert_eq!(result.status.code(), Some(1));
            assert!(result.stdout.is_empty());
            assert_eq!(String::from_utf8(result.stderr).unwrap(), expected);
        }
        let result = std::process::Command::new(env!("CARGO_BIN_EXE_scarp"))
            .current_dir(&folder)
            .args(["modules", &filename])
            .output()
            .unwrap();
        assert!(result.status.success() && result.stderr.is_empty());
    }
}
