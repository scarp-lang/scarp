use scarp::{
    diagnostics::DiagnosticKind as Kind,
    numeric::{self, Builtin as B},
    value::{Value, ValueType},
    ExecutionOptions, RunError,
};

fn host(function: B, values: &[i64], work: u64) -> Result<i64, scarp::diagnostics::Diagnostic> {
    let arguments: Vec<_> = values.iter().copied().map(Value::Integer).collect();
    numeric::evaluate(function, &arguments, work).map(|value| value.as_integer().unwrap())
}

fn output(source: &str) -> String {
    let mut bytes = Vec::new();
    scarp::run(source, &mut bytes).unwrap();
    String::from_utf8(bytes).unwrap()
}

#[test]
fn exact_integer_builtins_have_pure_typed_signatures() {
    for (name, builtin, arity) in [
        ("integer_abs", B::IntegerAbs, 1),
        ("integer_sign", B::IntegerSign, 1),
        ("integer_gcd", B::IntegerGcd, 2),
        ("integer_hcf", B::IntegerGcd, 2),
        ("integer_lcm", B::IntegerLcm, 2),
    ] {
        assert_eq!(B::from_name(name), Some(builtin));
        assert_eq!(builtin.result_type(), ValueType::Integer);
        assert_eq!(builtin.parameter_types().len(), arity);
        assert!(builtin
            .parameter_types()
            .iter()
            .all(|kind| *kind == ValueType::Integer));
        assert_eq!(
            numeric::evaluate(builtin, &[], 100).unwrap_err().kind,
            Kind::CallArguments,
        );
    }
    for source in [
        "print(integer_abs(1.0));",
        "print(integer_sign(true));",
        "print(integer_gcd(3, 1.0));",
        "print(integer_lcm(false, 2));",
        "print(integer_abs());",
        "print(integer_gcd(2));",
        "print(integer_lcm(1));",
        "print(integer_hcf());",
        "print(integer_hcf(1, 2, false));",
        "print(integer_gcd(1, 2, 3.0));",
        "print(integer_lcm(1, 2, \"3\"));",
        "print(integer_hcf(a: 1, 2));",
        "bind a: boolean=integer_sign(0);",
    ] {
        let kind = scarp::compile(source).unwrap_err().kind;
        assert!(
            matches!(kind, Kind::TypeMismatch | Kind::CallArguments),
            "{source}"
        );
    }
}

#[test]
fn absolute_value_and_sign_cover_entire_signed_domain() {
    for (value, sign) in [
        (i64::MIN, -1),
        (i64::MIN + 1, -1),
        (-77, -1),
        (-1, -1),
        (0, 0),
        (1, 1),
        (77, 1),
        (i64::MAX, 1),
    ] {
        assert_eq!(host(B::IntegerSign, &[value], 1).unwrap(), sign);
        if value == i64::MIN {
            assert_eq!(
                host(B::IntegerAbs, &[value], 1).unwrap_err().kind,
                Kind::ArithmeticOverflow,
            );
        } else {
            assert_eq!(host(B::IntegerAbs, &[value], 1).unwrap(), value.abs());
        }
    }
}

#[test]
fn gcd_and_lcm_follow_nonnegative_exact_integer_conventions() {
    for (left, right, gcd, lcm) in [
        (0, 0, 0, 0),
        (0, 12, 12, 0),
        (-12, 0, 12, 0),
        (54, -24, 6, 216),
        (-54, -24, 6, 216),
        (7, 13, 1, 91),
        (21, 6, 3, 42),
        (i64::MIN, 2, 2, 0),
        (i64::MIN, 3, 1, 0),
    ] {
        assert_eq!(host(B::IntegerGcd, &[left, right], 256).unwrap(), gcd);
        if left == i64::MIN && right != 0 {
            assert_eq!(
                host(B::IntegerLcm, &[left, right], 256).unwrap_err().kind,
                Kind::ArithmeticOverflow
            );
        } else {
            assert_eq!(host(B::IntegerLcm, &[left, right], 256).unwrap(), lcm);
        }
        assert_eq!(host(B::IntegerGcd, &[right, left], 256).unwrap(), gcd);
    }
}

#[test]
fn integer_overflow_is_explicit_for_unrepresentable_magnitudes() {
    for (builtin, values) in [
        (B::IntegerAbs, vec![i64::MIN]),
        (B::IntegerGcd, vec![i64::MIN, 0]),
        (B::IntegerGcd, vec![i64::MIN, i64::MIN]),
        (B::IntegerLcm, vec![i64::MIN, 1]),
        (B::IntegerLcm, vec![i64::MAX, 2]),
        (B::IntegerLcm, vec![i64::MIN, i64::MIN]),
    ] {
        let error = host(builtin, &values, 256).unwrap_err();
        assert_eq!(error.kind, Kind::ArithmeticOverflow);
    }
    assert_eq!(host(B::IntegerLcm, &[0, i64::MIN], 256).unwrap(), 0);
    assert_eq!(host(B::IntegerGcd, &[i64::MIN, 2], 256).unwrap(), 2);
}

