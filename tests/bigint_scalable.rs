use scarp::{diagnostics::DiagnosticKind as Kind, ExecutionLimits, ExecutionOptions, RunError};

fn options(digits: usize) -> ExecutionOptions {
    ExecutionOptions {
        max_bigint_digits: digits,
        max_bigint_bytes: 64 * 1024 * 1024,
        limits: ExecutionLimits {
            max_steps: 100_000_000,
            ..ExecutionLimits::default()
        },
        ..ExecutionOptions::default()
    }
}

fn run(source: &str, options: ExecutionOptions) -> String {
    let mut output = Vec::new();
    scarp::run_with_options(source, &mut output, options).unwrap();
    String::from_utf8(output).unwrap()
}

// Independent reference: repeated single-digit-factor multiplication in base
// 10^4, rather than Scarp's base-10^9 recursive exponentiation algorithm.
fn reference_power(base: u32, exponent: usize) -> String {
    use std::fmt::Write as _;
    let mut parts = vec![1_u32];
    for _ in 0..exponent {
        let mut carry = 0;
        for part in &mut parts {
            let product = *part * base + carry;
            *part = product % 10_000;
            carry = product / 10_000;
        }
        while carry != 0 {
            parts.push(carry % 10_000);
            carry /= 10_000;
        }
    }
    let mut iter = parts.iter().rev();
    let mut text = iter.next().unwrap().to_string();
    for part in iter {
        write!(&mut text, "{part:04}").unwrap();
    }
    text
}

#[test]
fn large_dense_products_match_closed_form_across_kernel_boundaries() {
    for (a, b) in [
        (567, 567),
        (576, 576),
        (585, 576),
        (1152, 576),
        (1153, 576),
        (2000, 1700),
    ] {
        let left = "9".repeat(a);
        let right = "9".repeat(b);
        let expected = format!(
            "{}8{}{}1\n",
            "9".repeat(b - 1),
            "9".repeat(a - b),
            "0".repeat(b - 1)
        );
        let source = format!("print(bigint(\"{left}\")*bigint(\"{right}\"));");
        assert_eq!(run(&source, options(a + b)), expected, "{a} x {b}");
        let source = format!("print(bigint(\"-{left}\")*bigint(\"{right}\"));");
        assert_eq!(run(&source, options(a + b)), format!("-{expected}"));
    }
}

#[test]
fn large_powers_match_independent_oracle() {
    for (base, exponent) in [(2, 5000), (99, 2500), (250, 10_000)] {
        let expected = reference_power(base, exponent);
        let source = format!("print(bigint_pow(bigint(\"{base}\"),{exponent}));");
        assert_eq!(
            run(&source, options(expected.len())),
            format!("{expected}\n"),
            "{base}^{exponent}"
        );
    }
}

#[test]
fn sparse_decimal_powers_fit_a_linear_scale_work_budget() {
    let source = "print(bigint_pow(bigint(\"10\"),99999));";
    let mut opts = options(100_000);
    opts.limits.max_steps = 2_000_000;
    assert_eq!(run(source, opts), format!("1{}\n", "0".repeat(99_999)));
}

#[test]
fn multiplication_with_zero_low_limbs_preserves_exact_shift_and_sign() {
    let left = format!("-123456789{}", "0".repeat(900));
    let right = format!("987654321{}", "0".repeat(1800));
    let source = format!("print(bigint(\"{left}\")*bigint(\"{right}\"));");
    assert_eq!(
        run(&source, options(3000)),
        format!("-{}{}\n", 123456789_u64 * 987654321, "0".repeat(2700))
    );
}

#[test]
fn large_power_respects_digit_allocation_and_work_limits_atomically() {
    let source = "print(\"pending\"); print(bigint_pow(bigint(\"10\"),99999));";
    let base = options(100_000);
    let cases = [
        (
            ExecutionOptions {
                max_bigint_digits: 99_999,
                ..base
            },
            Kind::BigIntLimit,
        ),
        (
            ExecutionOptions {
                max_bigint_bytes: 1024,
                ..base
            },
            Kind::BigIntLimit,
        ),
        (
            ExecutionOptions {
                limits: ExecutionLimits {
                    max_steps: 1000,
                    ..base.limits
                },
                ..base
            },
            Kind::ExecutionLimit,
        ),
    ];
    for (opts, expected) in cases {
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let RunError::Diagnostic(error) =
            scarp::run_with_io_and_options(source, &mut out, &mut err, opts).unwrap_err()
        else {
            panic!("expected a resource diagnostic");
        };
        assert_eq!(error.kind, expected);
        assert_eq!(scarp::check_with_options(source, opts).unwrap_err(), error);
        assert!(out.is_empty() && err.is_empty());
    }
}

#[test]
#[ignore = "opt-in million-digit output stress test"]
fn million_digit_decimal_power_is_exact() {
    let source = "print(bigint_pow(bigint(\"10\"),999999));";
    let mut opts = options(1_000_000);
    opts.limits.max_steps = 20_000_000;
    assert_eq!(run(source, opts), format!("1{}\n", "0".repeat(999_999)));
}

#[test]
fn decimal_power_fast_path_preserves_sign_and_limb_boundaries() {
    for (base, exponent, zeros, negative) in [
        ("10", 8, 8, false),
        ("10", 9, 9, false),
        ("10", 10, 10, false),
        ("-100", 11, 22, true),
        ("-100", 12, 24, false),
        ("1000000000000000000", 7, 126, false),
    ] {
        let source = format!("print(bigint_pow(bigint(\"{base}\"),{exponent}));");
        let sign = if negative { "-" } else { "" };
        assert_eq!(
            run(&source, options(zeros + 1)),
            format!("{sign}1{}\n", "0".repeat(zeros))
        );
    }
}

#[test]
fn beyond_old_ceiling_is_supported_with_explicit_limits() {
    let source = "print(bigint_pow(bigint(\"10\"),1000000));";
    assert_eq!(
        run(source, options(1_000_001)),
        format!("1{}\n", "0".repeat(1_000_000))
    );
}

#[test]
#[ignore = "opt-in ten-million-digit output stress test"]
fn ten_million_digit_decimal_power_is_exact() {
    let source = "print(bigint_pow(bigint(\"10\"),9999999));";
    assert_eq!(
        run(source, options(10_000_000)),
        format!("1{}\n", "0".repeat(9_999_999))
    );
}
