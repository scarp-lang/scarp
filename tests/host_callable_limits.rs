use std::{io, sync::Arc};

use scarp::{
    diagnostics::DiagnosticKind as Kind,
    modules::{self, MemorySourceLoader, ModuleLimits},
    resolver::CallableCatalog,
    value::Value,
    ExecutionOptions, InputLimits, RunError,
};

fn kind(error: RunError) -> Kind {
    match error {
        RunError::Diagnostic(error) => error.kind,
        RunError::Output(error) => panic!("unexpected output error: {error}"),
    }
}

#[test]
fn host_initial_frame_and_nested_calls_obey_call_depth_count_and_work_limits() {
    let source = "pure funcra child() -> integer { return 7; }\n\
                  pure funcra parent() -> integer { return child(); }";
    let handle = CallableCatalog::from_program(scarp::compile(source).unwrap())
        .bind_exact(1)
        .unwrap();
    let defaults = ExecutionOptions::default();
    for options in [
        ExecutionOptions {
            max_call_depth: 0,
            ..defaults
        },
        ExecutionOptions {
            max_calls: 0,
            ..defaults
        },
        ExecutionOptions {
            max_live_slots: 0,
            ..defaults
        },
        ExecutionOptions {
            max_call_depth: 1,
            ..defaults
        },
        ExecutionOptions {
            max_calls: 1,
            ..defaults
        },
    ] {
        assert_eq!(
            kind(
                handle
                    .invoke_with_io_and_options(vec![], &mut Vec::new(), &mut Vec::new(), options)
                    .unwrap_err()
            ),
            Kind::CallLimit
        );
    }
    let mut no_work = defaults;
    no_work.limits.max_steps = 0;
    assert_eq!(
        kind(
            handle
                .invoke_with_io_and_options(vec![], &mut Vec::new(), &mut Vec::new(), no_work)
                .unwrap_err()
        ),
        Kind::ExecutionLimit
    );
    assert_eq!(
        handle
            .invoke_with_io_and_options(vec![], &mut Vec::new(), &mut Vec::new(), defaults)
            .unwrap(),
        Value::Integer(7)
    );
}

#[test]
fn host_terminal_self_recursion_reuses_storage_but_charges_logical_calls() {
    let source = "pure funcra tally(n: integer, count: integer = 0) -> integer\n\
                      needs n >= 0;\n\
                  { if n == 0 { return count; } return tally(n - 1, count + 1); }";
    let handle = CallableCatalog::from_program(scarp::compile(source).unwrap())
        .bind_exact(0)
        .unwrap();
    let options = ExecutionOptions {
        max_call_depth: 1,
        max_calls: 500,
        ..ExecutionOptions::default()
    };
    assert_eq!(
        handle
            .invoke_with_io_and_options(
                vec![Value::Integer(300)],
                &mut Vec::new(),
                &mut Vec::new(),
                options
            )
            .unwrap(),
        Value::Integer(300)
    );
    assert_eq!(
        kind(
            handle
                .invoke_with_io_and_options(
                    vec![Value::Integer(300)],
                    &mut Vec::new(),
                    &mut Vec::new(),
                    ExecutionOptions {
                        max_calls: 100,
                        ..options
                    }
                )
                .unwrap_err()
        ),
        Kind::CallLimit
    );
}

#[test]
fn output_capture_quota_applies_before_host_side_replay() {
    let handle = CallableCatalog::from_program(
        scarp::compile("funcra emit() -> unit { print(\"something\"); }").unwrap(),
    )
    .bind_exact(0)
    .unwrap();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    assert_eq!(
        kind(
            handle
                .invoke_with_io_and_options(
                    vec![],
                    &mut out,
                    &mut err,
                    ExecutionOptions {
                        max_capture_bytes: 0,
                        ..ExecutionOptions::default()
                    }
                )
                .unwrap_err()
        ),
        Kind::CaptureLimit
    );
    assert!(out.is_empty());
    assert!(err.is_empty());
}

#[test]
fn retained_handle_can_read_input_and_skip_file_entry_code() {
    let source = "funcra echo() -> string { return read_line(); }\nprint(\"entry\");";
    let original = Arc::new(scarp::compile(source).unwrap());
    let catalog = CallableCatalog::new(Arc::clone(&original));
    let handle = catalog.bind_exact(0).unwrap();
    drop(original);
    drop(catalog);
    let (mut out, mut err) = (Vec::new(), Vec::new());
    assert_eq!(
        kind(
            handle
                .invoke_with_io_and_options(vec![], &mut out, &mut err, ExecutionOptions::default())
                .unwrap_err()
        ),
        Kind::InputUnavailable
    );
    let mut input = io::Cursor::new(b"hello world\r\n");
    assert_eq!(
        handle
            .invoke_with_input_and_options(
                vec![],
                &mut input,
                &mut out,
                &mut err,
                ExecutionOptions::default(),
                InputLimits::default()
            )
            .unwrap(),
        Value::from("hello world")
    );
    assert!(out.is_empty());
    assert!(err.is_empty());
}

#[test]
fn imported_named_function_invokes_using_its_linked_snapshot() {
    let mut loader = MemorySourceLoader::new();
    loader
        .insert(
            "main.srp",
            "import \"math.srp\" as math; print(math.twice(8));",
        )
        .unwrap();
    loader
        .insert(
            "math.srp",
            "pub pure funcra twice(n: integer) -> integer { return n * 2; }",
        )
        .unwrap();
    let module =
        modules::compile_with_loader("main.srp", &loader, ModuleLimits::default()).unwrap();
    let catalog = CallableCatalog::from_program(module.program().clone());
    assert_eq!(catalog.len(), 1);
    assert_eq!(
        catalog
            .bind_exact(0)
            .unwrap()
            .invoke_with_io_and_options(
                vec![Value::Integer(9)],
                &mut Vec::new(),
                &mut Vec::new(),
                ExecutionOptions::default()
            )
            .unwrap(),
        Value::Integer(18)
    );
}

#[test]
fn output_writer_failure_surfaces_as_host_io_error_after_evaluation() {
    struct Reject;
    impl io::Write for Reject {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "host rejected output",
            ))
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let handle = CallableCatalog::from_program(
        scarp::compile("funcra print_it(n: integer) -> integer { print(n); return n; }").unwrap(),
    )
    .bind_exact(0)
    .unwrap();
    assert!(matches!(
        handle.invoke_with_io_and_options(
            vec![Value::Integer(2)],
            &mut Reject,
            &mut Vec::new(),
            ExecutionOptions::default()
        ),
        Err(RunError::Output(_))
    ));
}
