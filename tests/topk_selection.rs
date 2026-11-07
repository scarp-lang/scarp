use scarp::{
    diagnostics::DiagnosticKind as Kind,
    numeric::{self, Builtin},
    value::{NumericVector, Value, MAX_VECTOR_LENGTH},
    ExecutionOptions, RunError,
};

fn output(source: &str) -> String {
    scarp::check(source).unwrap();
    let mut stdout = Vec::new();
    scarp::run(source, &mut stdout).unwrap();
    String::from_utf8(stdout).unwrap()
}

fn fails_atomically(source: &str, expected: Kind) -> scarp::diagnostics::Diagnostic {
    let options = ExecutionOptions::default();
    let checked = scarp::check_with_options(source, options).unwrap_err();
    assert_eq!(checked.kind, expected, "{source}");
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let RunError::Diagnostic(executed) =
        scarp::run_with_io_and_options(source, &mut stdout, &mut stderr, options).unwrap_err()
    else {
        panic!("expected a diagnostic");
    };
    assert_eq!(checked, executed);
    assert!(stdout.is_empty() && stderr.is_empty());
    checked
}

fn inputs(scores: &[f64], count: i64) -> Vec<Value> {
    vec![
        Value::Vector(NumericVector::new(scores).unwrap()),
        Value::Integer(count),
    ]
}

fn indexes(scores: &[f64], count: usize) -> Vec<i64> {
    let result =
        numeric::evaluate(Builtin::TopK, &inputs(scores, count as i64), 1_000_000).unwrap();
    result.as_integers().unwrap().as_slice().to_vec()
}

#[test]
fn topk_and_topk_values_are_typed_and_work_in_user_functions() {
    assert_eq!(
        output(
            "bind scores=[0.1, 0.9, 0.9, -1.0]; print(topk(scores,3)); print(topk_values(scores,3));"
        ),
        "[integer: 1, 2, 0]\n[0.9, 0.9, 0.1]\n"
    );
    assert_eq!(
        output(
            "pure funcra choose(scores: vector, count: integer) -> integers { return topk(scores, count); } print(choose([1.0, 4.0, 2.0], 2));"
        ),
        "[integer: 1, 2]\n"
    );
    assert_eq!(
        output("print(topk([],0)); print(topk_values([],0)); print(topk([4.0],0));"),
        "[integer:]\n[]\n[integer:]\n"
    );
    // User declarations still take priority over similarly named intrinsics.
    assert_eq!(output("bind topk=7; print(topk);"), "7\n");
}

#[test]
fn topk_is_stable_on_equal_scores_signed_zero_and_subnormals() {
    assert_eq!(indexes(&[4.0, 4.0, 4.0, 4.0], 3), vec![0, 1, 2]);
    assert_eq!(indexes(&[-0.0, 0.0, -0.0, 0.0], 4), vec![0, 1, 2, 3]);
    let tiny = f64::from_bits(1);
    assert_eq!(indexes(&[-tiny, tiny, 0.0, tiny], 3), vec![1, 3, 2]);
    let original = inputs(&[-0.0, 0.0, -0.0], 3);
    let selected = numeric::evaluate(Builtin::TopKValues, &original, 100).unwrap();
    let bits: Vec<u64> = selected
        .as_vector()
        .unwrap()
        .as_slice()
        .iter()
        .map(|v| v.get().to_bits())
        .collect();
    assert_eq!(
        bits,
        vec![
            (-0.0_f64).to_bits(),
            (0.0_f64).to_bits(),
            (-0.0_f64).to_bits()
        ]
    );
}

#[test]
fn topk_respects_visible_windows_and_input_is_immutable() {
    assert_eq!(
        output(
            "bind original=[100.0, 1.0, 3.0, 3.0, 200.0]; bind middle=window(original,1,3); print(topk(middle,2)); print(topk_values(middle,2)); print(original);"
        ),
        "[integer: 1, 2]\n[3.0, 3.0]\n[100.0, 1.0, 3.0, 3.0, 200.0]\n"
    );
}

