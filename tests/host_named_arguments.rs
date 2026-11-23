use std::io::Cursor;

use scarp::{
    diagnostics::{DiagnosticKind, Span},
    modules::{self, MemorySourceLoader, ModuleLimits},
    resolver::{CallableCatalog, HostArguments, NamedCallableRef},
    value::{Value, MAX_TEXT_BYTES},
    ExecutionOptions, InputLimits, RunError,
};

fn invoke(handle: &NamedCallableRef, args: HostArguments) -> Result<Value, RunError> {
    handle.invoke_bound_with_io_and_options(
        args,
        &mut Vec::new(),
        &mut Vec::new(),
        ExecutionOptions::default(),
    )
}

fn failure(error: RunError, kind: DiagnosticKind) -> Span {
    match error {
        RunError::Diagnostic(actual) => {
            assert_eq!(actual.kind, kind);
            actual.span
        }
        RunError::Output(other) => panic!("unexpected writer failure: {other}"),
    }
}

#[test]
fn binds_named_and_positional_values_in_declaration_order() {
    let program = scarp::compile(
        "pure funcra calc(start: integer, step: integer = 2, end: integer = step + 1) -> integer\n\
             needs start > 0;\n\
             ensures result == start * 100 + step * 10 + end;\n\
         { return start * 100 + step * 10 + end; }\n\
         print(calc(1));",
    )
    .unwrap();
    let handle = CallableCatalog::from_program(program)
        .bind_exact(0)
        .unwrap();
    assert_eq!(
        invoke(
            &handle,
            HostArguments::new(
                vec![Value::Integer(7)],
                vec![("end".into(), Value::Integer(9))]
            ),
        )
        .unwrap(),
        Value::Integer(729)
    );
    assert_eq!(
        invoke(
            &handle,
            HostArguments::new(
                vec![],
                vec![
                    ("end".into(), Value::Integer(7)),
                    ("start".into(), Value::Integer(3)),
                    ("step".into(), Value::Integer(4)),
                ],
            ),
        )
        .unwrap(),
        Value::Integer(347)
    );
    assert_eq!(
        invoke(&handle, HostArguments::positional(vec![Value::Integer(5)])).unwrap(),
        Value::Integer(523)
    );
    assert_eq!(
        invoke(
            &handle,
            HostArguments::new(
                vec![Value::Integer(6), Value::Integer(5)],
                vec![("end".into(), Value::Integer(4))],
            ),
        )
        .unwrap(),
        Value::Integer(654)
    );
    assert_eq!(
        failure(
            invoke(&handle, HostArguments::new(vec![Value::Integer(0)], vec![])).unwrap_err(),
            DiagnosticKind::ContractViolation,
        ),
        handle.metadata().contracts[0].span
    );
}

#[test]
fn omitted_late_default_sees_prior_named_parameter() {
    let handle = CallableCatalog::from_program(
        scarp::compile(
            "pure funcra chain(a: integer, b: integer = a + 1, c: integer = b + 1) -> integer {\n\
             return a * 100 + b * 10 + c;\n\
             }",
        )
        .unwrap(),
    )
    .bind_exact(0)
    .unwrap();
    assert_eq!(
        invoke(
            &handle,
            HostArguments::new(vec![], vec![("a".into(), Value::Integer(3))])
        )
        .unwrap(),
        Value::Integer(345)
    );
    assert_eq!(
        invoke(
            &handle,
            HostArguments::new(
                vec![],
                vec![
                    ("c".into(), Value::Integer(8)),
                    ("a".into(), Value::Integer(3))
                ],
            ),
        )
        .unwrap(),
        Value::Integer(348)
    );
}

#[test]
fn supplied_named_default_suppresses_selected_failure_path() {
    let handle = CallableCatalog::from_program(
        scarp::compile(
            "pure funcra safe(a: integer, b: integer = 12 / a) -> integer { return b; }",
        )
        .unwrap(),
    )
    .bind_exact(0)
    .unwrap();
    assert_eq!(
        invoke(
            &handle,
            HostArguments::new(
                vec![],
                vec![
                    ("b".into(), Value::Integer(5)),
                    ("a".into(), Value::Integer(0)),
                ]
            ),
        )
        .unwrap(),
        Value::Integer(5)
    );
    failure(
        invoke(
            &handle,
            HostArguments::new(vec![], vec![("a".into(), Value::Integer(0))]),
        )
        .unwrap_err(),
        DiagnosticKind::DivisionByZero,
    );
}

