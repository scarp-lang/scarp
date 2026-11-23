use std::io::{self, Cursor, Write};

use scarp::{
    diagnostics::DiagnosticKind,
    limits::ExecutionUsage,
    modules::{self, MemorySourceLoader, ModuleCallableCatalog, ModuleLimits, ModuleRunError, ExportedSlotInvokeError},
    resolver::{CallableCatalog, CallableSlotInvokeError, HostArguments},
    value::Value,
    ExecutionOptions, InputLimits, RunError,
};

fn simple(source: &str, index: usize) -> scarp::resolver::CallableSlot {
    let program = scarp::compile(source).unwrap();
    let signature = program.function_type_id(index).unwrap();
    CallableCatalog::from_program(program)
        .slot_bound(signature, index)
        .unwrap()
}

#[test]
fn exact_success_receipt_counts_initial_host_call_and_real_work() {
    let slot = simple(
        "pure funcra twice(n: integer) -> integer { return n * 2; }\n\
         print(twice(5));",
        0,
    );
    let mut usage = ExecutionUsage { steps: 999, calls: 999, ..ExecutionUsage::default() };
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let result = slot.invoke_with_usage_and_options(
        HostArguments::positional(vec![Value::Integer(3)]),
        &mut out, &mut err,
        ExecutionOptions::default(), &mut usage,
    ).unwrap();
    assert_eq!(result, Value::Integer(6));
    assert!(out.is_empty() && err.is_empty());
    assert!(usage.steps > 0);
    assert_eq!(usage.calls, 1);
    assert_eq!(usage.prints, 0);
    assert_eq!(usage.capture_bytes, 0);
    assert_eq!(usage.input_reads, 0);
    assert_eq!(usage.input_bytes, 0);
}

#[test]
fn nested_calls_and_default_arguments_are_metered_not_just_entry_work() {
    let defaulted = simple(
        "pure funcra leaf(n: integer) -> integer { return n + 1; }\n\
         pure funcra chain(n: integer, offset: integer = leaf(n)) -> integer {\n\
             return leaf(offset);\n\
         }",
        1,
    );
    let mut usage = ExecutionUsage::default();
    assert_eq!(
        defaulted.invoke_with_usage_and_options(
            HostArguments::positional(vec![Value::Integer(3)]),
            &mut Vec::new(), &mut Vec::new(),
            ExecutionOptions::default(), &mut usage,
        ).unwrap(),
        Value::Integer(5)
    );
    assert_eq!(usage.calls, 3);
    assert!(usage.steps >= usage.calls);
    let mut supplied = ExecutionUsage::default();
    assert_eq!(
        defaulted.invoke_with_usage_and_options(
            HostArguments::new(
                vec![Value::Integer(3)],
                vec![("offset".into(), Value::Integer(9))],
            ),
            &mut Vec::new(), &mut Vec::new(),
            ExecutionOptions::default(), &mut supplied,
        ).unwrap(),
        Value::Integer(10)
    );
    assert_eq!(supplied.calls, 2);
    assert!(supplied.steps < usage.steps);
}

#[test]
fn failed_contract_retains_spent_call_output_and_step_charges() {
    let slot = simple(
        "funcra unsafe(n: integer) -> integer ensures result > 0; {\n\
             print(\"attempt\"); return n;\n\
         }",
        0,
    );
    let mut usage = ExecutionUsage::default();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let result = slot.invoke_with_usage_and_options(
        HostArguments::positional(vec![Value::Integer(0)]),
        &mut out, &mut err,
        ExecutionOptions::default(), &mut usage,
    );
    assert!(matches!(
        result,
        Err(CallableSlotInvokeError::Runtime(RunError::Diagnostic(diagnostic)))
            if diagnostic.kind == DiagnosticKind::ContractViolation
    ));
    assert_eq!(usage.calls, 1);
    assert_eq!(usage.prints, 1);
    assert!(usage.steps > 0);
    assert!(usage.capture_bytes >= 64);
    assert!(out.is_empty() && err.is_empty());
}

#[test]
fn invalid_arguments_and_unbound_slot_reset_stale_usage_without_io() {
    let program = scarp::compile(
        "pure funcra validate(n: integer) -> integer { return n; }",
    ).unwrap();
    let signature = program.function_type_id(0).unwrap();
    let catalog = CallableCatalog::from_program(program);
    let mut slot = catalog.slot_bound(signature, 0).unwrap();
    let mut usage = ExecutionUsage { steps: 900, calls: 900, input_reads: 900, ..ExecutionUsage::default() };
    let error = slot.invoke_with_usage_and_options(
        HostArguments::positional(vec![Value::from("invalid")]),
        &mut Vec::new(), &mut Vec::new(), ExecutionOptions::default(),
        &mut usage,
    );
    assert!(matches!(
        error,
        Err(CallableSlotInvokeError::Runtime(RunError::Diagnostic(diag)))
            if diag.kind == DiagnosticKind::TypeMismatch
    ));
    assert_eq!(usage, ExecutionUsage::default());
    slot.clear();
    usage.calls = 500;
    assert!(matches!(
        slot.invoke_with_usage_and_options(
            HostArguments::default(), &mut Vec::new(), &mut Vec::new(),
            ExecutionOptions::default(), &mut usage,
        ),
        Err(CallableSlotInvokeError::Unbound)
    ));
    assert_eq!(usage, ExecutionUsage::default());
}

