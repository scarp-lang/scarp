use scarp::{
    diagnostics::{DiagnosticKind as Kind, Span},
    numeric::{self, Builtin},
    value::{FiniteFloat, Value, MAX_TEXT_BYTES},
    ExecutionLimits, ExecutionOptions, RunError,
};
fn f(n: f64) -> Value {
    Value::Float(FiniteFloat::new(n).unwrap())
}
fn host(function: Builtin, args: &[Value]) -> Value {
    numeric::evaluate(function, args, u64::MAX).unwrap()
}
fn output(source: &str) -> String {
    let mut out = Vec::new();
    scarp::run(source, &mut out).unwrap();
    String::from_utf8(out).unwrap()
}
fn atomic(source: &str, options: ExecutionOptions) -> scarp::diagnostics::Diagnostic {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let RunError::Diagnostic(error) =
        scarp::run_with_io_and_options(source, &mut out, &mut err, options).unwrap_err()
    else {
        panic!("expected diagnostic")
    };
    assert!(out.is_empty() && err.is_empty());
    assert_eq!(
        scarp::check_with_options(source, options).unwrap_err(),
        error
    );
    error
}

#[test]
fn decimal_integer_conversion_is_explicit_and_exact() {
    for n in [
        i64::MIN,
        -9999999999999999,
        -1,
        0,
        1,
        9999999999999999,
        i64::MAX,
    ] {
        let text = host(Builtin::IntegerToText, &[Value::Integer(n)]);
        assert_eq!(text.as_str(), Some(n.to_string().as_str()));
        assert_eq!(host(Builtin::TextToInteger, &[text]), Value::Integer(n));
    }
    for (text, n) in [("+00042", 42), ("-00042", -42), ("-0", 0), ("000", 0)] {
        assert_eq!(
            host(Builtin::TextToInteger, &[Value::from(text)]),
            Value::Integer(n)
        );
    }
}

#[test]
fn integer_text_rejects_overflow_and_non_decimal_input() {
    for text in [
        "",
        "+",
        "-",
        " 1",
        "1 ",
        "1.0",
        "1e2",
        "0xff",
        "0b10",
        "1_000",
        "१२",
        "١",
        "1\0",
        "9223372036854775808",
        "-9223372036854775809",
    ] {
        assert_eq!(
            numeric::evaluate(Builtin::TextToInteger, &[Value::from(text)], u64::MAX)
                .unwrap_err()
                .kind,
            Kind::NumericConversion,
            "{text:?}"
        );
    }
}

#[test]
fn finite_float_text_round_trips_bits_including_signed_zero_and_subnormals() {
    for n in [
        -f64::MAX,
        -1e100,
        -1e-100,
        -0.0,
        0.0,
        f64::from_bits(1),
        f64::MIN_POSITIVE,
        0.1,
        1.0,
        1e100,
        f64::MAX,
    ] {
        let text = host(Builtin::FloatToText, &[f(n)]);
        assert_eq!(text.as_str(), Some(format!("{n:?}").as_str()));
        let actual = host(Builtin::TextToFloat, &[text]).as_float().unwrap();
        assert_eq!(actual.to_bits(), n.to_bits(), "{n:?}");
    }
    assert_eq!(host(Builtin::TextToFloat, &[Value::from("+001")]), f(1.0));
    assert_eq!(
        host(Builtin::TextToFloat, &[Value::from("1e-4000")]),
        f(0.0)
    );
    assert_eq!(
        host(Builtin::TextToFloat, &[Value::from("-1e-4000")])
            .as_float()
            .unwrap()
            .to_bits(),
        (-0.0_f64).to_bits()
    );
}

#[test]
fn float_parse_uses_complete_finite_ascii_decimal_grammar() {
    for text in [
        "",
        "+",
        ".5",
        "1.",
        "1e",
        "1e+",
        "1e-",
        "1e2e3",
        "1..0",
        "NaN",
        "inf",
        "Infinity",
        "-inf",
        "1e309",
        "-1e9999",
        " 1.0",
        "1.0 ",
        "0x1",
        "1_2.0",
        "1,2",
        "१२.५",
        "1.0é",
    ] {
        assert_eq!(
            numeric::evaluate(Builtin::TextToFloat, &[Value::from(text)], u64::MAX)
                .unwrap_err()
                .kind,
            Kind::NumericConversion,
            "{text:?}"
        );
    }
    for text in ["1", "01", "-1.5", "+1.5", "1e2", "1E+2", "1.5e-2", "-0.0"] {
        assert_eq!(
            host(Builtin::TextToFloat, &[Value::from(text)])
                .as_float()
                .unwrap()
                .to_bits(),
            text.parse::<f64>().unwrap().to_bits()
        );
    }
}