#[test]
fn unknown_duplicate_missing_and_excess_labels_are_preflight_errors() {
    let source = "funcra f(required: integer, option: integer = 2) -> integer {\n\
                  print(\"MUST NOT RUN\"); return required + option; }";
    let handle = CallableCatalog::from_program(scarp::compile(source).unwrap())
        .bind_exact(0)
        .unwrap();
    let cases = [
        HostArguments::default(),
        HostArguments::new(
            vec![Value::Integer(1)],
            vec![("required".into(), Value::Integer(2))],
        ),
        HostArguments::new(
            vec![],
            vec![
                ("option".into(), Value::Integer(3)),
                ("option".into(), Value::Integer(4)),
            ],
        ),
        HostArguments::new(
            vec![Value::Integer(1)],
            vec![("typo".into(), Value::Integer(5))],
        ),
        HostArguments::new(vec![], vec![("✨".into(), Value::Integer(5))]),
        HostArguments::new(
            vec![Value::Integer(1), Value::Integer(2)],
            vec![("option".into(), Value::Integer(3))],
        ),
    ];
    for args in cases {
        let (mut out, mut err) = (Vec::new(), Vec::new());
        failure(
            handle
                .invoke_bound_with_io_and_options(
                    args,
                    &mut out,
                    &mut err,
                    ExecutionOptions::default(),
                )
                .unwrap_err(),
            DiagnosticKind::CallArguments,
        );
        assert!(out.is_empty() && err.is_empty());
    }
}

#[test]
fn type_checking_uses_target_parameter_metadata_not_signature_names() {
    let catalog = CallableCatalog::from_program(
        scarp::compile(
            "pure funcra one(alpha: integer) -> integer { return alpha; }\n\
             pure funcra two(beta: integer) -> integer { return beta; }",
        )
        .unwrap(),
    );
    let first = catalog.bind_exact(0).unwrap();
    let second = catalog.bind_exact(1).unwrap();
    assert_eq!(first.signature(), second.signature());
    assert_eq!(
        invoke(
            &second,
            HostArguments::new(vec![], vec![("beta".into(), Value::Integer(3))])
        )
        .unwrap(),
        Value::Integer(3)
    );
    failure(
        invoke(
            &second,
            HostArguments::new(vec![], vec![("alpha".into(), Value::Integer(3))]),
        )
        .unwrap_err(),
        DiagnosticKind::CallArguments,
    );
    let span = failure(
        invoke(
            &first,
            HostArguments::new(vec![], vec![("alpha".into(), Value::Boolean(false))]),
        )
        .unwrap_err(),
        DiagnosticKind::TypeMismatch,
    );
    assert_eq!(span, first.metadata().parameters[0].span);
}

#[test]
fn named_text_checks_utf8_ceiling_without_emitting_program_output() {
    let handle = CallableCatalog::from_program(
        scarp::compile(
            "funcra echo(word: string) -> string { print(\"side effect\"); return word; }",
        )
        .unwrap(),
    )
    .bind_exact(0)
    .unwrap();
    assert_eq!(
        invoke(
            &handle,
            HostArguments::new(vec![], vec![("word".into(), Value::from("नमस्ते"))])
        )
        .unwrap(),
        Value::from("नमस्ते")
    );
    let (mut out, mut err) = (Vec::new(), Vec::new());
    failure(
        handle
            .invoke_bound_with_io_and_options(
                HostArguments::new(
                    vec![],
                    vec![(
                        "word".into(),
                        Value::from("é".repeat(MAX_TEXT_BYTES / 2 + 1)),
                    )],
                ),
                &mut out,
                &mut err,
                ExecutionOptions::default(),
            )
            .unwrap_err(),
        DiagnosticKind::TextConstruction,
    );
    assert!(out.is_empty() && err.is_empty());
}

