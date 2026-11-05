use scarp::{
    diagnostics::DiagnosticKind as K,
    modules::{self, MemorySourceLoader, ModuleLimits},
    numeric::{self, Builtin as B},
    value::{NumericVector, Value},
    ExecutionOptions,
};
fn vector(values: &[f64]) -> Value {
    Value::Vector(NumericVector::new(values).unwrap())
}
fn rms(values: &[f64]) -> f64 {
    numeric::evaluate(B::Rms, &[vector(values)], u64::MAX)
        .unwrap()
        .as_float()
        .unwrap()
}
fn near(a: f64, b: f64) {
    assert!(
        (a - b).abs() <= a.abs().max(b.abs()) * 8.0 * f64::EPSILON,
        "{a:?} != {b:?}"
    );
}
#[test]
fn finite_rms_avoids_norm_and_squared_sum_overflow() {
    assert_eq!(rms(&[1e308, 1e308]), 1e308);
    assert_eq!(rms(&[-f64::MAX, f64::MAX]), f64::MAX);
    assert_eq!(
        numeric::evaluate(B::Norm, &[vector(&[f64::MAX, f64::MAX])], u64::MAX)
            .unwrap_err()
            .kind,
        K::NonFiniteNumber
    );
    near(rms(&[3.0, 4.0]), 12.5_f64.sqrt());
    near(rms(&[0.0, f64::MAX]), f64::MAX / 2.0_f64.sqrt());
}
#[test]
fn zero_sign_singletons_and_subnormals_remain_finite_and_nonnegative() {
    assert_eq!(rms(&[0.0, -0.0]).to_bits(), 0.0_f64.to_bits());
    for value in [
        0.0,
        -0.0,
        1.0,
        -3.0,
        f64::MAX,
        f64::MIN_POSITIVE,
        f64::from_bits(1),
        -f64::from_bits(1),
    ] {
        assert_eq!(rms(&[value]).to_bits(), value.abs().to_bits());
    }
    let tiny = f64::from_bits(1);
    assert_eq!(rms(&[tiny, -tiny]), tiny);
    assert_eq!(
        rms(&[f64::MIN_POSITIVE, -f64::MIN_POSITIVE]),
        f64::MIN_POSITIVE
    );
}
#[test]
fn native_work_and_allocation_limits_have_exact_boundaries() {
    let value = vector(&[3.0, 4.0]);
    numeric::evaluate_with_vector_limit(B::Rms, std::slice::from_ref(&value), 8, 0).unwrap();
    assert_eq!(
        numeric::evaluate_with_vector_limit(B::Rms, &[value], 7, 0)
            .unwrap_err()
            .kind,
        K::ExecutionLimit
    );
    let error = numeric::evaluate(B::Rms, &[vector(&[])], 0).unwrap_err();
    assert_eq!(error.kind, K::NumericDomain);
    assert!(error.message.contains("nonempty"));
}
#[test]
fn visible_windows_exclude_hidden_extremes_and_preserve_inputs() {
    let original = vector(&[1e308, 3.0, 4.0, 1e308]);
    let window = numeric::evaluate(
        B::Window,
        &[original.clone(), Value::Integer(1), Value::Integer(2)],
        u64::MAX,
    )
    .unwrap();
    let actual = numeric::evaluate(B::Rms, std::slice::from_ref(&window), 8)
        .unwrap()
        .as_float()
        .unwrap();
    near(actual, 12.5_f64.sqrt());
    assert_eq!(window.as_vector().unwrap().len(), 2);
    assert_eq!(original.as_vector().unwrap().len(), 4);
}
#[test]
fn static_types_signatures_shadowing_and_unsupported_native_subset_are_preserved() {
    for source in [
        "print(rms(1));",
        "print(rms([integer:1]));",
        "bind x: integer=rms([1.0]);",
    ] {
        assert_eq!(scarp::compile(source).unwrap_err().kind, K::TypeMismatch);
    }
    assert_eq!(
        scarp::compile("print(rms());").unwrap_err().kind,
        K::CallArguments
    );
    assert_eq!(
        scarp::compile("print(rms(values: [1.0]));")
            .unwrap_err()
            .kind,
        K::CallArguments
    );
    let program = scarp::compile("print(rms([1.0]),norm([1.0]));").unwrap();
    assert_eq!(
        program.builtin_type_id(B::Rms),
        program.builtin_type_id(B::Norm)
    );
    let mut out = Vec::new();
    scarp::run(
        "pure funcra rms(x: integer)->integer{return x+1;} print(rms(3));",
        &mut out,
    )
    .unwrap();
    assert_eq!(out, b"4\n");
    assert_eq!(
        scarp::c_backend::emit_c("print(rms([1.0]));")
            .unwrap_err()
            .kind,
        K::NativeUnsupported
    );
}
#[test]
fn source_failures_are_atomic_and_point_at_the_empty_argument() {
    let source = "print(\"OUT\"); eprint(\"ERR\"); print(rms([]));";
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let error = scarp::run_with_io(source, &mut out, &mut err).unwrap_err();
    let scarp::RunError::Diagnostic(d) = error else {
        panic!("expected diagnostic")
    };
    assert!(out.is_empty() && err.is_empty());
    assert_eq!(d.kind, K::NumericDomain);
    assert_eq!(&source[d.span.start..d.span.end], "[]");
}
#[test]
fn module_functions_defaults_contracts_and_navigation_share_the_builtin() {
    let mut loader = MemorySourceLoader::new();
    loader
        .insert("main.srp", "import \"lib.srp\" as l; print(l.level());")
        .unwrap();
    loader.insert("lib.srp","pub pure funcra level(v: vector=[3.0,4.0])->float ensures result>=0.0; {return rms(v);}").unwrap();
    let p = modules::compile_indexed_with_loader(
        "main.srp",
        &loader,
        ModuleLimits::default(),
        Default::default(),
    )
    .unwrap();
    let symbol = p
        .index()
        .symbols()
        .iter()
        .find(|d| d.name.as_ref() == "rms")
        .unwrap();
    assert!(symbol.location.is_none());
    assert_eq!(p.index().find_references(symbol.id).count(), 1);
    let (mut out, mut err) = (Vec::new(), Vec::new());
    p.program()
        .execute(&mut out, &mut err, ExecutionOptions::default())
        .unwrap();
    assert!(err.is_empty());
    near(
        String::from_utf8(out).unwrap().trim().parse().unwrap(),
        12.5_f64.sqrt(),
    );
    loader = MemorySourceLoader::new();
    loader.insert("main.srp", "print(rms([]));").unwrap();
    modules::compile_indexed_with_loader(
        "main.srp",
        &loader,
        Default::default(),
        Default::default(),
    )
    .unwrap();
}
#[test]
fn moderate_generated_inputs_match_a_direct_independent_formula() {
    let mut state = 19_u64;
    for len in 1..40 {
        let mut values = Vec::new();
        for _ in 0..len {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
            values.push(((state >> 32) % 20001) as f64 / 100.0 - 100.0);
        }
        let expected = (values.iter().map(|x| x * x).sum::<f64>() / len as f64).sqrt();
        near(rms(&values), expected);
    }
}

#[test]
fn empty_domain_has_exact_matching_cli_and_embedding_diagnostics() {
    let folder = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/rms");
    let source = std::fs::read_to_string(folder.join("empty.srp")).unwrap();
    let expected = std::fs::read_to_string(folder.join("empty.stderr")).unwrap();
    assert_eq!(
        format!(
            "{}\n",
            scarp::check(&source)
                .unwrap_err()
                .render("empty.srp", &source)
        ),
        expected
    );
    for command in ["exec", "check"] {
        let result = std::process::Command::new(env!("CARGO_BIN_EXE_scarp"))
            .current_dir(&folder)
            .args([command, "empty.srp"])
            .output()
            .unwrap();
        assert_eq!(result.status.code(), Some(1));
        assert!(result.stdout.is_empty());
        assert_eq!(String::from_utf8(result.stderr).unwrap(), expected);
    }
}
