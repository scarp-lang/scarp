use scarp::{diagnostics::DiagnosticKind as Kind, ExecutionLimits, ExecutionOptions, RunError};

fn run(source: &str) -> String {
    let mut output = Vec::new();
    scarp::run(source, &mut output).unwrap();
    String::from_utf8(output).unwrap()
}

fn fails(source: &str, options: ExecutionOptions, expected: Kind) {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let RunError::Diagnostic(error) =
        scarp::run_with_io_and_options(source, &mut out, &mut err, options).unwrap_err()
    else {
        panic!("expected diagnostic");
    };
    assert_eq!(error.kind, expected);
    assert_eq!(
        scarp::check_with_options(source, options).unwrap_err(),
        error
    );
    assert!(out.is_empty() && err.is_empty());
}

#[test]
fn decimal_inspection_uses_magnitude_and_canonical_zero() {
    assert_eq!(
        run(r#"
            bind n=bigint("-000123450000000000000000");
            print(bigint_digits(n),bigint_digit_sum(n),bigint_trailing_zeros(n));
            print(bigint_digits(bigint("-000")),bigint_digit_sum(bigint("0")));
            print(bigint_log2(bigint("-1024")),bigint_log2(bigint("1023")));
        "#),
        "21 15 16\n1 0\n10 9\n"
    );
}

#[test]
fn signed_predicates_and_parity() {
    assert_eq!(
        run(r#"
            bind a=bigint("-123456789012345678901");
            bind b=bigint("42");
            bind z=bigint("-0");
            print(bigint_sign(a),bigint_sign(b),bigint_sign(z));
            print(bigint_is_odd(a),bigint_is_even(b),bigint_is_even(z));
            print(bigint_is_positive(b),bigint_is_negative(a),bigint_is_zero(z));
            print(bigint_is_odd(z),bigint_is_positive(z),bigint_is_negative(z));
        "#),
        "-1 1 0\ntrue true true\ntrue true true\nfalse false false\n"
    );
}

#[test]
fn factorial_exact_known_values() {
    assert_eq!(
        run("print(bigint_factorial(0),bigint_factorial(1),bigint_factorial(10));"),
        "1 1 3628800\n"
    );
    assert_eq!(
        run("print(bigint_factorial(100));"),
        "93326215443944152681699238856266700490715968264381621468592963895217599993229915608941463976156518286253697920827223758251185210916864000000000000000000000000\n"
    );
}

#[test]
fn roots_floor_and_degree_boundaries() {
    assert_eq!(
        run(r#"
            print(bigint_root(bigint("0"),7),bigint_root(bigint("1"),99));
            print(bigint_root(bigint("7"),3),bigint_root(bigint("8"),3),bigint_root(bigint("9"),3));
            print(bigint_root(bigint("999999"),3),bigint_root(bigint("1000000"),3));
            print(bigint_root(bigint("123456789"),1),bigint_root(bigint("16"),2));
            print(bigint_root(bigint("100000000000000000000"),9223372036854775807));
            print(bigint_root(bigint_pow(bigint("10"),100),4));
        "#),
        "0 1\n1 2 2\n99 100\n123456789 4\n1\n10000000000000000000000000\n"
    );
}

#[test]
fn deterministic_primality_covers_signed_values_and_unsigned_64_bit_boundary() {
    assert_eq!(
        run(r#"
            print(bigint_is_prime(bigint("-7")),bigint_is_prime(bigint("0")),bigint_is_prime(bigint("1")));
            print(bigint_is_prime(bigint("2")),bigint_is_prime(bigint("97")),bigint_is_prime(bigint("100")));
            print(bigint_is_prime(bigint("341550071728321")),bigint_is_prime(bigint("3825123056546413051")));
            print(bigint_is_prime(bigint("18446744073709551557")),bigint_is_prime(bigint("18446744073709551615")));
        "#),
        "false false false\ntrue true false\nfalse false\ntrue false\n"
    );
}

#[test]
fn large_probable_prime_screen_is_explicitly_distinct_from_proof() {
    let source = r#"
        print(bigint_is_probable_prime(bigint("618970019642690137449562111")));
        print(bigint_is_prime(bigint("18446744073709551617")));
        print(bigint_is_probable_prime(bigint("18446744073709551618")));
    "#;
    let options = ExecutionOptions {
        limits: ExecutionLimits {
            max_steps: 10_000_000,
            ..ExecutionLimits::default()
        },
        ..ExecutionOptions::default()
    };
    let mut out = Vec::new();
    scarp::run_with_options(source, &mut out, options).unwrap();
    assert_eq!(out, b"true\nfalse\nfalse\n");
}

#[test]
fn domain_and_resource_errors_remain_atomic() {
    for expression in [
        "bigint_factorial(-1)",
        "bigint_root(bigint(\"8\"),0)",
        "bigint_root(bigint(\"-8\"),3)",
        "bigint_log2(bigint(\"0\"))",
        "bigint_trailing_zeros(bigint(\"0\"))",
    ] {
        fails(
            &format!("print(\"pending\"); print({expression});"),
            ExecutionOptions::default(),
            Kind::NumericDomain,
        );
    }
    fails(
        "print(\"pending\"); print(bigint_factorial(100));",
        ExecutionOptions {
            max_bigint_digits: 100,
            ..ExecutionOptions::default()
        },
        Kind::BigIntLimit,
    );
    fails(
        "print(\"pending\"); print(bigint_is_prime(bigint(\"18446744073709551557\")));",
        ExecutionOptions {
            limits: ExecutionLimits {
                max_steps: 100,
                ..ExecutionLimits::default()
            },
            ..ExecutionOptions::default()
        },
        Kind::ExecutionLimit,
    );
    for invalid in [
        "bigint_digits(123)",
        "bigint_factorial(bigint(\"10\"))",
        "bigint_root(bigint(\"10\"),2.0)",
        "bigint_is_prime(\"97\")",
    ] {
        assert!(scarp::compile(&format!("print({invalid});")).is_err());
    }
}

#[test]
fn new_builtins_work_as_typed_pure_callables() {
    assert_eq!(
        run(r#"
            pure funcra measure(n: bigint) -> integer {
                return bigint_digits(n);
            }
            print(measure(bigint("999999")),bigint_is_prime(bigint("7")));
        "#),
        "6 true\n"
    );
}

#[test]
fn digit_count_does_not_materialize_decimal_text() {
    let source = r#"
        bind n = bigint_pow(bigint("10"), 99999);
        print(bigint_digits(n),bigint_is_even(n),bigint_sign(n));
    "#;
    let options = ExecutionOptions {
        max_bigint_digits: 100_000,
        max_text_bytes: 16,
        ..ExecutionOptions::default()
    };
    let mut out = Vec::new();
    scarp::run_with_options(source, &mut out, options).unwrap();
    assert_eq!(out, b"100000 true 1\n");
}
