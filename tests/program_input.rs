use std::io::{self, BufRead, BufReader, Cursor, Read};

use scarp::{
    diagnostics::{Diagnostic, DiagnosticKind as Kind},
    interpreter,
    types::{FunctionEffect, TypeKind},
    ExecutionOptions, InputLimits, RunError,
};

fn run(source: &str, bytes: &[u8]) -> (Vec<u8>, Vec<u8>) {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    scarp::run_with_input(source, &mut Cursor::new(bytes), &mut out, &mut err).unwrap();
    (out, err)
}

fn failure(
    source: &str,
    bytes: &[u8],
    options: ExecutionOptions,
    limits: InputLimits,
    kind: Kind,
) -> Diagnostic {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let RunError::Diagnostic(error) = scarp::run_with_input_and_options(
        source,
        &mut Cursor::new(bytes),
        &mut out,
        &mut err,
        options,
        limits,
    )
    .unwrap_err() else {
        panic!("expected a source diagnostic")
    };
    assert_eq!(error.kind, kind, "{error:?}");
    assert!(out.is_empty() && err.is_empty());
    error
}

#[test]
fn lines_preserve_unicode_empty_text_bare_cr_and_unterminated_final_text() {
    for (bytes, expected) in [
        (b"\n".as_slice(), "\n"),
        (b"\r\n".as_slice(), "\n"),
        (b"a\rb\n".as_slice(), "a\rb\n"),
        (b"x\r".as_slice(), "x\r\n"),
        (b"\r".as_slice(), "\r\n"),
        (b"last".as_slice(), "last\n"),
        ("नमस्ते 🌍 e\u{301}\0\r\n".as_bytes(), "नमस्ते 🌍 e\u{301}\0\n"),
    ] {
        assert_eq!(
            run("print(read_line());", bytes),
            (expected.as_bytes().to_vec(), Vec::new())
        );
    }
    assert_eq!(
        run("print(read_line(),read_line(),read_line());", b"a\n\nlast"),
        (b"a  last\n".to_vec(), Vec::new())
    );
}

#[test]
fn eof_invalid_encoding_and_later_failures_are_distinct_and_atomic() {
    let prefix = "print(\"before\"); eprint(\"before\"); ";
    for (suffix, bytes, kind) in [
        ("print(read_line());", b"".as_slice(), Kind::InputEof),
        (
            "print(read_line());",
            b"\xff\n".as_slice(),
            Kind::InputEncoding,
        ),
        (
            "print(read_line(),read_line());",
            b"\n".as_slice(),
            Kind::InputEof,
        ),
        (
            "print(read_line()); print(1/0);",
            b"ok\n".as_slice(),
            Kind::DivisionByZero,
        ),
    ] {
        failure(
            &format!("{prefix}{suffix}"),
            bytes,
            ExecutionOptions::default(),
            InputLimits::default(),
            kind,
        );
    }
}

#[test]
fn reads_follow_selected_paths_short_circuits_and_left_to_right_arguments() {
    let source = r#"
        funcra line() -> string { return read_line(); }
        funcra unused() -> string { return read_line(); }
        bind first: string = line();
        if false { print(read_line()); } elif first == "yes" { print(line()); } else { print("no"); }
        print(false and read_line() == "bad", true or read_line() == "bad");
        var i = 0;
        while i < 2 { i += 1; if i == 1 { continue; } print(line()); }
        repeat 3 as j { if j == 2 { break; } write(line()); }
        print(line(), line());
    "#;
    assert_eq!(
        run(source, b"yes\nbranch\nloop\nA\nB\nC\nD\n"),
        (b"branch\nfalse true\nloop\nABC D\n".to_vec(), Vec::new())
    );
}

