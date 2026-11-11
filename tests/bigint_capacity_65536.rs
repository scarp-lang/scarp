use scarp::{diagnostics::DiagnosticKind as Kind, ExecutionLimits, ExecutionOptions, RunError};

fn source() -> String {
    // 10^19728 is 65,535 bits. Multiplication by 2 produces an exact
    // 65,536-bit positive integer without depending on a test-only BigInt crate.
    let decimal = format!("2{}", "0".repeat(19_728));
    format!(
        "bind n=bigint(\"{decimal}\"); \
         print(bigint_bit_length(n),bigint_bit_length(-n)); \
         print(bigint_to_text(n)==\"{decimal}\"); \
         print(bigint_bit_length(n+bigint(\"1\")));"
    )
}

fn options() -> ExecutionOptions {
    ExecutionOptions {
        limits: ExecutionLimits {
            max_steps: 30_000_000,
            ..ExecutionLimits::default()
        },
        max_bigint_digits: 19_729,
        ..ExecutionOptions::default()
    }
}

fn failure(src: &str, opts: ExecutionOptions, kind: Kind) {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let RunError::Diagnostic(actual) =
        scarp::run_with_io_and_options(src, &mut out, &mut err, opts).unwrap_err()
    else {
        panic!("expected bounded failure");
    };
    assert_eq!(actual.kind, kind, "{actual:?}");
    assert_eq!(scarp::check_with_options(src, opts).unwrap_err(), actual);
    assert!(
        out.is_empty() && err.is_empty(),
        "all outputs must remain buffered"
    );
}

#[test]
fn sixty_five_thousand_five_hundred_thirty_six_bit_value_is_exact() {
    let src = source();
    assert!(scarp::compile(&src).is_ok());
    assert!(scarp::check_with_options(&src, options()).is_ok());
    let mut output = Vec::new();
    scarp::run_with_options(&src, &mut output, options()).unwrap();
    assert_eq!(
        String::from_utf8(output).unwrap(),
        "65536 65536\ntrue\n65536\n"
    );
}

#[test]
fn giant_integer_remains_opt_in_and_bounded_by_digits_and_work() {
    let src = source();
    failure(&src, ExecutionOptions::default(), Kind::BigIntLimit);
    failure(
        &src,
        ExecutionOptions {
            max_bigint_digits: 19_728,
            ..options()
        },
        Kind::BigIntLimit,
    );
    failure(
        &src,
        ExecutionOptions {
            limits: ExecutionLimits {
                max_steps: 1_000_000,
                ..ExecutionLimits::default()
            },
            ..options()
        },
        Kind::ExecutionLimit,
    );
}
