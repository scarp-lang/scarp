use scarp::{diagnostics::DiagnosticKind as Kind, ExecutionOptions, RunError};

fn run(source: &str) -> String {
    let mut output = Vec::new();
    scarp::run(source, &mut output).unwrap();
    String::from_utf8(output).unwrap()
}

fn fail(source: &str, options: ExecutionOptions, kind: Kind) {
    let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
    let RunError::Diagnostic(error) =
        scarp::run_with_io_and_options(source, &mut stdout, &mut stderr, options).unwrap_err()
    else {
        panic!("expected diagnostic")
    };
    assert_eq!(error.kind, kind);
    assert!(stdout.is_empty() && stderr.is_empty(), "outputs must be atomic");
    assert_eq!(scarp::check_with_options(source, options).unwrap_err(), error);
}

#[test]
fn radix_parse_and_format_are_exact_and_canonical() {
    assert_eq!(
        run("print(bigint_from_radix(\"-0000\",10),bigint_from_radix(\"+7FfFfFfF\",16),bigint_to_radix(bigint(\"255\"),2),bigint_to_radix(bigint(\"255\"),8));"),
        "0 2147483647 11111111 377\n",
    );
    assert_eq!(
        run("print(bigint_from_radix(\"FFFFFFFFFFFFFFFF\",16),bigint_to_radix(bigint(\"18446744073709551615\"),16),bigint_to_radix(bigint(\"-36\"),36),bigint_from_radix(\"z\",36));"),
        "18446744073709551615 ffffffffffffffff -10 35\n",
    );
    assert_eq!(
        run("print(bigint_from_radix(\"100000000000000000000000000000000\",2),bigint_to_radix(bigint(\"0\"),36),bigint_to_radix(bigint(\"-0\"),2));"),
        "4294967296 0 0\n",
    );
}

#[test]
fn large_power_of_two_radix_roundtrips_preserve_every_bit() {
    for width in [64, 128, 256, 512, 1024] {
        let bits = format!("1{}", "0".repeat(width));
        let source = format!(
            "bind n=bigint_from_radix(\"{bits}\",2); print(n==bigint_pow(bigint(\"2\"),{width}),bigint_to_radix(n,2)==\"{bits}\");"
        );
        assert_eq!(run(&source), "true true\n", "width={width}");
        assert!(scarp::check(&source).is_ok());
    }
}

#[test]
fn radix_conversion_supports_typed_functions_and_text_operations() {
    assert_eq!(
        run("pure funcra hx(x:bigint)->string { return bigint_to_radix(x,16); } bind s=hx(bigint(\"65535\")); print(s,text_len(s),bigint_from_radix(s,16)==bigint(\"65535\"));"),
        "ffff 4 true\n",
    );
}

#[test]
fn malformed_radix_digits_and_invalid_base_are_rejected() {
    for (text, radix) in [
        ("", 10), ("+", 16), ("-", 2), ("0x10", 16), ("1_0", 2), ("9", 8),
        ("g", 16), ("１２", 10), (" 1", 10), ("1 ", 10),
    ] {
        let source = format!("print(\"pending\"); print(bigint_from_radix(\"{text}\",{radix}));");
        fail(&source, ExecutionOptions::default(), Kind::InvalidBigInt);
    }
    for radix in [0, 1, 37, 100, -2] {
        let source = format!("print(\"pending\"); print(bigint_from_radix(\"10\",{radix}));");
        fail(&source, ExecutionOptions::default(), Kind::NumericDomain);
        let source = format!("print(\"pending\"); print(bigint_to_radix(bigint(\"10\"),{radix}));");
        fail(&source, ExecutionOptions::default(), Kind::NumericDomain);
    }
    for expression in [
        "bigint_from_radix(123,16)",
        "bigint_from_radix(\"ab\",16.0)",
        "bigint_to_radix(\"255\",16)",
        "bigint_to_radix(bigint(\"255\"),16.0)",
    ] {
        assert!(scarp::compile(&format!("print({expression});")).is_err());
    }
}

#[test]
fn radix_limits_bound_input_work_scratch_and_text() {
    fail(
        "print(\"pending\"); print(bigint_from_radix(\"000\",10));",
        ExecutionOptions { max_bigint_digits: 2, ..ExecutionOptions::default() },
        Kind::BigIntLimit,
    );
    fail(
        "print(\"pending\"); print(bigint_from_radix(\"ff\",16));",
        ExecutionOptions { max_bigint_bytes: 0, ..ExecutionOptions::default() },
        Kind::BigIntLimit,
    );
    fail(
        "print(\"pending\"); print(bigint_to_radix(bigint(\"255\"),2));",
        ExecutionOptions { max_text_bytes: 7, ..ExecutionOptions::default() },
        Kind::TextConstruction,
    );
    fail(
        "print(\"pending\"); print(bigint_to_radix(bigint(\"4294967296\"),16));",
        ExecutionOptions { max_bigint_bytes: 8, ..ExecutionOptions::default() },
        Kind::BigIntLimit,
    );
    fail(
        "print(\"pending\"); print(bigint_from_radix(\"FFFFFFFFFFFFFFFFFFFFFFFF\",16));",
        ExecutionOptions {
            limits: scarp::ExecutionLimits { max_steps: 20, ..scarp::ExecutionLimits::default() },
            ..ExecutionOptions::default()
        },
        Kind::ExecutionLimit,
    );
}