#[test]
fn reader_is_not_consumed_on_static_failure_or_unused_input() {
    let mut reader = Cursor::new(b"sentinel\n");
    for source in [
        "print(read_line()); print(missing);",
        "if false { print(read_line()); }",
        "funcra unused() -> string { return read_line(); }",
    ] {
        let mut out = Vec::new();
        let mut err = Vec::new();
        let _ = scarp::run_with_input(source, &mut reader, &mut out, &mut err);
        assert_eq!(reader.position(), 0);
    }
}

#[test]
fn pure_bodies_defaults_contracts_and_constants_cannot_read_even_when_unselected() {
    for source in [
        "pure funcra f() -> string { return read_line(); }",
        "pure funcra f() -> string { if false { return read_line(); } return \"ok\"; }",
        "funcra f(x: string=read_line()) -> string { return x; }",
        "funcra f() -> string needs read_line() == \"x\"; { return \"x\"; }",
        "funcra f() -> string ensures read_line() == result; { return \"x\"; }",
        "funcra impure() -> string { return read_line(); } pure funcra f() -> string { return impure(); }",
    ] {
        assert_eq!(scarp::compile(source).unwrap_err().kind, Kind::ImpureFunction, "{source}");
    }
    let mut loader = scarp::modules::MemorySourceLoader::new();
    loader.insert("main.srp", "bind x = read_line();").unwrap();
    assert!(scarp::modules::compile_with_loader("main.srp", &loader, Default::default()).is_ok());
    for source in ["pub bind x = read_line();", "import \"lib.srp\" as lib;"] {
        let mut loader = scarp::modules::MemorySourceLoader::new();
        loader.insert("main.srp", source).unwrap();
        loader
            .insert("lib.srp", "pub bind x = read_line();")
            .unwrap();
        assert_eq!(
            scarp::modules::compile_with_loader("main.srp", &loader, Default::default())
                .unwrap_err()
                .diagnostic
                .kind,
            Kind::ModuleSyntax
        );
    }
}

#[test]
fn arity_typing_shadowing_and_impure_signature_are_explicit() {
    for source in ["print(read_line(1));", "print(read_line(x: 1));"] {
        assert_eq!(
            scarp::compile(source).unwrap_err().kind,
            Kind::CallArguments
        );
    }
    assert_eq!(
        scarp::compile("bind x: integer=read_line();")
            .unwrap_err()
            .kind,
        Kind::TypeMismatch
    );
    assert_eq!(
        run(
            "pure funcra read_line() -> integer { return 7; } print(read_line());",
            b"unused"
        ),
        (b"7\n".to_vec(), Vec::new())
    );
    let program = scarp::compile("print(read_line());").unwrap();
    let signature = program
        .typed_expressions()
        .find_map(|expression| expression.nodes().find_map(|node| node.call_signature))
        .unwrap();
    assert!(
        matches!(program.type_registry().get(signature), Some(TypeKind::Function { parameters, result, effect }) if parameters.is_empty() && *result == scarp::value::ValueType::String.id() && *effect == FunctionEffect::Impure)
    );
}

#[test]
fn writer_only_and_evaluating_check_apis_fail_without_ambient_input() {
    let source = "print(\"before\"); eprint(read_line());";
    let (mut out, mut err) = (Vec::new(), Vec::new());
    for result in [
        scarp::run(source, &mut out),
        scarp::run_with_io(source, &mut out, &mut err),
    ] {
        assert!(matches!(
            result,
            Err(RunError::Diagnostic(Diagnostic {
                kind: Kind::InputUnavailable,
                ..
            }))
        ));
    }
    for result in [
        scarp::check(source),
        scarp::check_with_limits(source, Default::default()),
        scarp::check_with_options(source, Default::default()),
    ] {
        assert_eq!(result.unwrap_err().kind, Kind::InputUnavailable);
    }
    assert!(out.is_empty() && err.is_empty());
    scarp::check("if false { print(read_line()); }").unwrap();
    assert!(scarp::compile(source).is_ok());
}

