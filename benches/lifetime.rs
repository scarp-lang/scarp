//! Repeatable Lifetime view/clone microbenchmarks. No physical RSS claims.
//! Run: cargo bench --bench lifetime -- --quick
//! Compare on the same host with the identical benchmark source at both commits.

use scarp::value::{IntegerVector, NumericVector};
use std::{hint::black_box, time::Instant};

fn median_ns(iterations: usize, samples: usize, mut run: impl FnMut()) -> f64 {
    for _ in 0..64 {
        run();
    }
    let mut times = Vec::with_capacity(samples);
    for _ in 0..samples {
        let start = Instant::now();
        for _ in 0..iterations {
            run();
        }
        times.push(start.elapsed().as_secs_f64() * 1_000_000_000.0 / iterations as f64);
    }
    times.sort_by(|a, b| a.partial_cmp(b).unwrap());
    times[samples / 2]
}

fn main() {
    let mut quick = false;
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "--quick" => quick = true,
            "--bench" => {}
            _ => {
                eprintln!("Usage: cargo bench --bench lifetime -- [--quick]");
                std::process::exit(2);
            }
        }
    }
    let (iterations, samples) = if quick { (1000, 3) } else { (10000, 9) };
    let numerics = NumericVector::new(&vec![1.0; 65_536]).unwrap();
    let integers = IntegerVector::new(&vec![1; 65_536]).unwrap();
    println!("workload,median_ns_per_operation");
    for (name, time) in [
        (
            "numeric_empty_view",
            median_ns(iterations, samples, || {
                black_box(numerics.window(32768, 0).unwrap());
            }),
        ),
        (
            "numeric_nonempty_view",
            median_ns(iterations, samples, || {
                black_box(numerics.window(32768, 8).unwrap());
            }),
        ),
        (
            "numeric_empty_constructor",
            median_ns(iterations, samples, || {
                black_box(NumericVector::new(&[]).unwrap());
            }),
        ),
        (
            "numeric_clone",
            median_ns(iterations, samples, || {
                black_box(numerics.clone());
            }),
        ),
        (
            "integer_empty_view",
            median_ns(iterations, samples, || {
                black_box(integers.window(32768, 0).unwrap());
            }),
        ),
        (
            "integer_nonempty_view",
            median_ns(iterations, samples, || {
                black_box(integers.window(32768, 8).unwrap());
            }),
        ),
        (
            "integer_empty_constructor",
            median_ns(iterations, samples, || {
                black_box(IntegerVector::new(&[]).unwrap());
            }),
        ),
    ] {
        println!("{name},{time:.3}");
    }
}
