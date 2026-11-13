//! EOF-only fallback for explicitly visible, flushed interactive prompts.
use std::{
    io::{self, BufRead, Cursor, Read, Write},
    process::{Command, Stdio},
};

use scarp::{
    diagnostics::DiagnosticKind as Kind,
    types::{FunctionEffect, TypeKind},
    ExecutionOptions, InputLimits, RunError,
};

fn output(source: &str, bytes: &[u8]) -> (Vec<u8>, Vec<u8>) {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    scarp::run_with_interactive_input(source, &mut Cursor::new(bytes), &mut out, &mut err).unwrap();
    (out, err)
}

#[test]
fn eof_fallback_preserves_visible_prompt_and_empty_line_distinction() {
    let program = r#"print(ask_or("Name: ", "Guest"));"#;
    assert_eq!(
        output(program, b""),
        (b"Name: Guest\n".to_vec(), Vec::new())
    );
    assert_eq!(output(program, b"\n"), (b"Name: \n".to_vec(), Vec::new()));
    assert_eq!(output(program, b"\r\n"), (b"Name: \n".to_vec(), Vec::new()));
    assert_eq!(
        output(program, b"Ada"),
        (b"Name: Ada\n".to_vec(), Vec::new())
    );
    assert_eq!(
        output(r#"print(ask_or("नाम: ", "अतिथि"));"#, b""),
        ("नाम: अतिथि\n".as_bytes().to_vec(), Vec::new())
    );
    assert_eq!(
        output(
            r#"print(ask_or("नाम: ", "अतिथि"));"#,
            "नमस्ते 🌍\r\n".as_bytes()
        ),
        ("नाम: नमस्ते 🌍\n".as_bytes().to_vec(), Vec::new())
    );
}

#[test]
fn fallback_never_hides_encoding_reader_or_quota_errors() {
    let source = r#"print("buffered"); eprint("buffered"); print(ask_or("Name: ", "Guest"));"#;
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let failure =
        scarp::run_with_interactive_input(source, &mut Cursor::new(b"\xff\n"), &mut out, &mut err)
            .unwrap_err();
    assert!(matches!(failure, RunError::Diagnostic(ref d) if d.kind == Kind::InputEncoding));
    assert_eq!(out, b"Name: ");
    assert!(err.is_empty());

    for limits in [
        InputLimits {
            max_reads: 0,
            ..Default::default()
        },
        InputLimits {
            max_total_bytes: 0,
            ..Default::default()
        },
        InputLimits {
            max_line_bytes: 0,
            ..Default::default()
        },
    ] {
        out.clear();
        let failure = scarp::run_with_interactive_input_and_options(
            source,
            &mut Cursor::new(b"Ada\n"),
            &mut out,
            &mut err,
            ExecutionOptions::default(),
            limits,
        )
        .unwrap_err();
        assert!(matches!(failure, RunError::Diagnostic(ref d) if d.kind == Kind::InputLimit));
        assert!(out.is_empty() && err.is_empty());
    }
}

struct FailedReader;

impl Read for FailedReader {
    fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
        Err(io::Error::other("secret-reader-details"))
    }
}

impl BufRead for FailedReader {
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        Err(io::Error::other("secret-reader-details"))
    }

    fn consume(&mut self, _: usize) {}
}

#[test]
fn reader_failure_is_not_eof_and_cannot_trigger_the_fallback() {
    let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
    let error = scarp::run_with_interactive_input(
        r#"print("buffered"); print(ask_or("Prompt: ", "Guest"));"#,
        &mut FailedReader,
        &mut stdout,
        &mut stderr,
    )
    .unwrap_err();
    let RunError::Diagnostic(diagnostic) = error else {
        panic!("expected input error");
    };
    assert_eq!(diagnostic.kind, Kind::InputIo);
    assert!(!diagnostic.message.contains("secret-reader-details"));
    assert_eq!(stdout, b"Prompt: ");
    assert!(stderr.is_empty());
}

struct FailedPrompt;
impl Write for FailedPrompt {
    fn write(&mut self, _: &[u8]) -> io::Result<usize> {
        Err(io::Error::new(
            io::ErrorKind::BrokenPipe,
            "secret-host-details",
        ))
    }
    fn flush(&mut self) -> io::Result<()> {
        Err(io::Error::new(
            io::ErrorKind::BrokenPipe,
            "secret-host-details",
        ))
    }
}

