use scarp::{
    diagnostics::{Diagnostic, DiagnosticKind as Kind},
    numeric::{self, Builtin as B},
    value::{Value, ValueType},
    ExecutionOptions, RunError,
};

fn host(function: B, args: &[Value], steps: u64, text_bytes: usize) -> Result<Value, Diagnostic> {
    numeric::evaluate_with_text_limit(function, args, steps, text_bytes)
}

fn output(source: &str) -> String {
    let mut bytes = Vec::new();
    scarp::run_with_options(source, &mut bytes, ExecutionOptions::default()).unwrap();
    String::from_utf8(bytes).unwrap()
}

#[test]
fn new_intrinsics_have_checked_public_names_and_types() {
    for (name, kind, parameters) in [
        (
            "text_from_codepoint",
            B::TextFromCodepoint,
            &[ValueType::Integer][..],
        ),
        (
            "text_rfind",
            B::TextRFind,
            &[ValueType::String, ValueType::String][..],
        ),
        (
            "text_count",
            B::TextCount,
            &[ValueType::String, ValueType::String][..],
        ),
    ] {
        assert_eq!(B::from_name(name), Some(kind));
        assert_eq!(kind.parameter_types(), parameters);
        let expected_result = if kind == B::TextFromCodepoint {
            ValueType::String
        } else {
            ValueType::Integer
        };
        assert_eq!(kind.result_type(), expected_result);
        assert_eq!(
            numeric::evaluate(kind, &[], 100).unwrap_err().kind,
            Kind::CallArguments
        );
    }
    for source in [
        "print(text_from_codepoint(true));",
        "print(text_rfind(1,\"x\"));",
        "print(text_count(\"x\",1));",
        "print(text_rfind(\"x\"));",
        "bind x: integer=text_from_codepoint(65);",
    ] {
        assert!(matches!(
            scarp::compile(source).unwrap_err().kind,
            Kind::TypeMismatch | Kind::CallArguments
        ));
    }
}

#[test]
fn codepoint_constructor_inverts_existing_unicode_scalar_query() {
    for codepoint in [
        0, 1, 9, 10, 65, 127, 128, 255, 0x7ff, 0x800, 0x301, 0xd7ff, 0xe000, 0xffff, 0x10000,
        0x1f30d, 0x10ffff,
    ] {
        let scalar = char::from_u32(codepoint).unwrap();
        let expected = scalar.to_string();
        let generated = host(
            B::TextFromCodepoint,
            &[Value::Integer(i64::from(codepoint))],
            5,
            4,
        )
        .unwrap();
        assert_eq!(generated.as_str(), Some(expected.as_str()));
        assert_eq!(
            host(B::TextCodepoint, &[generated, Value::Integer(0)], 4, 0).unwrap(),
            Value::Integer(i64::from(codepoint))
        );
    }
}

#[test]
fn invalid_unicode_values_reject_surrogates_negative_and_overflow() {
    for scalar in [
        -1,
        i64::MIN,
        0xd800,
        0xdbff,
        0xdc00,
        0xdfff,
        0x110000,
        i64::MAX,
    ] {
        let error = host(
            B::TextFromCodepoint,
            &[Value::Integer(scalar)],
            u64::MAX,
            usize::MAX,
        )
        .unwrap_err();
        assert_eq!(error.kind, Kind::NumericDomain);
        assert!(error.help.unwrap().contains("surrogates"));
    }
    let source = "print(text_from_codepoint(55296));";
    let error = scarp::check(source).unwrap_err();
    assert_eq!(error.kind, Kind::NumericDomain);
    assert_eq!(&source[error.span.start..error.span.end], "55296");
    assert!(scarp::compile(source).is_ok()); // Static verification does not run it.
}

#[test]
fn reverse_find_and_count_report_unicode_scalar_positions() {
    for (text, needle, rfind, count) in [
        ("", "", 0, 1),
        ("", "a", -1, 0),
        ("a", "", 1, 2),
        ("aé🌍é", "é", 3, 2),
        ("aé🌍é", "🌍", 2, 1),
        ("aé🌍é", "É", -1, 0),
        ("e\u{301}é", "é", 2, 1),
        ("e\u{301}é", "e\u{301}", 0, 1),
        ("aaaa", "aa", 2, 2),
        ("aaa", "aa", 1, 1),
        ("abababa", "aba", 4, 2),
        ("a\0b\0", "\0", 3, 2),
        ("é🌍", "", 2, 3),
        ("é🌍", "🌍é", -1, 0),
    ] {
        let args = [Value::from(text), Value::from(needle)];
        assert_eq!(
            host(B::TextRFind, &args, u64::MAX, 0).unwrap(),
            Value::Integer(rfind),
            "rfind text={text:?} needle={needle:?}"
        );
        assert_eq!(
            host(B::TextCount, &args, u64::MAX, 0).unwrap(),
            Value::Integer(count),
            "count text={text:?} needle={needle:?}"
        );
    }
}

fn oracle(text: &str, needle: &str) -> (i64, i64) {
    let haystack: Vec<char> = text.chars().collect();
    let pattern: Vec<char> = needle.chars().collect();
    if pattern.is_empty() {
        return (haystack.len() as i64, haystack.len() as i64 + 1);
    }
    if pattern.len() > haystack.len() {
        return (-1, 0);
    }
    let mut last = -1;
    let mut cursor = 0;
    let mut count = 0;
    while cursor + pattern.len() <= haystack.len() {
        if haystack[cursor..cursor + pattern.len()] == pattern {
            count += 1;
            cursor += pattern.len();
        } else {
            cursor += 1;
        }
    }
    for start in 0..=haystack.len() - pattern.len() {
        if haystack[start..start + pattern.len()] == pattern {
            last = start as i64;
        }
    }
    (last, count)
}

