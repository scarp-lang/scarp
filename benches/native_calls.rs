//! Compare frontend, end-to-end native emission, and interpreter throughput.

use scarp::native_object::emit_elf64_bounded_integer_entry;
use std::{hint::black_box, time::Instant};

fn sample(batch: usize, mut action: impl FnMut()) -> f64 {
    let start = Instant::now();
    for _ in 0..batch {
        action();
    }
    start.elapsed().as_secs_f64() * 1_000_000.0 / batch as f64
}

fn text_size(object: &[u8]) -> u64 {
    let read = |at| u64::from_le_bytes(object[at..at + 8].try_into().unwrap());
    let headers = usize::try_from(read(40)).unwrap();
    read(headers + 64 + 32)
}

fn run(name: &str, script: &str, entry: &str, expected: &[u8], samples: usize, batch: usize) {
    let (definitions, _) = script.split_once("\nprint(").unwrap();
    let execution = scarp::compile(script).unwrap();
    let mut output = Vec::new();
    scarp::interpreter::execute(&execution, &mut output).unwrap();
    assert_eq!(output, expected, "{name}");
    let syntax = scarp::parse(definitions).unwrap();
    let artifact = emit_elf64_bounded_integer_entry(definitions, entry, 2000).unwrap();
    let text_bytes = text_size(&artifact.object);
    let object_bytes = artifact.object.len();
    for _ in 0..10 {
        black_box(emit_elf64_bounded_integer_entry(definitions, entry, 2000).unwrap());
        scarp::interpreter::execute(&execution, &mut std::io::sink()).unwrap();
    }
    for trial in 1..=samples {
        // Prepare AST ownership outside the timed resolve batching.
        let mut trees = vec![syntax.clone(); batch].into_iter();
        let parse = sample(batch, || {
            black_box(scarp::parse(black_box(definitions)).unwrap());
        });
        let resolve = sample(batch, || {
            black_box(scarp::resolver::resolve(black_box(trees.next().unwrap())).unwrap());
        });
        let emit = sample(batch, || {
            black_box(
                emit_elf64_bounded_integer_entry(black_box(definitions), entry, 2000).unwrap(),
            );
        });
        let interpret = sample(batch, || {
            scarp::interpreter::execute(black_box(&execution), &mut std::io::sink()).unwrap();
        });
        println!("{name},{trial},{parse:.3},{resolve:.3},{emit:.3},{interpret:.3},{text_bytes},{object_bytes}");
    }
}

fn main() {
    let mut quick = false;
    for argument in std::env::args().skip(1) {
        match argument.as_str() {
            "--quick" => quick = true,
            "--bench" => {}
            _ => {
                eprintln!("Usage: cargo bench --bench native_calls -- [--quick]");
                std::process::exit(2);
            }
        }
    }
    let (samples, batch) = if quick { (3, 20) } else { (7, 200) };
    eprintln!(
        "Microseconds per operation; {samples} trials, batch size {batch}; arch={}, os={}",
        std::env::consts::ARCH,
        std::env::consts::OS
    );
    println!("workload,trial,parse_us,resolve_us,emit_end_to_end_us,interpret_us,text_bytes,object_bytes");
    run(
        "register_calls",
        include_str!("../examples/native_direct_calls.srp"),
        "evaluate",
        include_bytes!("../examples/native_direct_calls.stdout"),
        samples,
        batch,
    );
    run(
        "mixed_calls",
        include_str!("../examples/native_wide_calls.srp"),
        "entry",
        include_bytes!("../examples/native_wide_calls.stdout"),
        samples,
        batch,
    );
    run(
        "aggregate_locals",
        concat!(
            include_str!("../examples/native_aggregate_locals.srp"),
            "\nprint(entry(1, 7));"
        ),
        "entry",
        b"17\n",
        samples,
        batch,
    );
    for (name, signature) in [
        (
            "concrete_integer_call",
            "pure funcra square(x: integer) -> integer",
        ),
        (
            "generic_integer_call",
            "pure funcra square<T: Numeric>(x: T) -> T",
        ),
    ] {
        let script = format!(
            "{signature} {{ return x * x; }}\n\
             pure funcra entry(x: integer) -> integer {{ return square(x); }}\n\
             print(entry(9));"
        );
        run(name, &script, "entry", b"81\n", samples, batch);
    }
}
