use scarp::{
    diagnostics::{DiagnosticKind as Kind, Span},
    numeric::{self, Builtin},
    types::PrimitiveType,
    value::Value,
    ExecutionLimits, ExecutionOptions, RunError,
};

fn parse(text: &str) -> Value {
    numeric::evaluate(Builtin::BigInt, &[Value::from(text)], 1_000_000).unwrap()
}
fn run(source: &str) -> String {
    let mut output = Vec::new();
    scarp::run(source, &mut output).unwrap();
    String::from_utf8(output).unwrap()
}
fn fail(source: &str, options: ExecutionOptions) -> scarp::diagnostics::Diagnostic {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let RunError::Diagnostic(error) =
        scarp::run_with_io_and_options(source, &mut out, &mut err, options).unwrap_err()
    else {
        panic!("expected diagnostic")
    };
    assert!(
        out.is_empty() && err.is_empty(),
        "output must remain atomic"
    );
    let checked = scarp::check_with_options(source, options).unwrap_err();
    assert_eq!(checked, error);
    error
}

#[test]
fn canonical_decimal_and_signed_zero() {
    for (input, expected) in [
        ("0", "0"),
        ("-0", "0"),
        ("+0000", "0"),
        ("0000000001", "1"),
        ("-0001000000000", "-1000000000"),
        (
            "9999999991000000000000000001",
            "9999999991000000000000000001",
        ),
    ] {
        let value = parse(input);
        assert_eq!(value.kind(), PrimitiveType::BigInt);
        assert_eq!(value.to_string(), expected);
        let big = value.as_bigint().unwrap();
        assert_eq!(big.decimal_digits(), expected.trim_start_matches('-').len());
        assert_eq!(big.is_negative(), expected.starts_with('-'));
    }
    assert_eq!(parse("-000"), parse("+0"));
}

#[test]
fn malformed_decimal_inputs_have_dedicated_diagnostics() {
    for text in [
        "", "+", "-", " 1", "1 ", "1_0", "1.0", "0x10", "1e4", "--1", "१२", "١٢", "1\n", "1\0",
    ] {
        assert_eq!(
            numeric::evaluate(Builtin::BigInt, &[Value::from(text)], 100)
                .unwrap_err()
                .kind,
            Kind::InvalidBigInt,
            "{text:?}"
        );
    }
    let source = "print(\"हिन्दी\"); print(bigint(\"12é\"));";
    let error = fail(source, ExecutionOptions::default());
    assert_eq!(error.kind, Kind::InvalidBigInt);
    assert_eq!(&source[error.span.start..error.span.end], "\"12é\"");
    assert!(error.render("big.srp", source).contains("E0051"));
}

#[test]
fn exact_small_integer_conversion_handles_both_boundaries() {
    for n in [i64::MIN, -1, 0, 1, i64::MAX] {
        let value = numeric::evaluate(Builtin::BigIntFromInteger, &[Value::Integer(n)], 3).unwrap();
        assert_eq!(value.to_string(), n.to_string());
        assert_eq!(value.as_bigint().unwrap().to_integer(), Some(n));
        assert_eq!(
            numeric::evaluate(Builtin::BigIntToInteger, &[value], 1).unwrap(),
            Value::Integer(n)
        );
    }
    for text in [
        "9223372036854775808",
        "-9223372036854775809",
        "100000000000000000000000000000000000000",
    ] {
        let value = parse(text);
        assert_eq!(value.as_bigint().unwrap().to_integer(), None);
        assert_eq!(
            numeric::evaluate(Builtin::BigIntToInteger, &[value], 1)
                .unwrap_err()
                .kind,
            Kind::NumericConversion
        );
    }
    assert_eq!(
        numeric::evaluate(Builtin::BigIntFromInteger, &[Value::Integer(1)], 2)
            .unwrap_err()
            .kind,
        Kind::ExecutionLimit
    );
}

