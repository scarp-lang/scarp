use scarp::{
    ast::{ExpressionKind, PrintArgument, Statement},
    diagnostics::{Diagnostic, DiagnosticKind as Kind},
    types::{
        PrimitiveType as P, SourceType, TypeKind, MAX_ARRAY_LENGTH, MAX_COMPOSITE_DEPTH,
        MAX_EXPANDED_VALUE_NODES, MAX_SOURCE_TYPE_NODES,
    },
    value::{ArrayValue, Value},
    ExecutionOptions, RunError,
};

fn run(source: &str) -> (Vec<u8>, Vec<u8>) {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    scarp::run_with_io(source, &mut out, &mut err).unwrap();
    (out, err)
}

fn failure(source: &str, options: ExecutionOptions, kind: Kind) -> Diagnostic {
    let expected = scarp::check_with_options(source, options).unwrap_err();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let RunError::Diagnostic(actual) =
        scarp::run_with_io_and_options(source, &mut out, &mut err, options).unwrap_err()
    else {
        panic!("expected a diagnostic");
    };
    assert_eq!(actual, expected);
    assert_eq!(actual.kind, kind, "{actual:?}: {source}");
    assert!(out.is_empty() && err.is_empty());
    actual
}

#[test]
fn grammar_supports_contextual_marker_typed_empty_and_adjacent_closers() {
    let source = r#"
        bind array = 2.5;
        bind plain = [array];
        bind nested:tuple<tuple<array<integer, 1>>>=(([array: 4,],),);
        bind empty:array<array<integer, 1>, 0>=[array: <array<integer, 1>>];
        bind mixed=[array: <tuple<array<integer, 1>>>];
        print(array,plain[0],nested[0][0][0],empty,mixed,[integer: 3]);
        var bits=8; bits>>=1; print(bits,8>>>1,8>=1);
    "#;
    assert_eq!(
        run(source).0,
        b"2.5 2.5 4 [array: <array<integer, 1>>] [array: <tuple<array<integer, 1>>>] [integer: 3]\n4 4 true\n"
    );
    assert_eq!(run("bind array=true; if array{print(array);}").0, b"true\n");
    assert_eq!(
        run("pure funcra array(x:integer)->integer{return x;} print(array(5));").0,
        b"5\n"
    );
    let parsed = scarp::parse("print([array: <integer>], [array: 9]);").unwrap();
    let Statement::Output { arguments, .. } = &parsed.statements[0] else {
        panic!("expected output statement");
    };
    for (argument, empty) in arguments.iter().zip([true, false]) {
        let PrintArgument::Expression(expr) = argument else {
            panic!("expected array expression");
        };
        let ExpressionKind::Array {
            elements,
            empty_element_type,
        } = &expr.nodes()[expr.root()].kind
        else {
            panic!("expected array literal");
        };
        assert_eq!(elements.is_empty(), empty);
        assert_eq!(
            empty_element_type.as_ref(),
            empty.then_some(&SourceType::from(P::Integer))
        );
    }
}

#[test]
fn invalid_lengths_and_literals_have_precise_source_diagnostics() {
    for length in [
        "-1",
        "+1",
        "0x1",
        "0b1",
        "4_097",
        "4097",
        "1 + 1",
        "1.0",
        "n",
        "1__0",
        "1_",
        "1 _0",
        "18446744073709551616",
    ] {
        let source = format!("// नमस्ते\r\nfuncra f(x: array<integer, {length}>) -> unit {{}}");
        let error = scarp::parse(&source).unwrap_err();
        assert_eq!(error.kind, Kind::ArrayLimit, "{error:?}");
        assert_eq!(&source[error.span.start..error.span.end], length);
    }
    let source = "print([array:]);";
    let error = scarp::parse(source).unwrap_err();
    assert_eq!(&source[error.span.start..error.span.end], "]");
    assert!(error
        .help
        .as_deref()
        .unwrap()
        .contains("[array: <integer>]"));
    for source in [
        "print([array: <unit>]);",
        "print([array: <integer>,]);",
        "print([array: ,1]);",
        "print([array: 1,,2]);",
        "funcra f(x: array<unit, 0>) -> unit {}",
    ] {
        assert!(scarp::compile(source).is_err(), "{source}");
    }
}

