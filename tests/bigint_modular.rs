use scarp::{diagnostics::DiagnosticKind as Kind, ExecutionOptions, RunError};

fn run(source: &str) -> String {
    let mut output = Vec::new();
    scarp::run(source, &mut output).unwrap();
    String::from_utf8(output).unwrap()
}

fn diagnostic(source: &str, options: ExecutionOptions, kind: Kind) {
    let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
    let RunError::Diagnostic(error) =
        scarp::run_with_io_and_options(source, &mut stdout, &mut stderr, options).unwrap_err()
    else {
        panic!("expected typed diagnostic")
    };
    assert_eq!(error.kind, kind);
    assert!(
        stdout.is_empty() && stderr.is_empty(),
        "output must be atomic"
    );
    assert_eq!(
        scarp::check_with_options(source, options).unwrap_err(),
        error
    );
}

#[test]
fn modular_exponentiation_matches_independent_python_oracles() {
    assert_eq!(
        run("print(bigint_pow_mod(bigint(\"2\"),1000,bigint(\"1009\")),bigint_pow_mod(bigint(\"-2\"),11,bigint(\"7\")),bigint_pow_mod(bigint(\"-5\"),3,bigint(\"13\")));"),
        "942 3 5\n",
    );
    assert_eq!(
        run("print(bigint_pow_mod(bigint(\"0\"),0,bigint(\"13\")),bigint_pow_mod(bigint(\"0\"),0,bigint(\"1\")),bigint_pow_mod(bigint(\"0\"),12,bigint(\"13\")));"),
        "1 0 0\n",
    );
    assert_eq!(
        run("print(bigint_pow_mod(bigint(\"123456789012345678901234567890\"),12345,bigint(\"9999999967\")));"),
        "421180610\n",
    );
    // pow(2, 1000) is intentionally never materialized to calculate the residue.
    assert_eq!(
        run("bind m=bigint(\"97\"); bind x=bigint_pow_mod(bigint(\"2\"),1000,m); print(x>=bigint(\"0\"),x<m);"),
        "true true\n",
    );
}

#[test]
fn modular_exponentiation_handles_modulus_one_and_zero_exponent() {
    assert_eq!(
        run("print(bigint_pow_mod(bigint(\"-999999999999999\"),0,bigint(\"3\")),bigint_pow_mod(bigint(\"123456789\"),0,bigint(\"1\")));"),
        "1 0\n",
    );
    assert_eq!(
        run("print(bigint_pow_mod(bigint(\"2\"),9223372036854775807,bigint(\"1\")));"),
        "0\n",
    );
}

#[test]
fn modular_exponentiation_rejects_invalid_domains_and_types() {
    for expression in [
        "bigint_pow_mod(bigint(\"2\"),-1,bigint(\"7\"))",
        "bigint_pow_mod(bigint(\"2\"),2,bigint(\"0\"))",
        "bigint_pow_mod(bigint(\"2\"),2,bigint(\"-7\"))",
    ] {
        diagnostic(
            &format!("print(\"pending\"); print({expression});"),
            ExecutionOptions::default(),
            Kind::NumericDomain,
        );
    }
    for expression in [
        "bigint_pow_mod(2,2,bigint(\"7\"))",
        "bigint_pow_mod(bigint(\"2\"),2.0,bigint(\"7\"))",
        "bigint_pow_mod(bigint(\"2\"),2,7)",
    ] {
        assert!(scarp::compile(&format!("print({expression});")).is_err());
    }
}

#[test]
fn intermediate_product_and_shared_work_limits_cannot_be_bypassed() {
    assert_eq!(
        run("print(bigint_pow_mod(bigint(\"998\"),2,bigint(\"999\")));"),
        "1\n",
    );
    // Although the final residue needs only one digit, the first square is a
    // six-digit exact BigInt, which correctly exceeds this 3-digit cap.
    diagnostic(
        "print(\"pending\"); print(bigint_pow_mod(bigint(\"998\"),2,bigint(\"999\")));",
        ExecutionOptions {
            max_bigint_digits: 3,
            ..ExecutionOptions::default()
        },
        Kind::BigIntLimit,
    );
    diagnostic(
        "print(\"pending\"); print(bigint_pow_mod(bigint(\"2\"),1000,bigint(\"1009\")));",
        ExecutionOptions {
            limits: scarp::ExecutionLimits {
                max_steps: 30,
                ..scarp::ExecutionLimits::default()
            },
            ..ExecutionOptions::default()
        },
        Kind::ExecutionLimit,
    );
}

#[test]
fn modular_arithmetic_remains_exact_under_functions_and_signs() {
    assert_eq!(
        run("pure funcra mod_power(base:bigint, exponent:integer, modulus:bigint)->bigint { return bigint_pow_mod(base,exponent,modulus); } print(mod_power(bigint(\"-2\"),3,bigint(\"5\")));"),
        "2\n",
    );
}
