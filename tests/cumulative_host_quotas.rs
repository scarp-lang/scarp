use std::io::{self, Cursor, Write};

use scarp::{
    diagnostics::DiagnosticKind,
    embedding::{HostInvocationCaps, HostInvocationError, HostInvocationLedger, HostQuotaResource},
    limits::ExecutionUsage,
    modules::{
        self, ExportedSlotInvokeError, MemorySourceLoader, ModuleCallableCatalog, ModuleLimits,
        ModuleRunError,
    },
    resolver::{CallableCatalog, CallableSlot, CallableSlotInvokeError, HostArguments},
    value::Value,
    ExecutionOptions, InputLimits, RunError,
};

fn slot(source: &str) -> CallableSlot {
    let program = scarp::compile(source).unwrap();
    let required = program.function_type_id(0).unwrap();
    CallableCatalog::from_program(program)
        .slot_bound(required, 0)
        .unwrap()
}

fn one(n: i64) -> HostArguments {
    HostArguments::positional(vec![Value::Integer(n)])
}

fn invoke(
    ledger: &mut HostInvocationLedger,
    target: &CallableSlot,
    n: i64,
) -> Result<Value, HostInvocationError<CallableSlotInvokeError>> {
    ledger.invoke_slot(
        target,
        one(n),
        (&mut Vec::new(), &mut Vec::new()),
        ExecutionOptions::default(),
    )
}

#[test]
fn attempts_are_bounded_across_successful_calls() {
    let target = slot("pure funcra id(n: integer) -> integer { return n; }");
    let caps = HostInvocationCaps {
        max_attempts: 2,
        ..HostInvocationCaps::default()
    };
    let mut ledger = HostInvocationLedger::new(caps);
    assert_eq!(ledger.caps(), caps);
    assert_eq!(invoke(&mut ledger, &target, 3).unwrap(), Value::Integer(3));
    assert_eq!(invoke(&mut ledger, &target, 4).unwrap(), Value::Integer(4));
    let before = ledger.spent();
    assert_eq!(ledger.attempts(), 2);
    assert_eq!(before.calls, 2);
    assert!(before.steps > 0);
    assert!(matches!(
        invoke(&mut ledger, &target, 5),
        Err(HostInvocationError::Quota(ref err)) if err.resource == HostQuotaResource::Attempts
    ));
    assert_eq!(ledger.attempts(), 2);
    assert_eq!(ledger.spent(), before);
}

#[test]
fn exact_spent_work_exhausts_shared_steps_even_after_valid_result() {
    let target = slot("pure funcra calc(n: integer) -> integer { return n * 3; }");
    let mut measured = ExecutionUsage::default();
    assert_eq!(
        target
            .invoke_with_usage_and_options(
                one(2),
                &mut Vec::new(),
                &mut Vec::new(),
                ExecutionOptions::default(),
                &mut measured,
            )
            .unwrap(),
        Value::Integer(6)
    );
    assert!(measured.steps > 0);
    let mut caps = HostInvocationCaps::default();
    caps.max_attempts = 5;
    caps.execution.limits.max_steps = measured.steps;
    let mut ledger = HostInvocationLedger::new(caps);
    assert_eq!(invoke(&mut ledger, &target, 2).unwrap(), Value::Integer(6));
    assert_eq!(ledger.spent().steps, measured.steps);
    assert_eq!(ledger.remaining_execution().limits.max_steps, 0);
    assert!(matches!(
        invoke(&mut ledger, &target, 3),
        Err(HostInvocationError::Quota(ref err)) if err.resource == HostQuotaResource::Steps
    ));
    assert_eq!(ledger.attempts(), 1);
}

