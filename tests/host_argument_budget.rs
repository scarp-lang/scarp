use std::{io::Cursor, sync::Arc};

use scarp::{
    embedding::{
        HostArgumentLimits, HostArgumentUsage, HostInvocationCaps, HostInvocationError,
        HostInvocationLedger, HostQuotaResource,
    },
    modules::{self, MemorySourceLoader, ModuleCallableCatalog, ModuleLimits},
    resolver::{CallableCatalog, CallableSlot, CallableSlotInvokeError, HostArguments},
    value::Value,
    ExecutionOptions, InputLimits,
};

fn slot(source: &str) -> CallableSlot {
    let program = scarp::compile(source).unwrap();
    let signature = program.function_type_id(0).unwrap();
    CallableCatalog::from_program(program)
        .slot_bound(signature, 0)
        .unwrap()
}

fn invoke(
    ledger: &mut HostInvocationLedger,
    callable: &CallableSlot,
    args: HostArguments,
) -> Result<Value, HostInvocationError<CallableSlotInvokeError>> {
    ledger.invoke_slot(
        callable,
        args,
        (&mut Vec::new(), &mut Vec::new()),
        ExecutionOptions::default(),
    )
}
fn text_arg(s: &str) -> HostArguments {
    HostArguments::positional(vec![Value::from(s)])
}

#[test]
fn multibyte_unicode_and_named_labels_are_billed_as_raw_utf8() {
    let callable =
        slot("pure funcra keep(message: string, number: integer) -> string { return message; }");
    let mut ledger = HostInvocationLedger::new(HostInvocationCaps::default());
    let args = HostArguments::new(
        vec![],
        vec![
            ("message".into(), Value::from("é🍀")),
            ("number".into(), Value::Integer(7)),
        ],
    );
    assert_eq!(
        invoke(&mut ledger, &callable, args).unwrap(),
        Value::from("é🍀")
    );
    assert_eq!(
        ledger.argument_usage(),
        HostArgumentUsage {
            supplied_values: 2,
            utf8_bytes: 19
        }
    );
    assert_eq!(ledger.spent().calls, 1);
    assert_eq!(ledger.spent().text_bytes, 0);
}

#[test]
fn too_many_values_are_denied_before_binding_or_host_io() {
    let callable = slot("funcra f(n: integer) -> integer { print(n); return n; }");
    let caps = HostInvocationCaps {
        arguments: HostArgumentLimits {
            max_values_per_call: 1,
            ..HostArgumentLimits::default()
        },
        ..HostInvocationCaps::default()
    };
    let mut ledger = HostInvocationLedger::new(caps);
    let (mut out, mut err) = (Vec::new(), Vec::new());
    assert!(matches!(
        ledger.invoke_slot(
            &callable,
            HostArguments::positional(vec![Value::Integer(1), Value::Integer(2)]),
            (&mut out, &mut err),
            ExecutionOptions::default(),
        ),
        Err(HostInvocationError::Quota(error))
            if error.resource == HostQuotaResource::ArgumentValues
    ));
    assert!(out.is_empty() && err.is_empty());
    assert_eq!(ledger.attempts(), 0);
    assert_eq!(ledger.argument_usage(), HostArgumentUsage::default());
    assert_eq!(ledger.spent().calls, 0);
}

#[test]
fn cumulative_value_count_is_not_reset_between_calls() {
    let callable = slot("pure funcra id(n: integer) -> integer { return n; }");
    let caps = HostInvocationCaps {
        arguments: HostArgumentLimits {
            max_total_values: 2,
            ..HostArgumentLimits::default()
        },
        ..HostInvocationCaps::default()
    };
    let mut ledger = HostInvocationLedger::new(caps);
    for n in [1, 2] {
        assert_eq!(
            invoke(
                &mut ledger,
                &callable,
                HostArguments::positional(vec![Value::Integer(n)])
            )
            .unwrap(),
            Value::Integer(n)
        );
    }
    assert_eq!(ledger.remaining_arguments().max_total_values, 0);
    assert!(matches!(
        invoke(&mut ledger, &callable,
            HostArguments::positional(vec![Value::Integer(3)])),
        Err(HostInvocationError::Quota(error))
            if error.resource == HostQuotaResource::ArgumentValues
    ));
    assert_eq!(ledger.attempts(), 2);
    assert_eq!(ledger.argument_usage().supplied_values, 2);
}