#[test]
fn seeded_independent_scalar_oracle_covers_unicode_combinations() {
    let alphabet = ['a', 'é', '🌍', '\u{301}', '\0', 'a', 'ब', 'ø'];
    let mut seed = 0x243f_6a88_u64;
    for _ in 0..256 {
        let mut text = String::new();
        for _ in 0..(seed % 27) {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            text.push(alphabet[(seed as usize) % alphabet.len()]);
        }
        for needle in ["", "a", "é", "🌍", "aé", "é🌍", "\u{301}", "\0", "ब", "aa"] {
            let args = [Value::from(text.as_str()), Value::from(needle)];
            let (last, count) = oracle(&text, needle);
            assert_eq!(
                host(B::TextRFind, &args, u64::MAX, 0).unwrap(),
                Value::Integer(last),
                "last {text:?}, {needle:?}"
            );
            assert_eq!(
                host(B::TextCount, &args, u64::MAX, 0).unwrap(),
                Value::Integer(count),
                "count {text:?}, {needle:?}"
            );
        }
    }
}

#[test]
fn exact_work_limits_apply_before_search_and_unicode_materialization() {
    let args = [Value::from("aé🌍"), Value::from("🌍")];
    // Seven haystack bytes, four needle bytes: 7*4 + 7 + 4 = 39.
    assert_eq!(host(B::TextRFind, &args, 39, 0).unwrap(), Value::Integer(2));
    assert_eq!(host(B::TextCount, &args, 39, 0).unwrap(), Value::Integer(1));
    for function in [B::TextRFind, B::TextCount] {
        assert_eq!(
            host(function, &args, 38, 0).unwrap_err().kind,
            Kind::ExecutionLimit
        );
        let empty = [Value::from("é🌍"), Value::from("")];
        assert!(host(function, &empty, 6, 0).is_ok());
        assert_eq!(
            host(function, &empty, 5, 0).unwrap_err().kind,
            Kind::ExecutionLimit
        );
    }
    let cp = [Value::Integer(233)]; // UTF-8 two bytes; one dispatch + two output bytes
    assert_eq!(
        host(B::TextFromCodepoint, &cp, 3, 2).unwrap().as_str(),
        Some("é")
    );
    assert_eq!(
        host(B::TextFromCodepoint, &cp, 2, 2).unwrap_err().kind,
        Kind::ExecutionLimit
    );
    assert_eq!(
        host(B::TextFromCodepoint, &cp, 3, 1).unwrap_err().kind,
        Kind::TextConstruction
    );
}

#[test]
fn typed_funcs_defaults_and_shadowing_work_without_new_language_syntax() {
    let program = r#"
        pure funcra scalar(c:integer=127757)->string {
            return text_from_codepoint(c);
        }
        pure funcra count(s:string)->integer {
            return text_count(s,scalar());
        }
        bind s=text_concat(scalar(),"a");
        print(s,count(s),text_rfind(s,scalar()));
    "#;
    assert_eq!(output(program), "🌍a 1 0\n");
    assert_eq!(
        output("pure funcra text_count(s:string,n:string)->integer{return 99;} print(text_count(\"a\",\"b\"));"),
        "99\n"
    );
}

#[test]
fn source_execution_returns_codepoints_and_precise_reverse_positions() {
    assert_eq!(
        output(
            "bind text=\"aé🌍é\"; print(text_rfind(text,\"é\"),text_count(text,\"é\"),text_from_codepoint(text_codepoint(text,2)));",
        ),
        "3 2 🌍\n"
    );
    assert_eq!(output("print(text_from_codepoint(0));"), "\0\n");
    assert_eq!(
        output("print(text_count(\"\",\"\"),text_rfind(\"\",\"\"));"),
        "1 0\n"
    );
}

#[test]
fn cumulative_text_quota_is_charged_for_each_new_codepoint_string() {
    let source = "bind first=text_from_codepoint(233); bind second=text_from_codepoint(233); print(first,second);";
    let options = ExecutionOptions {
        max_text_bytes: 4,
        ..ExecutionOptions::default()
    };
    let mut bytes = Vec::new();
    scarp::run_with_options(source, &mut bytes, options).unwrap();
    assert_eq!(bytes, "é é\n".as_bytes());
    let options = ExecutionOptions {
        max_text_bytes: 3,
        ..ExecutionOptions::default()
    };
    assert_eq!(
        scarp::check_with_options(source, options).unwrap_err().kind,
        Kind::TextConstruction
    );
}

#[test]
fn runtime_failures_keep_stdout_and_stderr_empty_and_source_spans_precise() {
    let source = "print(\"early\"); eprint(\"logged\"); print(text_from_codepoint(55296));";
    let expected = scarp::check(source).unwrap_err();
    assert_eq!(expected.kind, Kind::NumericDomain);
    assert_eq!(&source[expected.span.start..expected.span.end], "55296");
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let RunError::Diagnostic(actual) =
        scarp::run_with_io_and_options(source, &mut out, &mut err, ExecutionOptions::default())
            .unwrap_err()
    else {
        panic!("expected a diagnostic");
    };
    assert_eq!(actual, expected);
    assert!(out.is_empty() && err.is_empty());
}

#[test]
fn experimental_native_backends_continue_to_reject_new_calls() {
    for source in [
        "print(text_from_codepoint(65));",
        "print(text_rfind(\"a\",\"a\"));",
        "print(text_count(\"a\",\"a\"));",
    ] {
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
