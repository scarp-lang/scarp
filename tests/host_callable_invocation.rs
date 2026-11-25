use scarp::{
    diagnostics::DiagnosticKind as Kind,
    resolver::CallableCatalog,
    value::{FiniteFloat, Value, MAX_TEXT_BYTES},
    ExecutionOptions, RunError,
};

fn kind(error: RunError) -> Kind {
    match error {
        RunError::Diagnostic(error) => error.kind,
        RunError::Output(error) => panic!("unexpected output error: {error}"),
    }
}

fn call(handle: &scarp::resolver::NamedCallableRef, args: Vec<Value>) -> Result<Value, RunError> {
    handle.invoke_with_io_and_options(
        args,
        &mut Vec::new(),
        &mut Vec::new(),
        ExecutionOptions::default(),
    )
}

#[test]
fn host_invoke_runs_named_targets_not_entry_script() {
    let source = "pure funcra add(n: integer) -> integer { return n + 3; }\n\
                  pure funcra triple(n: integer) -> integer { return n * 3; }\n\
                  print(add(4), triple(4));";
    let program = scarp::compile(source).unwrap();
    let catalog = CallableCatalog::from_program(program.clone());
    let add = catalog.bind_exact(0).unwrap();
    let triple = catalog.bind_exact(1).unwrap();
    assert_eq!(add.signature(), triple.signature());
    assert!(!add.same_target(&triple));
    assert_eq!(
        call(&add, vec![Value::Integer(4)]).unwrap(),
        Value::Integer(7)
    );
    assert_eq!(
        call(&triple, vec![Value::Integer(4)]).unwrap(),
        Value::Integer(12)
    );
    let (mut out, mut err) = (Vec::new(), Vec::new());
    scarp::interpreter::execute_with_io(&program, &mut out, &mut err).unwrap();
    assert_eq!(out, b"7 12\n");
    assert!(err.is_empty());
}

#[test]
fn host_invoke_executes_defaults_needs_ensures_and_implicit_unit() {
    let source = "pure funcra scale(x: integer, factor: integer = 3) -> integer\n\
                      needs factor > 0;\n\
                      ensures result == x * factor;\n\
                  { return x * factor; }\n\
                  pure funcra noop() -> unit {}";
    let catalog = CallableCatalog::from_program(scarp::compile(source).unwrap());
    let scale = catalog.bind_exact(0).unwrap();
    assert_eq!(
        call(&scale, vec![Value::Integer(4)]).unwrap(),
        Value::Integer(12)
    );
    assert_eq!(
        call(&scale, vec![Value::Integer(4), Value::Integer(2)]).unwrap(),
        Value::Integer(8)
    );
    assert_eq!(
        kind(call(&scale, vec![Value::Integer(4), Value::Integer(0)]).unwrap_err()),
        Kind::ContractViolation
    );
    assert_eq!(
        call(&catalog.bind_exact(1).unwrap(), vec![]).unwrap(),
        Value::Unit
    );
}

#[test]
fn host_invocation_preserves_dual_streams_and_atomic_failure() {
    let source = "funcra emit(n: integer, suffix: string = \"!\") -> unit {\n\
                      print(n); eprint(suffix); return;\n\
                  }\n\
                  funcra failed(n: integer) -> integer\n\
                      ensures result > 0;\n\
                  { print(\"pending\"); eprint(\"pending\"); return 0 / n; }";
    let catalog = CallableCatalog::from_program(scarp::compile(source).unwrap());
    let (mut out, mut err) = (Vec::new(), Vec::new());
    assert_eq!(
        catalog
            .bind_exact(0)
            .unwrap()
            .invoke_with_io_and_options(
                vec![Value::Integer(5)],
                &mut out,
                &mut err,
                ExecutionOptions::default(),
            )
            .unwrap(),
        Value::Unit
    );
    assert_eq!(out, b"5\n");
    assert_eq!(err, b"!\n");
    out.clear();
    err.clear();
    let failed = catalog.bind_exact(1).unwrap();
    for (arg, diagnostic) in [(1, Kind::ContractViolation), (0, Kind::DivisionByZero)] {
        assert_eq!(
            kind(
                failed
                    .invoke_with_io_and_options(
                        vec![Value::Integer(arg)],
                        &mut out,
                        &mut err,
                        ExecutionOptions::default(),
                    )
                    .unwrap_err()
            ),
            diagnostic
        );
        assert!(out.is_empty());
        assert!(err.is_empty());
    }
}

