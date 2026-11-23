use std::{
    io::{self, Cursor, Write},
    panic::{catch_unwind, AssertUnwindSafe},
};

use scarp::{
    embedding::{HostInvocationCaps, HostInvocationError, HostInvocationLedger, HostQuotaResource},
    modules::{self, MemorySourceLoader, ModuleCallableCatalog, ModuleLimits},
    resolver::{CallableCatalog, CallableSlot, HostArguments},
    value::Value,
    ExecutionOptions, InputLimits,
};

fn slot(code: &str) -> CallableSlot {
    let program = scarp::compile(code).unwrap();
    let signature = program.function_type_id(0).unwrap();
    CallableCatalog::from_program(program)
        .slot_bound(signature, 0)
        .unwrap()
}

fn one(n: i64) -> HostArguments {
    HostArguments::positional(vec![Value::Integer(n)])
}

struct PanickingWriter;
impl Write for PanickingWriter {
    fn write(&mut self, _: &[u8]) -> io::Result<usize> {
        panic!("deliberate host writer failure");
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

struct PanickingFlushWriter {
    contents: Vec<u8>,
}
impl Write for PanickingFlushWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.contents.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        panic!("deliberate host flush failure");
    }
}

#[test]
fn panicking_writer_cannot_erase_completed_interpreter_work_or_call_quota() {
    let callable = slot("funcra emit(n: integer) -> integer { print(n); return n; }");
    let mut caps = HostInvocationCaps::default();
    caps.execution.max_calls = 1;
    let mut ledger = HostInvocationLedger::new(caps);
    let unwind = catch_unwind(AssertUnwindSafe(|| {
        let _ = ledger.invoke_slot(
            &callable, one(3),
            (&mut PanickingWriter, &mut Vec::new()),
            ExecutionOptions::default(),
        );
    }));
    assert!(unwind.is_err());
    assert_eq!(ledger.attempts(), 1);
    assert_eq!(ledger.spent().calls, 1);
    assert_eq!(ledger.spent().prints, 1);
    assert!(ledger.spent().steps > 0);
    assert!(ledger.spent().capture_bytes >= 64);
    assert_eq!(ledger.argument_usage().supplied_values, 1);
    assert!(matches!(
        ledger.invoke_slot(
            &callable, one(4),
            (&mut Vec::new(), &mut Vec::new()),
            ExecutionOptions::default(),
        ),
        Err(HostInvocationError::Quota(error))
            if error.resource == HostQuotaResource::Calls
    ));
    assert_eq!(ledger.attempts(), 1);
    assert_eq!(ledger.spent().calls, 1);
}

