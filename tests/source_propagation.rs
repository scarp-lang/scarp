use scarp::{diagnostics::DiagnosticKind, value::Value, ExecutionOptions, RunError};
use std::mem::size_of;

fn run(source: &str) -> String {
    let mut output = Vec::new();
    scarp::run(source, &mut output).unwrap();
    String::from_utf8(output).unwrap()
}

fn invalid(source: &str, expected: DiagnosticKind, token: &str) {
    let error = scarp::compile(source).unwrap_err();
    assert_eq!(error.kind, expected, "{error:?}");
    assert_eq!(&source[error.span.start..error.span.end], token);
}

#[test]
fn typed_propagation_task_submission_matches_golden() {
    assert_eq!(
        run(include_str!("../examples/typed_propagation.srp")).as_bytes(),
        include_bytes!("../examples/typed_propagation.stdout"),
    );
}

#[test]
fn propagation_evaluates_once_and_skips_downstream_work_on_error() {
    let source = r#"
        funcra step(fail: boolean) -> result<integer, string> {
            print("step");
            if fail { return new result<integer, string>::Err("blocked"); }
            return new result<integer, string>::Ok(4);
        }
        funcra consume(fail: boolean) -> result<integer, string> {
            bind value = step(fail)?;
            print("after");
            return new result<integer, string>::Ok(value + 2);
        }
        print(consume(true));
        print(consume(false));
    "#;
    assert_eq!(
        run(source),
        "step\nnew result<integer, string>::Err(\"blocked\")\nstep\nafter\nnew result<integer, string>::Ok(6)\n"
    );
}

#[test]
fn option_propagation_can_change_success_type() {
    let source = r#"
        pure funcra transform(input: option<integer>) -> option<string> {
            bind payload = input?;
            if payload > 0 { return new option<string>::Some("positive"); }
            return new option<string>::None();
        }
        print(transform(new option<integer>::None()));
        print(transform(new option<integer>::Some(10)));
    "#;
    assert_eq!(
        run(source),
        "new option<string>::None()\nnew option<string>::Some(\"positive\")\n"
    );
}

#[test]
fn incompatible_propagation_families_and_error_types_are_diagnosed() {
    for source in [
        "funcra f() -> option<integer> { bind x = new result<integer, string>::Ok(1)?; return new option<integer>::Some(x); }",
        "funcra f() -> result<integer, string> { bind x = new option<integer>::Some(1)?; return new result<integer, string>::Ok(x); }",
        "funcra f() -> result<integer, string> { bind x = new result<integer, boolean>::Err(true)?; return new result<integer, string>::Ok(x); }",
        "funcra f() -> integer { bind x = new option<integer>::Some(1)?; return x; }",
        "funcra f() -> option<integer> { bind x = 5?; return new option<integer>::Some(x); }",
    ] {
        invalid(source, DiagnosticKind::InvalidReturn, "?");
    }
}

#[test]
fn propagation_is_for_immutable_function_bindings_only() {
    invalid(
        "bind x = new option<integer>::Some(1)?;",
        DiagnosticKind::InvalidReturn,
        "?",
    );
    invalid(
        "var x = new option<integer>::Some(1)?;",
        DiagnosticKind::InvalidReturn,
        "?",
    );
}

#[test]
fn propagation_checks_bound_payload_annotation_not_wrapper_type() {
    let source = "funcra f() -> option<integer> { bind x: string = new option<integer>::Some(1)?; return new option<integer>::Some(0); }";
    let error = scarp::compile(source).unwrap_err();
    assert_eq!(error.kind, DiagnosticKind::TypeMismatch);
    assert!(error.message.contains("string") || error.message.contains("integer"));
}

#[test]
fn failed_postcondition_on_early_return_uses_normal_completion() {
    let source = r#"
        pure funcra f(value: option<integer>) -> option<integer>
            ensures result == new option<integer>::Some(4);
        {
            bind n = value?;
            return new option<integer>::Some(n);
        }
        print(f(new option<integer>::None()));
    "#;
    let mut output = Vec::new();
    let error = scarp::run(source, &mut output).unwrap_err();
    assert!(
        format!("{error}").contains("ensures")
            || format!("{error:?}").contains("ContractViolation")
    );
    assert!(output.is_empty());
}

#[test]
fn nested_scope_retirement_and_unicode_cache_spans_remain_safe() {
    let source = r#"
        pure funcra read(value: option<integer>) -> option<integer> {
            if true {
                bind scratch = (1, 2);
                bind count = value?;
                return new option<integer>::Some(count + scratch[0]);
            }
            return new option<integer>::None();
        }
        print(read(new option<integer>::Some(7)));
        print(read(new option<integer>::None()));
    "#;
    assert_eq!(
        run(source),
        "new option<integer>::Some(8)\nnew option<integer>::None()\n"
    );
}

#[test]
fn forwarding_none_into_a_different_option_does_not_allocate_a_payload() {
    let source = r#"
        pure funcra forward(value: option<integer>) -> option<boolean> {
            bind payload = value?;
            return new option<boolean>::Some(payload > 0);
        }
        print(forward(new option<integer>::None()));
    "#;
    let options = ExecutionOptions {
        max_tuple_bytes: 0,
        ..ExecutionOptions::default()
    };
    let mut stdout = Vec::new();
    scarp::run_with_options(source, &mut stdout, options).unwrap();
    assert_eq!(stdout, b"new option<boolean>::None()\n");
}

#[test]
fn forwarding_an_error_reuses_its_payload_under_the_exact_allocation_limit() {
    let source = r#"
        pure funcra forward(value: result<integer, string>) -> result<boolean, string> {
            bind payload = value?;
            return new result<boolean, string>::Ok(payload > 0);
        }
        print("buffered");
        eprint("also buffered");
        bind original = new result<integer, string>::Err("बंद");
        print(forward(original));
        print(original);
    "#;
    let exact = size_of::<Value>() + 2 * size_of::<usize>();
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let options = ExecutionOptions {
        max_tuple_bytes: exact - 1,
        ..ExecutionOptions::default()
    };
    assert!(matches!(
        scarp::run_with_io_and_options(source, &mut stdout, &mut stderr, options),
        Err(RunError::Diagnostic(error)) if error.kind == DiagnosticKind::VariantAllocation
    ));
    assert!(stdout.is_empty() && stderr.is_empty());

    let options = ExecutionOptions {
        max_tuple_bytes: exact,
        ..options
    };
    scarp::run_with_io_and_options(source, &mut stdout, &mut stderr, options).unwrap();
    assert_eq!(
        String::from_utf8(stdout).unwrap(),
        "buffered\nnew result<boolean, string>::Err(\"बंद\")\nnew result<integer, string>::Err(\"बंद\")\n"
    );
    assert_eq!(stderr, b"also buffered\n");
}

#[test]
fn propagation_checks_the_initializer_in_a_pure_context() {
    invalid(
        r#"
            funcra read() -> option<integer> {
                print("side effect");
                return new option<integer>::None();
            }
            pure funcra forward() -> option<integer> {
                bind value = read()?;
                return new option<integer>::Some(value);
            }
        "#,
        DiagnosticKind::ImpureFunction,
        "read",
    );
}
