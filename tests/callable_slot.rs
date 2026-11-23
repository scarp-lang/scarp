use std::{io::Cursor, sync::Arc};

use scarp::{
    diagnostics::DiagnosticKind,
    resolver::{
        CallableBindError, CallableCatalog, CallableSlotError, CallableSlotInvokeError,
        HostArguments,
    },
    types::{PrimitiveType, TypeRegistryError},
    value::Value,
    ExecutionOptions, InputLimits, RunError,
};

fn run(
    slot: &scarp::resolver::CallableSlot,
    arguments: HostArguments,
) -> Result<Value, CallableSlotInvokeError> {
    slot.invoke_with_io_and_options(
        arguments,
        &mut Vec::new(),
        &mut Vec::new(),
        ExecutionOptions::default(),
    )
}

#[test]
fn host_can_switch_same_signature_targets_without_source_level_function_values() {
    let program = scarp::compile(
        "pure funcra increment(n: integer) -> integer { return n + 1; }\n\
         pure funcra triple(n: integer) -> integer { return n * 3; }\n\
         print(increment(4));",
    ).unwrap();
    let required = program.function_type_id(0).unwrap();
    let catalog = CallableCatalog::from_program(program);
    let mut slot = catalog.slot_bound(required, 0).unwrap();
    assert_eq!(slot.required_signature(), required);
    assert!(slot.is_bound());
    assert_eq!(
        run(&slot, HostArguments::positional(vec![Value::Integer(3)])).unwrap(),
        Value::Integer(4),
    );
    let old = slot.current().unwrap().clone();
    slot.select(1).unwrap();
    assert!(!slot.current().unwrap().same_target(&old));
    assert_eq!(
        run(&slot, HostArguments::positional(vec![Value::Integer(3)])).unwrap(),
        Value::Integer(9),
    );

    let saved = slot.clone();
    slot.clear();
    assert!(!slot.is_bound());
    assert!(matches!(
        run(&slot, HostArguments::default()),
        Err(CallableSlotInvokeError::Unbound)
    ));
    assert!(saved.is_bound());
    assert_eq!(
        run(&saved, HostArguments::positional(vec![Value::Integer(3)])).unwrap(),
        Value::Integer(9),
    );
}

#[test]
fn selected_target_controls_parameter_names_even_with_identical_signatures() {
    let program = scarp::compile(
        "pure funcra start(value: integer) -> integer { return value; }\n\
         pure funcra finish(answer: integer) -> integer { return answer + 10; }",
    ).unwrap();
    let required = program.function_type_id(0).unwrap();
    let catalog = CallableCatalog::from_program(program);
    let mut slot = catalog.slot_bound(required, 0).unwrap();
    assert_eq!(
        run(
            &slot,
            HostArguments::new(vec![], vec![("value".into(), Value::Integer(2))]),
        ).unwrap(),
        Value::Integer(2),
    );
    slot.select(1).unwrap();
    assert_eq!(
        run(
            &slot,
            HostArguments::new(vec![], vec![("answer".into(), Value::Integer(2))]),
        ).unwrap(),
        Value::Integer(12),
    );
    assert!(matches!(
        run(
            &slot,
            HostArguments::new(vec![], vec![("value".into(), Value::Integer(2))]),
        ),
        Err(CallableSlotInvokeError::Runtime(RunError::Diagnostic(diagnostic)))
            if diagnostic.kind == DiagnosticKind::CallArguments
    ));
}