#[test]
fn invalid_positional_args_and_types_fail_before_program_output() {
    let source = "funcra f(x: integer, y: integer = 2) -> integer {\n\
                      print(\"called\"); return x + y;\n\
                  }";
    let handle = CallableCatalog::from_program(scarp::compile(source).unwrap())
        .bind_exact(0)
        .unwrap();
    for args in [
        vec![],
        vec![Value::Integer(1), Value::Integer(2), Value::Integer(3)],
    ] {
        assert_eq!(kind(call(&handle, args).unwrap_err()), Kind::CallArguments);
    }
    assert_eq!(
        kind(call(&handle, vec![Value::Boolean(true)]).unwrap_err()),
        Kind::TypeMismatch
    );
}

#[test]
fn floats_and_unicode_are_accepted_but_excessive_host_text_is_rejected() {
    let source = "pure funcra message(text: string, x: float) -> string {\n\
                      if x > 0.0 { return text; } return \"no\";\n\
                  }";
    let handle = CallableCatalog::from_program(scarp::compile(source).unwrap())
        .bind_exact(0)
        .unwrap();
    let positive = Value::Float(FiniteFloat::new(1.0).unwrap());
    assert_eq!(
        call(&handle, vec![Value::from("नमस्ते"), positive.clone()]).unwrap(),
        Value::from("नमस्ते")
    );
    assert_eq!(
        kind(
            call(
                &handle,
                vec![Value::from("a".repeat(MAX_TEXT_BYTES + 1)), positive]
            )
            .unwrap_err()
        ),
        Kind::TextConstruction
    );
    assert!(FiniteFloat::new(f64::NAN).is_none());
}

#[test]
fn host_supplied_numeric_vectors_remain_explicitly_unsupported() {
    let source = "pure funcra identity(v: vector) -> vector { return v; }";
    let handle = CallableCatalog::from_program(scarp::compile(source).unwrap())
        .bind_exact(0)
        .unwrap();
    let vector = scarp::value::NumericVector::new(&[1.0]).unwrap();
    assert_eq!(
        kind(call(&handle, vec![Value::Vector(vector)]).unwrap_err()),
        Kind::FunctionUnsupported
    );
}

#[test]
fn returned_host_snapshot_survives_sibling_calls_and_failed_frames_release_arguments() {
    use std::sync::Arc;
    let source = r#"
        pure funcra copy(s: string) -> string { return s; }
        pure funcra pair(s: string, ok: boolean) -> tuple<string, string>
            ensures ok;
        {
            bind first = copy(s);
            bind second = copy("next");
            return (first, second);
        }
    "#;
    let catalog = CallableCatalog::from_program(scarp::compile(source).unwrap());
    let pair = catalog.bind_exact(1).unwrap();
    for ok in [false, true, false] {
        let text: Arc<str> = Arc::from("श".repeat(4096));
        let weak = Arc::downgrade(&text);
        let result = call(&pair, vec![Value::String(text), Value::Boolean(ok)]);
        if ok {
            let value = result.unwrap();
            let snapshot = value.as_tuple().unwrap();
            assert_eq!(
                snapshot.elements()[0].as_str().unwrap().chars().count(),
                4096
            );
            assert_eq!(snapshot.elements()[1].as_str(), Some("next"));
            assert!(weak.upgrade().is_some());
            drop(value);
        } else {
            assert_eq!(kind(result.unwrap_err()), Kind::ContractViolation);
        }
        assert!(weak.upgrade().is_none());
    }
}