#[test]
fn topk_rejects_bad_counts_types_and_arity_without_output_leaks() {
    for name in ["topk", "topk_values"] {
        for bad in ["-1", "3", "9223372036854775807"] {
            let source = format!("print(\"earlier\"); print({name}([1.0,2.0],{bad}));");
            let error = fails_atomically(&source, Kind::VectorIndex);
            assert_eq!(&source[error.span.start..error.span.end], bad);
        }
        fails_atomically(
            &format!("print({name}([1.0,2.0],1.0));"),
            Kind::TypeMismatch,
        );
        fails_atomically(&format!("print({name}([1,2],1));"), Kind::TypeMismatch);
        fails_atomically(&format!("print({name}([1.0]));"), Kind::CallArguments);
        fails_atomically(&format!("print({name}([1.0],1,2));"), Kind::CallArguments);
    }
}

#[test]
fn topk_precharges_work_and_bounded_heap_memory() {
    let args = inputs(&[1.0, 5.0, 3.0], 2);
    // (n+k) * floor(log2(k)+1) = (3+2)*2 = 10 work steps.
    assert_eq!(
        numeric::evaluate_with_vector_limit(Builtin::TopK, &args, 10, 16)
            .unwrap()
            .as_integers()
            .unwrap()
            .as_slice(),
        &[1, 2]
    );
    assert_eq!(
        numeric::evaluate_with_vector_limit(Builtin::TopK, &args, 9, 16)
            .unwrap_err()
            .kind,
        Kind::ExecutionLimit
    );
    assert_eq!(
        numeric::evaluate_with_vector_limit(Builtin::TopK, &args, 10, 15)
            .unwrap_err()
            .kind,
        Kind::VectorBudget
    );
    assert_eq!(
        numeric::evaluate_with_vector_limit(Builtin::TopKValues, &args, 10, 32)
            .unwrap()
            .as_vector()
            .unwrap()
            .as_slice()
            .iter()
            .map(|v| v.get())
            .collect::<Vec<_>>(),
        vec![5.0, 3.0]
    );
    assert_eq!(
        numeric::evaluate_with_vector_limit(Builtin::TopKValues, &args, 10, 31)
            .unwrap_err()
            .kind,
        Kind::VectorBudget
    );
    for function in [Builtin::TopK, Builtin::TopKValues] {
        let zero = inputs(&[], 0);
        numeric::evaluate_with_vector_limit(function, &zero, 0, 0).unwrap();
    }
}

#[test]
fn topk_matches_full_sort_reference_across_many_small_inputs() {
    let mut state = 0x5eed_1234_u64;
    for len in [0usize, 1, 2, 3, 7, 16, 64] {
        for _ in 0..32 {
            let mut scores = Vec::new();
            for _ in 0..len {
                state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
                // Frequent ties, negative scores, and signed-zero changes.
                let value = ((state >> 32) % 9) as i64 - 4;
                scores.push(if value == 0 && state & 1 == 1 {
                    -0.0
                } else {
                    value as f64
                });
            }
            for count in [0, 1, 2, len / 2, len] {
                if count > len {
                    continue;
                }
                let mut reference: Vec<i64> = (0..len as i64).collect();
                reference.sort_unstable_by(|&a, &b| {
                    scores[b as usize]
                        .partial_cmp(&scores[a as usize])
                        .unwrap()
                        .then_with(|| a.cmp(&b))
                });
                reference.truncate(count);
                assert_eq!(
                    indexes(&scores, count),
                    reference,
                    "scores={scores:?} k={count}"
                );
            }
        }
    }
}

#[test]
fn large_equal_input_has_bounded_ranked_results() {
    let vector = vec![7.0; MAX_VECTOR_LENGTH];
    assert_eq!(indexes(&vector, 3), vec![0, 1, 2]);
    let selected = numeric::evaluate(
        Builtin::TopK,
        &inputs(&vector, MAX_VECTOR_LENGTH as i64),
        2_500_000,
    )
    .unwrap();
    let selected = selected.as_integers().unwrap().as_slice();
    assert_eq!(selected.len(), MAX_VECTOR_LENGTH);
    assert_eq!(selected[0], 0);
    assert_eq!(
        selected[MAX_VECTOR_LENGTH - 1],
        MAX_VECTOR_LENGTH as i64 - 1
    );
}