#[test]
fn array_types_are_exact_and_errors_relate_their_inference_or_annotation_site() {
    for (source, primary, related) in [
        ("bind x=[array: (1,true), (1,2)];", "(1,2)", "(1,true)"),
        (
            "bind x:array<integer, 2>=[array: 1];",
            "[array: 1]",
            "array<integer, 2>",
        ),
        (
            "bind x:array<integer, 0>=[array: <string>];",
            "[array: <string>]",
            "array<integer, 0>",
        ),
    ] {
        let error = failure(source, Default::default(), Kind::TypeMismatch);
        assert_eq!(&source[error.span.start..error.span.end], primary);
        let span = error.related[0].span;
        assert_eq!(&source[span.start..span.end], related);
    }
    for source in [
        "var x=[array: 1]; x=[array: 1,2];",
        "var x=[array: 1]; x=[array: 1.0];",
        "var x=[array: 1]; x+=[array: 2];",
        "print([array: 1] < [array: 2]);",
        "print([array: 1] + [array: 2]);",
        "print([array: 1] == [integer: 1]);",
        "print([array: 1] == [array: 1,2]);",
        "print([array: 1][true]);",
        "print([array: 1][0.0]);",
        "funcra f(x:array<float, 1>)->unit{} f([array: 1]);",
    ] {
        failure(source, Default::default(), Kind::TypeMismatch);
    }
    failure(
        "bind x=[array: 1]; x=[array: 2];",
        Default::default(),
        Kind::ImmutableAssignment,
    );
    failure(
        "funcra f()->array<integer, 0>{return [array: 1];}",
        Default::default(),
        Kind::InvalidReturn,
    );
    failure(
        "funcra f()->unit{} bind a=[array: f()];",
        Default::default(),
        Kind::InvalidReturn,
    );
    assert!(scarp::compile("var a=[array: 1]; a[0]=2;").is_err());
}

#[test]
fn projection_evaluates_receiver_elements_and_index_once_in_source_order() {
    let source = r#"
        funcra item(value:integer)->integer { write(value); return value; }
        funcra receiver()->array<integer, 2> {
            write("r"); return [array: item(1), item(2)];
        }
        funcra index()->integer { write("i"); return 1; }
        print(receiver()[index()]);
        var n=1;
        bind grid=[array: [array: 1,2], [array: 3,4]];
        print(grid[n][n-1], grid[(n-1)][n]);
        print(([array: 7],)[0][n-1], [array: (5,"a")][n-1][1]);
    "#;
    assert_eq!(run(source).0, b"r12i2\n3 2\n7 a\n");
    failure(
        "bind n=0; print((7,)[n]);",
        Default::default(),
        Kind::TupleIndex,
    );
}

#[test]
fn static_verification_leaves_all_fixed_array_bounds_to_execution() {
    for (value, index) in [
        ("[array: <integer>]", "0"),
        ("[array: 7]", "-1"),
        ("[array: 7]", "1"),
        ("[array: 7]", "i + 1"),
        ("[array: 7]", "(i - 1)"),
        ("[array: 7]", "-9223372036854775808"),
        ("[array: 7]", "9223372036854775807"),
    ] {
        let source = format!(
            "// 🌍\r\nprint(\"before\"); eprint(\"before\"); bind i=0; print({value}[{index}]);"
        );
        scarp::compile(&source).unwrap();
        let error = failure(&source, Default::default(), Kind::ArrayIndex);
        assert_eq!(&source[error.span.start..error.span.end], index);
    }
    assert_eq!(run("if false { print([array: <integer>][0]); }").0, b"");
}