#[test]
fn failed_contract_is_billed_and_cannot_reset_the_total_work_quota() {
    let target = slot(
        "funcra fail(n: integer) -> integer ensures result > 0; {\n\
             print(\"pending\"); return n; }",
    );
    let mut measured = ExecutionUsage::default();
    assert!(matches!(
        target.invoke_with_usage_and_options(
            one(0), &mut Vec::new(), &mut Vec::new(),
            ExecutionOptions::default(), &mut measured,
        ),
        Err(CallableSlotInvokeError::Runtime(RunError::Diagnostic(diag)))
            if diag.kind == DiagnosticKind::ContractViolation
    ));
    assert!(measured.steps > 0);
    assert_eq!(measured.calls, 1);
    assert_eq!(measured.prints, 1);
    let mut caps = HostInvocationCaps::default();
    caps.execution.limits.max_steps = measured.steps;
    let mut ledger = HostInvocationLedger::new(caps);
    let (mut out, mut err) = (Vec::new(), Vec::new());
    assert!(matches!(
        ledger.invoke_slot(
            &target, one(0), (&mut out, &mut err), ExecutionOptions::default(),
        ),
        Err(HostInvocationError::Invocation(CallableSlotInvokeError::Runtime(
            RunError::Diagnostic(diag)
        ))) if diag.kind == DiagnosticKind::ContractViolation
    ));
    assert_eq!(ledger.spent().steps, measured.steps);
    assert_eq!(ledger.spent().prints, 1);
    assert_eq!(ledger.attempts(), 1);
    assert!(out.is_empty() && err.is_empty());
    assert!(matches!(
        invoke(&mut ledger, &target, 3),
        Err(HostInvocationError::Quota(ref err)) if err.resource == HostQuotaResource::Steps
    ));
}

#[test]
fn total_call_limit_cannot_be_reset_by_repeated_host_invocations() {
    let target = slot("pure funcra id(n: integer) -> integer { return n; }");
    let mut caps = HostInvocationCaps::default();
    caps.execution.max_calls = 1;
    let mut ledger = HostInvocationLedger::new(caps);
    assert_eq!(invoke(&mut ledger, &target, 1).unwrap(), Value::Integer(1));
    assert_eq!(ledger.spent().calls, 1);
    assert_eq!(ledger.remaining_execution().max_calls, 0);
    assert!(matches!(
        invoke(&mut ledger, &target, 2),
        Err(HostInvocationError::Quota(ref err)) if err.resource == HostQuotaResource::Calls
    ));
}

#[test]
fn second_print_is_rejected_without_publishing_failed_output_but_bills_work() {
    let target = slot("funcra say(n: integer) -> integer { print(n); return n; }");
    let mut caps = HostInvocationCaps::default();
    caps.execution.limits.max_prints = 1;
    let mut ledger = HostInvocationLedger::new(caps);
    let (mut out, mut err) = (Vec::new(), Vec::new());
    assert_eq!(
        ledger
            .invoke_slot(
                &target,
                one(1),
                (&mut out, &mut err),
                ExecutionOptions::default()
            )
            .unwrap(),
        Value::Integer(1)
    );
    assert_eq!(out, b"1\n");
    let spent = ledger.spent();
    assert_eq!(spent.prints, 1);
    assert!(matches!(
        ledger.invoke_slot(&target, one(2), (&mut out, &mut err), ExecutionOptions::default()),
        Err(HostInvocationError::Invocation(CallableSlotInvokeError::Runtime(
            RunError::Diagnostic(diag)
        ))) if diag.kind == DiagnosticKind::OutputLimit
    ));
    assert_eq!(out, b"1\n");
    assert!(err.is_empty());
    assert_eq!(ledger.spent().prints, 1);
    assert!(ledger.spent().steps > spent.steps);
    assert_eq!(ledger.spent().calls, 2);
}

