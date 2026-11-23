use std::{io::Cursor, sync::Arc};

use scarp::{
    diagnostics::DiagnosticKind,
    modules::{
        self, ExportedSlotBindError, ExportedSlotInvokeError, MemorySourceLoader,
        ModuleCallableBindError, ModuleCallableCatalog, ModuleLimits, ModuleRunError,
    },
    resolver::HostArguments,
    value::Value,
    ExecutionOptions, InputLimits,
};

fn linked() -> modules::ModuleProgram {
    let mut loader = MemorySourceLoader::new();
    loader.insert(
        "main.srp",
        "import \"lib.srp\" as lib; import \"extra.srp\" as extra; print(lib.first(3));",
    ).unwrap();
    loader.insert(
        "lib.srp",
        "// café 🌍\r\n\
         pure funcra hidden(n: integer, extra: integer = 0) -> integer { return n; }\r\n\
         pub bind magic = 42;\r\n\
         pub pure funcra first(n: integer, extra: integer = 2) -> integer {\r\n\
             return n + extra;\r\n\
         }\r\n\
         pub pure funcra second(value: integer, extra: integer = 4) -> integer {\r\n\
             return value * extra;\r\n\
         }\r\n\
         pub funcra effects(n: integer, extra: integer = 2) -> integer {\r\n\
             print(\"pending\"); return 10 / n;\r\n\
         }\r\n\
         pub funcra ask(n: integer) -> string { return read_line(); }\r\n",
    ).unwrap();
    loader.insert(
        "extra.srp",
        "pub pure funcra second(value: integer, extra: integer = 4) -> integer {\n\
             return value * extra + 100;\n\
         }",
    ).unwrap();
    modules::compile_with_loader("main.srp", &loader, ModuleLimits::default()).unwrap()
}

fn call(
    slot: &modules::ExportedCallableSlot,
    arguments: HostArguments,
) -> Result<Value, ExportedSlotInvokeError> {
    slot.invoke_with_io_and_options(
        arguments,
        &mut Vec::new(),
        &mut Vec::new(),
        ExecutionOptions::default(),
    )
}

fn source_ids(linked: &modules::ModuleProgram) -> (usize, usize) {
    let imports = linked.imports_in(0).unwrap();
    (imports[0].target, imports[1].target)
}

#[test]
fn public_rebinding_selects_correct_function_in_same_and_other_modules() {
    let source = linked();
    let (lib, extra) = source_ids(&source);
    let signature = source.program().function_type_id(1).unwrap();
    let catalog = ModuleCallableCatalog::from_program(source);
    let mut slot = catalog.slot_exported(lib, "first", signature).unwrap();
    assert_eq!(slot.required_signature(), signature);
    assert_eq!(slot.selected(), Some((lib, "first")));
    assert_eq!(
        call(&slot, HostArguments::positional(vec![Value::Integer(3)])).unwrap(),
        Value::Integer(5)
    );
    slot.select_exported(lib, "second").unwrap();
    assert_eq!(slot.selected(), Some((lib, "second")));
    assert_eq!(
        call(
            &slot,
            HostArguments::new(
                vec![],
                vec![("value".into(), Value::Integer(3))],
            ),
        ).unwrap(),
        Value::Integer(12)
    );
    slot.select_exported(extra, "second").unwrap();
    assert_eq!(slot.selected(), Some((extra, "second")));
    assert_eq!(
        call(&slot, HostArguments::positional(vec![Value::Integer(3)])).unwrap(),
        Value::Integer(112)
    );
}

#[test]
fn private_and_nonfunction_selection_cannot_replace_existing_public_target() {
    let linked = linked();
    let (lib, extra) = source_ids(&linked);
    let signature = linked.program().function_type_id(1).unwrap();
    let catalog = ModuleCallableCatalog::from_program(linked);
    let mut slot = catalog.slot_exported(lib, "first", signature).unwrap();
    for (source, name, expected) in [
        (lib, "hidden", ModuleCallableBindError::PrivateSymbol),
        (lib, "magic", ModuleCallableBindError::NotFunction),
        (lib, "missing", ModuleCallableBindError::UnknownSymbol),
        (extra, "missing", ModuleCallableBindError::UnknownSymbol),
        (usize::MAX, "first", ModuleCallableBindError::UnknownSource(usize::MAX)),
    ] {
        assert_eq!(
            slot.select_exported(source, name),
            Err(ExportedSlotBindError::Export(expected))
        );
        assert_eq!(slot.selected(), Some((lib, "first")));
        assert_eq!(
            call(&slot, HostArguments::positional(vec![Value::Integer(1)])).unwrap(),
            Value::Integer(3)
        );
    }
    assert_eq!(
        catalog.slot_exported(lib, "hidden", signature).unwrap_err(),
        ExportedSlotBindError::Export(ModuleCallableBindError::PrivateSymbol)
    );
}

