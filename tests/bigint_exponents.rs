use scarp::{
    diagnostics::DiagnosticKind,
    numeric::{evaluate, evaluate_with_options, Builtin},
    value::Value,
    ExecutionLimits, ExecutionOptions, RunError,
};

fn integer(text: &str) -> Value {
    evaluate(Builtin::BigInt, &[Value::from(text)], 1_000).unwrap()
}

#[test]
fn wide_expansion_handles_signs_zero_and_unit_bases() {
    let even = format!("1{}", "0".repeat(100));
    let odd = format!("{even}1");
    let source = format!(
        "bind even = bigint(\"{even}\"); bind odd = bigint(\"{odd}\"); \
        print(bigint_pow_big(bigint(\"-1\"), even), bigint_pow_big(bigint(\"-1\"), odd), \
        bigint_pow_big(bigint(\"0\"), even), bigint_pow_big(bigint(\"1\"), odd), \
        bigint_pow_big(bigint(\"0\"), bigint(\"0\")), bigint_pow_big(bigint(\"-10\"), bigint(\"17\")));"
    );
    let mut output = Vec::new();
    scarp::run(&source, &mut output).unwrap();
    assert_eq!(output, b"1 -1 0 1 1 -100000000000000000\n");
}

#[test]
fn exact_powers_fit_at_their_digit_boundary() {
    for base in [2_u128, 3, 6, 9, 10, 11, 99, 203, 999] {
        let big = integer(&base.to_string());
        for exponent in [2, 3, 5, 9] {
            let expected = base.pow(exponent).to_string();
            let options = ExecutionOptions {
                max_bigint_digits: expected.len(),
                ..ExecutionOptions::default()
            };
            let actual = evaluate_with_options(
                Builtin::BigIntPowBig,
                &[big.clone(), integer(&exponent.to_string())],
                options,
            )
            .unwrap();
            assert_eq!(actual.as_bigint().unwrap().to_string(), expected);
        }
    }
}

#[test]
fn proven_digit_and_byte_bounds_reject_before_squaring() {
    let work = ExecutionLimits {
        max_steps: 1,
        ..ExecutionLimits::default()
    };
    for (base, exponent, options, expected) in [
        (
            "999",
            "100",
            ExecutionOptions {
                max_bigint_digits: 250,
                limits: work,
                ..ExecutionOptions::default()
            },
            "digit limit",
        ),
        (
            "999999999",
            "100",
            ExecutionOptions {
                max_bigint_bytes: 390,
                limits: work,
                ..ExecutionOptions::default()
            },
            "allocation limit",
        ),
        (
            "2",
            "340282366920938463463374607431768211455",
            ExecutionOptions {
                limits: work,
                ..ExecutionOptions::default()
            },
            "addressable result size",
        ),
        (
            "2",
            "340282366920938463463374607431768211456",
            ExecutionOptions {
                limits: work,
                ..ExecutionOptions::default()
            },
            "addressable result size",
        ),
    ] {
        let error = evaluate_with_options(
            Builtin::BigIntPowBig,
            &[integer(base), integer(exponent)],
            options,
        )
        .unwrap_err();
        assert_eq!(error.kind, DiagnosticKind::BigIntLimit);
        assert!(error.message.contains(expected), "{error:?}");
    }
}

#[test]
fn unchanged_value_and_first_factor_can_share_their_limb_storage() {
    let base = integer("123456789012345678901234567890");
    let options = ExecutionOptions {
        max_bigint_bytes: 0,
        ..ExecutionOptions::default()
    };
    assert_eq!(
        evaluate_with_options(
            Builtin::BigIntPow,
            &[base.clone(), Value::Integer(1)],
            options
        )
        .unwrap(),
        base
    );
    assert_eq!(
        evaluate_with_options(
            Builtin::BigIntPowBig,
            &[base.clone(), integer("1")],
            options
        )
        .unwrap(),
        base
    );
}

#[test]
fn wide_modular_exponents_match_independent_integer_oracles() {
    // Expected values were produced with Python's three-argument pow.
    let sparse = format!("1{}7", "0".repeat(98));
    let dense = "9".repeat(150);
    for (base, exponent, modulus, expected) in [
        (
            "-12345678901234567890987654321",
            "340282366920938463463374607431768211455",
            "4294967291",
            "4081334919",
        ),
        (
            "-12345678901234567890987654321",
            "340282366920938463463374607431768211456",
            "4294967291",
            "3252763948",
        ),
        ("1234567", &sparse, "1009", "200"),
        ("-9876543210987654321", &dense, "1000000007", "999748919"),
        ("0", &dense, "101", "0"),
        ("0", "0", "101", "1"),
        ("1234", &dense, "1", "0"),
    ] {
        let result = evaluate(
            Builtin::BigIntPowModBig,
            &[integer(base), integer(exponent), integer(modulus)],
            1_000_000,
        )
        .unwrap();
        assert_eq!(result.as_bigint().unwrap().to_string(), expected);
    }
}

#[test]
fn source_types_domain_and_quota_failures_are_atomic() {
    for source in [
        "bigint_pow_big(bigint(\"2\"), 3)",
        "bigint_pow_mod_big(bigint(\"2\"), bigint(\"3\"), 7)",
    ] {
        assert!(scarp::compile(&format!("print({source});")).is_err());
    }
    for (source, kind) in [
        ("bigint_pow_big(bigint(\"2\"), bigint(\"-1\"))", DiagnosticKind::NumericDomain),
        ("bigint_pow_mod_big(bigint(\"2\"), bigint(\"-1\"), bigint(\"7\"))", DiagnosticKind::NumericDomain),
        ("bigint_pow_mod_big(bigint(\"2\"), bigint(\"0\"), bigint(\"0\"))", DiagnosticKind::NumericDomain),
        ("bigint_pow_mod_big(bigint(\"2\"), bigint(\"0\"), bigint(\"-3\"))", DiagnosticKind::NumericDomain),
        ("bigint_pow_big(bigint(\"2\"), bigint(\"10000000000000000000000000000000000000000000000\"))", DiagnosticKind::BigIntLimit),
    ] {
        let mut output = Vec::new();
        let error = scarp::run(&format!("print(\"pending\"); print({source});"), &mut output).unwrap_err();
        assert!(matches!(error, RunError::Diagnostic(error) if error.kind == kind));
        assert!(output.is_empty());
    }
    let huge = integer(&"9".repeat(80));
    let error = evaluate(
        Builtin::BigIntPowModBig,
        &[integer("23"), huge, integer("997")],
        1_000,
    )
    .unwrap_err();
    assert_eq!(error.kind, DiagnosticKind::ExecutionLimit);
}
