//! Verified numerical workloads; native API and interpreter scopes are explicit.
use scarp::{
    numeric::{self, Builtin},
    value::{FiniteFloat, Value},
    ExecutionLimits, ExecutionOptions,
};
use std::{hint::black_box, time::Instant};

#[derive(Clone, Copy)]
enum Native {
    Dot(usize),
    Calls,
    Allocation,
    Statistics(usize),
    Activations(usize),
    Rms(usize),
    SampleArea(usize),
    SampleSlopes(usize),
    SampleCumulativeArea(usize),
}

fn call(kernel: Builtin, args: &[Value]) -> Value {
    numeric::evaluate(kernel, args, 10_000_000).unwrap()
}
fn float(value: f64) -> Value {
    Value::Float(FiniteFloat::new(value).unwrap())
}
fn fill(value: f64, count: usize) -> Value {
    call(Builtin::Fill, &[float(value), Value::Integer(count as i64)])
}

fn native(workload: Native) -> Value {
    match workload {
        Native::Rms(count) => call(Builtin::Rms, &[fill(3.0, count)]),
        Native::SampleArea(count) => {
            let x: Vec<f64> = (0..count).map(|i| i as f64).collect();
            let x = Value::Vector(scarp::value::NumericVector::new(&x).unwrap());
            call(Builtin::SampleArea, &[x, fill(2.0, count)])
        }
        Native::SampleCumulativeArea(count) => {
            let x: Vec<f64> = (0..count).map(|i| i as f64).collect();
            let x = Value::Vector(scarp::value::NumericVector::new(&x).unwrap());
            let trajectory = call(Builtin::SampleCumulativeArea, &[x, fill(2.0, count)]);
            call(Builtin::Sum, &[trajectory])
        }
        Native::SampleSlopes(count) => {
            let x: Vec<f64> = (0..count).map(|i| i as f64).collect();
            let x = Value::Vector(scarp::value::NumericVector::new(&x).unwrap());
            let y = call(Builtin::Scale, &[x.clone(), float(2.0)]);
            let slopes = call(Builtin::SampleSlopes, &[x, y]);
            call(Builtin::Sum, &[slopes])
        }
        Native::Statistics(count) => {
            let values = call(
                Builtin::Stitch,
                &[fill(0.0, count / 2), fill(2.0, count / 2)],
            );
            float(
                call(Builtin::Variance, std::slice::from_ref(&values))
                    .as_float()
                    .unwrap()
                    + call(Builtin::StdDev, &[values]).as_float().unwrap(),
            )
        }
        Native::Activations(count) => call(
            Builtin::Sum,
            &[call(
                Builtin::Sigmoid,
                &[call(
                    Builtin::Tanh,
                    &[call(Builtin::Relu, &[fill(-1.0, count)])],
                )],
            )],
        ),
        Native::Dot(count) => call(Builtin::Dot, &[fill(1.0, count), fill(2.0, count)]),
        Native::Calls => {
            let values = fill(1.0, 1024);
            let mut sum = 0.0;
            for _ in 0..64 {
                sum += call(Builtin::Dot, &[values.clone(), values.clone()])
                    .as_float()
                    .unwrap();
            }
            float(sum)
        }
        Native::Allocation => {
            let mut values = fill(1.0, 1024);
            for _ in 0..32 {
                values = call(
                    Builtin::Add,
                    &[call(Builtin::Scale, &[values, float(0.5)]), fill(0.5, 1024)],
                );
            }
            call(Builtin::Sum, &[values])
        }
    }
}

fn median(repetitions: usize, samples: usize, mut action: impl FnMut()) -> f64 {
    for _ in 0..2 {
        action();
    }
    let mut times = Vec::new();
    for _ in 0..samples {
        let start = Instant::now();
        for _ in 0..repetitions {
            action();
        }
        times.push(start.elapsed().as_secs_f64() * 1_000_000.0 / repetitions as f64);
    }
    times.sort_by(|a, b| a.partial_cmp(b).unwrap());
    times[samples / 2]
}