#[test]
fn array_values_survive_defaults_contracts_recursion_and_scope_retirement() {
    let source = r#"
        pure funcra descend(n:integer, values:array<string, 2>=[array:"नमस्ते", "🌍"])
          ->array<string, 2> needs n>=0; ensures result==values; {
            if n==0 { { bind saved=values; return saved; } }
            return descend(n-1, values:values);
        }
        pure funcra empty(value:array<integer, 0>=[array: <integer>])->array<integer, 0> {
            return value;
        }
        var current=descend(8);
        bind snapshot=current;
        print(current);
        current=descend(values:[array:"new", "pair"], n:2);
        eprint(snapshot, current, empty());
    "#;
    let (out, err) = run(source);
    assert_eq!(out, "[array: \"नमस्ते\", \"🌍\"]\n".as_bytes());
    assert_eq!(
        err,
        "[array: \"नमस्ते\", \"🌍\"] [array: \"new\", \"pair\"] [array: <integer>]\n".as_bytes()
    );
    failure(
        "funcra effect()->integer{print(1);return 1;} pure funcra f()->array<integer, 1>{return [array:effect()];}",
        Default::default(),
        Kind::ImpureFunction,
    );
}

#[test]
fn nested_display_and_equality_preserve_primitive_behavior() {
    let source = r#"
        bind s="नमस्ते 🌍\n\r\t\\\"\u{0}";
        print([array: (s, -0.0)],s);
        print([array: (s, -0.0)]==[array: (s, 0.0)]);
        print([array: [1.0], [2.0]]!=[array: [1.0], [3.0]]);
        print([array: <tuple<integer, string>>]==[array: <tuple<integer, string>>]);
        ewrite([array: "err"]);
    "#;
    let (out, err) = run(source);
    assert_eq!(
        out,
        "[array: (\"नमस्ते 🌍\\n\\r\\t\\\\\\\"\\0\", -0.0)] नमस्ते 🌍\n\r\t\\\"\0\ntrue\ntrue\ntrue\n"
            .as_bytes()
    );
    assert_eq!(err, b"[array: \"err\"]");
}

#[test]
fn host_constructors_and_registry_preserve_fixed_shapes_and_sharing() {
    let value = ArrayValue::new(P::Integer, vec![Value::Integer(3)]).unwrap();
    let shared = value.clone();
    assert!(std::ptr::eq(value.elements(), shared.elements()));
    assert_eq!(value.element_type(), &SourceType::from(P::Integer));
    assert_eq!(value.kind().to_string(), "array<integer, 1>");
    assert!(ArrayValue::new(P::Unit, vec![]).is_none());
    assert!(ArrayValue::new(P::Integer, vec![Value::Unit]).is_none());
    assert!(ArrayValue::new(P::Integer, vec![Value::Boolean(true)]).is_none());
    assert!(ArrayValue::new(P::Integer, vec![Value::Integer(0); MAX_ARRAY_LENGTH + 1]).is_none());
    let empty = ArrayValue::new(P::String, vec![]).unwrap();
    assert!(empty.is_empty());
    assert_eq!(empty.to_string(), "[array: <string>]");

    let source =
        "bind a=[array: 1]; bind b=[array: 2]; bind c=[array: 3,4]; bind d=[array: <integer>];";
    let program = scarp::compile(source).unwrap();
    let ids = program.binding_type_ids();
    assert_eq!(ids[0], ids[1]);
    for (id, length) in [(ids[0], 1), (ids[2], 2), (ids[3], 0)] {
        assert!(matches!(
            program.type_registry().get(id),
            Some(TypeKind::Sequence { element, length: Some(n) }) if *element == P::Integer.id() && *n == length
        ));
    }
    let other = scarp::compile(source).unwrap();
    assert_eq!(program.binding_types(), other.binding_types());
    assert_ne!(ids[0], other.binding_type_ids()[0]);
    assert_eq!(
        program.type_registry().get(P::Integer.id()),
        Some(&TypeKind::Primitive(P::Integer))
    );
}

