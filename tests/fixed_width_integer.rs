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
        "print(integer_lcm(1, 2, 3));",
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