#[test]
fn work_budgets_charge_each_euclidean_remainder() {
    for builtin in [B::IntegerAbs, B::IntegerSign] {
        assert!(host(builtin, &[5], 1).is_ok());
        assert_eq!(
            host(builtin, &[5], 0).unwrap_err().kind,
            Kind::ExecutionLimit
        );
    }
    for builtin in [B::IntegerGcd, B::IntegerLcm] {
        assert!(host(builtin, &[54, 24], 3).is_ok());
        assert_eq!(
            host(builtin, &[54, 24], 2).unwrap_err().kind,
            Kind::ExecutionLimit
        );
        assert!(host(builtin, &[0, 0], 1).is_ok());
        assert_eq!(
            host(builtin, &[0, 0], 0).unwrap_err().kind,
            Kind::ExecutionLimit
        );
    }
}

#[test]
fn source_functions_and_user_shadowing_preserve_integer_types() {
    assert_eq!(
        output("pure funcra common(a:integer,b:integer)->integer{return integer_gcd(a,b);} print(integer_abs(-12),integer_sign(-12),common(54,-24),integer_lcm(21,6));"),
        "12 -1 6 42\n",
    );
    assert_eq!(
        output("pure funcra integer_abs(v:integer)->integer{return v+1;} print(integer_abs(2));"),
        "3\n",
    );
}

#[test]
fn overflow_has_precise_argument_span_and_preserves_atomic_output() {
    let source = "print(1); eprint(2); print(integer_abs(-9223372036854775808));";
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let RunError::Diagnostic(error) = scarp::run_with_io_and_options(
        source,
        &mut stdout,
        &mut stderr,
        ExecutionOptions::default(),
    )
    .unwrap_err() else {
        panic!("expected an arithmetic diagnostic");
    };
    assert_eq!(error.kind, Kind::ArithmeticOverflow);
    assert_eq!(
        &source[error.span.start..error.span.end],
        "-9223372036854775808"
    );
    assert!(stdout.is_empty() && stderr.is_empty());
}

#[test]
fn seeded_reference_matches_i128_magnitude_arithmetic_without_truncation() {
    let mut state = 0x15c4_2ab9_37df_e681u64;
    for _ in 0..2048 {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        let left = state as i64;
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        let right = state as i64;
        let (mut a, mut b) = ((left as i128).abs(), (right as i128).abs());
        while b != 0 {
            (a, b) = (b, a % b);
        }
        let gcd = a;
        let lcm = if gcd == 0 {
            0
        } else {
            ((left as i128).abs() / gcd) * (right as i128).abs()
        };
        for (builtin, expected) in [(B::IntegerGcd, gcd), (B::IntegerLcm, lcm)] {
            let actual = host(builtin, &[left, right], 256);
            if expected > i64::MAX as i128 {
                assert_eq!(actual.unwrap_err().kind, Kind::ArithmeticOverflow);
            } else {
                assert_eq!(actual.unwrap(), expected as i64);
            }
        }
    }
}

#[test]
fn incompatible_host_types_and_unsupported_native_emitters_are_rejected() {
    for function in [B::IntegerAbs, B::IntegerSign, B::IntegerGcd, B::IntegerLcm] {
        let arguments = vec![Value::Boolean(false); function.parameter_types().len()];
        assert_eq!(
            numeric::evaluate(function, &arguments, 100)
                .unwrap_err()
                .kind,
            Kind::TypeMismatch
        );
    }
    for source in [
        "print(integer_abs(-2));",
        "print(integer_gcd(12,8));",
        "print(integer_lcm(12,8));",
        "print(integer_hcf(12,8,4));",
        "print(integer_lcm(12,8,4));",
    ] {
        assert_eq!(
            scarp::c_backend::emit_c(source).unwrap_err().kind,
            Kind::NativeUnsupported
        );
        assert_eq!(
            scarp::native_object::emit_elf64_integer_kernel(source)
                .unwrap_err()
                .kind,
            Kind::NativeUnsupported,
        );
    }
}

