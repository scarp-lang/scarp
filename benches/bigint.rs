//! Reproducible interpreter timings; compile once, then execute the real plan.
//! --quick is CI-sized. --large enables only linear work at 100k/1m digits.
//! Times include dispatch, parsing constructors and output formatting to a sink.
use scarp::{ExecutionLimits, ExecutionOptions};
use std::{hint::black_box, io, time::Instant};

fn measure(name: &str, digits: usize, source: &str, repeats: usize, options: ExecutionOptions) {
    let plan = scarp::compile(source).unwrap();
    scarp::interpreter::execute_with_options(&plan, &mut io::sink(), options).unwrap();
    let start = Instant::now();
    for _ in 0..repeats {
        scarp::interpreter::execute_with_options(black_box(&plan), &mut io::sink(), options)
            .unwrap();
    }
    println!(
        "{name},{digits},{repeats},{}",
        start.elapsed().as_nanos() / repeats as u128
    );
}
fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let quick = args.iter().any(|arg| arg == "--quick");
    let large = args.iter().any(|arg| arg == "--large");
    assert!(
        args.iter()
            .all(|arg| matches!(arg.as_str(), "--quick" | "--large" | "--bench")),
        "use --quick or --large"
    );
    println!("operation,decimal_digits,repetitions,nanoseconds_per_execution");
    let sizes: &[usize] = if quick {
        &[19, 100]
    } else if large {
        &[19, 100, 1000, 10000, 100000, 1000000]
    } else {
        &[19, 100, 1000, 10000]
    };
    for &digits in sizes {
        let text = "8".repeat(digits);
        let other = "7".repeat((digits / 2).max(1));
        let prefix = format!("bind a=bigint(\"{text}\"); bind b=bigint(\"{other}\");");
        let options = ExecutionOptions {
            max_bigint_digits: (digits * 2).min(scarp::bigint::MAX_BIGINT_DIGITS),
            limits: ExecutionLimits {
                max_steps: 100_000_000,
                ..ExecutionLimits::default()
            },
            ..ExecutionOptions::default()
        };
        let repeats = if quick {
            2
        } else if digits >= 100000 {
            3
        } else {
            15
        };
        for (name, suffix) in [
            ("parse", ""),
            ("add", "bind result=a+b;"),
            ("subtract", "bind result=a-b;"),
            ("compare", "bind result=a>b;"),
            ("format", "print(a);"),
        ] {
            measure(name, digits, &format!("{prefix}{suffix}"), repeats, options);
        }
        // Schoolbook multiplication/general division are intentionally skipped
        // for 100k/1m fixtures, even with --large.
        if digits <= 10000 {
            for (name, suffix) in [
                ("multiply", "bind result=a*b;"),
                ("divide", "bind result=a/b;"),
                ("remainder", "bind result=a%b;"),
            ] {
                measure(name, digits, &format!("{prefix}{suffix}"), repeats, options);
            }
        }
    }
}
