use scarp::{
    diagnostics::{Diagnostic, DiagnosticKind as Kind},
    ExecutionOptions, RunError,
};

fn run(source: &str) -> Vec<u8> {
    let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
    scarp::run_with_io(source, &mut stdout, &mut stderr).unwrap();
    assert!(stderr.is_empty());
    stdout
}

fn failure(source: &str, options: ExecutionOptions, kind: Kind) -> Diagnostic {
    let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
    let RunError::Diagnostic(diagnostic) =
        scarp::run_with_io_and_options(source, &mut stdout, &mut stderr, options).unwrap_err()
    else {
        panic!("expected source diagnostic for {source}");
    };
    assert_eq!(diagnostic.kind, kind, "{source}");
    assert!(stdout.is_empty() && stderr.is_empty());
    diagnostic
}

#[test]
fn reverse_and_swap_preserve_array_type_values_and_original_aliases() {
    let source = r#"
        pure funcra reorder(items: array<integer, 4>) -> array<integer, 4> {
            return array_swap(array_reverse(items), 0, 3);
        }
        bind original = [array: 1, 2, 3, 4];
        bind reversed = array_reverse(original);
        bind swapped = array_swap(reversed, 1, 3);
        print(original[0], reversed[0], swapped[1], swapped[3]);
        print(reorder(original));
        print(array_reverse(array_reverse(original)));
        print(array_swap(original, 2, 2));
        print(array_reverse([array: <integer>]));
        bind nested = [array: [array: "red"], [array: "blue"]];
        bind updated = array_swap(nested, 0, 1);
        print(nested[0][0], updated[0][0]);
    "#;
    assert_eq!(
        run(source),
        b"1 4 1 3\n[array: 1, 3, 2, 4]\n[array: 1, 2, 3, 4]\n[array: 1, 2, 3, 4]\n[array: <integer>]\nred blue\n"
    );
    assert_eq!(
        run(
            "pure funcra array_reverse(n: integer) -> integer { return n + 1; } \
             pure funcra array_swap(n: integer) -> integer { return n + 2; } \
             print(array_reverse(3), array_swap(4));"
        ),
        b"4 6\n"
    );
}

#[test]
fn reordering_arguments_evaluate_once_in_source_order() {
    let source = r#"
        funcra source() -> array<integer, 2> { print("source"); return [array: 7, 8]; }
        funcra left() -> integer { print("left"); return 0; }
        funcra right() -> integer { print("right"); return 1; }
        print(array_swap(source(), left(), right()));
        print(array_reverse(source()));
    "#;
    assert_eq!(
        run(source),
        b"source\nleft\nright\n[array: 8, 7]\nsource\n[array: 8, 7]\n"
    );
}

#[test]
fn typed_reordering_errors_keep_exact_utf8_and_crlf_source_spans() {
    for (call, kind, highlighted) in [
        ("array_reverse()", Kind::CallArguments, "array_reverse"),
        (
            "array_swap([array: 1], 0)",
            Kind::CallArguments,
            "array_swap",
        ),
        (
            "array_reverse([integer: 1])",
            Kind::TypeMismatch,
            "[integer: 1]",
        ),
        (
            "array_swap([array: 1], true, 0)",
            Kind::TypeMismatch,
            "true",
        ),
        (
            "array_swap([array: 1], 0, false)",
            Kind::TypeMismatch,
            "false",
        ),
    ] {
        let source = format!("// नमस्ते 🌏\r\nprint({call});");
        let diagnostic = scarp::compile(&source).unwrap_err();
        assert_eq!(diagnostic.kind, kind, "{call}");
        assert_eq!(
            &source[diagnostic.span.start..diagnostic.span.end],
            highlighted
        );
    }
}