#[test]
fn variadic_integer_reductions_and_hcf_alias() {
    assert_eq!(
        output("print(integer_hcf(12,18),integer_hcf(12,18,24),integer_gcd(24,36,48,60),integer_lcm(4,6,8,12));"),
        "6 6 12 24\n"
    );
    assert_eq!(
        output("pure funcra common(a:integer,b:integer=integer_hcf(12,18,24))->integer{return integer_gcd(a,b,18);} print(common(30));"),
        "6\n"
    );
    assert_eq!(
        output("pure funcra integer_hcf(a:integer,b:integer)->integer{return a+b;} print(integer_hcf(2,3),integer_gcd(2,3,4));"),
        "5 1\n"
    );
    for (values, expected_gcd, expected_lcm) in [
        (vec![0, 0, 0], 0, 0),
        (vec![-12, 18, -24], 6, 72),
        (vec![25, 0, 15], 5, 0),
        (vec![7, 11, 13], 1, 1001),
        (vec![9, 9, 9, 9], 9, 9),
    ] {
        assert_eq!(host(B::IntegerGcd, &values, 256).unwrap(), expected_gcd);
        assert_eq!(host(B::IntegerLcm, &values, 256).unwrap(), expected_lcm);
        let reversed: Vec<_> = values.iter().rev().copied().collect();
        assert_eq!(host(B::IntegerGcd, &reversed, 256).unwrap(), expected_gcd);
        assert_eq!(host(B::IntegerLcm, &reversed, 256).unwrap(), expected_lcm);
    }
}

#[test]
fn variadic_integer_extremes_and_zero_are_order_independent() {
    assert_eq!(host(B::IntegerGcd, &[i64::MIN, 0, 2], 256).unwrap(), 2);
    assert_eq!(
        host(B::IntegerGcd, &[i64::MIN, i64::MIN, 0], 256)
            .unwrap_err()
            .kind,
        Kind::ArithmeticOverflow
    );
    for values in [[i64::MAX, 2, 0], [0, i64::MAX, 2], [i64::MIN, 0, i64::MAX]] {
        assert_eq!(host(B::IntegerLcm, &values, 256).unwrap(), 0);
    }
    assert_eq!(
        host(B::IntegerLcm, &[i64::MAX, 2, 3], 256)
            .unwrap_err()
            .kind,
        Kind::ArithmeticOverflow
    );
    assert_eq!(
        numeric::evaluate(
            B::IntegerLcm,
            &[Value::Integer(0), Value::Integer(1), Value::Boolean(false)],
            256,
        )
        .unwrap_err()
        .kind,
        Kind::TypeMismatch
    );
}

#[test]
fn variadic_arity_and_work_limits_are_enforced() {
    for function in [B::IntegerGcd, B::IntegerLcm] {
        assert!(function.is_variadic());
        assert!(!function.accepts_arity(1));
        assert!(function.accepts_arity(1024));
        assert!(!function.accepts_arity(1025));
        assert_eq!(function.parameter_type_at(1023), Some(ValueType::Integer));
        assert_eq!(function.parameter_type_at(1024), None);
        assert_eq!(host(function, &[0; 1024], 1023).unwrap(), 0);
        assert_eq!(
            host(function, &[0; 1024], 1022).unwrap_err().kind,
            Kind::ExecutionLimit
        );
        assert_eq!(
            host(function, &[0; 1025], 4096).unwrap_err().kind,
            Kind::CallArguments
        );
    }
    assert!(!B::IntegerAbs.is_variadic());
    assert!(!B::IntegerAbs.accepts_arity(2));
    let args = vec!["0"; 1024].join(",");
    assert_eq!(output(&format!("print(integer_hcf({args}));")), "0\n");
    let args = vec!["0"; 1025].join(",");
    assert!(scarp::compile(&format!("print(integer_lcm({args}));")).is_err());
}

#[test]
fn variadic_typed_metadata_tracks_each_concrete_call_signature() {
    use scarp::ast::ExpressionKind;
    use scarp::types::{FunctionEffect, TypeKind};
    let program =
        scarp::compile("print(integer_hcf(12,18,24),integer_gcd(12,18),integer_lcm(4,6,8,12));")
            .unwrap();
    let mut arities = Vec::new();
    for expression in program.typed_expressions() {
        for node in expression.nodes() {
            if let ExpressionKind::UserCall { arguments, .. } = node.syntax {
                let signature = node.call_signature.unwrap();
                assert!(matches!(
                    program.type_registry().get(signature),
                    Some(TypeKind::Function { parameters, result, effect: FunctionEffect::Pure })
                        if parameters.len() == arguments.len()
                            && parameters.iter().all(|kind| *kind == ValueType::Integer.id())
                            && *result == ValueType::Integer.id()
                ));
                arities.push(arguments.len());
            }
        }
    }
    assert_eq!(arities, vec![3, 2, 4]);
    assert_ne!(
        program.builtin_type_id_for_arity(B::IntegerGcd, 2),
        program.builtin_type_id_for_arity(B::IntegerGcd, 3)
    );
    assert_eq!(
        program.builtin_type_id(B::IntegerGcd),
        program.builtin_type_id_for_arity(B::IntegerGcd, 2)
    );
    assert_eq!(program.builtin_type_id_for_arity(B::IntegerGcd, 4), None);
}
