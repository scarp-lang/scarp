//! Acceptance tests for the 100-million-decimal-digit milestone.
//! Large runs are opt-in; ordinary CI exercises the same writer on small values.
#![cfg(target_pointer_width = "64")]

use scarp::{diagnostics::DiagnosticKind as Kind, ExecutionLimits, ExecutionOptions, RunError};
use std::io::{self, Write};

const TARGET: usize = 100_000_000;

fn options(digits: usize) -> ExecutionOptions {
    ExecutionOptions {
        // The base literal 10 requires two digits even when exponent is zero.
        max_bigint_digits: digits.max(2),
        max_bigint_bytes: 512 * 1024 * 1024,
        max_capture_bytes: digits + 1024,
        limits: ExecutionLimits {
            max_steps: 2_000_000_000,
            ..ExecutionLimits::default()
        },
        ..ExecutionOptions::default()
    }
}

// Closed-form decimal oracle; no full output string or reference BigInt.
// Check every byte, including the sign, first digit, and final newline.
struct ExactOutput {
    prefix: &'static [u8],
    repeated: u8,
    repeats: usize,
    seen: usize,
}

impl Write for ExactOutput {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        for &byte in bytes {
            let expected = if self.seen < self.prefix.len() {
                self.prefix[self.seen]
            } else if self.seen < self.prefix.len() + self.repeats {
                self.repeated
            } else if self.seen == self.prefix.len() + self.repeats {
                b'\n'
            } else {
                panic!("extra output at byte {}", self.seen);
            };
            assert_eq!(byte, expected, "byte {}", self.seen);
            self.seen += 1;
        }
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn assert_output(source: &str, digits: usize, prefix: &'static [u8], repeated: u8) {
    let sign = usize::from(prefix.first() == Some(&b'-'));
    let mut out = ExactOutput {
        prefix,
        repeated,
        repeats: digits + sign - prefix.len(),
        seen: 0,
    };
    scarp::run_with_options(source, &mut out, options(digits)).unwrap();
    assert_eq!(out.seen, digits + sign + 1);
}

fn dense_add(digits: usize) {
    // 2*10^(d-1)-1 = 1 followed by d-1 nines. No intermediate
    // exceeds the selected decimal capacity.
    let source = format!(
        "bind p=bigint_pow(bigint(\"10\"),{}); print(-(p+(p-bigint(\"1\"))));",
        digits - 1
    );
    assert_output(&source, digits, b"-1", b'9');
}

fn dense_divide(digits: usize) {
    // (10^d-1)/3 is d threes. Construct without a d+1 digit
    // intermediate and exercise small-factor multiplication and division.
    let source = format!(
        "bind p=bigint_pow(bigint(\"10\"),{}); \
         print((p*bigint(\"9\")+(p-bigint(\"1\")))/bigint(\"3\"));",
        digits - 1
    );
    assert_output(&source, digits, b"", b'3');
}

#[test]
fn chunk_boundaries_preserve_signed_dense_digits_and_single_limb_division() {
    for digits in [1, 9, 10, 9216, 9217, 9218, 18433, 18434] {
        dense_add(digits);
        dense_divide(digits);
    }
}

#[test]
fn target_preflight_rejects_one_digit_over_and_low_allocation_budget() {
    assert!(scarp::bigint::MAX_BIGINT_DIGITS >= TARGET);
    let source = "print(\"pending\"); print(bigint_pow(bigint(\"10\"),99999999));";
    let cases = [
        ExecutionOptions {
            max_bigint_digits: TARGET - 1,
            ..options(TARGET)
        },
        ExecutionOptions {
            max_bigint_bytes: 1024,
            ..options(TARGET)
        },
    ];
    for opts in cases {
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let RunError::Diagnostic(error) =
            scarp::run_with_io_and_options(source, &mut out, &mut err, opts).unwrap_err()
        else {
            panic!("expected resource diagnostic");
        };
        assert_eq!(error.kind, Kind::BigIntLimit);
        assert!(out.is_empty() && err.is_empty());
    }
}

#[test]
#[ignore = "opt-in 100-million-digit signed dense addition and formatting"]
fn hundred_million_digit_dense_addition() {
    dense_add(TARGET);
}

#[test]
#[ignore = "opt-in 100-million-digit single-limb multiplication and division"]
fn hundred_million_digit_dense_division() {
    dense_divide(TARGET);
}

#[test]
#[ignore = "opt-in 100-million-digit sparse power and formatting"]
fn hundred_million_digit_sparse_power() {
    assert_output(
        "print(bigint_pow(bigint(\"10\"),99999999));",
        TARGET,
        b"1",
        b'0',
    );
}