#[test]
fn input_quota_tracks_actual_crlf_bytes_without_consuming_the_next_line() {
    let target = slot("funcra read(n: integer) -> string { return read_line(); }");
    let mut caps = HostInvocationCaps::default();
    caps.input.max_reads = 1;
    caps.input.max_total_bytes = 6;
    let mut ledger = HostInvocationLedger::new(caps);
    let mut input = Cursor::new(b"first\nsecond\n");
    assert_eq!(
        ledger
            .invoke_slot_with_input(
                &target,
                one(1),
                &mut input,
                (&mut Vec::new(), &mut Vec::new()),
                ExecutionOptions::default(),
                InputLimits::default(),
            )
            .unwrap(),
        Value::from("first")
    );
    assert_eq!(ledger.spent().input_reads, 1);
    assert_eq!(ledger.spent().input_bytes, 6);
    assert_eq!(ledger.remaining_input().max_reads, 0);
    assert_eq!(ledger.remaining_input().max_total_bytes, 0);
    let position = input.position();
    assert!(matches!(
        ledger.invoke_slot_with_input(
            &target, one(2), &mut input,
            (&mut Vec::new(), &mut Vec::new()),
            ExecutionOptions::default(), InputLimits::default(),
        ),
        Err(HostInvocationError::Invocation(CallableSlotInvokeError::Runtime(
            RunError::Diagnostic(diag)
        ))) if diag.kind == DiagnosticKind::InputLimit
    ));
    assert_eq!(input.position(), position);
    assert_eq!(ledger.spent().input_bytes, 6);
    assert_eq!(ledger.spent().input_reads, 1);
    assert_eq!(ledger.attempts(), 2);
}

#[test]
fn invalid_and_unbound_attempts_consume_attempts_not_interpreter_budget() {
    let program = scarp::compile("pure funcra id(n: integer) -> integer { return n; }").unwrap();
    let required = program.function_type_id(0).unwrap();
    let catalog = CallableCatalog::from_program(program);
    let mut unbound = catalog.slot(required).unwrap();
    let caps = HostInvocationCaps {
        max_attempts: 2,
        ..HostInvocationCaps::default()
    };
    let mut ledger = HostInvocationLedger::new(caps);
    assert!(matches!(
        ledger.invoke_slot(
            &unbound,
            one(1),
            (&mut Vec::new(), &mut Vec::new()),
            ExecutionOptions::default(),
        ),
        Err(HostInvocationError::Invocation(
            CallableSlotInvokeError::Unbound
        ))
    ));
    assert_eq!(ledger.spent(), ExecutionUsage::default());
    unbound.select(0).unwrap();
    assert!(matches!(
        ledger.invoke_slot(
            &unbound,
            HostArguments::positional(vec![Value::from("wrong")]),
            (&mut Vec::new(), &mut Vec::new()),
            ExecutionOptions::default(),
        ),
        Err(HostInvocationError::Invocation(CallableSlotInvokeError::Runtime(
            RunError::Diagnostic(diag)
        ))) if diag.kind == DiagnosticKind::TypeMismatch
    ));
    assert_eq!(ledger.attempts(), 2);
    assert_eq!(ledger.spent(), ExecutionUsage::default());
    assert!(matches!(
        invoke(&mut ledger, &unbound, 1),
        Err(HostInvocationError::Quota(ref err)) if err.resource == HostQuotaResource::Attempts
    ));
}

