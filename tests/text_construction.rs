use scarp::{
    diagnostics::{Diagnostic, DiagnosticKind as Kind},
    modules::{self, MemorySourceLoader, ModuleLimits},
    numeric::{self, Builtin as B},
    value::{Value, MAX_TEXT_BYTES},
    ExecutionOptions, RunError,
};
fn options(bytes: usize) -> ExecutionOptions {
    ExecutionOptions {
        max_text_bytes: bytes,
        ..ExecutionOptions::default()
    }
}
fn run(source: &str, bytes: usize) -> Vec<u8> {
    scarp::check_with_options(source, options(bytes)).unwrap();
    let mut out = Vec::new();
    scarp::run_with_options(source, &mut out, options(bytes)).unwrap();
    out
}
fn failure(source: &str, bytes: usize, kind: Kind) -> Diagnostic {
    let expected = scarp::check_with_options(source, options(bytes)).unwrap_err();
    assert_eq!(expected.kind, kind, "{expected:?}");
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let RunError::Diagnostic(actual) =
        scarp::run_with_io_and_options(source, &mut out, &mut err, options(bytes)).unwrap_err()
    else {
        panic!("expected diagnostic")
    };
    assert_eq!(actual, expected);
    assert!(out.is_empty() && err.is_empty());
    expected
}
fn host(function: B, args: &[Value], steps: u64, bytes: usize) -> Result<Value, Diagnostic> {
    numeric::evaluate_with_text_limit(function, args, steps, bytes)
}

