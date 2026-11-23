use std::{io, sync::Arc};

use scarp::{
    diagnostics::DiagnosticKind,
    modules::{
        self, MemorySourceLoader, ModuleCallableBindError as BindError,
        ModuleCallableCatalog, ModuleLimits, ModuleRunError,
    },
    resolver::HostArguments,
    value::Value,
    ExecutionOptions, InputLimits,
};

fn fixture() -> modules::ModuleProgram {
    let mut loader = MemorySourceLoader::new();
    loader
        .insert(
            "main.srp",
            "import \"one.srp\" as one; import \"two.srp\" as two;\n\
             print(one.calculation(3));",
        )
        .unwrap();
    loader
        .insert(
            "one.srp",
            "// नमस्ते 🌍\r\n\
             pure funcra hidden() -> integer { return 99; }\r\n\
             pub bind greeting = 4;\r\n\
             pub pure funcra calculation(value: integer, factor: integer = 3) -> integer {\r\n\
                 return value * factor;\r\n\
             }\r\n\
             pub pure funcra identity(x: integer) -> integer { return x; }\r\n\
             pub funcra fault(n: integer) -> integer { print(\"pending\"); return 10 / n; }\r\n\
             pub funcra ask(n: integer) -> string { return read_line(); }\r\n",
        )
        .unwrap();
    loader
        .insert(
            "two.srp",
            "pub pure funcra calculation(value: integer, factor: integer = 3) -> integer {\n\
                 return value * factor + 10;\n\
             }",
        )
        .unwrap();
    modules::compile_with_loader("main.srp", &loader, ModuleLimits::default()).unwrap()
}

fn invoke(
    target: &modules::ExportedNamedCallable,
    args: HostArguments,
) -> Result<Value, ModuleRunError> {
    target.invoke_with_io_and_options(
        args,
        &mut Vec::new(),
        &mut Vec::new(),
        ExecutionOptions::default(),
    )
}

#[test]
fn exported_bind_uses_original_source_names_and_correct_linker_index() {
    let linked = fixture();
    let one = linked.imports_in(0).unwrap()[0].target;
    let two = linked.imports_in(0).unwrap()[1].target;
    let catalog = ModuleCallableCatalog::from_program(linked);
    let first = catalog.bind_exported(one, "calculation").unwrap();
    let second = catalog.bind_exported(two, "calculation").unwrap();
    assert_eq!(first.source(), one);
    assert_eq!(second.source(), two);
    assert_eq!(first.name(), "calculation");
    assert_eq!(first.signature(), second.signature());
    assert_eq!(
        invoke(
            &first,
            HostArguments::new(
                vec![],
                vec![
                    ("factor".into(), Value::Integer(4)),
                    ("value".into(), Value::Integer(3)),
                ],
            ),
        )
        .unwrap(),
        Value::Integer(12)
    );
    assert_eq!(
        invoke(&second, HostArguments::positional(vec![Value::Integer(3)])).unwrap(),
        Value::Integer(19)
    );
    assert_eq!(
        invoke(
            &catalog.bind_exported(one, "identity").unwrap(),
            HostArguments::positional(vec![Value::Integer(5)]),
        )
        .unwrap(),
        Value::Integer(5)
    );
}

#[test]
fn private_symbols_constants_and_unknown_modules_cannot_be_bound() {
    let linked = fixture();
    let one = linked.imports_in(0).unwrap()[0].target;
    let catalog = ModuleCallableCatalog::from_program(linked);
    assert_eq!(
        catalog.bind_exported(one, "hidden").unwrap_err(),
        BindError::PrivateSymbol
    );
    assert_eq!(
        catalog.bind_exported(one, "greeting").unwrap_err(),
        BindError::NotFunction
    );
    assert_eq!(
        catalog.bind_exported(one, "absent").unwrap_err(),
        BindError::UnknownSymbol
    );
    assert_eq!(
        catalog.bind_exported(usize::MAX, "calculation").unwrap_err(),
        BindError::UnknownSource(usize::MAX)
    );
    assert_eq!(
        catalog.bind_exported(0, "calculation").unwrap_err(),
        BindError::UnknownSymbol
    );
}