#[test]
fn pure_can_fill_impure_but_impure_never_fills_pure_slot() {
    let program = scarp::compile(
        "pure funcra quiet(n: integer) -> integer { return n; }\n\
         funcra loud(n: integer) -> integer { print(\"effect\"); return n; }\n\
         pure funcra text(s: string) -> string { return s; }",
    ).unwrap();
    let pure_signature = program.function_type_id(0).unwrap();
    let impure_signature = program.function_type_id(1).unwrap();
    let catalog = CallableCatalog::from_program(program);
    let mut pure_slot = catalog.slot_bound(pure_signature, 0).unwrap();
    let loud = catalog.bind_exact(1).unwrap();
    assert_eq!(
        pure_slot.set(&loud).unwrap_err(),
        CallableSlotError::IncompatibleSignature {
            required: pure_signature,
            actual: impure_signature,
        },
    );
    assert_eq!(pure_slot.current().unwrap().declaration_index(), 0);
    assert_eq!(
        pure_slot.select(1).unwrap_err(),
        CallableBindError::IncompatibleSignature {
            expected: pure_signature,
            actual: impure_signature,
        },
    );
    assert_eq!(pure_slot.current().unwrap().declaration_index(), 0);
    let mut impure_slot = catalog.slot(impure_signature).unwrap();
    impure_slot.set(pure_slot.current().unwrap()).unwrap();
    assert_eq!(run(&impure_slot, HostArguments::positional(vec![Value::Integer(7)])).unwrap(), Value::Integer(7));
    impure_slot.set(&loud).unwrap();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    assert_eq!(
        impure_slot
            .invoke_with_io_and_options(
                HostArguments::positional(vec![Value::Integer(7)]),
                &mut out,
                &mut err,
                ExecutionOptions::default(),
            )
            .unwrap(),
        Value::Integer(7)
    );
    assert_eq!(out, b"effect\n");
    assert!(err.is_empty());
    let text = catalog.bind_exact(2).unwrap();
    assert!(matches!(
        impure_slot.set(&text),
        Err(CallableSlotError::IncompatibleSignature { .. })
    ));
    assert_eq!(impure_slot.current().unwrap().declaration_index(), 1);
}

#[test]
fn foreign_compiled_snapshots_cannot_rebind_even_when_source_is_identical() {
    let code = "pure funcra id(n: integer) -> integer { return n; }";
    let source = Arc::new(scarp::compile(code).unwrap());
    let required = source.function_type_id(0).unwrap();
    let catalog = CallableCatalog::new(Arc::clone(&source));
    let mut slot = catalog.slot_bound(required, 0).unwrap();
    let equivalent = CallableCatalog::from_program(scarp::compile(code).unwrap());
    let foreign = equivalent.bind_exact(0).unwrap();
    assert_eq!(slot.set(&foreign), Err(CallableSlotError::ForeignProgram));
    assert!(slot.current().unwrap().belongs_to(&catalog));
    assert_eq!(
        catalog.slot(foreign.signature()).unwrap_err(),
        CallableSlotError::Registry(TypeRegistryError::UnknownType(foreign.signature()))
    );
    assert_eq!(
        catalog.slot(PrimitiveType::Integer.id()).unwrap_err(),
        CallableSlotError::NotCallable(PrimitiveType::Integer.id())
    );
    drop(source);
    assert_eq!(
        run(&slot, HostArguments::positional(vec![Value::Integer(11)])).unwrap(),
        Value::Integer(11)
    );
}

#[test]
fn constructor_rejects_missing_declarations_and_keeps_explicit_type_errors() {
    let program = scarp::compile("pure funcra id(n: integer) -> integer { return n; }").unwrap();
    let signature = program.function_type_id(0).unwrap();
    let catalog = CallableCatalog::from_program(program);
    assert_eq!(
        catalog.slot_bound(signature, 999).unwrap_err(),
        CallableSlotError::UnknownFunction(999)
    );
    let mut slot = catalog.slot_bound(signature, 0).unwrap();
    assert_eq!(
        slot.select(usize::MAX).unwrap_err(),
        CallableBindError::UnknownFunction(usize::MAX)
    );
    assert_eq!(slot.current().unwrap().declaration_index(), 0);
}