struct BrokenWriter;
impl Write for BrokenWriter {
    fn write(&mut self, _: &[u8]) -> io::Result<usize> {
        Err(io::Error::other("host output broken"))
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[test]
fn failed_writer_replay_still_debits_selected_scarrp_print_and_steps() {
    let target = slot("funcra emit(n: integer) -> integer { print(n); return n; }");
    let mut ledger = HostInvocationLedger::new(HostInvocationCaps::default());
    let result = ledger.invoke_slot(
        &target,
        one(7),
        (&mut BrokenWriter, &mut Vec::new()),
        ExecutionOptions::default(),
    );
    assert!(matches!(
        result,
        Err(HostInvocationError::Invocation(
            CallableSlotInvokeError::Runtime(RunError::Output(_))
        ))
    ));
    assert_eq!(ledger.spent().calls, 1);
    assert_eq!(ledger.spent().prints, 1);
    assert!(ledger.spent().steps > 0);
}

#[test]
fn exported_module_invocation_preserves_source_diagnostics_and_shared_calls() {
    let mut loader = MemorySourceLoader::new();
    loader
        .insert("main.srp", "import \"ops.srp\" as ops; print(ops.f(3));")
        .unwrap();
    loader
        .insert(
            "ops.srp",
            "pub funcra f(n: integer) -> integer { print(\"pending\"); return 12 / n; }",
        )
        .unwrap();
    let module =
        modules::compile_with_loader("main.srp", &loader, ModuleLimits::default()).unwrap();
    let source = module.imports_in(0).unwrap()[0].target;
    let required = module.program().function_type_id(0).unwrap();
    let exported = ModuleCallableCatalog::from_program(module)
        .slot_exported(source, "f", required)
        .unwrap();
    let mut ledger = HostInvocationLedger::new(HostInvocationCaps::default());
    let (mut out, mut err) = (Vec::new(), Vec::new());
    match ledger.invoke_exported(
        &exported,
        one(0),
        (&mut out, &mut err),
        ExecutionOptions::default(),
    ) {
        Err(HostInvocationError::Invocation(ExportedSlotInvokeError::Runtime(
            ModuleRunError::Diagnostic(diag),
        ))) => {
            assert_eq!(diag.diagnostic.kind, DiagnosticKind::DivisionByZero);
            assert!(diag.render().contains("ops.srp"));
        }
        other => panic!("expected source-mapped runtime diagnostic, got {other:?}"),
    }
    assert!(out.is_empty() && err.is_empty());
    assert_eq!(ledger.spent().calls, 1);
    assert_eq!(ledger.spent().prints, 1);
    assert_eq!(ledger.attempts(), 1);
    assert_eq!(
        ledger
            .invoke_exported(
                &exported,
                one(3),
                (&mut out, &mut err),
                ExecutionOptions::default(),
            )
            .unwrap(),
        Value::Integer(4)
    );
    assert_eq!(ledger.spent().calls, 2);
    assert_eq!(out, b"pending\n");
}

#[test]
fn exported_reader_uses_the_same_shared_input_and_attempt_allowances() {
    let mut loader = MemorySourceLoader::new();
    loader
        .insert("main.srp", "import \"ops.srp\" as ops; print(ops.read(1));")
        .unwrap();
    loader
        .insert(
            "ops.srp",
            "pub funcra read(n: integer) -> string { return read_line(); }",
        )
        .unwrap();
    let linked =
        modules::compile_with_loader("main.srp", &loader, ModuleLimits::default()).unwrap();
    let source = linked.imports_in(0).unwrap()[0].target;
    let signature = linked.program().function_type_id(0).unwrap();
    let slot = ModuleCallableCatalog::from_program(linked)
        .slot_exported(source, "read", signature)
        .unwrap();
    let mut caps = HostInvocationCaps::default();
    caps.input.max_reads = 1;
    let mut ledger = HostInvocationLedger::new(caps);
    let mut input = Cursor::new(b"ok\nlater\n");
    assert_eq!(
        ledger
            .invoke_exported_with_input(
                &slot,
                one(1),
                &mut input,
                (&mut Vec::new(), &mut Vec::new()),
                ExecutionOptions::default(),
                InputLimits::default(),
            )
            .unwrap(),
        Value::from("ok")
    );
    assert!(matches!(
        ledger.invoke_exported_with_input(
            &slot, one(1), &mut input,
            (&mut Vec::new(), &mut Vec::new()),
            ExecutionOptions::default(),
            InputLimits::default(),
        ),
        Err(HostInvocationError::Invocation(ExportedSlotInvokeError::Runtime(
            ModuleRunError::Diagnostic(diag)
        ))) if diag.diagnostic.kind == DiagnosticKind::InputLimit
    ));
    assert_eq!(ledger.spent().input_reads, 1);
    assert_eq!(ledger.spent().input_bytes, 3);
    assert_eq!(input.position(), 3);
}