#[test]
fn swap_rejects_first_and_second_invalid_positions_and_empty_arrays() {
    for (source, highlighted) in [
        ("print(1); print(array_swap([array: 8, 9], -1, 1));", "-1"),
        ("print(1); print(array_swap([array: 8, 9], 0, 2));", "2"),
        (
            "print(1); print(array_swap([array: <integer>], 0, 0));",
            "0",
        ),
    ] {
        let diagnostic = failure(source, ExecutionOptions::default(), Kind::ArrayIndex);
        assert_eq!(
            &source[diagnostic.span.start..diagnostic.span.end],
            highlighted
        );
    }
}

fn minimum(mut passes: impl FnMut(usize) -> bool) -> usize {
    let (mut low, mut high) = (0, 100_000);
    assert!(passes(high));
    while low < high {
        let mid = low + (high - low) / 2;
        if passes(mid) {
            high = mid;
        } else {
            low = mid + 1;
        }
    }
    low
}

#[test]
fn each_reverse_or_swap_snapshot_consumes_checked_shared_array_quota() {
    let options = |bytes| ExecutionOptions {
        max_array_bytes: bytes,
        ..Default::default()
    };
    for source in [
        "bind a=[array: 1, 2]; print(array_reverse(a));",
        "bind a=[array: 1, 2]; print(array_swap(a, 0, 1));",
        "bind a=[array: <integer>]; print(array_reverse(a));",
    ] {
        let exact = minimum(|bytes| scarp::check_with_options(source, options(bytes)).is_ok());
        assert!(exact > 0);
        failure(source, options(exact - 1), Kind::ArrayAllocation);
        scarp::check_with_options(source, options(exact)).unwrap();
    }
    let single = "bind a=[array: 1, 2]; print(array_reverse(a));";
    let twice = "bind a=[array: 1, 2]; print(array_reverse(array_reverse(a)));";
    let single_min = minimum(|bytes| scarp::check_with_options(single, options(bytes)).is_ok());
    let twice_min = minimum(|bytes| scarp::check_with_options(twice, options(bytes)).is_ok());
    assert!(twice_min > single_min);
    failure(twice, options(twice_min - 1), Kind::ArrayAllocation);
}

#[test]
fn reordering_operations_charge_execution_work_before_copying() {
    let source = "bind a=[array: 1, 2, 3, 4]; print(array_reverse(a));";
    let options = |steps| ExecutionOptions {
        limits: scarp::ExecutionLimits {
            max_steps: steps as u64,
            ..Default::default()
        },
        ..Default::default()
    };
    let exact = minimum(|steps| scarp::check_with_options(source, options(steps)).is_ok());
    assert!(exact > 0);
    failure(source, options(exact - 1), Kind::ExecutionLimit);
    scarp::check_with_options(source, options(exact)).unwrap();
}

