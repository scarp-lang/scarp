use scarp::{diagnostics::DiagnosticKind as Kind, ExecutionOptions};
use scarp::{
    numeric::{self, Builtin},
    value::{IntegerVector, NumericVector, Value, MAX_VECTOR_LENGTH},
};

fn ints(values: &[i64]) -> Value {
    Value::Integers(IntegerVector::new(values).unwrap())
}
fn output(source: &str) -> Vec<u8> {
    let mut bytes = Vec::new();
    scarp::run(source, &mut bytes).unwrap();
    bytes
}

#[test]
fn typed_integer_literals_keep_float_vectors_unambiguous() {
    let mut bytes = Vec::new();
    scarp::run(
        "bind data: integers=[integer: 3,1,4]; print(data,data[1],ints_sum(data));",
        &mut bytes,
    )
    .unwrap();
    assert_eq!(bytes, b"[integer: 3, 1, 4] 1 8\n");
}

#[test]
fn collection_updates_are_values_and_counted_iteration_is_bounded() {
    let mut bytes = Vec::new();
    scarp::run("bind original: integers=ints_range(1,4,1); var edited: integers=ints_set(original,0,9); var total: integer=0; repeat ints_len(original) as index {total+=original[index];} print(original,edited,total);",&mut bytes).unwrap();
    assert_eq!(bytes, b"[integer: 1, 2, 3, 4] [integer: 9, 2, 3, 4] 10\n");
}

#[test]
fn empty_collections_equality_and_existing_vector_syntax_are_distinct() {
    assert_eq!(output("bind integer=2.0; print([integer],[],[integer:],[integer:1,2]==[integer:1,2],[integer:1]!=[integer:2]);"), b"[2.0] [] [integer:] true true\n");
    for source in [
        "print([integer:1.0]);",
        "print([integer:true]);",
        "print([integer:[integer:1]]);",
        "print([integer:1]==[1.0]);",
        "print(len([integer:1]));",
    ] {
        assert_eq!(
            scarp::compile(source).unwrap_err().kind,
            Kind::TypeMismatch,
            "{source}"
        );
    }
}

#[test]
fn indexing_checks_visible_bounds_and_integer_type() {
    assert_eq!(
        output("print([integer:-9223372036854775808,9223372036854775807][0]);"),
        b"-9223372036854775808\n"
    );
    for index in [-1, 2, i64::MAX] {
        assert_eq!(
            scarp::check(&format!("print([integer:1,2][{index}]);"))
                .unwrap_err()
                .kind,
            Kind::VectorIndex
        );
    }
    assert_eq!(
        scarp::compile("print([integer:1][0.0]);").unwrap_err().kind,
        Kind::TypeMismatch
    );
    assert_eq!(
        output("print(ints_window([integer:1,2,3],1,1)[0]);"),
        b"2\n"
    );
}

#[test]
fn functions_defaults_contracts_and_bindings_accept_integer_collections() {
    assert_eq!(output("pure funcra tail(data: integers=[integer:1,2,3])->integers needs ints_len(data)>0; ensures ints_len(result)==ints_len(data)-1; {return ints_window(data,1,ints_len(data)-1);} var data: integers=tail(); data=ints_set(data,0,9); print(tail(),data);"), b"[integer: 2, 3] [integer: 9, 3]\n");
}

#[test]
fn host_windows_share_storage_and_keep_only_visible_value_semantics() {
    let original = IntegerVector::new(&[1, 2, 3, 4]).unwrap();
    let window = original.window(1, 2).unwrap();
    assert!(std::ptr::eq(&original.as_slice()[1], &window.as_slice()[0]));
    assert_eq!(window, IntegerVector::new(&[2, 3]).unwrap());
    let nested = window.window(1, 1).unwrap();
    drop(original);
    drop(window);
    assert_eq!(nested.as_slice(), &[3]);
    assert!(nested.window(1, 0).unwrap().is_empty());
    assert!(nested.window(usize::MAX, 1).is_none());
    assert!(nested.window(0, 2).is_none());
    assert!(IntegerVector::new(&vec![0; MAX_VECTOR_LENGTH + 1]).is_none());
}