#[test]
fn unbound_slot_never_executes_and_never_consumes_host_input() {
    let program = scarp::compile(
        "funcra read(n: integer) -> string { print(n); return read_line(); }",
    ).unwrap();
    let required = program.function_type_id(0).unwrap();
    let catalog = CallableCatalog::from_program(program);
    let slot = catalog.slot(required).unwrap();
    let mut input = Cursor::new(b"hello\r\n");
    let (mut out, mut err) = (Vec::new(), Vec::new());
    assert!(matches!(
        slot.invoke_with_input_and_options(
            HostArguments::positional(vec![Value::Integer(7)]),
            &mut input,
            &mut out,
            &mut err,
            ExecutionOptions::default(),
            InputLimits::default(),
        ),
        Err(CallableSlotInvokeError::Unbound)
    ));
    assert_eq!(input.position(), 0);
    assert!(out.is_empty() && err.is_empty());
}

#[test]
fn selected_host_target_preserves_input_defaults_and_output_transactions() {
    let program = scarp::compile(
        "funcra read(n: integer, suffix: string = \"!\") -> string {\n\
             print(suffix); return read_line();\n\
         }",
    ).unwrap();
    let signature = program.function_type_id(0).unwrap();
    let catalog = CallableCatalog::from_program(program);
    let slot = catalog.slot_bound(signature, 0).unwrap();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let mut input = Cursor::new(b"नमस्ते\r\n");
    assert_eq!(
        slot.invoke_with_input_and_options(
            HostArguments::new(vec![], vec![("n".into(), Value::Integer(1))]),
            &mut input,
            &mut out,
            &mut err,
            ExecutionOptions::default(),
            InputLimits::default(),
        ).unwrap(),
        Value::from("नमस्ते")
    );
    assert_eq!(out, b"!\n");
    assert!(err.is_empty());
}

#[test]
fn contract_failures_and_quotas_remain_runtime_errors_without_output_leak() {
    let program = scarp::compile(
        "funcra checked(n: integer) -> integer\n\
             ensures result > 0;\n\
         { print(\"pending\"); return n; }",
    ).unwrap();
    let signature = program.function_type_id(0).unwrap();
    let catalog = CallableCatalog::from_program(program);
    let slot = catalog.slot_bound(signature, 0).unwrap();
    for (n, options, kind) in [
        (0, ExecutionOptions::default(), DiagnosticKind::ContractViolation),
        (1, ExecutionOptions { max_calls: 0, ..ExecutionOptions::default() }, DiagnosticKind::CallLimit),
        (1, ExecutionOptions { max_capture_bytes: 0, ..ExecutionOptions::default() }, DiagnosticKind::CaptureLimit),
    ] {
        let (mut out, mut err) = (Vec::new(), Vec::new());
        assert!(matches!(
            slot.invoke_with_io_and_options(
                HostArguments::positional(vec![Value::Integer(n)]),
                &mut out,
                &mut err,
                options,
            ),
            Err(CallableSlotInvokeError::Runtime(RunError::Diagnostic(diagnostic)))
                if diagnostic.kind == kind
        ));
        assert!(out.is_empty() && err.is_empty());
        assert_eq!(slot.current().unwrap().declaration_index(), 0);
    }
}

#[test]
fn slot_and_clone_retain_one_program_until_final_owner_drops() {
    let program = Arc::new(scarp::compile(
        "pure funcra id(n: integer) -> integer { return n; }",
    ).unwrap());
    let weak = Arc::downgrade(&program);
    let required = program.function_type_id(0).unwrap();
    let catalog = CallableCatalog::new(Arc::clone(&program));
    let mut slot = catalog.slot_bound(required, 0).unwrap();
    let copy = slot.clone();
    drop(program);
    drop(catalog);
    assert!(weak.upgrade().is_some());
    slot.clear();
    assert!(weak.upgrade().is_some());
    drop(slot);
    assert!(weak.upgrade().is_some());
    assert_eq!(
        run(&copy, HostArguments::positional(vec![Value::Integer(8)])).unwrap(),
        Value::Integer(8)
    );
    drop(copy);
    assert!(weak.upgrade().is_none());
}
