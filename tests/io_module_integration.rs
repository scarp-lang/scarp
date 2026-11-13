//! Cross-module input capability regressions for the development I/O integration.
use std::io::Cursor;

use scarp::{
    diagnostics::DiagnosticKind as Kind,
    modules::{self, MemorySourceLoader, ModuleLimits, ModuleProgram, ModuleRunError},
    ExecutionOptions, InputLimits,
};

fn compile(entry: &str, library: &str) -> ModuleProgram {
    let mut loader = MemorySourceLoader::new();
    loader.insert("main.srp", entry).unwrap();
    loader.insert("lib.srp", library).unwrap();
    modules::compile_with_loader("main.srp", &loader, ModuleLimits::default()).unwrap()
}

#[test]
fn imported_impure_function_can_prompt_and_read_the_next_line() {
    let program = compile(
        "import \"lib.srp\" as lib; print(\"buffered\"); print(lib.greet());",
        "pub funcra greet() -> string { return ask(\"Name: \"); }",
    );
    let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
    program
        .execute_with_interactive_input(
            &mut Cursor::new(b"Ada\r\n"),
            &mut stdout,
            &mut stderr,
            ExecutionOptions::default(),
            InputLimits::default(),
        )
        .unwrap();
    // Prompts flush immediately; ordinary print events replay afterward.
    assert_eq!(stdout, b"Name: buffered\nAda\n");
    assert!(stderr.is_empty());
}

#[test]
fn imported_interactive_errors_report_library_and_caller_spans() {
    let program = compile(
        "import \"lib.srp\" as lib; print(\"buffered\"); print(lib.greet());",
        "// 🌍\npub funcra greet() -> string { return ask(\"Name: \"); }",
    );
    let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
    let error = program
        .execute_with_interactive_input(
            &mut Cursor::new(b""),
            &mut stdout,
            &mut stderr,
            ExecutionOptions::default(),
            InputLimits::default(),
        )
        .unwrap_err();
    let ModuleRunError::Diagnostic(error) = error else {
        panic!("expected source-mapped input diagnostic");
    };
    assert_eq!(error.diagnostic.kind, Kind::InputEof);
    let rendered = error.render();
    assert!(rendered.contains("lib.srp:2:"), "{rendered}");
    assert!(rendered.contains("main.srp:1:"), "{rendered}");
    assert_eq!(stdout, b"Name: ");
    assert!(stderr.is_empty());
}

#[test]
fn imported_eof_fallback_does_not_hide_invalid_utf8() {
    let program = compile(
        "import \"lib.srp\" as lib; print(\"buffered\"); print(lib.next());",
        "pub funcra next() -> string { return read_line_or(\"Guest\"); }",
    );
    let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
    program
        .execute_with_input(
            &mut Cursor::new(b""),
            &mut stdout,
            &mut stderr,
            ExecutionOptions::default(),
            InputLimits::default(),
        )
        .unwrap();
    assert_eq!(stdout, b"buffered\nGuest\n");
    assert!(stderr.is_empty());

    stdout.clear();
    let error = program
        .execute_with_input(
            &mut Cursor::new(b"\xff\n"),
            &mut stdout,
            &mut stderr,
            ExecutionOptions::default(),
            InputLimits::default(),
        )
        .unwrap_err();
    let ModuleRunError::Diagnostic(error) = error else {
        panic!("expected invalid-encoding diagnostic");
    };
    assert_eq!(error.diagnostic.kind, Kind::InputEncoding);
    assert!(error.render().contains("lib.srp:1:"));
    assert!(stdout.is_empty() && stderr.is_empty());
}

#[test]
fn imported_user_function_named_ask_shadows_builtin_without_capability() {
    let program = compile(
        "import \"lib.srp\" as lib; print(lib.greet());",
        "pub pure funcra greet() -> string { return ask(\"shadowed\"); } pure funcra ask(value: string) -> string { return value; }",
    );
    let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
    program
        .execute(&mut stdout, &mut stderr, ExecutionOptions::default())
        .unwrap();
    assert_eq!(stdout, b"shadowed\n");
    assert!(stderr.is_empty());
}

#[test]
fn pure_imported_function_cannot_acquire_interactive_input() {
    let mut loader = MemorySourceLoader::new();
    loader
        .insert("main.srp", "import \"lib.srp\" as lib; print(lib.greet());")
        .unwrap();
    loader
        .insert(
            "lib.srp",
            "pub pure funcra greet() -> string { return ask(\"Name: \"); }",
        )
        .unwrap();
    let error =
        modules::compile_with_loader("main.srp", &loader, ModuleLimits::default()).unwrap_err();
    assert_eq!(error.diagnostic.kind, Kind::ImpureFunction);
    assert!(error.render().contains("lib.srp:1:"));
}
