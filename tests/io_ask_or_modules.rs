//! Imported interactive fallback must preserve module source mapping and I/O isolation.
use std::io::Cursor;

use scarp::{
    diagnostics::DiagnosticKind as Kind,
    modules::{self, MemorySourceLoader, ModuleLimits, ModuleProgram, ModuleRunError},
    ExecutionOptions, InputLimits,
};

fn module_program(library: &str) -> ModuleProgram {
    let mut loader = MemorySourceLoader::new();
    loader
        .insert(
            "main.srp",
            r#"import "lib.srp" as lib; print("buffered"); print(lib.answer());"#,
        )
        .unwrap();
    loader.insert("lib.srp", library).unwrap();
    modules::compile_with_loader("main.srp", &loader, ModuleLimits::default()).unwrap()
}

#[test]
fn imported_ask_or_eof_fallback_preserves_prompt_and_replays_buffered_output() {
    let program = module_program(
        "// 🌍\npub funcra answer() -> string { return ask_or(\"Name: \", \"Guest\"); }",
    );
    let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
    program
        .execute_with_interactive_input(
            &mut Cursor::new(b""),
            &mut stdout,
            &mut stderr,
            ExecutionOptions::default(),
            InputLimits::default(),
        )
        .unwrap();
    assert_eq!(stdout, b"Name: buffered\nGuest\n");
    assert!(stderr.is_empty());

    // A line containing only CRLF is an empty string, never the fallback.
    stdout.clear();
    program
        .execute_with_interactive_input(
            &mut Cursor::new(b"\r\n"),
            &mut stdout,
            &mut stderr,
            ExecutionOptions::default(),
            InputLimits::default(),
        )
        .unwrap();
    assert_eq!(stdout, b"Name: buffered\n\n");
    assert!(stderr.is_empty());
}

#[test]
fn imported_ask_or_encoding_failure_keeps_prompt_and_source_trace() {
    let program = module_program(
        "// 🌍\npub funcra answer() -> string { return ask_or(\"Name: \", \"Guest\"); }",
    );
    let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
    let error = program
        .execute_with_interactive_input(
            &mut Cursor::new(b"\xff\n"),
            &mut stdout,
            &mut stderr,
            ExecutionOptions::default(),
            InputLimits::default(),
        )
        .unwrap_err();
    let ModuleRunError::Diagnostic(diagnostic) = error else {
        panic!("expected source-mapped input diagnostic");
    };
    assert_eq!(diagnostic.diagnostic.kind, Kind::InputEncoding);
    let message = diagnostic.render();
    assert!(message.contains("lib.srp:2:"), "{message}");
    assert!(message.contains("main.srp:1:"), "{message}");
    assert_eq!(stdout, b"Name: ");
    assert!(stderr.is_empty());
}

#[test]
fn imported_ask_or_never_turns_noninteractive_unavailability_into_fallback() {
    let program = module_program(
        "pub funcra answer() -> string { return ask_or(\"Name: \", \"Guest\"); }",
    );
    let mut reader = Cursor::new(b"unused\n");
    let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
    let error = program
        .execute_with_input(
            &mut reader,
            &mut stdout,
            &mut stderr,
            ExecutionOptions::default(),
            InputLimits::default(),
        )
        .unwrap_err();
    let ModuleRunError::Diagnostic(diagnostic) = error else {
        panic!("expected unavailable-input diagnostic");
    };
    assert_eq!(diagnostic.diagnostic.kind, Kind::InputUnavailable);
    assert_eq!(reader.position(), 0);
    assert!(stdout.is_empty() && stderr.is_empty());
}
