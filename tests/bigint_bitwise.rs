use scarp::{diagnostics::DiagnosticKind as Kind, ExecutionOptions, RunError};

fn run(source: &str) -> String {
    let mut stdout = Vec::new();
    scarp::run(source, &mut stdout).unwrap();
    String::from_utf8(stdout).unwrap()
}

fn fail(source: &str, options: ExecutionOptions, kind: Kind) {
    let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
    let RunError::Diagnostic(error) =
        scarp::run_with_io_and_options(source, &mut stdout, &mut stderr, options).unwrap_err()
    else {
        panic!("expected a diagnostic")
    };
    assert_eq!(error.kind, kind);
    assert!(stdout.is_empty() && stderr.is_empty());
    assert_eq!(
        scarp::check_with_options(source, options).unwrap_err(),
        error
    );
}

#[test]
fn signed_bitwise_matches_infinite_twos_complement_semantics() {
    assert_eq!(
        run("print(bigint_and(bigint(\"5\"),bigint(\"3\")),bigint_or(bigint(\"5\"),bigint(\"3\")),bigint_xor(bigint(\"5\"),bigint(\"3\")),bigint_not(bigint(\"5\")));"),
        "1 7 6 -6\n",
    );
    assert_eq!(
        run("print(bigint_and(bigint(\"-5\"),bigint(\"3\")),bigint_or(bigint(\"-5\"),bigint(\"3\")),bigint_xor(bigint(\"-5\"),bigint(\"3\")),bigint_not(bigint(\"-5\")));"),
        "3 -5 -8 4\n",
    );
    assert_eq!(
        run("print(bigint_not(bigint(\"0\")),bigint_not(bigint(\"-1\")),bigint_and(bigint(\"0\"),bigint(\"-1\")),bigint_or(bigint(\"0\"),bigint(\"-1\")));"),
        "-1 0 0 -1\n",
    );
}

#[test]
fn large_signed_operations_match_independent_python_integer_oracle() {
    const A: &str = "-123456789012345678901234567890";
    const B: &str = "98765432109876543210987654321";
    let source = format!(
        "bind a=bigint(\"{A}\"); bind b=bigint(\"{B}\"); print(bigint_and(a,b)); print(bigint_or(a,b)); print(bigint_xor(a,b)); print(bigint_not(a)); print(bigint_shl(a,37)); print(bigint_shr(a,37));"
    );
    assert_eq!(
        run(&source),
        "15165747737836113450097443872\n-39857104640305249140344357441\n-55022852378141362590441801313\n123456789012345678901234567889\n-16967771880870298596087029859591735214080\n-898266364037013256\n",
    );
}

#[test]
fn bitwise_identities_hold_for_large_negative_and_positive_values() {
    assert_eq!(
        run("bind x=bigint_pow(bigint(\"2\"),512); print(bigint_and(bigint(\"-1\"),x)==x,bigint_or(bigint(\"-1\"),x)==bigint(\"-1\"),bigint_xor(x,x)==bigint(\"0\"),bigint_xor(bigint(\"-1\"),x)==bigint_not(x));"),
        "true true true true\n",
    );
    assert_eq!(
        run("bind a=bigint(\"-12345678901234567890\"); bind b=bigint(\"987654321\"); print(bigint_xor(a,b)==bigint_or(a,b)-bigint_and(a,b));"),
        "true\n",
    );
}

#[test]
fn shifts_are_exact_and_arithmetic_for_negative_numbers() {
    assert_eq!(
        run("print(bigint_shl(bigint(\"-3\"),5),bigint_shr(bigint(\"-3\"),1),bigint_shr(bigint(\"-9\"),2),bigint_shr(bigint(\"9\"),2));"),
        "-96 -2 -3 2\n",
    );
    assert_eq!(
        run("print(bigint_shr(bigint(\"-1\"),9223372036854775807),bigint_shr(bigint(\"1\"),9223372036854775807),bigint_shl(bigint(\"0\"),9223372036854775807));"),
        "-1 0 0\n",
    );
    assert_eq!(
        run("print(bigint_bit_length(bigint_shl(bigint(\"1\"),1000)),bigint_shr(bigint_shl(bigint(\"3\"),513),513));"),
        "1001 3\n",
    );
}

#[test]
fn signed_bitwise_works_in_typed_functions() {
    assert_eq!(
        run("pure funcra mask(n:bigint,m:bigint)->bigint { return bigint_and(n,m); } print(mask(bigint(\"-1\"),bigint(\"255\")));"),
        "255\n",
    );
}

#[test]
fn invalid_shifts_types_and_resource_limits_are_explicit() {
    for expr in [
        "bigint_shl(bigint(\"2\"),-1)",
        "bigint_shr(bigint(\"-1\"),-1)",
    ] {
        fail(
            &format!("print(\"pending\"); print({expr});"),
            ExecutionOptions::default(),
            Kind::NumericDomain,
        );
    }
    fail(
        "print(\"pending\"); print(bigint_shl(bigint(\"2\"),1000000));",
        ExecutionOptions::default(),
        Kind::BigIntLimit,
    );
    fail(
        "print(\"pending\"); print(bigint_and(bigint(\"255\"),bigint(\"127\")));",
        ExecutionOptions {
            max_bigint_bytes: 8,
            ..ExecutionOptions::default()
        },
        Kind::BigIntLimit,
    );
    fail(
        "print(\"pending\"); print(bigint_shl(bigint(\"3\"),100));",
        ExecutionOptions {
            limits: scarp::ExecutionLimits {
                max_steps: 40,
                ..scarp::ExecutionLimits::default()
            },
            ..ExecutionOptions::default()
        },
        Kind::ExecutionLimit,
    );
    for expr in [
        "bigint_and(1,bigint(\"1\"))",
        "bigint_or(bigint(\"1\"),1)",
        "bigint_xor(bigint(\"1\"),1.5)",
        "bigint_not(1)",
        "bigint_shl(bigint(\"1\"),1.0)",
        "bigint_shr(1,1)",
    ] {
        assert!(scarp::compile(&format!("print({expr});")).is_err());
    }
}