#[test]
fn limits_cover_literal_length_mixed_depth_and_expanded_children() {
    let entries = vec!["1"; MAX_ARRAY_LENGTH];
    let body = entries.join(",");
    assert_eq!(
        run(&format!(
            "bind a:array<integer, 4_096>=[array:{body},]; print(a[4095]);"
        ))
        .0,
        b"1\n"
    );
    let source = format!("bind a=[array:{body},2];");
    let error = scarp::parse(&source).unwrap_err();
    assert_eq!(error.kind, Kind::ArrayLimit);
    assert_eq!(&source[error.span.start..error.span.end], "2");

    let child = SourceType::array(P::Integer, 256).unwrap();
    let exact = SourceType::array(&child, 255).unwrap();
    assert_eq!(exact.expanded_value_nodes(), MAX_EXPANDED_VALUE_NODES);
    assert_eq!(exact.node_count(), 3);
    assert!(SourceType::array(&child, 256).is_none());
    assert!(SourceType::tuple(vec![exact.clone()]).is_none());
    assert!(SourceType::array(exact, 0).is_some());
    for source in [
        "funcra f(x:array<array<integer, 256>, 256>)->unit{}",
        "funcra f(x:array<array<array<integer, 256>, 256>, 0>)->unit{}",
        "print([array: <array<array<integer, 256>, 256>>]);",
    ] {
        assert_eq!(scarp::parse(source).unwrap_err().kind, Kind::ArrayLimit);
    }
    let mut shape = SourceType::from(P::Integer);
    for index in 0..MAX_COMPOSITE_DEPTH {
        shape = if index % 2 == 0 {
            SourceType::array(shape, 0)
        } else {
            SourceType::tuple(vec![shape])
        }
        .unwrap();
    }
    assert_eq!(shape.structural_depth(), MAX_COMPOSITE_DEPTH);
    assert_eq!(shape.tuple_depth(), 1);
    assert!(SourceType::array(shape, 0).is_none());
}

fn tuple_type(members: &[&str]) -> String {
    format!("tuple<{}>", members.join(","))
}

#[test]
fn parser_bounds_combined_metadata_and_accepts_mixed_depth_on_a_small_stack() {
    let leaf = tuple_type(&["integer"; 32]);
    let group = tuple_type(&[leaf.as_str(); 32]);
    let tail_group = tuple_type(&["integer"; 30]);
    let mut tail_members = vec![leaf.as_str(); 27];
    tail_members.push(&tail_group);
    let tail = tuple_type(&tail_members);
    let exact = tuple_type(&[&group, &group, &group, &tail]);
    let source = format!("funcra exact(a:array<{exact}, 0>)->unit{{}}");
    let program = scarp::compile(&source).unwrap();
    assert_eq!(
        program.functions()[0].parameters[0].kind.node_count(),
        MAX_SOURCE_TYPE_NODES
    );
    let over = tuple_type(&[&group, &group, &group, &tail, "integer"]);
    assert_eq!(
        scarp::parse(&format!("funcra over(a:array<{over}, 0>)->unit{{}}"))
            .unwrap_err()
            .kind,
        Kind::ArrayLimit
    );
    std::thread::Builder::new()
        .stack_size(256 * 1024)
        .spawn(|| {
            for mixed in [false, true] {
                let (mut ty, mut value) = ("integer".to_string(), "1".to_string());
                for index in 0..MAX_COMPOSITE_DEPTH {
                    if mixed && index % 2 == 1 {
                        ty = format!("tuple<{ty}>");
                        value = format!("({value},)");
                    } else {
                        ty = format!("array<{ty}, 1>");
                        value = format!("[array: {value}]");
                    }
                }
                let source = format!("bind a:{ty}={value}; print(a==a);");
                assert_eq!(run(&source).0, b"true\n");
                assert_eq!(
                    scarp::parse(&format!("print([array: <{ty}>]);"))
                        .unwrap_err()
                        .kind,
                    Kind::ArrayLimit
                );
            }
        })
        .unwrap()
        .join()
        .unwrap();
}

fn minimum(mut accepts: impl FnMut(usize) -> bool) -> usize {
    let (mut low, mut high) = (0, 4096);
    assert!(accepts(high));
    while low < high {
        let mid = low + (high - low) / 2;
        if accepts(mid) {
            high = mid;
        } else {
            low = mid + 1;
        }
    }
    low
}