#[test]
fn reader_named_invocation_does_not_read_input_on_preflight_error() {
    let handle = CallableCatalog::from_program(
        scarp::compile(
            "funcra read(token: integer, suffix: string = \"!\") -> string {\n\
             print(suffix); return read_line(); }",
        )
        .unwrap(),
    )
    .bind_exact(0)
    .unwrap();
    let mut input = Cursor::new(b"hello\r\n");
    let (mut out, mut err) = (Vec::new(), Vec::new());
    failure(
        handle
            .invoke_bound_with_input_and_options(
                HostArguments::new(vec![], vec![("token".into(), Value::from("wrong"))]),
                &mut input,
                &mut out,
                &mut err,
                ExecutionOptions::default(),
                InputLimits::default(),
            )
            .unwrap_err(),
        DiagnosticKind::TypeMismatch,
    );
    assert_eq!(input.position(), 0);
    assert!(out.is_empty() && err.is_empty());
    assert_eq!(
        handle
            .invoke_bound_with_input_and_options(
                HostArguments::new(
                    vec![],
                    vec![
                        ("suffix".into(), Value::from("suffix")),
                        ("token".into(), Value::Integer(1))
                    ],
                ),
                &mut input,
                &mut out,
                &mut err,
                ExecutionOptions::default(),
                InputLimits::default(),
            )
            .unwrap(),
        Value::from("hello")
    );
    assert_eq!(out, b"suffix\n");
    assert!(err.is_empty());
}

#[test]
fn imported_function_uses_linked_declaration_parameter_labels() {
    let mut loader = MemorySourceLoader::new();
    loader
        .insert(
            "main.srp",
            "import \"math.srp\" as math; print(math.calc(4));",
        )
        .unwrap();
    loader
        .insert(
            "math.srp",
            "pub pure funcra calc(value: integer, factor: integer = 2) -> integer {\n\
         return value * factor; }",
        )
        .unwrap();
    let linked =
        modules::compile_with_loader("main.srp", &loader, ModuleLimits::default()).unwrap();
    let handle = CallableCatalog::from_program(linked.program().clone())
        .bind_exact(0)
        .unwrap();
    assert_eq!(
        invoke(
            &handle,
            HostArguments::new(
                vec![],
                vec![
                    ("factor".into(), Value::Integer(3)),
                    ("value".into(), Value::Integer(4))
                ],
            ),
        )
        .unwrap(),
        Value::Integer(12)
    );
}

#[test]
fn named_host_calls_charge_existing_call_and_output_limits() {
    let handle = CallableCatalog::from_program(
        scarp::compile(
            "funcra test(value: integer, next: integer = 0) -> integer {\n\
             print(value); return value + next; }",
        )
        .unwrap(),
    )
    .bind_exact(0)
    .unwrap();
    let args = HostArguments::new(vec![], vec![("value".into(), Value::Integer(4))]);
    for options in [
        ExecutionOptions {
            max_calls: 0,
            ..ExecutionOptions::default()
        },
        ExecutionOptions {
            max_call_depth: 0,
            ..ExecutionOptions::default()
        },
        ExecutionOptions {
            max_live_slots: 0,
            ..ExecutionOptions::default()
        },
    ] {
        failure(
            handle
                .invoke_bound_with_io_and_options(
                    args.clone(),
                    &mut Vec::new(),
                    &mut Vec::new(),
                    options,
                )
                .unwrap_err(),
            DiagnosticKind::CallLimit,
        );
    }
    let (mut out, mut err) = (Vec::new(), Vec::new());
    failure(
        handle
            .invoke_bound_with_io_and_options(
                args,
                &mut out,
                &mut err,
                ExecutionOptions {
                    max_capture_bytes: 0,
                    ..ExecutionOptions::default()
                },
            )
            .unwrap_err(),
        DiagnosticKind::CaptureLimit,
    );
    assert!(out.is_empty() && err.is_empty());
}