#[test]
fn seeded_scalar_round_trips_preserve_full_range_and_nan_never_enters_values() {
    let mut seed = 0x7363617270746578_u64;
    let mut finite = 0;
    for _ in 0..10000 {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        let integer = Value::Integer(seed as i64);
        assert_eq!(
            host(
                Builtin::TextToInteger,
                &[host(Builtin::IntegerToText, std::slice::from_ref(&integer))]
            ),
            integer
        );
        let n = f64::from_bits(seed);
        if n.is_finite() {
            finite += 1;
            let text = host(Builtin::FloatToText, &[f(n)]);
            let actual = host(Builtin::TextToFloat, &[text]).as_float().unwrap();
            assert_eq!(actual.to_bits(), n.to_bits());
        }
    }
    assert!(finite > 9900);
    assert!(FiniteFloat::new(f64::NAN).is_none());
}

#[test]
fn unicode_trim_preserves_inner_text_combining_marks_and_non_whitespace() {
    for (text, expected) in [
        ("", ""),
        (" \t\n\r", ""),
        ("\u{2003} café 🌍 \u{3000}", "café 🌍"),
        ("  e\u{301}  ", "e\u{301}"),
        ("\u{feff}text\u{200b}", "\u{feff}text\u{200b}"),
        (" a  b ", "a  b"),
    ] {
        assert_eq!(
            host(Builtin::TextTrim, &[Value::from(text)]).as_str(),
            Some(expected)
        );
    }
}

#[test]
fn replacement_is_literal_non_overlapping_and_unicode_safe() {
    for (text, pattern, replacement, expected) in [
        ("aaaaa", "aa", "b", "bba"),
        ("café café", "é", "🌍", "caf🌍 caf🌍"),
        ("abcabc", "abc", "", ""),
        ("a\0b\0", "\0", "!", "a!b!"),
        ("e\u{301} é", "é", "E", "e\u{301} E"),
        ("abc", "x", "long", "abc"),
        ("a", "a", "a", "a"),
    ] {
        assert_eq!(
            host(
                Builtin::TextReplace,
                &[
                    Value::from(text),
                    Value::from(pattern),
                    Value::from(replacement)
                ]
            )
            .as_str(),
            Some(expected)
        );
    }
    assert_eq!(
        numeric::evaluate(
            Builtin::TextReplace,
            &[Value::from("x"), Value::from(""), Value::from("y")],
            100
        )
        .unwrap_err()
        .kind,
        Kind::NumericDomain
    );
}

#[test]
fn identity_operations_share_existing_storage_without_payload_charge() {
    for (function, args) in [
        (Builtin::TextTrim, vec![Value::from("unchanged")]),
        (
            Builtin::TextReplace,
            vec![Value::from("abc"), Value::from("x"), Value::from("long")],
        ),
        (
            Builtin::TextReplace,
            vec![Value::from("abc"), Value::from("b"), Value::from("b")],
        ),
    ] {
        let result = numeric::evaluate_with_text_limit(function, &args, 1000, 0).unwrap();
        let (Value::String(before), Value::String(after)) = (&args[0], result) else {
            panic!("strings")
        };
        assert!(std::sync::Arc::ptr_eq(before, &after));
    }
}

#[test]
fn formatting_and_parsing_obey_work_and_text_boundaries() {
    let args = [Value::Integer(123)];
    assert_eq!(
        numeric::evaluate_with_text_limit(Builtin::IntegerToText, &args, 67, 3).unwrap(),
        Value::from("123")
    );
    assert_eq!(
        numeric::evaluate_with_text_limit(Builtin::IntegerToText, &args, 66, 3)
            .unwrap_err()
            .kind,
        Kind::ExecutionLimit
    );
    assert_eq!(
        numeric::evaluate_with_text_limit(Builtin::IntegerToText, &args, 67, 2)
            .unwrap_err()
            .kind,
        Kind::TextConstruction
    );
    assert_eq!(
        numeric::evaluate_with_text_limit(Builtin::TextToInteger, &[Value::from("123")], 3, 0)
            .unwrap(),
        Value::Integer(123)
    );
    assert_eq!(
        numeric::evaluate(Builtin::TextToInteger, &[Value::from("123")], 2)
            .unwrap_err()
            .kind,
        Kind::ExecutionLimit
    );
    assert_eq!(
        numeric::evaluate(Builtin::TextTrim, &[Value::from(" abc ")], 7)
            .unwrap_err()
            .kind,
        Kind::ExecutionLimit
    );
}

