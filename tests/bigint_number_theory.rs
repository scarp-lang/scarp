use scarp::{diagnostics::DiagnosticKind as Kind, ExecutionOptions, RunError};

fn run(source: &str) -> String {
    let mut stdout = Vec::new();
    scarp::run(source, &mut stdout).unwrap();
    String::from_utf8(stdout).unwrap()
}

fn failure(source: &str, options: ExecutionOptions, expected: Kind) {
    let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
    let RunError::Diagnostic(error) =
        scarp::run_with_io_and_options(source, &mut stdout, &mut stderr, options).unwrap_err()
    else {
        panic!("expected a compiler diagnostic")
    };
    assert_eq!(error.kind, expected);
    assert!(stdout.is_empty() && stderr.is_empty());
    assert_eq!(
        scarp::check_with_options(source, options).unwrap_err(),
        error
    );
}

#[test]
fn gcd_handles_signs_zero_and_large_exact_operands() {
    assert_eq!(
        run("print(bigint_gcd(bigint(\"-48\"),bigint(\"18\")),bigint_gcd(bigint(\"0\"),bigint(\"0\")),bigint_gcd(bigint(\"0\"),bigint(\"-17\")),bigint_gcd(bigint(\"-17\"),bigint(\"0\")));"),
        "6 0 17 17\n",
    );
    assert_eq!(
        run("print(bigint_gcd(bigint(\"123456789012345678901234567890\"),bigint(\"987654321098765432109876543210\")));"),
        "9000000000900000000090\n",
    );
    // Every 2^512 - 1 is divisible by 2^256 - 1.
    assert_eq!(
        run("bind n = bigint_pow(bigint(\"2\"),256)-bigint(\"1\"); print(bigint_gcd(bigint_pow(bigint(\"2\"),512)-bigint(\"1\"),n)==n);"),
        "true\n",
    );
}

#[test]
fn lcm_is_nonnegative_reduced_and_zero_safe() {
    assert_eq!(
        run("print(bigint_lcm(bigint(\"-21\"),bigint(\"6\")),bigint_lcm(bigint(\"0\"),bigint(\"12345678901234567890\")),bigint_lcm(bigint(\"0\"),bigint(\"0\")));"),
        "42 0 0\n",
    );
    assert_eq!(
        run("print(bigint_lcm(bigint(\"123456789012345678901234567890\"),bigint(\"987654321098765432109876543210\")));"),
        "13548070124980948012498094801236261410\n",
    );
    assert_eq!(
        run("bind a=bigint(\"-48\"); bind b=bigint(\"18\"); print(bigint_gcd(a,b)*bigint_lcm(a,b)==bigint_abs(a*b));"),
        "true\n",
    );
}

#[test]
fn integer_sqrt_floors_perfect_squares_and_neighbors() {
    assert_eq!(
        run("print(bigint_sqrt(bigint(\"0\")),bigint_sqrt(bigint(\"1\")),bigint_sqrt(bigint(\"2\")),bigint_sqrt(bigint(\"3\")),bigint_sqrt(bigint(\"4\")),bigint_sqrt(bigint(\"8\")),bigint_sqrt(bigint(\"9\")),bigint_sqrt(bigint(\"15\")),bigint_sqrt(bigint(\"16\")),bigint_sqrt(bigint(\"17\")));"),
        "0 1 1 1 2 2 3 3 4 4\n",
    );
    assert_eq!(
        run("print(bigint_sqrt(bigint(\"99\")),bigint_sqrt(bigint(\"100\")),bigint_sqrt(bigint(\"101\")));"),
        "9 10 10\n",
    );
    assert_eq!(
        run("bind x=bigint_pow(bigint(\"2\"),256); print(bigint_sqrt(x*x)==x); print(bigint_sqrt(x*x-bigint(\"1\"))==x-bigint(\"1\"));"),
        "true\ntrue\n",
    );
}

#[test]
fn newton_sqrt_handles_large_decimal_boundaries_exactly() {
    const N: &str = "9999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999";
    const ROOT: &str = "99999999999999999999999999999999999999999999999999";
    // Python math.isqrt(10^100 - 1) yields 10^50 - 1.
    let source = format!("print(bigint_to_text(bigint_sqrt(bigint(\"{N}\"))));");
    assert_eq!(run(&source), format!("{ROOT}\n"));
    assert!(scarp::check(&source).is_ok());
}

#[test]
fn number_theory_errors_are_typed_and_atomic() {
    failure(
        "print(\"pending\"); print(bigint_sqrt(bigint(\"-1\")));",
        ExecutionOptions::default(),
        Kind::NumericDomain,
    );
    failure(
        "print(\"pending\"); print(bigint_lcm(bigint(\"999\"),bigint(\"998\")));",
        ExecutionOptions {
            max_bigint_digits: 3,
            ..ExecutionOptions::default()
        },
        Kind::BigIntLimit,
    );
    for expr in [
        "bigint_gcd(1,bigint(\"2\"))",
        "bigint_lcm(bigint(\"2\"), 3)",
        "bigint_sqrt(16)",
    ] {
        assert!(scarp::compile(&format!("print({expr});")).is_err());
    }
}

#[test]
fn sqrt_with_a_one_decimal_digit_quota_does_not_need_an_extra_digit() {
    let source = "print(bigint_sqrt(bigint(\"9\")));";
    let mut stdout = Vec::new();
    scarp::run_with_options(
        source,
        &mut stdout,
        ExecutionOptions {
            max_bigint_digits: 1,
            ..ExecutionOptions::default()
        },
    )
    .unwrap();
    assert_eq!(String::from_utf8(stdout).unwrap(), "3\n");
}