#[test]
fn host_kernel_work_and_payload_limits_have_exact_boundaries() {
    for (function, args, work, bytes) in [
        (Builtin::IntsLength, vec![ints(&[1, 2, 3])], 1, 0),
        (Builtin::IntsSum, vec![ints(&[1, 2, 3])], 3, 0),
        (
            Builtin::IntsWindow,
            vec![ints(&[1, 2, 3]), Value::Integer(1), Value::Integer(2)],
            1,
            0,
        ),
        (
            Builtin::IntsSet,
            vec![ints(&[1, 2, 3]), Value::Integer(1), Value::Integer(8)],
            3,
            24,
        ),
        (
            Builtin::IntsJoin,
            vec![ints(&[1, 2, 3]), ints(&[4, 5])],
            5,
            40,
        ),
        (
            Builtin::IntsRange,
            vec![Value::Integer(1), Value::Integer(4), Value::Integer(2)],
            4,
            32,
        ),
        (Builtin::IntsToVector, vec![ints(&[1, 2, 3])], 3, 24),
        (
            Builtin::IntsFromVector,
            vec![Value::Vector(NumericVector::new(&[1.0, 2.0]).unwrap())],
            2,
            16,
        ),
    ] {
        let before = args.clone();
        numeric::evaluate_with_vector_limit(function, &args, work, bytes).unwrap();
        assert_eq!(
            numeric::evaluate(function, &args, work - 1)
                .unwrap_err()
                .kind,
            Kind::ExecutionLimit,
            "{function:?}"
        );
        if bytes > 0 {
            assert_eq!(
                numeric::evaluate_with_vector_limit(function, &args, work, bytes - 1)
                    .unwrap_err()
                    .kind,
                Kind::VectorBudget,
                "{function:?}"
            );
        }
        assert_eq!(args, before);
    }
    let mut options = ExecutionOptions {
        max_vector_bytes: 24,
        ..ExecutionOptions::default()
    };
    scarp::check_with_options(
        "bind a=[integer:1,2,3]; bind b=ints_window(a,1,2);",
        options,
    )
    .unwrap();
    options.max_vector_bytes = 23;
    assert_eq!(
        scarp::check_with_options("bind a=[integer:1,2,3];", options)
            .unwrap_err()
            .kind,
        Kind::VectorBudget
    );
}

#[test]
fn bounds_overflow_and_failed_updates_are_atomic() {
    for (expression, kind) in [
        (
            "ints_sum([integer:9223372036854775807,1])",
            Kind::ArithmeticOverflow,
        ),
        (
            "ints_range(9223372036854775807,2,1)",
            Kind::ArithmeticOverflow,
        ),
        ("ints_range(0,-1,1)", Kind::VectorLength),
        ("ints_range(0,65537,1)", Kind::VectorLength),
        ("ints_set([integer:1],-1,3)", Kind::VectorIndex),
        ("ints_window([integer:1],1,1)", Kind::VectorIndex),
        ("ints_window([integer:1],0,-1)", Kind::VectorLength),
    ] {
        let source = format!("print(\"before\"); eprint(\"before\"); print({expression});");
        let (mut out, mut err) = (Vec::new(), Vec::new());
        assert!(scarp::run_with_io_and_options(
            &source,
            &mut out,
            &mut err,
            ExecutionOptions::default()
        )
        .is_err());
        assert!(out.is_empty() && err.is_empty());
        assert_eq!(scarp::check(&source).unwrap_err().kind, kind, "{source}");
    }
    assert_eq!(
        output("print(ints_range(9223372036854775807,1,1),ints_range(0,0,1),ints_range(7,3,0));"),
        b"[integer: 9223372036854775807] [integer:] [integer: 7, 7, 7]\n"
    );
    assert_eq!(
        numeric::evaluate(
            Builtin::IntsJoin,
            &[ints(&vec![0; MAX_VECTOR_LENGTH]), ints(&[0])],
            u64::MAX
        )
        .unwrap_err()
        .kind,
        Kind::VectorLength
    );
}

#[test]
fn conversions_are_elementwise_exact_and_signatures_are_checked() {
    assert_eq!(
        output("print(ints_to_vector([integer:1,-2]),ints_from_vector([3.0,-4.0]));"),
        b"[1.0, -2.0] [integer: 3, -4]\n"
    );
    for source in [
        "print(ints_to_vector([integer:9223372036854775807]));",
        "print(ints_from_vector([0.5]));",
    ] {
        let error = scarp::check(source).unwrap_err();
        assert_eq!(error.kind, Kind::NumericConversion);
        assert!(error.message.contains("element 0"));
    }
    for source in [
        "print(ints_sum([1.0]));",
        "print(ints_set([integer:1],0,1.0));",
    ] {
        assert_eq!(scarp::compile(source).unwrap_err().kind, Kind::TypeMismatch);
    }
    assert_eq!(
        scarp::compile("print(ints_len());").unwrap_err().kind,
        Kind::CallArguments
    );
    assert_eq!(
        numeric::evaluate(Builtin::IntsLength, &[], 0)
            .unwrap_err()
            .kind,
        Kind::CallArguments
    );
}

