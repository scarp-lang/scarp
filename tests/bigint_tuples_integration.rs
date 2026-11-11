use scarp::{diagnostics::DiagnosticKind as Kind, ExecutionOptions, RunError};

fn run(source: &str) -> String {
    let mut stdout = Vec::new();
    scarp::run(source, &mut stdout).expect("valid source");
    String::from_utf8(stdout).expect("valid UTF-8")
}

#[test]
fn bigint_preserves_exact_precision_inside_typed_and_nested_tuples() {
    const N: &str = "98765432109876543210987654321012345678";
    let next = (N.parse::<u128>().expect("independent exact oracle") + 1).to_string();
    let source = format!(
        "pure funcra wrap(x:bigint)->tuple<bigint,string> {{ return (x,\"stored\"); }}          bind pair:tuple<bigint,string>=wrap(bigint(\"{N}\"));          print(pair[0]+bigint(\"1\"),pair[1]);          print((pair,bigint(\"-0\")));          print(pair==(bigint(\"{N}\"),\"stored\"));"
    );
    assert_eq!(
        run(&source),
        format!("{next} stored\n(({N}, \"stored\"), 0)\ntrue\n"),
    );
    assert!(scarp::check(&source).is_ok());
}

#[test]
fn bigint_tuple_snapshots_cannot_hide_decimal_capture_work() {
    let huge = "9".repeat(400);
    let source = format!("print(\"pending\"); print(((bigint(\"{huge}\"),),));");
    let options = ExecutionOptions {
        max_capture_bytes: 250,
        ..ExecutionOptions::default()
    };
    let expected = scarp::check_with_options(&source, options).unwrap_err();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let RunError::Diagnostic(actual) =
        scarp::run_with_io_and_options(&source, &mut out, &mut err, options).unwrap_err()
    else {
        panic!("expected capture diagnostic")
    };
    assert_eq!(actual, expected);
    assert_eq!(actual.kind, Kind::CaptureLimit);
    assert!(out.is_empty() && err.is_empty(), "output must remain atomic");
}

#[test]
fn mixed_numeric_tuple_kinds_do_not_coerce_bigint_to_i64_or_float() {
    for source in [
        "bind x:tuple<integer>=(bigint(\"1\"),);",
        "bind x:tuple<bigint>=(1,);",
        "print((bigint(\"1\"),)==(1,));",
        "print((bigint(\"1\"),)+(bigint(\"2\"),));",
    ] {
        assert!(scarp::compile(source).is_err(), "{source}");
    }
}