#[test]
fn exact_raw_line_total_read_and_text_boundaries_are_enforced() {
    let source = "print(read_line());";
    for bytes in [b"abc".as_slice(), b"ab\n".as_slice(), b"a\r\n".as_slice()] {
        let limits = InputLimits {
            max_line_bytes: 3,
            max_total_bytes: 3,
            max_reads: 1,
        };
        let (mut out, mut err) = (Vec::new(), Vec::new());
        scarp::run_with_input_and_options(
            source,
            &mut Cursor::new(bytes),
            &mut out,
            &mut err,
            ExecutionOptions {
                max_text_bytes: 3,
                ..Default::default()
            },
            limits,
        )
        .unwrap();
        for lower in [
            InputLimits {
                max_line_bytes: 2,
                ..limits
            },
            InputLimits {
                max_total_bytes: 2,
                ..limits
            },
            InputLimits {
                max_reads: 0,
                ..limits
            },
        ] {
            failure(source, bytes, Default::default(), lower, Kind::InputLimit);
        }
        failure(
            source,
            bytes,
            ExecutionOptions {
                max_text_bytes: 2,
                ..Default::default()
            },
            limits,
            Kind::TextConstruction,
        );
    }
    for limits in [
        InputLimits {
            max_total_bytes: 3,
            ..Default::default()
        },
        InputLimits {
            max_reads: 1,
            ..Default::default()
        },
    ] {
        failure(
            "print(read_line(),read_line());",
            b"a\nb\n",
            Default::default(),
            limits,
            Kind::InputLimit,
        );
    }
    let mut input = Cursor::new(b"123456789\n");
    let (mut out, mut err) = (Vec::new(), Vec::new());
    scarp::run_with_input_and_options(
        source,
        &mut input,
        &mut out,
        &mut err,
        Default::default(),
        InputLimits {
            max_line_bytes: 3,
            ..Default::default()
        },
    )
    .unwrap_err();
    assert_eq!(input.position(), 3);
}

struct FaultReader {
    interrupted: bool,
    calls: usize,
}
impl Read for FaultReader {
    fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
        unreachable!()
    }
}
impl BufRead for FaultReader {
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        self.calls += 1;
        Err(io::Error::new(
            if self.interrupted {
                io::ErrorKind::Interrupted
            } else {
                io::ErrorKind::Other
            },
            "host detail must not leak",
        ))
    }
    fn consume(&mut self, _: usize) {
        unreachable!()
    }
}

#[test]
fn reader_errors_have_bounded_diagnostics_and_interrupted_retries_cost_work() {
    for (interrupted, expected) in [(false, Kind::InputIo), (true, Kind::ExecutionLimit)] {
        let mut reader = FaultReader {
            interrupted,
            calls: 0,
        };
        let mut options = ExecutionOptions::default();
        options.limits.max_steps = 30;
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let RunError::Diagnostic(error) = scarp::run_with_input_and_options(
            "print(read_line());",
            &mut reader,
            &mut out,
            &mut err,
            options,
            Default::default(),
        )
        .unwrap_err() else {
            panic!()
        };
        assert_eq!(error.kind, expected);
        assert!(!error.message.contains("host detail"));
        assert!(reader.calls > 0 && reader.calls <= 30);
        assert!(out.is_empty() && err.is_empty());
    }
}

#[test]
fn zero_quotas_and_step_exhaustion_do_not_touch_the_reader() {
    for (limits, steps, kind) in [
        (
            InputLimits {
                max_reads: 0,
                ..Default::default()
            },
            100,
            Kind::InputLimit,
        ),
        (
            InputLimits {
                max_line_bytes: 0,
                ..Default::default()
            },
            100,
            Kind::InputLimit,
        ),
        (
            InputLimits {
                max_total_bytes: 0,
                ..Default::default()
            },
            100,
            Kind::InputLimit,
        ),
        (InputLimits::default(), 0, Kind::ExecutionLimit),
    ] {
        let mut reader = FaultReader {
            interrupted: false,
            calls: 0,
        };
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let mut options = ExecutionOptions::default();
        options.limits.max_steps = steps;
        let RunError::Diagnostic(error) = scarp::run_with_input_and_options(
            "print(read_line());",
            &mut reader,
            &mut out,
            &mut err,
            options,
            limits,
        )
        .unwrap_err() else {
            panic!()
        };
        assert_eq!(error.kind, kind);
        assert_eq!(reader.calls, 0);
    }
}