#[test]
fn captured_work_is_charged_once_for_normal_success_and_returned_writer_error() {
    let callable = slot("funcra emit(n: integer) -> integer { print(n); return n; }");
    let mut ledger = HostInvocationLedger::new(HostInvocationCaps::default());
    let mut normal = Vec::new();
    assert_eq!(
        ledger.invoke_slot(
            &callable, one(5),
            (&mut normal, &mut Vec::new()),
            ExecutionOptions::default(),
        ).unwrap(),
        Value::Integer(5)
    );
    assert_eq!(normal, b"5\n");
    let first = ledger.spent();
    assert_eq!(first.calls, 1);
    assert_eq!(first.prints, 1);
    struct FailingWriter;
    impl Write for FailingWriter {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            Err(io::Error::other("writer unavailable"))
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    assert!(ledger.invoke_slot(
        &callable, one(7),
        (&mut FailingWriter, &mut Vec::new()),
        ExecutionOptions::default(),
    ).is_err());
    assert_eq!(ledger.spent().calls, 2);
    assert_eq!(ledger.spent().prints, 2);
    assert_eq!(ledger.spent().steps, first.steps * 2);
    assert_eq!(ledger.attempts(), 2);
}

#[test]
fn flush_panic_after_stdout_event_still_charges_both_selected_stream_operations() {
    let callable = slot(
        "funcra emit(n: integer) -> integer { print(n); eprint(\"err\"); return n; }",
    );
    let mut ledger = HostInvocationLedger::new(HostInvocationCaps::default());
    let mut stdout = PanickingFlushWriter { contents: Vec::new() };
    let (mut stderr) = Vec::new();
    assert!(catch_unwind(AssertUnwindSafe(|| {
        let _ = ledger.invoke_slot(
            &callable, one(8), (&mut stdout, &mut stderr),
            ExecutionOptions::default(),
        );
    })).is_err());
    assert_eq!(stdout.contents, b"8\n");
    assert!(stderr.is_empty());
    assert_eq!(ledger.spent().calls, 1);
    assert_eq!(ledger.spent().prints, 2);
    assert!(ledger.spent().capture_bytes > 64);
}

#[test]
fn reader_consumption_and_work_survive_output_writer_panic() {
    let callable = slot(
        "funcra read(n: integer) -> string { bind s = read_line(); print(s); return s; }",
    );
    let mut ledger = HostInvocationLedger::new(HostInvocationCaps::default());
    let mut reader = Cursor::new(b"ok\r\nnext\r\n");
    assert!(catch_unwind(AssertUnwindSafe(|| {
        let _ = ledger.invoke_slot_with_input(
            &callable, one(1), &mut reader,
            (&mut PanickingWriter, &mut Vec::new()),
            ExecutionOptions::default(), InputLimits::default(),
        );
    })).is_err());
    assert_eq!(reader.position(), 4);
    assert_eq!(ledger.spent().input_reads, 1);
    assert_eq!(ledger.spent().input_bytes, 4);
    assert_eq!(ledger.spent().calls, 1);
    assert_eq!(ledger.spent().prints, 1);
    assert!(ledger.spent().text_bytes >= 4);
}

#[test]
fn exported_module_writer_panic_preserves_remaining_budget() {
    let mut loader = MemorySourceLoader::new();
    loader.insert("main.srp", "import \"lib.srp\" as lib; print(lib.emit(2));").unwrap();
    loader.insert(
        "lib.srp",
        "pub funcra emit(n: integer) -> integer { print(n); return n; }",
    ).unwrap();
    let linked = modules::compile_with_loader("main.srp", &loader, ModuleLimits::default()).unwrap();
    let source = linked.imports_in(0).unwrap()[0].target;
    let required = linked.program().function_type_id(0).unwrap();
    let callable = ModuleCallableCatalog::from_program(linked)
        .slot_exported(source, "emit", required).unwrap();
    let mut ledger = HostInvocationLedger::new(HostInvocationCaps::default());
    assert!(catch_unwind(AssertUnwindSafe(|| {
        let _ = ledger.invoke_exported(
            &callable, one(2),
            (&mut PanickingWriter, &mut Vec::new()),
            ExecutionOptions::default(),
        );
    })).is_err());
    assert_eq!(ledger.spent().calls, 1);
    assert_eq!(ledger.spent().prints, 1);
    assert_eq!(ledger.argument_usage().supplied_values, 1);
    let mut bytes = Vec::new();
    assert_eq!(
        ledger.invoke_exported(
            &callable, one(3), (&mut bytes, &mut Vec::new()),
            ExecutionOptions::default(),
        ).unwrap(),
        Value::Integer(3)
    );
    assert_eq!(bytes, b"3\n");
    assert_eq!(ledger.spent().calls, 2);
    assert_eq!(ledger.spent().prints, 2);
}

#[test]
fn exported_reader_receipt_survives_host_replay_panic() {
    let mut loader = MemorySourceLoader::new();
    loader.insert("main.srp", "import \"lib.srp\" as lib; print(lib.read(1));").unwrap();
    loader.insert(
        "lib.srp",
        "pub funcra read(n: integer) -> string { bind s = read_line(); print(s); return s; }",
    ).unwrap();
    let linked = modules::compile_with_loader("main.srp", &loader, ModuleLimits::default()).unwrap();
    let source = linked.imports_in(0).unwrap()[0].target;
    let required = linked.program().function_type_id(0).unwrap();
    let callable = ModuleCallableCatalog::from_program(linked)
        .slot_exported(source, "read", required).unwrap();
    let mut ledger = HostInvocationLedger::new(HostInvocationCaps::default());
    let mut input = Cursor::new(b"yes\n");
    assert!(catch_unwind(AssertUnwindSafe(|| {
        let _ = ledger.invoke_exported_with_input(
            &callable, one(1), &mut input,
            (&mut PanickingWriter, &mut Vec::new()),
            ExecutionOptions::default(), InputLimits::default(),
        );
    })).is_err());
    assert_eq!(input.position(), 4);
    assert_eq!(ledger.spent().input_bytes, 4);
    assert_eq!(ledger.spent().input_reads, 1);
    assert_eq!(ledger.spent().calls, 1);
    assert_eq!(ledger.spent().prints, 1);
}