#[test]
fn reader_receipt_counts_crlf_bytes_and_input_text_charge() {
    let slot = simple(
        "funcra read(n: integer) -> string { return read_line(); }",
        0,
    );
    let mut input = Cursor::new(b"hello\r\n");
    let mut usage = ExecutionUsage::default();
    assert_eq!(
        slot.invoke_with_input_and_usage_and_options(
            HostArguments::positional(vec![Value::Integer(1)]),
            &mut input,
            (&mut Vec::new(), &mut Vec::new()),
            ExecutionOptions::default(),
            InputLimits::default(),
            &mut usage,
        ).unwrap(),
        Value::from("hello")
    );
    assert_eq!(usage.input_reads, 1);
    assert_eq!(usage.input_bytes, 7);
    assert!(usage.text_bytes >= 7);
    assert_eq!(usage.calls, 1);
}

#[test]
fn eof_attempts_and_input_budget_errors_are_reflected_in_usage() {
    let slot = simple("funcra read() -> string { return read_line(); }", 0);
    let mut input = Cursor::new(Vec::<u8>::new());
    let mut usage = ExecutionUsage::default();
    assert!(matches!(
        slot.invoke_with_input_and_usage_and_options(
            HostArguments::default(),
            &mut input,
            (&mut Vec::new(), &mut Vec::new()),
            ExecutionOptions::default(),
            InputLimits::default(),
            &mut usage,
        ),
        Err(CallableSlotInvokeError::Runtime(RunError::Diagnostic(diag)))
            if diag.kind == DiagnosticKind::InputEof
    ));
    assert_eq!(usage.input_reads, 1);
    assert_eq!(usage.input_bytes, 0);
    assert!(usage.steps > 0);
    let mut blocked = ExecutionUsage { input_reads: 20, ..ExecutionUsage::default() };
    assert!(matches!(
        slot.invoke_with_input_and_usage_and_options(
            HostArguments::default(),
            &mut Cursor::new(b"x\n"),
            (&mut Vec::new(), &mut Vec::new()),
            ExecutionOptions::default(),
            InputLimits { max_reads: 0, ..InputLimits::default() },
            &mut blocked,
        ),
        Err(CallableSlotInvokeError::Runtime(RunError::Diagnostic(diag)))
            if diag.kind == DiagnosticKind::InputLimit
    ));
    assert_eq!(blocked.input_reads, 0);
    assert_eq!(blocked.input_bytes, 0);
    assert!(blocked.steps > 0);
}

#[test]
fn per_call_budget_exhaustion_never_claims_unspent_steps() {
    let slot = simple("pure funcra f(n: integer) -> integer { return n + 1; }", 0);
    let mut usage = ExecutionUsage::default();
    assert!(matches!(
        slot.invoke_with_usage_and_options(
            HostArguments::positional(vec![Value::Integer(1)]),
            &mut Vec::new(), &mut Vec::new(),
            ExecutionOptions {
                limits: scarp::ExecutionLimits { max_steps: 1, max_prints: 100 },
                ..ExecutionOptions::default()
            },
            &mut usage,
        ),
        Err(CallableSlotInvokeError::Runtime(RunError::Diagnostic(diag)))
            if diag.kind == DiagnosticKind::ExecutionLimit
    ));
    assert!(usage.steps <= 1);
    assert_eq!(usage.calls, 1);
}

struct BrokenWriter;
impl Write for BrokenWriter {
    fn write(&mut self, _buf: &[u8]) -> io::Result<usize> {
        Err(io::Error::other("unavailable"))
    }
    fn flush(&mut self) -> io::Result<()> { Ok(()) }
}

#[test]
fn writer_failure_does_not_erase_completed_interpreter_receipt() {
    let slot = simple(
        "funcra emit(n: integer) -> integer { print(n); return n; }",
        0,
    );
    let mut usage = ExecutionUsage::default();
    let result = slot.invoke_with_usage_and_options(
        HostArguments::positional(vec![Value::Integer(4)]),
        &mut BrokenWriter, &mut Vec::new(),
        ExecutionOptions::default(),
        &mut usage,
    );
    assert!(matches!(
        result, Err(CallableSlotInvokeError::Runtime(RunError::Output(_)))
    ));
    assert_eq!(usage.calls, 1);
    assert_eq!(usage.prints, 1);
    assert!(usage.capture_bytes >= 64);
    assert!(usage.steps > 0);
}

#[test]
fn export_aware_slot_receipt_preserves_original_source_diagnostic() {
    let mut loader = MemorySourceLoader::new();
    loader.insert("main.srp", "import \"ops.srp\" as ops; print(ops.f(1));").unwrap();
    loader.insert(
        "ops.srp",
        "pub funcra f(n: integer) -> integer { print(\"side\"); return 10 / n; }",
    ).unwrap();
    let module = modules::compile_with_loader("main.srp", &loader, ModuleLimits::default()).unwrap();
    let source = module.imports_in(0).unwrap()[0].target;
    let signature = module.program().function_type_id(0).unwrap();
    let slot = ModuleCallableCatalog::from_program(module)
        .slot_exported(source, "f", signature)
        .unwrap();
    let mut usage = ExecutionUsage::default();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    match slot.invoke_with_usage_and_options(
        HostArguments::positional(vec![Value::Integer(0)]),
        &mut out, &mut err,
        ExecutionOptions::default(), &mut usage,
    ).unwrap_err() {
        ExportedSlotInvokeError::Runtime(ModuleRunError::Diagnostic(diag)) => {
            assert_eq!(diag.diagnostic.kind, DiagnosticKind::DivisionByZero);
            assert!(diag.render().contains("ops.srp"));
        }
        other => panic!("expected source-mapped diagnostic, got {other}"),
    }
    assert_eq!(usage.calls, 1);
    assert_eq!(usage.prints, 1);
    assert!(usage.steps > 0);
    assert!(out.is_empty() && err.is_empty());
}