#[test]
fn per_call_utf8_limit_denies_without_input_or_output() {
    let callable = slot("funcra echo(s: string) -> string { print(s); return s; }");
    let caps = HostInvocationCaps {
        arguments: HostArgumentLimits {
            max_utf8_bytes_per_call: 4,
            ..HostArgumentLimits::default()
        },
        ..HostInvocationCaps::default()
    };
    let mut ledger = HostInvocationLedger::new(caps);
    let mut output = Vec::new();
    assert!(matches!(
        ledger.invoke_slot(
            &callable,
            text_arg("ééé"),
            (&mut output, &mut Vec::new()),
            ExecutionOptions::default(),
        ),
        Err(HostInvocationError::Quota(error))
            if error.resource == HostQuotaResource::ArgumentBytes
    ));
    assert!(output.is_empty());
    assert_eq!(ledger.attempts(), 0);
    assert_eq!(ledger.argument_usage(), HostArgumentUsage::default());
}

#[test]
fn cumulative_utf8_quota_is_exact_and_denied_calls_do_not_spend_it() {
    let callable = slot("pure funcra echo(s: string) -> string { return s; }");
    let caps = HostInvocationCaps {
        arguments: HostArgumentLimits {
            max_total_utf8_bytes: 3,
            ..HostArgumentLimits::default()
        },
        ..HostInvocationCaps::default()
    };
    let mut ledger = HostInvocationLedger::new(caps);
    assert_eq!(
        invoke(&mut ledger, &callable, text_arg("ok")).unwrap(),
        Value::from("ok")
    );
    assert_eq!(ledger.remaining_arguments().max_total_utf8_bytes, 1);
    assert!(matches!(
        invoke(&mut ledger, &callable, text_arg("no")),
        Err(HostInvocationError::Quota(error))
            if error.resource == HostQuotaResource::ArgumentBytes
    ));
    assert_eq!(ledger.attempts(), 1);
    assert_eq!(ledger.argument_usage().utf8_bytes, 2);
    assert_eq!(
        invoke(&mut ledger, &callable, text_arg("x")).unwrap(),
        Value::from("x")
    );
    assert_eq!(ledger.argument_usage().utf8_bytes, 3);
    assert_eq!(ledger.spent().calls, 2);
}

#[test]
fn invalid_labels_and_types_consume_admitted_argument_quota() {
    let callable = slot("pure funcra id(s: string) -> string { return s; }");
    let mut ledger = HostInvocationLedger::new(HostInvocationCaps::default());
    assert!(matches!(
        invoke(
            &mut ledger,
            &callable,
            HostArguments::new(vec![], vec![("not_a_parameter".into(), Value::from("abc"))])
        ),
        Err(HostInvocationError::Invocation(_))
    ));
    assert_eq!(ledger.argument_usage().supplied_values, 1);
    assert_eq!(ledger.argument_usage().utf8_bytes, 18);
    assert_eq!(ledger.spent().calls, 0);
    assert!(matches!(
        invoke(
            &mut ledger,
            &callable,
            HostArguments::positional(vec![Value::Integer(1)])
        ),
        Err(HostInvocationError::Invocation(_))
    ));
    assert_eq!(ledger.argument_usage().supplied_values, 2);
    assert_eq!(ledger.argument_usage().utf8_bytes, 18);
    assert_eq!(ledger.attempts(), 2);
}

#[test]
fn shared_arc_string_counted_on_every_argument_occurrence() {
    let callable = slot("pure funcra same(a: string, b: string) -> string { return a; }");
    let original: Arc<str> = Arc::from("共享🍀");
    let args = HostArguments::positional(vec![
        Value::String(Arc::clone(&original)),
        Value::String(Arc::clone(&original)),
    ]);
    let mut ledger = HostInvocationLedger::new(HostInvocationCaps::default());
    assert_eq!(
        invoke(&mut ledger, &callable, args).unwrap(),
        Value::String(Arc::clone(&original))
    );
    assert_eq!(ledger.argument_usage().utf8_bytes, original.len() * 2);
    assert_eq!(Arc::strong_count(&original), 1);
}

#[test]
fn unbound_slots_charge_admitted_host_values_but_no_scarp_work() {
    let program = scarp::compile("funcra id(s: string) -> string { return s; }").unwrap();
    let signature = program.function_type_id(0).unwrap();
    let unbound = CallableCatalog::from_program(program)
        .slot(signature)
        .unwrap();
    let mut ledger = HostInvocationLedger::new(HostInvocationCaps::default());
    assert!(matches!(
        invoke(&mut ledger, &unbound, text_arg("hello")),
        Err(HostInvocationError::Invocation(
            CallableSlotInvokeError::Unbound
        ))
    ));
    assert_eq!(
        ledger.argument_usage(),
        HostArgumentUsage {
            supplied_values: 1,
            utf8_bytes: 5
        }
    );
    assert_eq!(ledger.spent().steps, 0);
    assert_eq!(ledger.attempts(), 1);
}