#[test]
fn failed_prompt_writer_remains_error_and_does_not_read_input() {
    let source = r#"print(ask_or("Prompt: ", "Guest"));"#;
    let program = scarp::compile(source).unwrap();
    let mut input = Cursor::new(b"Ada\n");
    let mut stderr = Vec::new();
    let error = scarp::interpreter::execute_with_interactive_input_and_options(
        &program,
        &mut input,
        &mut FailedPrompt,
        &mut stderr,
        ExecutionOptions::default(),
        InputLimits::default(),
    )
    .unwrap_err();
    let RunError::Diagnostic(diagnostic) = error else {
        panic!("expected source diagnostic");
    };
    assert_eq!(diagnostic.kind, Kind::PromptIo);
    assert!(!diagnostic.message.contains("secret-host-details"));
    assert_eq!(
        &source[diagnostic.span.start..diagnostic.span.end],
        r#"ask_or("Prompt: ", "Guest")"#
    );
    assert_eq!(input.position(), 0);
    assert!(stderr.is_empty());
}

#[test]
fn default_host_modes_reject_ask_or_without_consuming_input_or_writing_output() {
    let source = r#"print(ask_or("Name: ", "Guest"));"#;
    let mut input = Cursor::new(b"Ada\n");
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let error = scarp::run_with_input(source, &mut input, &mut out, &mut err).unwrap_err();
    assert!(matches!(error, RunError::Diagnostic(ref d) if d.kind == Kind::InputUnavailable));
    if let RunError::Diagnostic(ref diagnostic) = error {
        assert!(
            diagnostic.message.contains("interactive input prompts"),
            "{}",
            diagnostic.message
        );
    }
    assert_eq!(input.position(), 0);
    assert!(out.is_empty() && err.is_empty());
    assert_eq!(
        scarp::check(source).unwrap_err().kind,
        Kind::InputUnavailable
    );
}

#[test]
fn typing_purity_shadowing_and_eager_argument_order() {
    for source in [
        r#"print(ask_or());"#,
        r#"print(ask_or("Name: "));"#,
        r#"print(ask_or("Name: ", "Guest", "extra"));"#,
        r#"print(ask_or(prompt: "Name: ", fallback: "Guest"));"#,
    ] {
        assert_eq!(
            scarp::compile(source).unwrap_err().kind,
            Kind::CallArguments
        );
    }
    for source in [
        r#"print(ask_or(8, "Guest"));"#,
        r#"print(ask_or("Name: ", true));"#,
    ] {
        assert_eq!(scarp::compile(source).unwrap_err().kind, Kind::TypeMismatch);
    }
    assert_eq!(
        scarp::compile(r#"pure funcra answer() -> string { return ask_or("Name: ", "Guest"); }"#)
            .unwrap_err()
            .kind,
        Kind::ImpureFunction
    );
    assert_eq!(
        output(
            r#"pure funcra ask_or(a: string, b: string) -> string { return a; } print(ask_or("shadow", "unused"));"#,
            b"",
        ),
        (b"shadow\n".to_vec(), Vec::new())
    );
    assert_eq!(
        output(
            r#"print(ask_or(read_line(), read_line()));"#,
            b"Prompt> \nFallback\nAda\n"
        ),
        (b"Prompt> Ada\n".to_vec(), Vec::new())
    );
}

#[test]
fn typed_metadata_includes_two_impure_string_parameters() {
    let program = scarp::compile(r#"print(ask_or("Name: ", "Guest"));"#).unwrap();
    let signature = program
        .typed_expressions()
        .find_map(|expression| expression.nodes().find_map(|node| node.call_signature))
        .expect("ask_or typed call");
    assert!(matches!(
        program.type_registry().get(signature),
        Some(TypeKind::Function { parameters, result, effect })
            if parameters.as_ref()
                == [scarp::value::ValueType::String.id(), scarp::value::ValueType::String.id()]
                && *result == scarp::value::ValueType::String.id()
                && *effect == FunctionEffect::Impure
    ));
}

#[test]
fn explicit_cli_input_file_and_interactive_flag_support_ask_or() {
    let folder = std::env::temp_dir().join(format!("scarp-ask-or-{}", std::process::id()));
    std::fs::create_dir_all(&folder).unwrap();
    let program = folder.join("greeting.srp");
    let input = folder.join("empty-input.txt");
    std::fs::write(&program, r#"print(ask_or("Name: ", "Guest"));"#).unwrap();
    std::fs::write(&input, b"").unwrap();

    for verb in ["exec", "run"] {
        let result = Command::new(env!("CARGO_BIN_EXE_scarp"))
            .arg(verb)
            .arg("--interactive")
            .arg("--input-file")
            .arg(&input)
            .arg(&program)
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert!(result.status.success(), "{result:?}");
        assert_eq!(result.stdout, b"Name: Guest\n");
        assert!(result.stderr.is_empty());
    }
    let _ = std::fs::remove_dir_all(folder);
}