#[test]
fn binding_as_required_signature_checks_purity_and_foreign_arena() {
    let linked = fixture();
    let one = linked.imports_in(0).unwrap()[0].target;
    let two = linked.imports_in(0).unwrap()[1].target;
    let catalog = ModuleCallableCatalog::from_program(linked);
    let first = catalog.bind_exported(one, "calculation").unwrap();
    let second = catalog.bind_exported(two, "calculation").unwrap();
    assert_eq!(first.signature(), second.signature());
    assert!(catalog
        .bind_exported_as(one, "calculation", second.signature())
        .is_ok());
    let foreign = ModuleCallableCatalog::from_program(fixture());
    let other_type = foreign
        .bind_exported(one, "calculation")
        .unwrap()
        .signature();
    assert!(matches!(
        catalog.bind_exported_as(one, "calculation", other_type),
        Err(BindError::Callable(_))
    ));
}

#[test]
fn original_module_source_map_survives_catalog_drop_and_renders_failure() {
    let original = Arc::new(fixture());
    let weak = Arc::downgrade(&original);
    let one = original.imports_in(0).unwrap()[0].target;
    let catalog = ModuleCallableCatalog::new(Arc::clone(&original));
    let bad = catalog.bind_exported(one, "fault").unwrap();
    drop(catalog);
    drop(original);
    // The exported handle owns the cloned resolved plan and source map,
    // but does not retain the larger linked ModuleProgram allocation.
    assert!(weak.upgrade().is_none());
    let (mut out, mut err) = (Vec::new(), Vec::new());
    match bad
        .invoke_with_io_and_options(
            HostArguments::new(vec![], vec![("n".into(), Value::Integer(0))]),
            &mut out,
            &mut err,
            ExecutionOptions::default(),
        )
        .unwrap_err()
    {
        ModuleRunError::Diagnostic(error) => {
            assert_eq!(error.diagnostic.kind, DiagnosticKind::DivisionByZero);
            assert!(error.render().contains("one.srp"));
        }
        ModuleRunError::Output(_) => panic!("expected source diagnostic"),
    }
    assert!(out.is_empty() && err.is_empty());
}

#[test]
fn linked_exported_input_and_preflight_reuse_original_diagnostics() {
    let linked = fixture();
    let one = linked.imports_in(0).unwrap()[0].target;
    let target = ModuleCallableCatalog::from_program(linked)
        .bind_exported(one, "ask")
        .unwrap();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let mut input = io::Cursor::new(b"hello\r\n");
    match target
        .invoke_with_input_and_options(
            HostArguments::new(vec![], vec![("n".into(), Value::Boolean(false))]),
            &mut input,
            &mut out,
            &mut err,
            ExecutionOptions::default(),
            InputLimits::default(),
        )
        .unwrap_err()
    {
        ModuleRunError::Diagnostic(error) => {
            assert_eq!(error.diagnostic.kind, DiagnosticKind::TypeMismatch);
            assert!(error.render().contains("one.srp"));
        }
        ModuleRunError::Output(_) => panic!("expected host preflight diagnostic"),
    }
    assert_eq!(input.position(), 0);
    assert!(out.is_empty() && err.is_empty());
    assert_eq!(
        target
            .invoke_with_input_and_options(
                HostArguments::new(vec![], vec![("n".into(), Value::Integer(1))]),
                &mut input,
                &mut out,
                &mut err,
                ExecutionOptions::default(),
                InputLimits::default(),
            )
            .unwrap(),
        Value::from("hello")
    );
}

#[test]
fn exported_host_execution_respects_quotas_with_original_diagnostics() {
    let linked = fixture();
    let one = linked.imports_in(0).unwrap()[0].target;
    let target = ModuleCallableCatalog::from_program(linked)
        .bind_exported(one, "calculation")
        .unwrap();
    match target
        .invoke_with_io_and_options(
            HostArguments::positional(vec![Value::Integer(5)]),
            &mut Vec::new(),
            &mut Vec::new(),
            ExecutionOptions { max_calls: 0, ..ExecutionOptions::default() },
        )
        .unwrap_err()
    {
        ModuleRunError::Diagnostic(error) => {
            assert_eq!(error.diagnostic.kind, DiagnosticKind::CallLimit);
            assert!(error.render().contains("one.srp"));
        }
        ModuleRunError::Output(_) => panic!("expected call limit diagnostic"),
    }
}