fn main() {
    let mut quick = false;
    for argument in std::env::args().skip(1) {
        match argument.as_str() {
            "--quick" => quick = true,
            "--bench" => {} // Cargo also supplies this for harness=false.
            _ => {
                eprintln!("usage: cargo bench --bench numerics -- [--quick]");
                std::process::exit(2);
            }
        }
    }
    let samples = if quick { 3 } else { 7 };
    let options = ExecutionOptions {
        limits: ExecutionLimits {
            max_steps: 10_000_000,
            ..ExecutionLimits::default()
        },
        ..ExecutionOptions::default()
    };
    eprintln!("Median microseconds/call; samples={samples}; warmups=2; native includes fresh inputs, checked kernels and allocations; Scarp validation excludes formatting; arch={}, os={}",std::env::consts::ARCH,std::env::consts::OS);
    println!(
        "workload,compile_us,validate_us,execute_us,native_us,repetitions,samples,source_bytes"
    );
    let mut cases = Vec::new();
    for count in [16, 32_768] {
        for (name, source, workload) in [
            (
                "sample_area",
                format!(
                    "bind x=ints_to_vector(ints_range(0,{count},1)); print(sample_area(x,fill(2.0,{count})));"
                ),
                Native::SampleArea(count),
            ),
            (
                "sample_cumulative_area",
                format!(
                    "bind x=ints_to_vector(ints_range(0,{count},1)); print(sum(sample_cumulative_area(x,fill(2.0,{count}))));"
                ),
                Native::SampleCumulativeArea(count),
            ),
            (
                "sample_slopes",
                format!(
                    "bind x=ints_to_vector(ints_range(0,{count},1)); print(sum(sample_slopes(x,scale(x,2.0))));"
                ),
                Native::SampleSlopes(count),
            ),
        ] {
            cases.push((
                format!("{name}_{count}"),
                source,
                if name == "sample_cumulative_area" {
                    (count * (count - 1)) as f64
                } else {
                    2.0 * (count - 1) as f64
                },
                workload,
                count,
            ));
        }
        cases.push((
            format!("root_mean_square_{count}"),
            format!("print(rms(fill(3.0,{count})));"),
            3.0,
            Native::Rms(count),
            count,
        ));
        cases.push((
            format!("population_statistics_{count}"),
            format!(
                "bind v=stitch(fill(0.0,{}),fill(2.0,{})); print(variance(v)+stddev(v));",
                count / 2,
                count / 2
            ),
            2.0,
            Native::Statistics(count),
            count,
        ));
        cases.push((
            format!("activation_pipeline_{count}"),
            format!("print(sum(sigmoid(tanh(relu(fill(-1.0,{count}))))));"),
            count as f64 / 2.0,
            Native::Activations(count),
            count,
        ));
        let inputs = format!("bind a=fill(1.0,{count}); bind b=fill(2.0,{count});");
        cases.push((
            format!("indexed_dot_{count}"),
            format!(
                "{inputs}var total=0.0; repeat len(a) as i {{total+=a[i]*b[i];}} print(total);"
            ),
            2.0 * count as f64,
            Native::Dot(count),
            count,
        ));
        cases.push((
            format!("kernel_dot_{count}"),
            format!("{inputs}print(dot(a,b));"),
            2.0 * count as f64,
            Native::Dot(count),
            count,
        ));
    }
    cases.push(("repeated_funcra_1024".into(),"pure funcra score(v: vector)->float{return dot(v,v);} bind v=fill(1.0,1024); var total=0.0; repeat 64 as i {total+=score(v);} print(total);".into(),65_536.0,Native::Calls,1024));
    cases.push((
        "allocation_pipeline_1024".into(),
        "var v=fill(1.0,1024); repeat 32 as i {v=add(scale(v,0.5),fill(0.5,1024));} print(sum(v));"
            .into(),
        1024.0,
        Native::Allocation,
        1024,
    ));
    for (name, source, expected, workload, size) in cases {
        let program = scarp::compile(&source).unwrap();
        let mut output = Vec::new();
        scarp::interpreter::execute_with_options(&program, &mut output, options).unwrap();
        assert_eq!(output, format!("{expected:?}\n").as_bytes(), "{name}");
        assert_eq!(native(workload).as_float(), Some(expected), "native {name}");
        let repetitions = if size == 16 {
            if quick {
                50
            } else {
                500
            }
        } else if quick {
            2
        } else {
            10
        };
        let compile = median(if quick { 2 } else { 10 }, samples, || {
            black_box(scarp::compile(black_box(&source)).unwrap());
        });
        let validate = median(repetitions, samples, || {
            scarp::interpreter::validate_with_options(black_box(&program), options).unwrap();
        });
        let execute = median(repetitions, samples, || {
            scarp::interpreter::execute_with_options(
                black_box(&program),
                &mut std::io::sink(),
                options,
            )
            .unwrap();
        });
        let native = median(repetitions, samples, || {
            black_box(native(black_box(workload)));
        });
        println!(
            "{name},{compile:.3},{validate:.3},{execute:.3},{native:.3},{repetitions},{samples},{}",
            source.len()
        );
    }
}
