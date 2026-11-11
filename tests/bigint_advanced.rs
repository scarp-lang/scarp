use scarp::{
    diagnostics::DiagnosticKind as Kind,
    numeric::{self, Builtin},
    value::Value,
    ExecutionOptions, RunError,
};

fn execute(source: &str) -> String {
    let mut out = Vec::new();
    scarp::run(source, &mut out).expect("valid Scarp program");
    String::from_utf8(out).expect("UTF-8 output")
}

fn fails_atomically(source: &str, options: ExecutionOptions, kind: Kind) {
    let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
    let RunError::Diagnostic(error) =
        scarp::run_with_io_and_options(source, &mut stdout, &mut stderr, options)
            .expect_err("expected bounded execution failure")
    else {
        panic!("expected a diagnostic")
    };
    assert_eq!(error.kind, kind);
    assert!(
        stdout.is_empty() && stderr.is_empty(),
        "all output must remain atomic"
    );
    assert_eq!(
        scarp::check_with_options(source, options).unwrap_err(),
        error
    );
}

#[test]
fn bigint_powers_are_exact_and_preserve_sign() {
    assert_eq!(
        execute("print(bigint_pow(bigint(\"0\"),0), bigint_pow(bigint(\"0\"),5), bigint_pow(bigint(\"-2\"),3), bigint_pow(bigint(\"-2\"),4), bigint_pow(bigint(\"2\"),10), bigint_pow(bigint(\"-1\"),1001));"),
        "1 0 -8 16 1024 -1\n",
    );
    assert_eq!(
        execute("print(bigint_bit_length(bigint_pow(bigint(\"2\"),1000)), text_len(bigint_to_text(bigint_pow(bigint(\"2\"),1000))));"),
        "1001 302\n",
    );
}

#[test]
fn bit_length_is_exact_around_binary_boundaries() {
    assert_eq!(
        execute("print(bigint_bit_length(bigint(\"0\")), bigint_bit_length(bigint(\"-0\")), bigint_bit_length(bigint(\"1\")), bigint_bit_length(bigint(\"-255\")), bigint_bit_length(bigint(\"256\")), bigint_bit_length(bigint(\"1024\")));"),
        "0 0 1 8 9 11\n",
    );
    assert_eq!(
        execute("print(bigint_bit_length(bigint(\"18446744073709551615\")), bigint_bit_length(bigint(\"18446744073709551616\")), bigint_bit_length(bigint(\"-18446744073709551617\")));"),
        "64 65 65\n",
    );
    for bits in [128, 256, 512, 1024, 4096] {
        let program = format!("print(bigint_bit_length(bigint_pow(bigint(\"2\"),{bits})));");
        assert_eq!(execute(&program), format!("{}\n", bits + 1));
    }
}

#[test]
fn decimal_text_round_trips_exactly_and_interoperates_with_text() {
    assert_eq!(
        execute("bind x = bigint(\"-00000123456789012345678901234567890\"); bind s = bigint_to_text(x); print(s, text_len(s), bigint(s)==x, text_concat(\"number:\",s)); print(bigint_to_text(bigint(\"-0\")));"),
        "-123456789012345678901234567890 31 true number:-123456789012345678901234567890\n0\n",
    );
    let value = numeric::evaluate(
        Builtin::BigInt,
        &[Value::from("1000000000000000000000")],
        100,
    )
    .unwrap();
    let text = numeric::evaluate(Builtin::BigIntToText, &[value.clone()], 100).unwrap();
    assert_eq!(text.as_str(), Some("1000000000000000000000"));
    assert_eq!(
        numeric::evaluate(Builtin::BigIntBitLength, &[value], 100).unwrap(),
        Value::Integer(70),
    );
}

#[test]
fn published_rust_bigint_addition_case_matches_independent_integer_oracle() {
    const A: &str = "124567894123154548712678612348421544815641541354574517451785678647867867864786786786455687454512451234423484245467415418785786745786478674867457484156145617486418617484561234512351";
    const B: &str = "789456123456324452123453134567897454567534537878546132456489754165754132452645453418665748678576574854865746587781564521356452454245453455453454234578756786758467867857847874874749";
    // Independently calculated using Python's arbitrary-precision int.
    const SUM: &str = "914024017579479000836131746916318999383176079233120649908275432813622000317432240205121436133089026089289230833248979940142239200031932130320911718734902404244886485342409109387100";
    let source =
        format!("bind a=bigint(\"{A}\"); bind b=bigint(\"{B}\"); print(bigint_to_text(a+b));");
    assert_eq!(execute(&source), format!("{SUM}\n"));
    assert!(scarp::compile(&source).is_ok());
    assert!(scarp::check(&source).is_ok());
}

#[test]
fn invalid_exponents_and_types_do_not_implicitly_coerce() {
    fails_atomically(
        "print(\"uncommitted\"); print(bigint_pow(bigint(\"2\"),-1));",
        ExecutionOptions::default(),
        Kind::NumericDomain,
    );
    for invalid in [
        "bigint_pow(2, 4)",
        "bigint_pow(bigint(\"2\"), 2.0)",
        "bigint_bit_length(256)",
        "bigint_to_text(1.5)",
    ] {
        assert!(
            scarp::compile(&format!("print({invalid});")).is_err(),
            "{invalid}"
        );
    }
}

#[test]
fn bigint_p0_limits_guard_digits_work_scratch_and_strings() {
    fails_atomically(
        "print(\"uncommitted\"); print(bigint_pow(bigint(\"2\"), 1000000000));",
        ExecutionOptions::default(),
        Kind::BigIntLimit,
    );
    fails_atomically(
        "print(\"uncommitted\"); print(bigint_pow(bigint(\"2\"),10));",
        ExecutionOptions {
            max_bigint_digits: 3,
            ..ExecutionOptions::default()
        },
        Kind::BigIntLimit,
    );
    fails_atomically(
        "print(\"uncommitted\"); print(bigint_to_text(bigint(\"1234\")));",
        ExecutionOptions {
            max_text_bytes: 3,
            ..ExecutionOptions::default()
        },
        Kind::TextConstruction,
    );
    fails_atomically(
        "print(\"uncommitted\"); print(bigint_bit_length(bigint(\"100000000000000000000\")));",
        ExecutionOptions {
            max_bigint_bytes: 16,
            ..ExecutionOptions::default()
        },
        Kind::BigIntLimit,
    );
    fails_atomically(
        "print(\"uncommitted\"); print(bigint_bit_length(bigint(\"100000000000000000000\")));",
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
