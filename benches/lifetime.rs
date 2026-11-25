//! Repeatable Lifetime view, clone and frame-reuse benchmarks.
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

    let wide_sum = format!("n{}", " + 1".repeat(180));
    let wide_tail = format!(
        "pure funcra descend(n: integer, total: integer = 0) -> integer {{
             if n == 0 {{ return total; }}
             return descend(n - 1, total + ({wide_sum}));
         }} print(descend(128));"
    );
    for (name, source, expected) in [
        (
            "ordinary_sibling_calls_512",
            "pure funcra next(n: integer) -> integer { return n + 1; }
             var total = 0; repeat 512 as n { total += next(n); } print(total);",
            &b"131328\n"[..],
        ),
        ("terminal_wide_calls_128", &wide_tail, &b"31296\n"[..]),
    ] {
        let program = scarp::compile(source).unwrap();
        let mut output = Vec::new();
        scarp::interpreter::execute(&program, &mut output).unwrap();
        assert_eq!(output, expected);
        let time = median_ns(if quick { 25 } else { 100 }, samples, || {
            scarp::interpreter::execute(black_box(&program), &mut std::io::sink()).unwrap();
        });
        println!("{name},{time:.3}");
    }
}
