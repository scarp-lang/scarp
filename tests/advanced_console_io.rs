use std::{
    cell::RefCell,
    io::{self, BufRead, Cursor, Read, Write},
    rc::Rc,
};

use scarp::{
    diagnostics::{Diagnostic, DiagnosticKind as Kind},
    types::{FunctionEffect, TypeKind},
    value::ValueType,
    ExecutionOptions, InputLimits, RunError,
};

fn diagnostic(error: RunError) -> Diagnostic {
    match error {
        RunError::Diagnostic(diagnostic) => diagnostic,
        other => panic!("expected language diagnostic, got {other}"),
    }
}

#[test]
fn eof_empty_and_final_lines_have_distinct_results() {
    let source = r#"
        bind empty: tuple<boolean, string> = try_read_line();
        bind line = try_read_line();
        bind final_line = try_read_line();
        assert(empty[0] and empty[1] == "");
        assert(line[0] and line[1] == "नमस्ते café 🌍");
        assert(final_line[0] and final_line[1] == "tail\r");
        assert(try_read_line() == (false, ""));
        assert(try_read_line() == (false, ""));
        print(line[1]);
    "#;
    let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
    scarp::run_with_input(
        source,
        &mut Cursor::new("\r\nनमस्ते café 🌍\r\ntail\r"),
        &mut stdout,
        &mut stderr,
    )
    .unwrap();
    assert_eq!(stdout, "नमस्ते café 🌍\n".as_bytes());
    assert!(stderr.is_empty());
}

struct BrokenReader;
impl Read for BrokenReader {
    fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
        Err(io::Error::other("private reader detail"))
    }
}
impl BufRead for BrokenReader {
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        Err(io::Error::other("private reader detail"))
    }
    fn consume(&mut self, _: usize) {}
}

#[test]
fn eof_api_propagates_reader_encoding_and_resource_failures() {
    let source = "print(try_read_line());";
    let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
    let error = scarp::run_with_io(source, &mut stdout, &mut stderr).unwrap_err();
    assert_eq!(diagnostic(error).kind, Kind::InputUnavailable);
    let error = diagnostic(
        scarp::run_with_input(source, &mut BrokenReader, &mut stdout, &mut stderr).unwrap_err(),
    );
    assert_eq!(error.kind, Kind::InputIo);
    assert!(!error.message.contains("private reader detail"));
    let error = scarp::run_with_input(source, &mut Cursor::new(b"\xff"), &mut stdout, &mut stderr)
        .unwrap_err();
    assert_eq!(diagnostic(error).kind, Kind::InputEncoding);
    let program = scarp::compile(source).unwrap();
    for (options, limits, expected) in [
        (
            ExecutionOptions {
                max_tuple_bytes: 0,
                ..Default::default()
            },
            InputLimits::default(),
            Kind::TupleAllocation,
        ),
        (
            ExecutionOptions::default(),
            InputLimits {
                max_reads: 0,
                ..Default::default()
            },
            Kind::InputLimit,
        ),
    ] {
        let error = scarp::interpreter::execute_with_input_and_options(
            &program,
            &mut Cursor::new(b""),
            &mut stdout,
            &mut stderr,
            options,
            limits,
        )
        .unwrap_err();
        assert_eq!(diagnostic(error).kind, expected);
    }
    assert!(stdout.is_empty() && stderr.is_empty());
}

#[test]
fn typed_input_preserves_types_values_and_prompt_order() {
    let source = r#"
        bind name: string = input("नाम> ");
        bind low: integer = input_integer("Integer> ");
        bind rate: float = input_float("Float> ");
        bind large: bigint = input_bigint("Bigint> ");
        assert(low == -9223372036854775807 - 1);
        assert(rate == 0.125);
        assert(large + bigint("1") == bigint("-999999999999999999999999999998"));
        print(name, low, rate, large);
    "#;
    let data = "Zoë 🌍\r\n-9223372036854775808\r\n+1.25e-1\n-999999999999999999999999999999";
    let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
    scarp::run_with_interactive_input(source, &mut Cursor::new(data), &mut stdout, &mut stderr)
        .unwrap();
    assert_eq!(
        stdout,
        "नाम> Integer> Float> Bigint> Zoë 🌍 -9223372036854775808 0.125 -999999999999999999999999999999\n".as_bytes()
    );
    assert!(stderr.is_empty());
}