#[test]
fn fragmented_multibyte_crlf_lines_and_reusable_programs_have_independent_readers() {
    let program = scarp::compile("print(read_line(),read_line());").unwrap();
    std::thread::scope(|scope| {
        for _ in 0..8 {
            let program = &program;
            scope.spawn(move || {
                let mut input = BufReader::with_capacity(1, Cursor::new("é🌍\r\nनमस्ते".as_bytes()));
                let (mut out, mut err) = (Vec::new(), Vec::new());
                interpreter::execute_with_input(program, &mut input, &mut out, &mut err).unwrap();
                assert_eq!(out, "é🌍 नमस्ते\n".as_bytes());
                assert!(err.is_empty());
            });
        }
    });
}

#[test]
fn diagnostics_keep_original_unicode_call_span_and_call_stack() {
    let source = "// 🌍\nfuncra f() -> string { return read_line(); } print(f());";
    let error = failure(
        source,
        b"",
        Default::default(),
        Default::default(),
        Kind::InputEof,
    );
    assert_eq!(&source[error.span.start..error.span.end], "read_line()");
    assert!(!error.related.is_empty());
    assert!(error.render("main.srp", source).contains("main.srp:2:"));
}

#[test]
fn input_values_preserve_all_output_forms_and_collections() {
    let source = r#"funcra f() -> unit { print(); println(read_line(), 1, 2.5, true, [1.0], [integer:2]); write(read_line()); eprint(read_line()); ewrite(read_line()); } f();"#;
    assert_eq!(
        run(source, "नमस्ते\nA\nB\nC\n".as_bytes()),
        (
            "\nनमस्ते 1 2.5 true [1.0] [integer: 2]\nA"
                .as_bytes()
                .to_vec(),
            b"B\nC".to_vec()
        )
    );
}

#[test]
fn oversized_lines_never_consume_past_the_raw_quota_or_bypass_the_string_ceiling() {
    let bytes = vec![b'x'; scarp::value::MAX_TEXT_BYTES + 1];
    let mut input = Cursor::new(&bytes);
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let mut options = ExecutionOptions::default();
    options.limits.max_steps = 20_000_000;
    let RunError::Diagnostic(error) = scarp::run_with_input_and_options(
        "print(read_line());",
        &mut input,
        &mut out,
        &mut err,
        options,
        InputLimits {
            max_line_bytes: usize::MAX,
            max_total_bytes: usize::MAX,
            max_reads: usize::MAX,
        },
    )
    .unwrap_err() else {
        panic!()
    };
    assert_eq!(error.kind, Kind::InputLimit);
    assert_eq!(input.position(), scarp::value::MAX_TEXT_BYTES as u64);
    assert!(out.is_empty() && err.is_empty());
    let mut input = BufReader::with_capacity(1, Cursor::new(vec![b'x'; 32_768]));
    scarp::run_with_input(
        "print(text_len(read_line()));",
        &mut input,
        &mut out,
        &mut err,
    )
    .unwrap();
    assert_eq!(out, b"32768\n");
}

#[test]
fn both_native_backends_explicitly_reject_input_without_reading_it() {
    for source in ["print(read_line());", "bind value = read_line();"] {
        assert_eq!(
            scarp::c_backend::emit_c(source).unwrap_err().kind,
            Kind::NativeUnsupported
        );
        assert_eq!(
            scarp::native_object::emit_elf64_integer_kernel(source)
                .unwrap_err()
                .kind,
            Kind::NativeUnsupported
        );
    }
}
