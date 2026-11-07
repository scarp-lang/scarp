use scarp::{
    diagnostics::DiagnosticKind as Kind,
    numeric::{self, Builtin},
    value::{FiniteFloat, NumericVector, Value, MAX_VECTOR_LENGTH},
    ExecutionOptions, RunError,
};

fn inputs(probabilities: &[f64], mass: f64) -> Vec<Value> {
    vec![
        Value::Vector(NumericVector::new(probabilities).unwrap()),
        Value::Float(FiniteFloat::new(mass).unwrap()),
    ]
}

fn indices(probabilities: &[f64], mass: f64) -> Vec<i64> {
    let result = numeric::evaluate(Builtin::TopP, &inputs(probabilities, mass), 2_500_000).unwrap();
    result.as_integers().unwrap().as_slice().to_vec()
}

fn output(source: &str) -> String {
    scarp::check(source).unwrap();
    let mut stdout = Vec::new();
    scarp::run(source, &mut stdout).unwrap();
    String::from_utf8(stdout).unwrap()
}

fn fails_atomically(source: &str, kind: Kind) -> scarp::diagnostics::Diagnostic {
    let options = ExecutionOptions::default();
    let expected = scarp::check_with_options(source, options).unwrap_err();
    assert_eq!(expected.kind, kind, "{source}");
    let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
    let RunError::Diagnostic(actual) =
        scarp::run_with_io_and_options(source, &mut stdout, &mut stderr, options).unwrap_err()
    else {
        panic!("expected a diagnostic");
    };
    assert_eq!(expected, actual, "{source}");
    assert!(stdout.is_empty() && stderr.is_empty(), "{source}");
    expected
}

#[test]
fn nucleus_builtins_are_typed_and_composable_with_softmax() {
    assert_eq!(
        output(
            "bind p=[0.5,0.25,0.25]; print(top_p(p,0.75)); print(top_p_values(p,0.75)); print(top_p(p,1.0));"
        ),
        "[integer: 0, 1]\n[0.5, 0.25]\n[integer: 0, 1, 2]\n"
    );
    assert_eq!(
        output(
            "pure funcra choose(probs: vector, mass: float) -> integers { return top_p(probs,mass); } print(choose([0.5,0.25,0.25],0.5));"
        ),
        "[integer: 0]\n"
    );
    assert_eq!(
        output("bind scores=[1.0,4.0,4.0,2.0]; print(top_p(softmax_temp(scores,0.5),0.9));"),
        "[integer: 1, 2]\n"
    );
    assert_eq!(output("bind top_p=7; print(top_p);"), "7\n");
}

#[test]
fn nucleus_is_stable_on_ties_and_preserves_signed_zero_payloads() {
    assert_eq!(indices(&[0.25, 0.25, 0.25, 0.25], 0.5), vec![0, 1]);
    assert_eq!(indices(&[1.0, 0.0, -0.0], 1.0), vec![0, 1, 2]);
    assert_eq!(indices(&[0.0, 1.0, 0.0], 0.999), vec![1]);
    assert_eq!(indices(&[0.0, 1.0, 0.0], 1.0), vec![1, 0, 2]);

    let result =
        numeric::evaluate(Builtin::TopPValues, &inputs(&[1.0, -0.0, 0.0], 1.0), 100).unwrap();
    let bits: Vec<u64> = result
        .as_vector()
        .unwrap()
        .as_slice()
        .iter()
        .map(|value| value.get().to_bits())
        .collect();
    assert_eq!(
        bits,
        vec![1.0_f64.to_bits(), (-0.0_f64).to_bits(), 0.0_f64.to_bits()]
    );
}

#[test]
fn nucleus_visible_windows_use_local_indices_and_keep_sources_immutable() {
    assert_eq!(
        output(
            "bind source=[7.0,0.5,0.25,0.25,9.0]; bind visible=window(source,1,3); print(top_p(visible,0.75)); print(top_p_values(visible,0.75)); print(source);"
        ),
        "[integer: 0, 1]\n[0.5, 0.25]\n[7.0, 0.5, 0.25, 0.25, 9.0]\n"
    );
}