#[test]
fn errors_identify_the_typed_call_without_exposing_invalid_input() {
    for (name, data, kind) in [
        (
            "input_integer",
            "9223372036854775808\n",
            Kind::NumericConversion,
        ),
        ("input_integer", " 42\n", Kind::NumericConversion),
        ("input_integer", "0xAB\n", Kind::NumericConversion),
        ("input_integer", "private marker\n", Kind::NumericConversion),
        ("input_float", "1e9999\n", Kind::NumericConversion),
        ("input_float", "NaN\n", Kind::NumericConversion),
        ("input_float", "1.\n", Kind::NumericConversion),
        ("input_bigint", "१२३\n", Kind::InvalidBigInt),
        ("input_bigint", "1_000\n", Kind::InvalidBigInt),
    ] {
        let call = format!(r#"{name}("value> ")"#);
        let source = format!(r#"print("buffered"); print({call});"#);
        let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
        let error = diagnostic(
            scarp::run_with_interactive_input(
                &source,
                &mut Cursor::new(data),
                &mut stdout,
                &mut stderr,
            )
            .unwrap_err(),
        );
        assert_eq!(error.kind, kind);
        assert!(error.message.contains(name));
        assert!(!error.message.contains(data.trim()));
        assert_eq!(&source[error.span.start..error.span.end], call);
        assert_eq!(stdout, b"value> ");
        assert!(stderr.is_empty());
    }
}

#[test]
fn nested_calls_share_input_and_bigint_quotas() {
    let source =
        r#"funcra value() -> bigint { return input_bigint("Value> "); } print(value(), value());"#;
    for (options, limits, kind) in [
        (
            ExecutionOptions {
                max_bigint_digits: 4,
                ..Default::default()
            },
            InputLimits::default(),
            Kind::BigIntLimit,
        ),
        (
            ExecutionOptions::default(),
            InputLimits {
                max_reads: 1,
                ..Default::default()
            },
            Kind::InputLimit,
        ),
    ] {
        let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
        let error = diagnostic(
            scarp::run_with_interactive_input_and_options(
                source,
                &mut Cursor::new(b"12345\n1\n"),
                &mut stdout,
                &mut stderr,
                options,
                limits,
            )
            .unwrap_err(),
        );
        assert_eq!(error.kind, kind);
        assert_eq!(stdout, b"Value> ");
        assert!(stderr.is_empty());
    }
}

#[test]
fn prompt_alias_is_explicit_and_supports_shadowing() {
    let mut reader = Cursor::new(b"x\n");
    let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
    let error = scarp::run_with_input(
        r#"print(input("Name> "));"#,
        &mut reader,
        &mut stdout,
        &mut stderr,
    )
    .unwrap_err();
    assert_eq!(diagnostic(error).kind, Kind::InputUnavailable);
    assert_eq!(reader.position(), 0);
    assert!(stdout.is_empty() && stderr.is_empty());
    scarp::run_with_io(
        r#"pure funcra input(value: string) -> string { return value; } print(input("local"));"#,
        &mut stdout,
        &mut stderr,
    )
    .unwrap();
    assert_eq!(stdout, b"local\n");
}

#[test]
fn type_metadata_reports_each_result_and_impurity() {
    let cases = [
        ("try_read_line()", None),
        (r#"input("x")"#, Some(ValueType::String)),
        (r#"input_integer("x")"#, Some(ValueType::Integer)),
        (r#"input_float("x")"#, Some(ValueType::Float)),
        (r#"input_bigint("x")"#, Some(ValueType::BigInt)),
        ("flush_stdout()", Some(ValueType::Unit)),
        ("flush_stderr()", Some(ValueType::Unit)),
    ];
    for (call, primitive) in cases {
        let statement = if primitive == Some(ValueType::Unit) {
            format!("{call};")
        } else {
            format!("bind result = {call};")
        };
        let program = scarp::compile(&statement).unwrap();
        let signature = program
            .typed_expressions()
            .find_map(|expression| expression.nodes().find_map(|node| node.call_signature))
            .unwrap();
        let TypeKind::Function {
            parameters,
            result,
            effect,
        } = program.type_registry().get(signature).unwrap()
        else {
            panic!("{signature:?}");
        };
        assert_eq!(*effect, FunctionEffect::Impure);
        if call.contains("\"x\"") {
            assert_eq!(parameters.as_ref(), [ValueType::String.id()]);
        } else {
            assert!(parameters.is_empty());
        }
        if let Some(primitive) = primitive {
            assert_eq!(*result, primitive.id());
        } else {
            assert!(matches!(program.type_registry().get(*result),
                Some(TypeKind::Tuple(fields)) if fields.as_ref() ==
                    [ValueType::Boolean.id(), ValueType::String.id()]));
        }
        let pure = format!("pure funcra run() -> integer {{ {statement} return 1; }}");
        assert_eq!(
            scarp::compile(&pure).unwrap_err().kind,
            Kind::ImpureFunction,
            "{pure}"
        );
    }
}

type Trace = Rc<RefCell<Vec<(&'static str, Vec<u8>)>>>;

#[derive(Clone)]
struct RecordingWriter {
    stream: &'static str,
    trace: Trace,
    fail_flush: bool,
}
impl Write for RecordingWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.trace.borrow_mut().push((self.stream, bytes.to_vec()));
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        self.trace.borrow_mut().push((self.stream, Vec::new()));
        if self.fail_flush {
            return Err(io::Error::other("private writer detail"));
        }
        Ok(())
    }
}

#[test]
fn interactive_flushes_publish_only_their_stream_even_if_execution_fails() {
    let trace = Trace::default();
    let mut stdout = RecordingWriter {
        stream: "out",
        trace: Rc::clone(&trace),
        fail_flush: false,
    };
    let mut stderr = RecordingWriter {
        stream: "err",
        ..stdout.clone()
    };
    let source = r#"
        write("a"); ewrite("b"); write("c"); ewrite("d");
        flush_stdout(); flush_stdout(); write("buffered"); flush_stderr();
        ewrite("also buffered"); bind impossible = 1 / 0;
    "#;
    assert!(scarp::run_with_interactive_input(
        source,
        &mut Cursor::new(b""),
        &mut stdout,
        &mut stderr
    )
    .is_err());
    assert_eq!(
        *trace.borrow(),
        [
            ("out", b"a".to_vec()),
            ("out", b"c".to_vec()),
            ("out", vec![]),
            ("out", vec![]),
            ("err", b"b".to_vec()),
            ("err", b"d".to_vec()),
            ("err", vec![])
        ]
    );
    trace.borrow_mut().clear();
    assert!(scarp::run_with_io(source, &mut stdout, &mut stderr).is_err());
    assert!(trace.borrow().is_empty());
    scarp::run_with_io(
        r#"write("a"); flush_stdout(); ewrite("b"); flush_stderr(); write("c");"#,
        &mut stdout,
        &mut stderr,
    )
    .unwrap();
    assert_eq!(
        *trace.borrow(),
        [
            ("out", b"a".to_vec()),
            ("out", vec![]),
            ("err", b"b".to_vec()),
            ("err", vec![]),
            ("out", b"c".to_vec())
        ]
    );
}

#[test]
fn explicit_interactive_flush_errors_are_source_located() {
    let trace = Trace::default();
    let mut stdout = Vec::new();
    let mut stderr = RecordingWriter {
        stream: "err",
        trace: Rc::clone(&trace),
        fail_flush: true,
    };
    let source = r#"write("buffered"); ewrite("visible"); flush_stderr();"#;
    let failure = diagnostic(
        scarp::run_with_interactive_input(source, &mut Cursor::new(b""), &mut stdout, &mut stderr)
            .unwrap_err(),
    );
    assert_eq!(failure.kind, Kind::StreamFlush);
    assert_eq!(
        &source[failure.span.start..failure.span.end],
        "flush_stderr()"
    );
    assert!(!failure.message.contains("private writer detail"));
    assert!(stdout.is_empty());
    assert_eq!(
        *trace.borrow(),
        [("err", b"visible".to_vec()), ("err", vec![])]
    );
}

#[test]
fn flush_calls_share_print_and_capture_budgets() {
    let program = scarp::compile("flush_stdout(); flush_stderr();").unwrap();
    let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
    for (options, kind) in [
        (
            ExecutionOptions {
                limits: scarp::ExecutionLimits {
                    max_prints: 1,
                    ..Default::default()
                },
                ..Default::default()
            },
            Kind::OutputLimit,
        ),
        (
            ExecutionOptions {
                max_capture_bytes: 0,
                ..Default::default()
            },
            Kind::CaptureLimit,
        ),
    ] {
        let failure = scarp::interpreter::execute_with_io_and_options(
            &program,
            &mut stdout,
            &mut stderr,
            options,
        )
        .unwrap_err();
        assert_eq!(diagnostic(failure).kind, kind);
    }
    assert!(stdout.is_empty() && stderr.is_empty());
}

#[test]
fn interactive_examples_match_golden_output_via_the_cli() {
    use std::{fs, path::Path, process::Command};
    let examples = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/interactive");
    for example in ["typed_calculator", "exact_integer"] {
        let source = examples.join(format!("{example}.srp"));
        let expected = fs::read(source.with_extension("stdout")).unwrap();
        let verify = Command::new(env!("CARGO_BIN_EXE_scarp"))
            .arg("verify")
            .arg(&source)
            .output()
            .unwrap();
        assert!(verify.status.success(), "{verify:?}");
        let result = Command::new(env!("CARGO_BIN_EXE_scarp"))
            .args(["exec", "--interactive", "--input-file"])
            .arg(source.with_extension("stdin"))
            .arg(&source)
            .output()
            .unwrap();
        assert!(result.status.success(), "{result:?}");
        assert!(result.stderr.is_empty(), "{result:?}");
        assert_eq!(result.stdout, expected, "{example}: {result:?}");
    }
}

#[test]
fn both_native_backends_reject_the_new_io_calls() {
    for source in [
        "bind line = try_read_line();",
        r#"bind number = input_integer("Value> ");"#,
        "flush_stdout();",
    ] {
        assert_eq!(
            scarp::c_backend::emit_c(source).unwrap_err().kind,
            Kind::NativeUnsupported,
        );
        assert_eq!(
            scarp::native_object::emit_elf64_integer_kernel(source)
                .unwrap_err()
                .kind,
            Kind::NativeUnsupported,
        );
    }
}

#[test]
fn named_or_non_string_prompt_arguments_are_rejected() {
    for (source, kind) in [
        (r#"input(prompt: "x");"#, Kind::CallArguments),
        ("input_integer(1);", Kind::TypeMismatch),
        (r#"try_read_line("x");"#, Kind::CallArguments),
        ("flush_stderr(1);", Kind::CallArguments),
        (
            r#"funcra f(x: string = input("x")) -> string { return x; }"#,
            Kind::ImpureFunction,
        ),
    ] {
        assert_eq!(scarp::compile(source).unwrap_err().kind, kind, "{source}");
    }
}
