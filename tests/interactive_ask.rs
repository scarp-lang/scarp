use std::{
    cell::Cell,
    io::{self, BufRead, Cursor, Read, Write},
    rc::Rc,
};

use scarp::{
    diagnostics::DiagnosticKind as Kind,
    types::{FunctionEffect, TypeKind},
    ExecutionOptions, InputLimits, RunError,
};

fn run(source: &str, bytes: &[u8]) -> (Vec<u8>, Vec<u8>) {
    let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
    scarp::run_with_interactive_input(source, &mut Cursor::new(bytes), &mut stdout, &mut stderr)
        .unwrap();
    (stdout, stderr)
}

#[test]
fn prompts_are_flushed_before_read_and_unicode_lines_preserve_content() {
    let source = r#"bind name: string = ask("नाम: "); print("Hello", name);"#;
    assert_eq!(
        run(source, "नमस्ते 🌍\r\n".as_bytes()),
        ("नाम: Hello नमस्ते 🌍\n".as_bytes().to_vec(), Vec::new())
    );
    assert_eq!(
        run(r#"print(ask("A> "), ask("B> "));"#, b"one\ntwo\n"),
        (b"A> B> one two\n".to_vec(), Vec::new())
    );
    assert_eq!(
        run(r#"print(ask("")); "#, b"\n"),
        (b"\n".to_vec(), Vec::new())
    );
}

#[test]
fn prompts_survive_later_failures_but_print_events_are_transactional() {
    let source = r#"print("buffered"); eprint("buffered"); print(ask("Name: "));"#;
    let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
    let failure =
        scarp::run_with_interactive_input(source, &mut Cursor::new(b""), &mut stdout, &mut stderr)
            .unwrap_err();
    assert!(matches!(failure, RunError::Diagnostic(ref d) if d.kind == Kind::InputEof));
    assert_eq!(stdout, b"Name: ");
    assert!(stderr.is_empty());

    stdout.clear();
    let failure = scarp::run_with_interactive_input(
        source,
        &mut Cursor::new(b"\xff\n"),
        &mut stdout,
        &mut stderr,
    )
    .unwrap_err();
    assert!(matches!(failure, RunError::Diagnostic(ref d) if d.kind == Kind::InputEncoding));
    assert_eq!(stdout, b"Name: ");
    assert!(stderr.is_empty());
}

#[test]
fn noninteractive_apis_reject_ask_without_prompt_or_read() {
    let source = r#"print(ask("Name: "));"#;
    let mut input = Cursor::new(b"Ada\n");
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let failure = scarp::run_with_input(source, &mut input, &mut out, &mut err).unwrap_err();
    assert!(matches!(failure, RunError::Diagnostic(ref d) if d.kind == Kind::InputUnavailable));
    assert_eq!(input.position(), 0);
    assert!(out.is_empty() && err.is_empty());
    assert_eq!(
        scarp::check(source).unwrap_err().kind,
        Kind::InputUnavailable
    );
    assert!(scarp::compile(source).is_ok());
}

#[test]
fn preflight_read_quotas_before_exposing_a_prompt() {
    for limits in [
        InputLimits {
            max_reads: 0,
            ..Default::default()
        },
        InputLimits {
            max_line_bytes: 0,
            ..Default::default()
        },
        InputLimits {
            max_total_bytes: 0,
            ..Default::default()
        },
    ] {
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let failure = scarp::run_with_interactive_input_and_options(
            r#"print(ask("Name: "));"#,
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

struct BrokenPrompt;
impl Write for BrokenPrompt {
    fn write(&mut self, _: &[u8]) -> io::Result<usize> {
        Err(io::Error::new(
            io::ErrorKind::BrokenPipe,
            "secret host error",
        ))
    }
    fn flush(&mut self) -> io::Result<()> {
        Err(io::Error::new(
            io::ErrorKind::BrokenPipe,
            "secret host error",
        ))
    }
}

#[test]
fn prompt_write_failure_is_source_located_and_does_not_read_input() {
    let source = r#"print(ask("Prompt> "));"#;
    let program = scarp::compile(source).unwrap();
    let mut input = Cursor::new(b"Ada\n");
    let mut stderr = Vec::new();
    let failure = scarp::interpreter::execute_with_interactive_input_and_options(
        &program,
        &mut input,
        &mut BrokenPrompt,
        &mut stderr,
        ExecutionOptions::default(),
        InputLimits::default(),
    )
    .unwrap_err();
    let RunError::Diagnostic(diagnostic) = failure else {
        panic!("expected prompt diagnostic");
    };
    assert_eq!(diagnostic.kind, Kind::PromptIo);
    assert_eq!(
        &source[diagnostic.span.start..diagnostic.span.end],
        r#"ask("Prompt> ")"#
    );
    assert!(!diagnostic.message.contains("secret host error"));
    assert_eq!(input.position(), 0);
    assert!(stderr.is_empty());
}

#[test]
fn prompt_argument_types_purity_shadowing_and_signature_are_checked() {
    for source in [
        "print(ask());",
        r#"print(ask("one", "two"));"#,
        r#"print(ask(prompt: "one"));"#,
    ] {
        assert_eq!(
            scarp::compile(source).unwrap_err().kind,
            Kind::CallArguments
        );
    }
    assert_eq!(
        scarp::compile("print(ask(8));").unwrap_err().kind,
        Kind::TypeMismatch
    );
    assert_eq!(
        scarp::compile(r#"pure funcra f() -> string { return ask("x"); }"#)
            .unwrap_err()
            .kind,
        Kind::ImpureFunction
    );
    assert_eq!(
        run(
            r#"pure funcra ask(prompt: string) -> string { return prompt; } print(ask("shadow"));"#,
            b""
        ),
        (b"shadow\n".to_vec(), Vec::new())
    );
    let program = scarp::compile(r#"print(ask("Prompt> "));"#).unwrap();
    let signature = program
        .typed_expressions()
        .find_map(|expression| expression.nodes().find_map(|node| node.call_signature))
        .unwrap();
    assert!(matches!(
        program.type_registry().get(signature),
        Some(TypeKind::Function { parameters, result, effect })
            if parameters.as_ref() == [scarp::value::ValueType::String.id()]
                && *result == scarp::value::ValueType::String.id()
                && *effect == FunctionEffect::Impure
    ));
}

#[test]
fn successful_prompts_preserve_normal_stdout_stderr_order_after_evaluation() {
    let result = run(
        r#"print("one"); bind name = ask("> "); eprint("warning"); write("two"); print(name);"#,
        b"Ada\n",
    );
    assert_eq!(result.0, b"> one\ntwoAda\n");
    assert_eq!(result.1, b"warning\n");
}

struct ObservedReader {
    input: Cursor<Vec<u8>>,
    flushed: Rc<Cell<bool>>,
    read_started: Rc<Cell<bool>>,
}

impl Read for ObservedReader {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        self.input.read(bytes)
    }
}

impl BufRead for ObservedReader {
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        assert!(self.flushed.get(), "read started before prompt flush");
        self.read_started.set(true);
        self.input.fill_buf()
    }

    fn consume(&mut self, amount: usize) {
        self.input.consume(amount);
    }
}

struct FlushObserver {
    contents: Vec<u8>,
    flushed: Rc<Cell<bool>>,
}

impl Write for FlushObserver {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.contents.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.flushed.set(true);
        Ok(())
    }
}

#[test]
fn buffered_reader_observes_flushed_prompt_before_first_read() {
    let flushed = Rc::new(Cell::new(false));
    let read_started = Rc::new(Cell::new(false));
    let mut reader = ObservedReader {
        input: Cursor::new(b"Ada\n".to_vec()),
        flushed: Rc::clone(&flushed),
        read_started: Rc::clone(&read_started),
    };
    let mut stdout = FlushObserver {
        contents: Vec::new(),
        flushed: Rc::clone(&flushed),
    };
    let mut stderr = Vec::new();
    scarp::run_with_interactive_input(
        r#"print(ask("Name: "));"#,
        &mut reader,
        &mut stdout,
        &mut stderr,
    )
    .unwrap();
    assert!(read_started.get());
    assert!(flushed.get());
    assert_eq!(stdout.contents, b"Name: Ada\n");
    assert!(stderr.is_empty());
}