#[test]
fn nucleus_rejects_invalid_probability_vectors_and_thresholds_atomically() {
    for name in ["top_p", "top_p_values"] {
        for bad in ["0.0", "-0.0", "-0.5", "1.5"] {
            let source = format!("print(\"earlier\"); print({name}([0.5,0.5],{bad}));");
            let error = fails_atomically(&source, Kind::NumericDomain);
            assert_eq!(&source[error.span.start..error.span.end], bad);
        }
        for bad in ["[]", "[0.5,0.25]", "[-0.1,0.6,0.5]", "[0.0,0.0]"] {
            let source = format!("print(\"earlier\"); print({name}({bad},0.5));");
            let error = fails_atomically(&source, Kind::NumericDomain);
            assert!(source[error.span.start..error.span.end].contains(bad));
        }
        fails_atomically(&format!("print({name}([1],0.5));"), Kind::TypeMismatch);
        fails_atomically(&format!("print({name}([1.0],1));"), Kind::TypeMismatch);
        fails_atomically(&format!("print({name}([1.0]));"), Kind::CallArguments);
        fails_atomically(
            &format!("print({name}([1.0],1.0,1.0));"),
            Kind::CallArguments,
        );
    }
}

#[test]
fn nucleus_has_exact_deterministic_work_and_payload_charges() {
    let args = inputs(&[0.5, 0.25, 0.25], 0.75);
    // n=3, bit-length(n)=2: (2+4)*3 = 18 charged work.
    assert_eq!(
        numeric::evaluate_with_vector_limit(Builtin::TopP, &args, 18, 24)
            .unwrap()
            .as_integers()
            .unwrap()
            .as_slice(),
        &[0, 1]
    );
    assert_eq!(
        numeric::evaluate_with_vector_limit(Builtin::TopP, &args, 17, 24)
            .unwrap_err()
            .kind,
        Kind::ExecutionLimit
    );
    assert_eq!(
        numeric::evaluate_with_vector_limit(Builtin::TopP, &args, 18, 23)
            .unwrap_err()
            .kind,
        Kind::VectorBudget
    );
    let values = numeric::evaluate_with_vector_limit(Builtin::TopPValues, &args, 18, 40).unwrap();
    let selected: Vec<f64> = values
        .as_vector()
        .unwrap()
        .as_slice()
        .iter()
        .map(|value| value.get())
        .collect();
    assert_eq!(selected, vec![0.5, 0.25]);
    assert_eq!(
        numeric::evaluate_with_vector_limit(Builtin::TopPValues, &args, 18, 39)
            .unwrap_err()
            .kind,
        Kind::VectorBudget
    );
}

#[test]
fn nucleus_matches_integer_mass_oracle_for_seeded_tie_heavy_inputs() {
    let mut seed = 0x533d_5aa5_u64;
    // All integer weights are <=4 and total <=256. Each threshold's
    // numerator is coprime to 1000, so no prefix can land on an exactly
    // representable rational threshold boundary for this sample size.
    let thresholds = [(173u64, 0.173), (313, 0.313), (613, 0.613), (833, 0.833)];
    for len in [1usize, 2, 3, 7, 16, 64] {
        for _ in 0..32 {
            let weights: Vec<u64> = (0..len)
                .map(|_| {
                    seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
                    (seed >> 32) % 5
                })
                .collect();
            let sum: u64 = weights.iter().sum();
            if sum == 0 {
                continue;
            }
            let probabilities: Vec<f64> = weights.iter().map(|&w| w as f64 / sum as f64).collect();
            let mut ranked: Vec<i64> = (0..len as i64).collect();
            ranked.sort_unstable_by(|&left, &right| {
                weights[right as usize]
                    .cmp(&weights[left as usize])
                    .then_with(|| left.cmp(&right))
            });
            for &(numerator, threshold) in &thresholds {
                let mut total = 0u64;
                let mut oracle = Vec::new();
                for &index in &ranked {
                    total += weights[index as usize];
                    oracle.push(index);
                    if total * 1000 >= numerator * sum {
                        break;
                    }
                }
                assert_eq!(
                    indices(&probabilities, threshold),
                    oracle,
                    "weights={weights:?}, cutoff={threshold}"
                );
            }
            assert_eq!(indices(&probabilities, 1.0), ranked);
        }
    }
}

#[test]
fn maximum_vector_remains_bounded_and_deterministic() {
    let mut probabilities = vec![0.0; MAX_VECTOR_LENGTH];
    probabilities[0] = 1.0;
    assert_eq!(indices(&probabilities, 0.99), vec![0]);
    let result = numeric::evaluate(Builtin::TopP, &inputs(&probabilities, 1.0), 2_000_000).unwrap();
    let full = result.as_integers().unwrap().as_slice();
    assert_eq!(full.len(), MAX_VECTOR_LENGTH);
    assert_eq!(full[0], 0);
    assert_eq!(full[MAX_VECTOR_LENGTH - 1], MAX_VECTOR_LENGTH as i64 - 1);
}
