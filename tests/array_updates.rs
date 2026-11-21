use scarp::{
    diagnostics::{Diagnostic, DiagnosticKind as Kind},
    ExecutionOptions, RunError,
};

fn run(source: &str) -> (Vec<u8>, Vec<u8>) {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    scarp::run_with_io(source, &mut out, &mut err).unwrap();
    (out, err)
}

fn failure(source: &str, options: ExecutionOptions, kind: Kind) -> Diagnostic {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let error = scarp::run_with_io_and_options(source, &mut out, &mut err, options)
        .unwrap_err();
    let RunError::Diagnostic(diagnostic) = error else {
        panic!("expected a source diagnostic");
    };
    assert_eq!(diagnostic.kind, kind, "{source}");
    assert!(out.is_empty() && err.is_empty(), "failed programs must be atomic");
    diagnostic
}

#[test]
fn updated_arrays_preserve_old_snapshots_exact_types_and_nested_values() {
    let source = r#"
        pure funcra replace_last(values: array<integer, 3>) -> array<integer, 3> {
            return array_set(values, 2, 100);
        }
        bind original = [array: 4, 8, 12];
        var current: array<integer, 3> = original;
        current = array_set(current, 1, 99);
        print(original[1], current[1], array_len(current));
        bind nested = [array: [array: "one"], [array: "two"]];
        bind changed = array_set(nested, 0, [array: "नमस्ते"]);
        print(nested[0][0], changed[0][0]);
        print(replace_last(current)[2]);
    "#;
    assert_eq!(run(source), ("8 99 3\none नमस्ते\n100\n".as_bytes().to_vec(), Vec::new()));
    assert_eq!(run("pure funcra array_set(n: integer) -> integer { return n + 1; } print(array_set(41));").0, b"42\n");
}

#[test]
fn update_arguments_run_once_in_source_order() {
    let source = r#"
        funcra source() -> array<integer, 1> { print("array"); return [array: 7]; }
        funcra position() -> integer { print("index"); return 0; }
        funcra replacement() -> integer { print("value"); return 8; }
        print(array_set(source(), position(), replacement())[0]);
    "#;
    assert_eq!(run(source).0, b"array\nindex\nvalue\n8\n");
}

#[test]
fn updates_reject_wrong_arity_and_types_at_exact_unicode_offsets() {
    for (call, kind, highlighted) in [
        ("array_set()", Kind::CallArguments, "array_set"),
        (
            "array_set(values: [array: 1], 0, 2)",
            Kind::CallArguments,
            "array_set",
        ),
        (
            "array_set([list: 1], 0, 2)",
            Kind::TypeMismatch,
            "[list: 1]",
        ),
        (
            "array_set([array: 1], false, 2)",
            Kind::TypeMismatch,
            "false",
        ),
        (
            "array_set([array: 1], 0, \"wrong\")",
            Kind::TypeMismatch,
            "\"wrong\"",
        ),
    ] {
        let source = format!("// नमस्ते 🌍\r\nprint({call});");
        let diagnostic = scarp::compile(&source).unwrap_err();
        assert_eq!(diagnostic.kind, kind, "{call}");
        assert_eq!(
            &source[diagnostic.span.start..diagnostic.span.end],
            highlighted
        );
    }
}

#[test]
fn update_bounds_fail_without_publishing_output_or_changing_aliases() {
    for (source, highlighted) in [
        ("print(123); bind original=[array: 1]; print(array_set(original, -1, 9));", "-1"),
        ("print(123); bind original=[array: 1]; print(array_set(original, 1, 9));", "1"),
        (
            "print(123); bind empty=[array: <integer>]; print(array_set(empty, 0, 9));",
            "0",
        ),
    ] {
        let diagnostic = failure(source, ExecutionOptions::default(), Kind::ArrayIndex);
        assert_eq!(&source[diagnostic.span.start..diagnostic.span.end], highlighted);
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
fn each_persistent_snapshot_is_charged_before_allocation() {
    let limits = |bytes| ExecutionOptions {
        max_array_bytes: bytes,
        ..Default::default()
    };
    let original = "bind a=[array: 1, 2, 3]; print(a);";
    let updated = "bind a=[array: 1, 2, 3]; print(array_set(a, 1, 9));";
    let original_minimum =
        minimum(|bytes| scarp::check_with_options(original, limits(bytes)).is_ok());
    let update_minimum =
        minimum(|bytes| scarp::check_with_options(updated, limits(bytes)).is_ok());
    assert!(update_minimum > original_minimum);
    failure(updated, limits(update_minimum - 1), Kind::ArrayAllocation);
    scarp::check_with_options(updated, limits(update_minimum)).unwrap();
}