#[test]
fn backing_quota_is_cumulative_independent_and_charges_even_empty_arrays() {
    let options = |max_array_bytes| ExecutionOptions {
        max_array_bytes,
        ..Default::default()
    };
    for literal in ["[array: <integer>]", "[array: 1]"] {
        let source = format!("print({literal});");
        let min = minimum(|limit| scarp::check_with_options(&source, options(limit)).is_ok());
        assert!(min > 0);
        failure(&source, options(min - 1), Kind::ArrayAllocation);
        scarp::check_with_options(&source, options(min)).unwrap();
        let aliases = format!("bind a={literal}; repeat 6 as i {{bind alias=a; print(alias);}}");
        scarp::check_with_options(&aliases, options(min)).unwrap();
        let repeated = format!("repeat 2 as i {{print({literal});}}");
        failure(&repeated, options(2 * min - 1), Kind::ArrayAllocation);
        scarp::check_with_options(&repeated, options(2 * min)).unwrap();
    }
    let source = "print(1); eprint(2); print([array: <integer>]);";
    failure(source, options(0), Kind::ArrayAllocation);
    scarp::check_with_options("print((1,),[1.0]);", options(0)).unwrap();
    scarp::check_with_options(
        "print([array: 1]);",
        ExecutionOptions {
            max_tuple_bytes: 0,
            max_vector_bytes: 0,
            max_text_bytes: 0,
            ..Default::default()
        },
    )
    .unwrap();
}

#[test]
fn equality_and_output_charge_visible_shared_values_and_empty_type_text() {
    let options = |max_capture_bytes| ExecutionOptions {
        max_capture_bytes,
        ..Default::default()
    };
    let compact = "print([array: <integer>]);";
    let capture = minimum(|limit| scarp::check_with_options(compact, options(limit)).is_ok());
    failure(compact, options(capture - 1), Kind::CaptureLimit);
    scarp::check_with_options(compact, options(capture)).unwrap();
    failure(
        "print([array: <tuple<integer,string,vector>>]);",
        options(capture),
        Kind::CaptureLimit,
    );
    let source = "bind text=text_repeat(\"x\",128); bind a=[array:text]; print([array:a,a]);";
    let capture = minimum(|limit| scarp::check_with_options(source, options(limit)).is_ok());
    assert!(capture > 2 * 128 * 6);
    failure(source, options(capture - 1), Kind::CaptureLimit);
    scarp::check_with_options(source, options(capture)).unwrap();

    let setup = "bind s=text_repeat(\"x\",256); bind a=[array:\"a\",s]; bind b=[array:\"b\",s];";
    let control = format!("{setup} print(false);");
    let work = |steps| {
        let mut options = ExecutionOptions::default();
        options.limits.max_steps = steps as u64;
        options
    };
    let baseline = minimum(|steps| scarp::check_with_options(&control, work(steps)).is_ok());
    let source = format!("{setup} print(a==b);");
    let error = failure(&source, work(baseline + 20), Kind::ExecutionLimit);
    assert_eq!(&source[error.span.start..error.span.end], "==");
    let steps = minimum(|steps| scarp::check_with_options(&source, work(steps)).is_ok());
    failure(&source, work(steps - 1), Kind::ExecutionLimit);
    scarp::check_with_options(&source, work(steps)).unwrap();
    assert_eq!(run(&source).0, b"false\n");
}

#[test]
fn generated_fixed_arrays_match_rust_equality_and_indexing() {
    use std::fmt::Write;

    fn literal(values: &[i64]) -> String {
        if values.is_empty() {
            return "[array: <integer>]".into();
        }
        let members = values.iter().map(i64::to_string).collect::<Vec<_>>();
        format!("[array: {}]", members.join(","))
    }

    let mut source = String::new();
    let mut expected = String::new();
    let mut state = 42u64;
    for case in 0..128 {
        let length = case % 17;
        let values = (0..length)
            .map(|_| {
                state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
                (state >> 32) as i32 as i64
            })
            .collect::<Vec<_>>();
        let mut other = values.clone();
        let index = case / 17 % length.max(1);
        if case % 2 == 1 && !other.is_empty() {
            other[index] += 1;
        }
        let left = literal(&values);
        let right = literal(&other);
        writeln!(
            source,
            "{{ bind a={left}; bind b={right}; print(a==b,a!=b);"
        )
        .unwrap();
        writeln!(expected, "{} {}", values == other, values != other).unwrap();
        if let Some(value) = values.get(index) {
            writeln!(source, "bind index={index}; print(a[index]);").unwrap();
            writeln!(expected, "{value}").unwrap();
        }
        writeln!(source, "}}").unwrap();
    }
    assert_eq!(run(&source).0, expected.as_bytes());
}