#[test]
fn zero_byte_budget_allows_only_scalar_or_empty_string_payloads() {
    let caps = HostInvocationCaps {
        arguments: HostArgumentLimits {
            max_utf8_bytes_per_call: 0,
            max_total_utf8_bytes: 0,
            ..HostArgumentLimits::default()
        },
        ..HostInvocationCaps::default()
    };
    let mut ledger = HostInvocationLedger::new(caps);
    let scalar = slot("pure funcra id(n: integer) -> integer { return n; }");
    assert_eq!(
        invoke(
            &mut ledger,
            &scalar,
            HostArguments::positional(vec![Value::Integer(2)])
        )
        .unwrap(),
        Value::Integer(2)
    );
    let echo = slot("pure funcra echo(s: string) -> string { return s; }");
    assert_eq!(
        invoke(&mut ledger, &echo, text_arg("")).unwrap(),
        Value::from("")
    );
    assert_eq!(ledger.argument_usage().utf8_bytes, 0);
    assert_eq!(ledger.spent().calls, 2);
}

#[test]
fn exported_reader_quota_denial_cannot_consume_input_or_output() {
    let mut loader = MemorySourceLoader::new();
    loader
        .insert(
            "main.srp",
            "import \"lib.srp\" as lib; print(lib.ask(\"seed\"));",
        )
        .unwrap();
    loader
        .insert(
            "lib.srp",
            "pub funcra ask(s: string) -> string { print(s); return read_line(); }",
        )
        .unwrap();
    let linked =
        modules::compile_with_loader("main.srp", &loader, ModuleLimits::default()).unwrap();
    let source = linked.imports_in(0).unwrap()[0].target;
    let required = linked.program().function_type_id(0).unwrap();
    let callable = ModuleCallableCatalog::from_program(linked)
        .slot_exported(source, "ask", required)
        .unwrap();
    let caps = HostInvocationCaps {
        arguments: HostArgumentLimits {
            max_utf8_bytes_per_call: 1,
            ..HostArgumentLimits::default()
        },
        ..HostInvocationCaps::default()
    };
    let mut ledger = HostInvocationLedger::new(caps);
    let mut reader = Cursor::new(b"never\r\n");
    let (mut out, mut err) = (Vec::new(), Vec::new());
    assert!(matches!(
        ledger.invoke_exported_with_input(
            &callable, text_arg("too long"), &mut reader,
            (&mut out, &mut err), ExecutionOptions::default(),
            InputLimits::default(),
        ),
        Err(HostInvocationError::Quota(error))
            if error.resource == HostQuotaResource::ArgumentBytes
    ));
    assert_eq!(reader.position(), 0);
    assert!(out.is_empty() && err.is_empty());
    assert_eq!(ledger.attempts(), 0);
    assert_eq!(ledger.argument_usage(), HostArgumentUsage::default());
}

#[test]
fn exported_named_call_accounts_for_both_label_and_string_payload() {
    let mut loader = MemorySourceLoader::new();
    loader
        .insert(
            "main.srp",
            "import \"lib.srp\" as lib; print(lib.echo(\"ok\"));",
        )
        .unwrap();
    loader
        .insert(
            "lib.srp",
            "pub pure funcra echo(value: string) -> string { return value; }",
        )
        .unwrap();
    let linked =
        modules::compile_with_loader("main.srp", &loader, ModuleLimits::default()).unwrap();
    let source = linked.imports_in(0).unwrap()[0].target;
    let required = linked.program().function_type_id(0).unwrap();
    let callable = ModuleCallableCatalog::from_program(linked)
        .slot_exported(source, "echo", required)
        .unwrap();
    let mut ledger = HostInvocationLedger::new(HostInvocationCaps::default());
    assert_eq!(
        ledger
            .invoke_exported(
                &callable,
                HostArguments::new(vec![], vec![("value".into(), Value::from("hi"))]),
                (&mut Vec::new(), &mut Vec::new()),
                ExecutionOptions::default(),
            )
            .unwrap(),
        Value::from("hi")
    );
    assert_eq!(ledger.argument_usage().utf8_bytes, 7);
    assert_eq!(ledger.argument_usage().supplied_values, 1);
}