#[test]
fn construction_is_explicit_typed_and_preserves_unicode() {
    assert_eq!(run("bind a: string=text_concat(\"é\",\"🌍\"); print(a,text_repeat(a,2),text_slice(a,1,1));",100),"é🌍 é🌍é🌍 🌍\n".as_bytes());
    for source in [
        "print(text_concat(1,\"x\"));",
        "print(text_slice(\"a\",true,1));",
        "print(text_repeat(\"a\",1.0));",
        "bind x: integer=text_repeat(\"a\",1);",
    ] {
        failure(source, 100, Kind::TypeMismatch);
    }
    failure("print(text_concat(\"a\"));", 100, Kind::CallArguments);
    failure("print(\"a\"+\"b\");", 100, Kind::TypeMismatch);
}
#[test]
fn slices_cover_scalar_boundaries_without_normalization() {
    let text = "aé🌍e\u{301}\0\r\n";
    let chars: Vec<_> = text.chars().collect();
    for start in 0..=chars.len() {
        for count in 0..=chars.len() - start {
            let expected: String = chars[start..start + count].iter().collect();
            let value = host(
                B::TextSlice,
                &[
                    Value::from(text),
                    Value::Integer(start as i64),
                    Value::Integer(count as i64),
                ],
                100,
                expected.len(),
            )
            .unwrap();
            assert_eq!(value.as_str(), Some(expected.as_str()));
        }
    }
    assert_eq!(
        host(
            B::TextSlice,
            &[Value::from(""), Value::Integer(0), Value::Integer(0)],
            2,
            0
        )
        .unwrap()
        .as_str(),
        Some("")
    );
}
#[test]
fn invalid_slices_have_argument_spans_and_actionable_repairs() {
    for (call, offending) in [
        ("text_slice(\"é\",-1,0)", "-1"),
        ("text_slice(\"é\",0,-1)", "-1"),
        ("text_slice(\"é\",2,0)", "2"),
        ("text_slice(\"é\",1,1)", "1)"),
        (
            "text_slice(\"é\",9223372036854775807,9223372036854775807)",
            "9223372036854775807",
        ),
    ] {
        let source = format!("print(\"OUT\"); eprint(\"ERR\"); print({call});");
        let error = failure(&source, 100, Kind::TextIndex);
        assert!(source[error.span.start..].starts_with(offending));
        assert!(error.help.unwrap().contains("start + count <= text_len"));
    }
}
#[test]
fn repetition_handles_zero_empty_negative_and_overflow_without_unbounded_work() {
    assert_eq!(
        run(
            "print(text_repeat(\"\",9223372036854775807),text_repeat(\"é\",0));",
            2
        ),
        b" \n"
    );
    let source = "print(text_repeat(\"a\",-1));";
    let error = failure(source, 100, Kind::TextConstruction);
    assert_eq!(&source[error.span.start..error.span.end], "-1");
    assert!(error.message.contains("nonnegative"));
    failure(
        "print(text_repeat(\"ab\",9223372036854775807));",
        usize::MAX,
        Kind::TextConstruction,
    );
}
#[test]
fn literals_and_each_new_payload_charge_exact_utf8_bytes() {
    let source="bind a=\"é\"; bind b=text_repeat(a,2); bind c=text_concat(b,a); bind d=text_slice(c,1,1); print(d);";
    assert_eq!(run(source, 14), "é\n".as_bytes()); // 2 + 4 + 6 + 2
    let error = failure(source, 13, Kind::TextConstruction);
    assert!(error.message.contains("13 bytes"));
    assert!(error.help.unwrap().contains("--max-text-bytes"));
    assert_eq!(run("bind a=\"é\"; print(a);", 2), "é\n".as_bytes());
    failure("bind a=\"é\"; print(a);", 1, Kind::TextConstruction);
}
#[test]
fn sharing_queries_calls_and_output_do_not_recharge_text_payload() {
    assert_eq!(run("pure funcra id(s: string)->string{return s;} bind a=\"é\"; bind b=id(a); print(a,b,text_len(b));",2),"é é 1\n".as_bytes());
    assert_eq!(
        run(
            "print(\"literal output\"); bind a=\"\"; print(text_concat(a,a));",
            0
        ),
        b"literal output\n\n"
    );
}
#[test]
fn constructors_materialize_even_identity_results_for_predictable_quotas() {
    for source in [
        "bind s=\"a\"; print(text_repeat(s,1));",
        "bind s=\"a\"; print(text_slice(s,0,1));",
        "bind s=\"a\"; print(text_concat(s,\"\"));",
    ] {
        assert_eq!(run(source, 2), b"a\n");
        failure(source, 1, Kind::TextConstruction);
    }
}
#[test]
fn retirement_does_not_refund_cumulative_allocation() {
    let source = "repeat 3 as index {bind s=\"ab\";} print(1);";
    assert_eq!(run(source, 6), b"1\n");
    failure(source, 5, Kind::TextConstruction);
}
#[test]
fn per_value_cap_and_host_payload_budget_are_independent_of_steps() {
    let args = [Value::from("x"), Value::Integer(MAX_TEXT_BYTES as i64)];
    let result = host(B::TextRepeat, &args, u64::MAX, MAX_TEXT_BYTES).unwrap();
    assert_eq!(result.as_str().unwrap().len(), MAX_TEXT_BYTES);
    assert_eq!(
        host(B::TextRepeat, &args, u64::MAX, MAX_TEXT_BYTES - 1)
            .unwrap_err()
            .kind,
        Kind::TextConstruction
    );
    assert_eq!(
        host(
            B::TextRepeat,
            &args,
            (MAX_TEXT_BYTES - 1) as u64,
            MAX_TEXT_BYTES
        )
        .unwrap_err()
        .kind,
        Kind::ExecutionLimit
    );
    let args = [Value::from("x"), Value::Integer(MAX_TEXT_BYTES as i64 + 1)];
    assert_eq!(
        host(B::TextRepeat, &args, u64::MAX, usize::MAX)
            .unwrap_err()
            .kind,
        Kind::TextConstruction
    );
    let huge = "a".repeat(MAX_TEXT_BYTES + 1);
    assert_eq!(
        host(
            B::TextConcat,
            &[Value::from(huge.as_str()), Value::from("")],
            u64::MAX,
            usize::MAX
        )
        .unwrap_err()
        .kind,
        Kind::TextConstruction
    );
}
#[test]
fn host_inputs_are_not_recharged_and_work_has_exact_boundaries() {
    let args = [Value::from("é"), Value::from("🌍")];
    assert_eq!(
        host(B::TextConcat, &args, 6, 6).unwrap().as_str(),
        Some("é🌍")
    );
    assert_eq!(
        host(B::TextConcat, &args, 5, 6).unwrap_err().kind,
        Kind::ExecutionLimit
    );
    let args = [Value::from("aé🌍"), Value::Integer(1), Value::Integer(1)];
    assert_eq!(host(B::TextSlice, &args, 9, 2).unwrap().as_str(), Some("é"));
    assert_eq!(
        host(B::TextSlice, &args, 8, 2).unwrap_err().kind,
        Kind::ExecutionLimit
    );
    assert_eq!(
        host(
            B::TextRepeat,
            &[Value::from(""), Value::Integer(i64::MAX)],
            1,
            0
        )
        .unwrap()
        .as_str(),
        Some("")
    );
}
#[test]
fn text_and_numerical_payload_budgets_are_independent() {
    let source = "bind a=[integer:1]; bind s=text_repeat(\"x\",2); print(a,s);";
    let mut out = Vec::new();
    scarp::run_with_options(
        source,
        &mut out,
        ExecutionOptions {
            max_vector_bytes: 8,
            max_text_bytes: 3,
            ..ExecutionOptions::default()
        },
    )
    .unwrap();
    assert_eq!(out, b"[integer: 1] xx\n");
}
#[test]
fn defaults_contracts_recursion_and_imports_share_one_text_quota() {
    let source="pure funcra f(n: integer,s: string=\"é\")->string ensures text_len(result)>=1; {if n==0{return s;} return f(n-1,text_concat(s,\"x\"));} print(f(2));";
    assert_eq!(run(source, 11), "éxx\n".as_bytes()); // default 2, literals 1+1, new 3+4
    failure(source, 10, Kind::TextConstruction);
    let mut loader = MemorySourceLoader::new();
    loader
        .insert("main.srp", "import \"lib.srp\" as lib; print(lib.f());")
        .unwrap();
    loader
        .insert(
            "lib.srp",
            "// é\r\npub pure funcra f()->string{return text_repeat(\"é\",2);}",
        )
        .unwrap();
    let program =
        modules::compile_with_loader("main.srp", &loader, ModuleLimits::default()).unwrap();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let modules::ModuleRunError::Diagnostic(error) =
        program.execute(&mut out, &mut err, options(5)).unwrap_err()
    else {
        panic!("expected diagnostic")
    };
    let location = error.sources.locate_span(error.diagnostic.span).unwrap();
    assert!(error.sources.files()[location.source]
        .name
        .ends_with("lib.srp"));
    assert!(out.is_empty() && err.is_empty());
}
#[test]
fn signatures_and_shadowing_follow_existing_builtin_rules() {
    let program = scarp::compile("bind x=text_slice(text_repeat(\"é\",2),0,1); print(x);").unwrap();
    for function in [B::TextRepeat, B::TextSlice] {
        let id = program.builtin_type_id(function).unwrap();
        assert!(program.type_registry().get(id).is_some());
    }
    assert_eq!(run("pure funcra text_concat(a: integer,b: integer)->integer{return a+b;} print(text_concat(2,3));",0),b"5\n");
}
#[test]
fn generated_text_cases_match_an_independent_scalar_model() {
    let alphabet = ['a', 'é', '🌍', '\u{301}', '\0'];
    for seed in 0..125 {
        let text: String = (0..3)
            .map(|i| alphabet[(seed / 5usize.pow(i)) % 5])
            .collect();
        for count in 0..4 {
            let expected = text.repeat(count);
            let got = host(
                B::TextRepeat,
                &[Value::from(text.as_str()), Value::Integer(count as i64)],
                100,
                100,
            )
            .unwrap();
            assert_eq!(got.as_str(), Some(expected.as_str()));
        }
        let got = host(
            B::TextConcat,
            &[Value::from(text.as_str()), Value::from("é")],
            100,
            100,
        )
        .unwrap();
        assert_eq!(got.as_str(), Some(format!("{text}é").as_str()));
    }
}

#[test]
fn text_example_has_verified_expected_output() {
    assert_eq!(
        run(include_str!("../examples/text_construction.srp"), 100),
        include_bytes!("../examples/text_construction.stdout")
    );
}
