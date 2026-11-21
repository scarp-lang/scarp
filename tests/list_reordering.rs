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
    let RunError::Diagnostic(error) =
        scarp::run_with_io_and_options(source, &mut stdout, &mut stderr, options).unwrap_err()
    else {
        panic!("expected source diagnostic for {source}");
    };
    assert_eq!(error.kind, kind, "{source}");
    assert!(stdout.is_empty() && stderr.is_empty());
    error
}

#[test]
fn reordering_typed_lists_preserves_original_aliases_and_nested_values() {
    let source = r#"
        pure funcra reorder(xs: list<integer>) -> list<integer> {
            return list_swap(list_reverse(xs), 0, 2);
        }
        bind original = [list: 1, 2, 3];
        bind reversed = list_reverse(original);
        bind swapped = list_swap(reversed, 0, 2);
        print(original, reversed, swapped);
        print(reorder(original));
        print(list_swap(original, 1, 1));
        print(list_reverse([list: <integer>]));
        bind nested = [list: [list: "one"], [list: "two"]];
        bind changed = list_swap(nested, 0, 1);
        print(nested[0][0], changed[0][0]);
    "#;
    assert_eq!(
        run(source),
        b"[list: 1, 2, 3] [list: 3, 2, 1] [list: 1, 2, 3]\n[list: 1, 2, 3]\n[list: 1, 2, 3]\n[list: <integer>]\none two\n"
    );
}

#[test]
fn reorder_functions_can_be_shadowed_and_arguments_evaluate_once() {
    assert_eq!(
        run(
            "pure funcra list_reverse(n: integer) -> integer { return n + 1; } \
             pure funcra list_swap(n: integer) -> integer { return n + 2; } \
             print(list_reverse(3), list_swap(4));"
        ),
        b"4 6\n"
    );
    let source = r#"
        funcra make() -> list<integer> { print("list"); return [list: 10, 20]; }
        funcra a() -> integer { print("first"); return 0; }
        funcra b() -> integer { print("second"); return 1; }
        print(list_swap(make(), a(), b()));
        print(list_reverse(make()));
    "#;
    assert_eq!(
        run(source),
        b"list\nfirst\nsecond\n[list: 20, 10]\nlist\n[list: 20, 10]\n"
    );
}

#[test]
fn static_type_and_arity_checks_preserve_unicode_crlf_offsets() {
    for (call, kind, highlighted) in [
        ("list_reverse()", Kind::CallArguments, "list_reverse"),
        ("list_swap([list: 1], 0)", Kind::CallArguments, "list_swap"),
        ("list_reverse([array: 1])", Kind::TypeMismatch, "[array: 1]"),
        ("list_swap([list: 1], true, 0)", Kind::TypeMismatch, "true"),
        ("list_swap([list: 1], 0, false)", Kind::TypeMismatch, "false"),
    ] {
        let source = format!("// नमस्ते 🌏\r\nprint({call});");
        let error = scarp::compile(&source).unwrap_err();
        assert_eq!(error.kind, kind);
        assert_eq!(&source[error.span.start..error.span.end], highlighted);
    }
}

#[test]
fn out_of_bounds_swaps_abort_output_and_highlight_the_first_bad_index() {
    for (source, highlighted) in [
        ("print(1); print(list_swap([list: 2, 3], -1, 1));", "-1"),
        ("print(1); print(list_swap([list: 2, 3], 0, 2));", "2"),
        ("print(1); print(list_swap([list: <integer>], 0, 0));", "0"),
    ] {
        let error = failure(source, ExecutionOptions::default(), Kind::ListIndex);
        assert_eq!(&source[error.span.start..error.span.end], highlighted);
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
fn immutable_list_reordering_is_metered_before_allocation() {
    let options = |bytes| ExecutionOptions {
        max_array_bytes: bytes,
        ..Default::default()
    };
    for source in [
        "bind a=[list: 1, 2]; print(list_reverse(a));",
        "bind a=[list: 1, 2]; print(list_swap(a, 0, 1));",
        "bind a=[list: <integer>]; print(list_reverse(a));",
    ] {
        let limit = minimum(|bytes| scarp::check_with_options(source, options(bytes)).is_ok());
        assert!(limit > 0);
        failure(source, options(limit - 1), Kind::ListAllocation);
        scarp::check_with_options(source, options(limit)).unwrap();
    }
    let once = "bind a=[list: 1, 2]; print(list_reverse(a));";
    let twice = "bind a=[list: 1, 2]; print(list_reverse(list_reverse(a)));";
    let first = minimum(|bytes| scarp::check_with_options(once, options(bytes)).is_ok());
    let second = minimum(|bytes| scarp::check_with_options(twice, options(bytes)).is_ok());
    assert!(second > first);
    failure(twice, options(second - 1), Kind::ListAllocation);
}

#[test]
fn generated_reversals_and_swaps_match_rust_reference_lists() {
    for len in 0..=16usize {
        let values = (0..len).map(|n| n as i64 * 13 - 40).collect::<Vec<_>>();
        let literal = if values.is_empty() {
            "[list: <integer>]".to_owned()
        } else {
            format!(
                "[list: {}]",
                values.iter().map(i64::to_string).collect::<Vec<_>>().join(", ")
            )
        };
        let mut expected = values.clone();
        expected.reverse();
        let result = if expected.is_empty() {
            "[list: <integer>]".to_owned()
        } else {
            format!(
                "[list: {}]",
                expected.iter().map(i64::to_string).collect::<Vec<_>>().join(", ")
            )
        };
        assert_eq!(
            run(&format!("print(list_reverse({literal}));")),
            format!("{result}\n").into_bytes(),
            "length={len}"
        );
        if len > 0 {
            let first = len / 2;
            let second = len - 1;
            let mut expected = values.clone();
            expected.swap(first, second);
            let result = format!(
                "[list: {}]\n",
                expected.iter().map(i64::to_string).collect::<Vec<_>>().join(", ")
            );
            assert_eq!(
                run(&format!("print(list_swap({literal}, {first}, {second}));")),
                result.into_bytes(),
                "length={len}"
            );
        }
    }
}