#[test]
fn rejected_purity_rebinding_preserves_old_target_and_effects_are_bounded() {
    let linked = linked();
    let (lib, _) = source_ids(&linked);
    let pure_sig = linked.program().function_type_id(1).unwrap();
    let impure_sig = linked.program().function_type_id(3).unwrap();
    let catalog = ModuleCallableCatalog::from_program(linked);
    let mut pure_slot = catalog.slot_exported(lib, "first", pure_sig).unwrap();
    assert!(matches!(
        pure_slot.select_exported(lib, "effects"),
        Err(ExportedSlotBindError::Export(ModuleCallableBindError::Callable(_)))
    ));
    assert_eq!(pure_slot.selected(), Some((lib, "first")));
    let mut impure_slot = catalog.slot_exported(lib, "first", impure_sig).unwrap();
    assert_eq!(
        call(&impure_slot, HostArguments::positional(vec![Value::Integer(7)])).unwrap(),
        Value::Integer(9)
    );
    impure_slot.select_exported(lib, "effects").unwrap();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    assert!(matches!(
        impure_slot.invoke_with_io_and_options(
            HostArguments::positional(vec![Value::Integer(0)]),
            &mut out,
            &mut err,
            ExecutionOptions::default(),
        ),
        Err(ExportedSlotInvokeError::Runtime(ModuleRunError::Diagnostic(diag)))
            if diag.diagnostic.kind == DiagnosticKind::DivisionByZero
    ));
    assert!(out.is_empty() && err.is_empty());
    assert_eq!(
        impure_slot.invoke_with_io_and_options(
            HostArguments::positional(vec![Value::Integer(2)]),
            &mut out,
            &mut err,
            ExecutionOptions::default(),
        ).unwrap(),
        Value::Integer(5)
    );
    assert_eq!(out, b"pending\n");
}

#[test]
fn currently_selected_parameter_names_change_with_the_actual_exported_target() {
    let linked = linked();
    let (lib, _) = source_ids(&linked);
    let signature = linked.program().function_type_id(1).unwrap();
    let catalog = ModuleCallableCatalog::from_program(linked);
    let mut slot = catalog.slot_exported(lib, "first", signature).unwrap();
    assert_eq!(
        call(
            &slot,
            HostArguments::new(vec![], vec![("n".into(), Value::Integer(3))]),
        ).unwrap(),
        Value::Integer(5)
    );
    slot.select_exported(lib, "second").unwrap();
    assert!(matches!(
        call(
            &slot,
            HostArguments::new(vec![], vec![("n".into(), Value::Integer(3))]),
        ),
        Err(ExportedSlotInvokeError::Runtime(ModuleRunError::Diagnostic(diag)))
            if diag.diagnostic.kind == DiagnosticKind::CallArguments
    ));
    assert_eq!(
        call(
            &slot,
            HostArguments::new(vec![], vec![("value".into(), Value::Integer(3))]),
        ).unwrap(),
        Value::Integer(12)
    );
}

#[test]
fn cleared_slot_does_not_consume_input_and_can_be_rebound_to_public_name() {
    let linked = linked();
    let (lib, _) = source_ids(&linked);
    let signature = linked.program().function_type_id(4).unwrap();
    let catalog = ModuleCallableCatalog::from_program(linked);
    let mut slot = catalog.slot_exported(lib, "ask", signature).unwrap();
    slot.clear();
    assert_eq!(slot.selected(), None);
    assert!(!slot.is_bound());
    let mut input = Cursor::new(b"answer\r\n");
    let (mut out, mut err) = (Vec::new(), Vec::new());
    assert!(matches!(
        slot.invoke_with_input_and_options(
            HostArguments::positional(vec![Value::Integer(1)]),
            &mut input,
            &mut out,
            &mut err,
            ExecutionOptions::default(),
            InputLimits::default(),
        ),
        Err(ExportedSlotInvokeError::Unbound)
    ));
    assert_eq!(input.position(), 0);
    assert!(out.is_empty() && err.is_empty());
    slot.select_exported(lib, "ask").unwrap();
    assert_eq!(
        slot.invoke_with_input_and_options(
            HostArguments::positional(vec![Value::Integer(1)]),
            &mut input,
            &mut out,
            &mut err,
            ExecutionOptions::default(),
            InputLimits::default(),
        ).unwrap(),
        Value::from("answer")
    );
}

#[test]
fn source_aware_diagnostic_survives_original_module_catalog_drop() {
    let linked = Arc::new(linked());
    let weak = Arc::downgrade(&linked);
    let (lib, _) = source_ids(&linked);
    let impure = linked.program().function_type_id(3).unwrap();
    let catalog = ModuleCallableCatalog::new(Arc::clone(&linked));
    let slot = catalog.slot_exported(lib, "effects", impure).unwrap();
    drop(catalog);
    drop(linked);
    assert!(weak.upgrade().is_some());
    let (mut out, mut err) = (Vec::new(), Vec::new());
    match slot.invoke_with_io_and_options(
        HostArguments::positional(vec![Value::Integer(0)]),
        &mut out,
        &mut err,
        ExecutionOptions::default(),
    ).unwrap_err() {
        ExportedSlotInvokeError::Runtime(ModuleRunError::Diagnostic(diag)) => {
            assert_eq!(diag.diagnostic.kind, DiagnosticKind::DivisionByZero);
            assert!(diag.render().contains("lib.srp"));
        }
        other => panic!("unexpected result: {other}"),
    }
    assert!(out.is_empty() && err.is_empty());
    drop(slot);
    assert!(weak.upgrade().is_none());
}

#[test]
fn cloned_exported_slot_has_independent_selection_but_shared_snapshot() {
    let linked = linked();
    let (lib, extra) = source_ids(&linked);
    let signature = linked.program().function_type_id(1).unwrap();
    let catalog = ModuleCallableCatalog::from_program(linked);
    let mut first = catalog.slot_exported(lib, "first", signature).unwrap();
    let second = first.clone();
    first.select_exported(extra, "second").unwrap();
    assert_eq!(first.selected(), Some((extra, "second")));
    assert_eq!(second.selected(), Some((lib, "first")));
    assert_eq!(
        call(&second, HostArguments::positional(vec![Value::Integer(2)])).unwrap(),
        Value::Integer(4)
    );
    assert_eq!(
        call(&first, HostArguments::positional(vec![Value::Integer(2)])).unwrap(),
        Value::Integer(108)
    );
}