#[test]
fn imported_signatures_preserve_collection_types_and_constants_remain_scalar() {
    use scarp::modules::{self, MemorySourceLoader, ModuleLimits};
    let mut loader = MemorySourceLoader::new();
    loader
        .insert(
            "main.srp",
            "import \"data.srp\" as data; print(data.values()[1]);",
        )
        .unwrap();
    loader
        .insert(
            "data.srp",
            "pub pure funcra values()->integers {return [integer:4,5];}",
        )
        .unwrap();
    let program =
        modules::compile_with_loader("main.srp", &loader, ModuleLimits::default()).unwrap();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    program
        .execute(&mut out, &mut err, ExecutionOptions::default())
        .unwrap();
    assert_eq!(out, b"5\n");
    assert!(err.is_empty());
    let mut loader = MemorySourceLoader::new();
    loader
        .insert("main.srp", "pub bind data=[integer:1];")
        .unwrap();
    assert_eq!(
        modules::compile_with_loader("main.srp", &loader, ModuleLimits::default())
            .unwrap_err()
            .diagnostic
            .kind,
        Kind::ModuleSyntax
    );
}

#[test]
fn generated_ranges_and_updates_match_an_independent_model() {
    for count in 0..32 {
        let expected: Vec<i64> = (0..count).map(|i| -11 + i * 3).collect();
        let result = numeric::evaluate(
            Builtin::IntsRange,
            &[
                Value::Integer(-11),
                Value::Integer(count),
                Value::Integer(3),
            ],
            100,
        )
        .unwrap();
        assert_eq!(result, ints(&expected));
        for index in 0..count {
            let updated = numeric::evaluate(
                Builtin::IntsSet,
                &[result.clone(), Value::Integer(index), Value::Integer(77)],
                100,
            )
            .unwrap();
            let mut expected = expected.clone();
            expected[index as usize] = 77;
            assert_eq!(updated, ints(&expected));
        }
    }
    let oversized = format!(
        "print([integer:{}]);",
        vec!["0"; MAX_VECTOR_LENGTH + 1].join(",")
    );
    assert_eq!(
        scarp::parse(&oversized).unwrap_err().kind,
        Kind::VectorLength
    );
}

#[test]
fn collection_capture_is_charged_and_snapshots_retain_old_values() {
    assert_eq!(
        output("var data=[integer:1,2]; print(data); data=ints_set(data,0,9); print(data);"),
        b"[integer: 1, 2]\n[integer: 9, 2]\n"
    );
    let options = ExecutionOptions {
        max_capture_bytes: 79,
        ..ExecutionOptions::default()
    };
    assert_eq!(
        scarp::check_with_options("print([integer:1,2]);", options)
            .unwrap_err()
            .kind,
        Kind::CaptureLimit
    );
}

#[test]
fn exact_diagnostics_agree_between_embedding_and_cli_without_partial_output() {
    let folder = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/integers");
    for name in ["element", "window"] {
        let filename = format!("{name}.srp");
        let source = std::fs::read_to_string(folder.join(&filename)).unwrap();
        let expected = std::fs::read_to_string(folder.join(format!("{name}.stderr"))).unwrap();
        assert_eq!(
            format!(
                "{}\n",
                scarp::check(&source)
                    .unwrap_err()
                    .render(&filename, &source)
            ),
            expected
        );
        for mode in ["run", "check"] {
            let result = std::process::Command::new(env!("CARGO_BIN_EXE_scarp"))
                .current_dir(&folder)
                .args([mode, &filename])
                .output()
                .unwrap();
            assert_eq!(result.status.code(), Some(1));
            assert!(result.stdout.is_empty());
            assert_eq!(String::from_utf8(result.stderr).unwrap(), expected);
        }
        let result = std::process::Command::new(env!("CARGO_BIN_EXE_scarp"))
            .current_dir(&folder)
            .args(["modules", &filename])
            .output()
            .unwrap();
        if name == "element" {
            assert_eq!(result.status.code(), Some(1));
            assert!(result.stdout.is_empty());
            assert_eq!(String::from_utf8(result.stderr).unwrap(), expected);
        } else {
            assert!(result.status.success() && result.stderr.is_empty());
        }
    }
}