#[test]
fn arithmetic_carries_borrows_products_and_negation() {
    assert_eq!(run(r#"
        bind a: bigint = bigint("999999999999999999999999999999999999");
        bind b = bigint("1");
        print(a+b); print(-a+b); print(a-a); print(-bigint("0"));
        print(bigint("1000000000000000000000000000")-b);
        print(bigint("999999999")*bigint("999999999"));
        print(bigint("-1000000000000000000")*bigint("1000000000"));
        print(bigint_abs(-a));
    "#), "1000000000000000000000000000000000000\n-999999999999999999999999999999999998\n0\n0\n999999999999999999999999999\n999999998000000001\n-1000000000000000000000000000\n999999999999999999999999999999999999\n");
}

#[test]
fn division_truncates_toward_zero_and_remainder_follows_dividend() {
    for a in [
        -1000000000000000000000001_i128,
        -20,
        -7,
        -1,
        0,
        1,
        7,
        20,
        1000000000000000000000001,
    ] {
        for b in [-1000000000000000001_i128, -7, -1, 1, 7, 1000000000000000001] {
            let source = format!("bind a=bigint(\"{a}\"); bind b=bigint(\"{b}\"); print(a/b); print(a%b); print((a/b)*b+a%b == a);");
            assert_eq!(
                run(&source),
                format!("{}\n{}\ntrue\n", a / b, a % b),
                "{a}/{b}"
            );
        }
    }
}

#[test]
fn order_comparisons_are_signed_and_exact() {
    for (a, b) in [
        ("-10000000000000000000", "-9999999999999999999"),
        ("-1", "0"),
        ("0", "1"),
        ("999999999", "1000000000"),
    ] {
        assert!(parse(a) != parse(b));
        assert!(parse(a).as_bigint().unwrap() < parse(b).as_bigint().unwrap());
        assert_eq!(run(&format!("bind a=bigint(\"{a}\"); bind b=bigint(\"{b}\"); print(a<b,a<=b,a>b,a>=b,a==b,a!=b);")), "true true false false false true\n");
    }
}

#[test]
fn typed_bindings_functions_defaults_contracts_and_updates() {
    assert_eq!(
        run(r#"
        pure funcra twice(n: bigint, extra: bigint = bigint("2")) -> bigint
        needs n >= bigint("0");
        ensures result >= n;
        { return n * extra; }
        var n: bigint = bigint("100000000000000000000");
        bind old = n; n += bigint("5"); n -= bigint("2");
        n *= bigint("2"); n /= bigint("3"); n %= bigint("11");
        print(old,n,twice(bigint("12345678901234567890")));
    "#),
        "100000000000000000000 2 24691357802469135780\n"
    );
}

#[test]
fn fibonacci_and_factorial_work_in_loops() {
    assert_eq!(
        run(r#"
        var a=bigint("0"); var b=bigint("1");
        repeat 100 as index { bind next=a+b; a=b; b=next; }
        print(a);
        var total=bigint("1"); var i=1;
        while i<=30 { total *= bigint_from_integer(i); i+=1; }
        print(total);
    "#),
        "354224848179261915075\n265252859812191058636308480000000\n"
    );
}

#[test]
fn unsupported_implicit_conversions_and_bit_operations_are_static_errors() {
    for expression in [
        "bigint(1)",
        "bigint_from_integer(1.0)",
        "to_float(bigint(\"1\"))",
        "to_integer(bigint(\"1\"))",
        "bigint(\"1\")+1",
        "bigint(\"1\")==1",
        "bigint(\"1\")+1.0",
        "bigint(\"1\")&bigint(\"1\")",
        "bigint(\"1\")<<bigint(\"1\")",
        "~bigint(\"1\")",
    ] {
        assert_eq!(
            scarp::compile(&format!("print({expression});"))
                .unwrap_err()
                .kind,
            Kind::TypeMismatch,
            "{expression}"
        );
    }
    assert_eq!(
        scarp::compile("bind x=bigint(\"1\"); x=bigint(\"2\");")
            .unwrap_err()
            .kind,
        Kind::ImmutableAssignment
    );
    assert_eq!(
        scarp::compile("var x=bigint(\"1\"); x &= bigint(\"2\");")
            .unwrap_err()
            .kind,
        Kind::TypeMismatch
    );
}

#[test]
fn integer_overflow_and_float_semantics_are_preserved() {
    for source in [
        "print(9223372036854775807+1);",
        "print(-9223372036854775808/-1);",
        "print(-(-9223372036854775808));",
    ] {
        assert_eq!(
            scarp::check(source).unwrap_err().kind,
            Kind::ArithmeticOverflow
        );
    }
    assert_eq!(run("print(7/3,-7%3,1.5+2.0);"), "2 -1 3.5\n");
    assert_eq!(
        scarp::compile("print(9223372036854775808);")
            .unwrap_err()
            .kind,
        Kind::InvalidInteger
    );
}

#[test]
fn division_failure_and_precision_loss_discard_both_streams() {
    for (expression, kind) in [
        ("bigint(\"7\")/bigint(\"-0\")", Kind::DivisionByZero),
        ("bigint(\"7\")%bigint(\"0\")", Kind::DivisionByZero),
        (
            "bigint_to_integer(bigint(\"9223372036854775808\"))",
            Kind::NumericConversion,
        ),
    ] {
        let source = format!("print(\"pending\"); eprint(\"also pending\"); print({expression});");
        assert_eq!(fail(&source, ExecutionOptions::default()).kind, kind);
    }
}

#[test]
fn input_result_and_cumulative_scratch_limits_are_independent() {
    let options = ExecutionOptions {
        max_bigint_digits: 9,
        ..ExecutionOptions::default()
    };
    assert!(scarp::check_with_options("print(bigint(\"999999999\"));", options).is_ok());
    for source in [
        "print(bigint(\"0000000000\"));",
        "print(bigint(\"999999999\")+bigint(\"1\"));",
        "print(bigint(\"100000000\")*bigint(\"10\"));",
    ] {
        assert_eq!(fail(source, options).kind, Kind::BigIntLimit);
    }
    assert!(scarp::check_with_options("print(bigint(\"1\")-bigint(\"1\"));", options).is_ok());
    let options = ExecutionOptions {
        max_bigint_bytes: 3,
        ..ExecutionOptions::default()
    };
    assert_eq!(
        fail("print(bigint(\"1\"));", options).kind,
        Kind::BigIntLimit
    );
    let options = ExecutionOptions {
        max_bigint_bytes: 4,
        ..options
    };
    assert!(scarp::check_with_options(
        "bind x=bigint(\"1\"); bind y=x; print(-y,bigint_abs(y));",
        options
    )
    .is_ok());
    assert_eq!(
        fail("bind x=bigint(\"1\"); bind y=bigint(\"1\");", options).kind,
        Kind::BigIntLimit
    );
    // Inputs reserve 24 bytes; division needs reusable quotient/remainder/trial
    // buffers. A quota sufficient only for operands must still reject division.
    let options = ExecutionOptions {
        max_bigint_bytes: 24,
        ..ExecutionOptions::default()
    };
    assert_eq!(fail("print(bigint(\"999999999999999999999999999999999999\")/bigint(\"999999999999999999\"));",options).kind,Kind::BigIntLimit);
}

#[test]
fn host_parser_has_exact_work_and_allocation_boundaries() {
    let arguments = [Value::from("1000000000")];
    let options = ExecutionOptions {
        max_bigint_bytes: 8,
        limits: ExecutionLimits {
            max_steps: 10,
            ..ExecutionLimits::default()
        },
        ..ExecutionOptions::default()
    };
    assert_eq!(
        numeric::evaluate_with_options(Builtin::BigInt, &arguments, options)
            .unwrap()
            .to_string(),
        "1000000000"
    );
    assert_eq!(
        numeric::evaluate_with_options(
            Builtin::BigInt,
            &arguments,
            ExecutionOptions {
                max_bigint_bytes: 7,
                ..options
            }
        )
        .unwrap_err()
        .kind,
        Kind::BigIntLimit
    );
    assert_eq!(
        numeric::evaluate_with_options(
            Builtin::BigInt,
            &arguments,
            ExecutionOptions {
                limits: ExecutionLimits {
                    max_steps: 9,
                    ..options.limits
                },
                ..options
            }
        )
        .unwrap_err()
        .kind,
        Kind::ExecutionLimit
    );
    assert_eq!(
        numeric::evaluate(Builtin::BigInt, &[], 100)
            .unwrap_err()
            .kind,
        Kind::CallArguments
    );
    assert_eq!(
        numeric::evaluate(Builtin::BigInt, &[Value::Integer(1)], 100)
            .unwrap_err()
            .span,
        Span::default()
    );
}

#[test]
fn output_captures_bigint_payload_and_formatting_work() {
    let source = "print(bigint(\"1234567890123456789012345678901234567890\"));";
    let error = fail(
        source,
        ExecutionOptions {
            max_capture_bytes: 90,
            ..ExecutionOptions::default()
        },
    );
    assert_eq!(error.kind, Kind::CaptureLimit);
    let options = ExecutionOptions {
        limits: ExecutionLimits {
            max_steps: 50,
            ..ExecutionLimits::default()
        },
        ..ExecutionOptions::default()
    };
    assert_eq!(fail(source, options).kind, Kind::ExecutionLimit);
    assert_eq!(
        run("var x=bigint(\"1\"); print(x); x+=bigint(\"1\"); print(x);"),
        "1\n2\n"
    );
}

#[test]
fn deterministic_randomized_i128_reference_checks_every_operator() {
    let mut seed = 0x7363_6172_7062_6967_u64;
    for _ in 0..250 {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        let a = i128::from(seed as i64);
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        let b = i128::from(if seed as i64 == 0 { 1 } else { seed as i64 }); // Includes short and long quotients in separate tests.
        let source = format!(
            "bind a=bigint(\"{a}\"); bind b=bigint(\"{b}\"); print(a+b,a-b,a*b,a/b,a%b,a<b,a==b);"
        );
        assert_eq!(
            run(&source),
            format!(
                "{} {} {} {} {} {} {}\n",
                a + b,
                a - b,
                a * b,
                a / b,
                a % b,
                a < b,
                a == b
            )
        );
    }
}

#[test]
fn ten_thousand_digit_values_parse_add_compare_and_format() {
    let large = "9".repeat(10_000);
    let source = format!("bind a=bigint(\"{large}\"); print(a+bigint(\"0\")==a); print(a);");
    assert_eq!(run(&source), format!("true\n{large}\n"));
    assert_eq!(
        fail(
            &format!("print(bigint(\"{large}\")+bigint(\"1\"));"),
            ExecutionOptions::default()
        )
        .kind,
        Kind::BigIntLimit
    );
}

#[test]
fn expensive_arithmetic_is_rejected_before_output_and_independent_runs_reset() {
    let text = "9".repeat(1_000);
    let source = format!("print(\"pending\"); bind a=bigint(\"{text}\"); print(a*a);");
    let options = ExecutionOptions {
        limits: ExecutionLimits {
            max_steps: 2_000,
            ..ExecutionLimits::default()
        },
        ..ExecutionOptions::default()
    };
    assert_eq!(fail(&source, options).kind, Kind::ExecutionLimit);
    let options = ExecutionOptions {
        max_bigint_bytes: 4,
        ..ExecutionOptions::default()
    };
    let program = scarp::compile("print(bigint(\"1\"));").unwrap();
    for _ in 0..5 {
        let mut output = Vec::new();
        scarp::interpreter::execute_with_options(&program, &mut output, options).unwrap();
        assert_eq!(output, b"1\n");
    }
}

#[test]
fn native_backends_explicitly_reject_bigints() {
    let source = "bind n: bigint=bigint(\"12345678901234567890\"); print(n);";
    assert_eq!(
        scarp::c_backend::emit_c(source).unwrap_err().kind,
        Kind::NativeUnsupported
    );
    assert_eq!(
        scarp::native_object::emit_elf64_integer_kernel(source)
            .unwrap_err()
            .kind,
        Kind::NativeUnsupported
    );
}

#[test]
fn repeated_comparisons_and_general_division_cannot_bypass_shared_work() {
    let large = "9".repeat(10_000);
    let source = format!("bind a=bigint(\"{large}\"); repeat 20 as index {{ print(a==a); }}");
    let options = ExecutionOptions {
        limits: ExecutionLimits {
            max_steps: 15_000,
            ..ExecutionLimits::default()
        },
        ..ExecutionOptions::default()
    };
    assert_eq!(fail(&source, options).kind, Kind::ExecutionLimit);
    let (a, b) = ("9".repeat(1000), "8".repeat(500));
    let source = format!("print(bigint(\"{a}\")/bigint(\"{b}\"));");
    assert_eq!(fail(&source, options).kind, Kind::ExecutionLimit);
}

#[test]
fn concurrent_executions_have_independent_bigint_ledgers() {
    let program = scarp::compile("print(bigint(\"1\"));").unwrap();
    let handles: Vec<_> = (0..4)
        .map(|_| {
            let plan = program.clone();
            std::thread::spawn(move || {
                let mut out = Vec::new();
                scarp::interpreter::execute_with_options(
                    &plan,
                    &mut out,
                    ExecutionOptions {
                        max_bigint_bytes: 4,
                        ..ExecutionOptions::default()
                    },
                )
                .unwrap();
                assert_eq!(out, b"1\n");
            })
        })
        .collect();
    for handle in handles {
        handle.join().unwrap();
    }
}