#[test]
fn generated_reversals_and_swaps_match_rust_reference_arrays() {
    for length in 0..=24usize {
        let elements = (0..length)
            .map(|value| value as i64 * 7 - 19)
            .collect::<Vec<_>>();
        let literal = if elements.is_empty() {
            "[array: <integer>]".to_owned()
        } else {
            format!(
                "[array: {}]",
                elements
                    .iter()
                    .map(i64::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        };
        let actual = run(&format!("print(array_reverse({literal}));"));
        let mut expected = elements.clone();
        expected.reverse();
        let display = if expected.is_empty() {
            "[array: <integer>]".to_owned()
        } else {
            format!(
                "[array: {}]",
                expected
                    .iter()
                    .map(i64::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        };
        assert_eq!(
            actual,
            format!("{display}\n").into_bytes(),
            "length={length}"
        );
        if length > 0 {
            let left = length / 3;
            let right = length - 1;
            let actual = run(&format!("print(array_swap({literal}, {left}, {right}));"));
            let mut expected = elements.clone();
            expected.swap(left, right);
            let display = format!(
                "[array: {}]\n",
                expected
                    .iter()
                    .map(i64::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            assert_eq!(actual, display.into_bytes(), "length={length}");
        }
    }
}

#[test]
fn concatenation_infers_lengths_evaluates_once_and_preserves_aliases() {
    let source = r#"
       funcra part(n: integer) -> array<integer, 1> { print(n); return [array: n]; }
       bind pair: array<integer, 2> = array_concat(part(7),part(8));
       bind nested = [array: pair];
       bind both = array_concat(nested, nested);
       print(both, pair);
       print(array_set(both, 0, [array: 1, 2]), nested);
    "#;
    assert_eq!(run(source), b"7\n8\n[array: [array: 7, 8], [array: 7, 8]] [array: 7, 8]\n[array: [array: 1, 2], [array: 7, 8]] [array: [array: 7, 8]]\n");
    assert_eq!(
        run(include_str!("../examples/array_concat.srp")),
        include_bytes!("../examples/array_concat.stdout")
    );
    assert_eq!(
        run("pure funcra array_concat(n:integer)->integer{return n+1;} print(array_concat(2));"),
        b"3\n"
    );
}

#[test]
fn concatenation_validates_both_types_and_the_complete_output_shape() {
    for (call, kind, highlighted) in [
        ("array_concat(1,[array:2])", Kind::TypeMismatch, "1"),
        ("array_concat([array:1],false)", Kind::TypeMismatch, "false"),
        (
            "array_concat([array:1],[array:<float>])",
            Kind::TypeMismatch,
            "[array:<float>]",
        ),
        (
            "array_concat([array:1])",
            Kind::CallArguments,
            "array_concat",
        ),
        (
            "array_concat(x:[array:1],y:[array:2])",
            Kind::CallArguments,
            "array_concat",
        ),
    ] {
        let source = format!("// नमस्ते 🌏\r\nprint({call});");
        let error = scarp::compile(&source).unwrap_err();
        assert_eq!(error.kind, kind);
        assert_eq!(&source[error.span.start..error.span.end], highlighted);
    }
    for (element, length) in [("integer", 2049), ("array<integer,31>", 1024)] {
        let source = format!("pure funcra f(a:array<{element},{length}>)->integer{{return array_len(array_concat(a,a));}}");
        let error = scarp::compile(&source).unwrap_err();
        assert_eq!(error.kind, Kind::ArrayLimit);
        assert_eq!(
            &source[error.span.start..error.span.end],
            "array_concat(a,a)"
        );
    }
    scarp::compile(
        "pure funcra f(a:array<integer,2048>)->array<integer,4096>{return array_concat(a,a);}",
    )
    .unwrap();
}

#[test]
fn concatenation_charges_one_snapshot_and_shares_typed_empty_identity() {
    let setup = "bind pair=[array:1,2]; bind empty=[array:<integer>];";
    let options = |bytes| ExecutionOptions {
        max_array_bytes: bytes,
        ..Default::default()
    };
    let threshold =
        |source: &str| minimum(|bytes| scarp::check_with_options(source, options(bytes)).is_ok());
    let base = threshold(setup);
    for operation in [
        "array_concat(pair,empty)",
        "array_concat(empty,pair)",
        "array_concat(empty,empty)",
    ] {
        let source = format!("{setup} print({operation});");
        assert_eq!(threshold(&source), base);
    }
    let joined = format!("{setup} bind joined=array_concat(pair,pair);");
    let single = threshold(&joined);
    assert!(single > base);
    failure(&joined, options(single - 1), Kind::ArrayAllocation);
    let twice = format!("{joined} bind other=array_concat(pair,pair);");
    assert_eq!(threshold(&twice) - single, single - base);
    let steps = |n| ExecutionOptions {
        limits: scarp::ExecutionLimits {
            max_steps: n as u64,
            ..Default::default()
        },
        ..Default::default()
    };
    let exact = minimum(|n| scarp::check_with_options(&joined, steps(n)).is_ok());
    failure(&joined, steps(exact - 1), Kind::ExecutionLimit);
}
