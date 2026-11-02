//! Correctness-checked frontend/metadata workloads; timings are not runtime speedups.
use scarp::{
    modules::{self, MemorySourceLoader, ModuleLimits},
    ExecutionOptions,
};
use std::{hint::black_box, time::Instant};

fn measure(name: &str, bytes: usize, iterations: u32, mut work: impl FnMut()) {
    for sample in 1..=3 {
        let start = Instant::now();
        for _ in 0..iterations {
            work();
        }
        let mean = start.elapsed().as_secs_f64() * 1_000_000.0 / f64::from(iterations);
        println!("{name},{bytes},{iterations},{sample},{mean:.3}");
    }
}
fn main() {
    let iterations = if std::env::args().any(|arg| arg == "--quick") {
        3
    } else {
        25
    };
    let mut source = String::from("var total: integer=0;\n");
    for index in 0..512 {
        source.push_str(&format!(
            "bind v{index}: integer={index}; total+=v{index};\n"
        ));
    }
    source.push_str("print(total);");
    let mut expected = Vec::new();
    scarp::run(&source, &mut expected).unwrap();
    assert_eq!(expected, b"130816\n");
    assert_eq!(scarp::compile(&source).unwrap().binding_count(), 513);

    let mut base = String::from("bind v0: integer=0;\n");
    for index in 1..=512 {
        base.push_str(&format!("bind v{index}: integer=v{}+1;\n", index - 1));
    }
    base.push_str("pub bind value: integer=v512;");
    let files=[("main.srp","import \"left.srp\" as left; import \"right.srp\" as right; print(left.value+right.value);"),
        ("left.srp","import \"base.srp\" as base; pub bind value: integer=base.value+1;"),
        ("right.srp","import \"base.srp\" as base; pub bind value: integer=base.value*2;"),("base.srp",base.as_str())];
    let mut loader = MemorySourceLoader::new();
    for (name, text) in files {
        loader.insert(name, text).unwrap();
    }
    let compile =
        || modules::compile_with_loader("main.srp", &loader, ModuleLimits::default()).unwrap();
    let program = compile();
    assert_eq!(program.modules().len(), 4);
    assert_eq!(program.constant_dependencies().len(), 515);
    let (mut out, mut err) = (Vec::new(), Vec::new());
    program
        .execute(&mut out, &mut err, ExecutionOptions::default())
        .unwrap();
    assert_eq!(out, b"1537\n");
    assert!(err.is_empty());
    let mut json = Vec::new();
    program.write_inspection_json(&mut json).unwrap();
    let size = json.len();
    assert!(json.starts_with(b"{\"schema_version\":1,"));
    let mut again = Vec::new();
    program.write_inspection_json(&mut again).unwrap();
    assert_eq!(json, again);

    println!("workload,input_bytes,iterations,sample,mean_us");
    measure("annotated_bindings_512", source.len(), iterations, || {
        black_box(scarp::compile(black_box(&source)).unwrap());
    });
    measure(
        "module_constants_4files",
        files.iter().map(|(_, text)| text.len()).sum(),
        iterations,
        || {
            black_box(compile());
        },
    );
    measure("inspection_json_4files", size, iterations, || {
        json.clear();
        program.write_inspection_json(black_box(&mut json)).unwrap();
        black_box(json.len());
    });
}