#[test]
fn replacement_checks_result_size_and_cumulative_payload_before_copying() {
    let args = [Value::from("aa"), Value::from("a"), Value::from("🌍")];
    assert!(numeric::evaluate_with_text_limit(Builtin::TextReplace, &args, 100, 8).is_ok());
    assert_eq!(
        numeric::evaluate_with_text_limit(Builtin::TextReplace, &args, 100, 7)
            .unwrap_err()
            .kind,
        Kind::TextConstruction
    );
    let huge = "x".repeat(MAX_TEXT_BYTES / 2 + 1);
    assert_eq!(
        numeric::evaluate(
            Builtin::TextReplace,
            &[Value::from("aa"), Value::from("a"), Value::from(huge)],
            u64::MAX
        )
        .unwrap_err()
        .kind,
        Kind::TextConstruction
    );
    let source = "print(integer_to_text(1)); print(integer_to_text(2));";
    assert_eq!(
        atomic(
            source,
            ExecutionOptions {
                max_text_bytes: 1,
                ..ExecutionOptions::default()
            }
        )
        .kind,
        Kind::TextConstruction
    );
}

#[test]
fn argument_spans_and_failed_conversion_discard_both_streams() {
    for (expression, argument, kind) in [
        (
            "text_to_integer(\"12é\")",
            "\"12é\"",
            Kind::NumericConversion,
        ),
        (
            "text_to_float(\"1e9999\")",
            "\"1e9999\"",
            Kind::NumericConversion,
        ),
        (
            "text_replace(\"abc\",\"\",\"x\")",
            "\"\"",
            Kind::NumericDomain,
        ),
    ] {
        let source = format!("print(\"हिन्दी\"); eprint(\"pending\"); print({expression});");
        let error = atomic(&source, ExecutionOptions::default());
        assert_eq!(error.kind, kind);
        assert_eq!(&source[error.span.start..error.span.end], argument);
    }
}

#[test]
fn typed_calls_preserve_purity_mutability_contracts_and_existing_conversions() {
    let source = r#"
        pure funcra parse(n:string=" 42 ")->integer ensures result>0; {
            return text_to_integer(text_trim(n));
        }
        var text:string=integer_to_text(parse());
        bind before=text; text=text_replace(text,"2","3");
        print(before,text,to_integer(text_to_float("42.0")),float_to_text(-0.0));
    "#;
    assert_eq!(output(source), "42 43 42 -0.0\n");
    for source in [
        "print(integer_to_text(1.0));",
        "print(float_to_text(1));",
        "print(text_to_integer(1));",
        "print(text_replace(\"a\",1,\"b\"));",
        "print(text_trim(\"a\",\"b\"));",
    ] {
        assert!(matches!(
            scarp::compile(source).unwrap_err().kind,
            Kind::TypeMismatch | Kind::CallArguments
        ));
    }
    assert_eq!(output("pure funcra text_to_integer(s:integer)->integer{return s+1;} print(text_to_integer(2));"),"3\n");
}

#[test]
fn static_checking_does_not_execute_decimal_parsing() {
    assert!(scarp::compile("print(text_to_integer(\"bad\"));").is_ok());
    assert_eq!(
        scarp::check("print(text_to_integer(\"bad\"));")
            .unwrap_err()
            .kind,
        Kind::NumericConversion
    );
    assert!(scarp::check("if false { print(text_to_integer(\"bad\")); }").is_ok());
    assert_eq!(
        atomic(
            "print(integer_to_text(42));",
            ExecutionOptions {
                limits: ExecutionLimits {
                    max_steps: 10,
                    ..ExecutionLimits::default()
                },
                ..ExecutionOptions::default()
            }
        )
        .kind,
        Kind::ExecutionLimit
    );
}

#[test]
fn old_integer_float_and_boolean_rules_are_not_coerced() {
    for source in [
        "print(1+1.0);",
        "if 1 { print(1); }",
        "print(\"1\"==1);",
        "print(to_float(9007199254740993));",
    ] {
        assert!(scarp::check(source).is_err());
    }
    assert_eq!(
        scarp::check("print(9223372036854775807+1);")
            .unwrap_err()
            .kind,
        Kind::ArithmeticOverflow
    );
    assert_eq!(
        numeric::evaluate(Builtin::TextToInteger, &[Value::Integer(1)], 100)
            .unwrap_err()
            .span,
        Span::default()
    );
}

#[test]
fn native_backends_reject_new_runtime_string_calls_explicitly() {
    let source = "print(integer_to_text(42));";
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
