use scarp::{diagnostics::DiagnosticKind as Kind, ExecutionLimits, ExecutionOptions, RunError};

fn run(source: &str) -> String {
    let mut out = Vec::new();
    scarp::run(source, &mut out).unwrap();
    String::from_utf8(out).unwrap()
}

fn fails(source: &str, options: ExecutionOptions, kind: Kind) {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let RunError::Diagnostic(error) =
        scarp::run_with_io_and_options(source, &mut out, &mut err, options).unwrap_err()
    else {
        panic!("expected diagnostic");
    };
    assert_eq!(error.kind, kind);
    assert_eq!(
        scarp::check_with_options(source, options).unwrap_err(),
        error
    );
    assert!(out.is_empty() && err.is_empty());
}

#[test]
fn fibonacci_matches_known_values_and_recurrence() {
    assert_eq!(
        run(r#"
            print(bigint_fibonacci(0),bigint_fibonacci(1),bigint_fibonacci(2));
            print(bigint_fibonacci(10),bigint_fibonacci(100));
            print(bigint_fibonacci(500)==bigint_fibonacci(499)+bigint_fibonacci(498));
        "#),
        "0 1 1\n55 354224848179261915075\ntrue\n"
    );
}

#[test]
fn combinatorics_matches_exact_values_and_identities() {
    assert_eq!(
        run(r#"
            print(bigint_binomial(5,2),bigint_binomial(0,0),bigint_binomial(100,50));
            print(bigint_permutation(5,2),bigint_permutation(100,0));
            print(bigint_binomial(50,7)==bigint_binomial(50,43));
            print(bigint_permutation(25,5)==bigint_binomial(25,5)*bigint_factorial(5));
        "#),
        "10 1 100891344545564193334812497256\n20 1\ntrue\ntrue\n"
    );
}

#[test]
fn prime_search_is_strict_and_exact() {
    assert_eq!(
        run(r#"
            print(bigint_next_prime(bigint("-10")),bigint_next_prime(bigint("1")));
            print(bigint_next_prime(bigint("2")),bigint_next_prime(bigint("97")));
            print(bigint_next_prime(bigint("1000000")));
        "#),
        "2 2\n3 101\n1000003\n"
    );
}

#[test]
fn square_and_power_of_two_predicates_cover_boundaries() {
    assert_eq!(
        run(r#"
            print(bigint_is_perfect_square(bigint("-4")),bigint_is_perfect_square(bigint("0")));
            print(bigint_is_perfect_square(bigint("144")),bigint_is_perfect_square(bigint("145")));
            print(bigint_is_perfect_square(bigint_pow(bigint("10"),100)));
            print(bigint_is_power_of_two(bigint("0")),bigint_is_power_of_two(bigint("-8")));
            print(bigint_is_power_of_two(bigint("1")),bigint_is_power_of_two(bigint("3")));
            bind n=bigint_pow(bigint("2"),100);
            print(bigint_is_power_of_two(n),bigint_is_power_of_two(n+bigint("2")));
        "#),
        "false true\ntrue false\ntrue\nfalse false\ntrue false\ntrue false\n"
    );
}

#[test]
fn decimal_reverse_and_palindrome_cross_limb_boundaries() {
    assert_eq!(
        run(r#"
            print(bigint_reverse(bigint("-1200")),bigint_reverse(bigint("0")));
            print(bigint_reverse(bigint("12345678901234567890")));
            print(bigint_is_palindrome(bigint("1234567890987654321")));
            print(bigint_is_palindrome(bigint("-121")),bigint_is_palindrome(bigint("10")));
            print(bigint_is_palindrome(bigint("0")));
        "#),
        "-21 0\n9876543210987654321\ntrue\nfalse false\ntrue\n"
    );
}

#[test]
fn min_max_preserve_signed_order_and_mod_inverse_is_canonical() {
    assert_eq!(
        run(r#"
            print(bigint_min(bigint("-12345678901234567890"),bigint("-7")));
            print(bigint_max(bigint("-7"),bigint("0")));
            print(bigint_mod_inverse(bigint("3"),bigint("11")));
            print(bigint_mod_inverse(bigint("-3"),bigint("11")));
            bind m=bigint("100000000000000000003");
            bind a=bigint("2");
            bind inv=bigint_mod_inverse(a,m);
            print((a*inv)%m);
        "#),
        "-12345678901234567890\n0\n4\n7\n1\n"
    );
}

#[test]
fn invalid_domains_types_and_budgets_are_rejected_atomically() {
    for expression in [
        "bigint_fibonacci(-1)",
        "bigint_binomial(5,6)",
        "bigint_permutation(5,-1)",
        "bigint_mod_inverse(bigint(\"2\"),bigint(\"4\"))",
        "bigint_mod_inverse(bigint(\"2\"),bigint(\"1\"))",
        "bigint_mod_inverse(bigint(\"2\"),bigint(\"-7\"))",
    ] {
        fails(
            &format!("print(\"pending\"); print({expression});"),
            ExecutionOptions::default(),
            Kind::NumericDomain,
        );
    }
    fails(
        "print(\"pending\"); print(bigint_fibonacci(1000000000));",
        ExecutionOptions::default(),
        Kind::BigIntLimit,
    );
    fails(
        "print(\"pending\"); print(bigint_next_prime(bigint(\"1000000\")));",
        ExecutionOptions {
            limits: ExecutionLimits {
                max_steps: 30,
                ..ExecutionLimits::default()
            },
            ..ExecutionOptions::default()
        },
        Kind::ExecutionLimit,
    );
    for expression in [
        "bigint_fibonacci(bigint(\"10\"))",
        "bigint_min(1,2)",
        "bigint_binomial(10,2.5)",
        "bigint_reverse(\"123\")",
    ] {
        assert!(scarp::compile(&format!("print({expression});")).is_err());
    }
}
