use scarp::{
    ast::{ExpressionKind as E, PrintArgument, Statement},
    diagnostics::{Diagnostic, DiagnosticKind as Kind, Span},
    interpreter,
    types::{
        FunctionEffect, PrimitiveType as P, SourceType, TypeKind, MAX_TUPLE_ARITY, MAX_TUPLE_DEPTH,
    },
    value::{TupleValue, Value},
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
        panic!("expected a diagnostic")
    };
    assert_eq!(actual, expected);
    assert_eq!(actual.kind, kind, "{actual:?}: {source}");
    assert!(out.is_empty() && err.is_empty());
    actual
}

#[test]
fn heterogeneous_singletons_groups_and_nested_projections_execute() {
    let source = r#"bind value = (10,"hello",true,2.5); print(value,value[0],value[1],value[2],value[3]); print((42,), (42), ((1,"x"),(false,))[0][1]);"#;
    assert_eq!(
        run(source),
        (
            b"(10, \"hello\", true, 2.5) 10 hello true 2.5\n(42,) 42 x\n".to_vec(),
            Vec::new()
        )
    );
    let parsed = scarp::parse("print((42,), (42));").unwrap();
    let Statement::Output { arguments, .. } = &parsed.statements[0] else {
        panic!()
    };
    for (arg, tuple) in arguments.iter().zip([true, false]) {
        let PrintArgument::Expression(expr) = arg else {
            panic!()
        };
        assert_eq!(
            matches!(expr.nodes()[expr.root()].kind, E::Tuple { .. }),
            tuple
        );
    }
}

#[test]
fn explicit_nested_types_accept_adjacent_angle_assignment_and_shift_tokens() {
    let source = r#"bind a: tuple<integer>=(1,); bind b: tuple<tuple<integer>>=((2,),); bind c: tuple<tuple<tuple<string,>>>=((("x",),),); print(a,b,c,c[0][0][0]); var n=8; n>>=1; n>>>=1; print(n,8>>1,8>>>1,8>=1);"#;
    assert_eq!(
        run(source).0,
        b"(1,) ((2,),) (((\"x\",),),) x\n2 4 4 true\n"
    );
}

#[test]
fn exact_structural_types_govern_annotations_assignments_arguments_and_returns() {
    for source in [
        "bind x: tuple<integer,string>=(1,true);",
        "bind x: tuple<integer>=(1.0,);",
        "var x=(1,\"x\"); x=(1.0,\"x\");",
        "var x=(1,); x=(1,2);",
        "funcra f(x:tuple<integer>)->unit {} f((1.0,));",
        "print((1,)==(1.0,));",
        "print((1,)<(2,));",
        "print((1,)+(2,));",
        "var x=(1,); x+=(2,);",
    ] {
        failure(source, Default::default(), Kind::TypeMismatch);
    }
    failure(
        "funcra f()->tuple<integer>{return (true,);} f();",
        Default::default(),
        Kind::InvalidReturn,
    );
    failure(
        "bind x=(1,); x=(2,);",
        Default::default(),
        Kind::ImmutableAssignment,
    );
}

#[test]
fn mutable_whole_values_share_immutable_snapshots_and_shadowing_is_lexical() {
    let source = r#"var x=(1,"old"); bind copy=x; print(x); {bind x=(false,); print(x);} x=(2,"new"); print(copy,x);"#;
    assert_eq!(
        run(source).0,
        b"(1, \"old\")\n(false,)\n(1, \"old\") (2, \"new\")\n"
    );
}

#[test]
fn tuples_cross_named_defaults_returns_recursive_frames_and_contracts() {
    let source = r#"
    pure funcra descend(n:integer, value:tuple<integer,string>=(9,"x"))->tuple<integer,string>
      needs n>=0; ensures result==value; { if n==0 {return value;} return descend(n-1,value:value); }
    funcra show(value:tuple<integer,string>)->unit { print(value); }
    show(descend(40)); print(descend(value:(7,"y"),n:2)[1]);
    "#;
    assert_eq!(run(source).0, b"(9, \"x\")\ny\n");
    failure("funcra effect()->integer {print(1);return 1;} pure funcra bad()->tuple<integer>{return (effect(),);}",Default::default(),Kind::ImpureFunction);
    failure(
        "funcra effect()->integer {return 1;} funcra bad(x:tuple<integer>=(effect(),))->unit {}",
        Default::default(),
        Kind::ImpureFunction,
    );
}

#[test]
fn structural_equality_handles_nested_text_numerical_collections_and_signed_zero() {
    assert_eq!(run(r#"print((1,("x",true),[1.0],[integer:2])==(1,("x",true),[1.0],[integer:2])); print((0.0,)==(-0.0,), (1,"a")!=(1,"b"));"#).0,b"true\ntrue true\n");
    assert_eq!(
        run("bind values=([1.0,2.0],[integer:4,5]); print(values[0][1],values[1][0]);").0,
        b"2.0 4\n"
    );
}

#[test]
fn invalid_tuple_indices_are_static_precise_and_vector_indices_stay_dynamic() {
    for index in ["-1", "1", "2", "true", "1.0", "0+0", "n"] {
        let source = format!("// 🌍\nprint(\"before\"); bind n=0; print((7,)[{index}]);");
        let error = failure(&source, Default::default(), Kind::TupleIndex);
        assert_eq!(&source[error.span.start..error.span.end], index);
    }
    assert_eq!(
        run("bind n=0; print((7,)[(0)],[1.0][n],[integer:2][n]);").0,
        b"7 1.0 2\n"
    );
    failure(
        "if false {print((1,)[2]);}",
        Default::default(),
        Kind::TupleIndex,
    );
    assert!(scarp::compile("print((1/0,)[0]);").is_ok());
}

#[test]
fn malformed_empty_unit_and_unimplemented_mutation_forms_fail_cleanly() {
    for source in [
        "print(());",
        "print((1,,));",
        "print((1 2));",
        "print((,1));",
        "print((1,2);",
        "bind x: tuple<>=(1,);",
        "bind x:tuple<integer string>=(1,\"x\");",
        "bind x:tuple<unit>=(1,);",
        "bind x:tuple<integer=(1,);",
        "var x=(1,); x[0]=2;",
        "bind (a,b)=(1,);",
    ] {
        assert!(scarp::compile(source).is_err(), "{source}");
    }
    failure(
        "funcra f()->unit {} bind x=(f(),);",
        Default::default(),
        Kind::InvalidReturn,
    );
    assert_eq!(run("bind tuple=(1,); pure funcra Tuple(x:integer)->integer{return x;} print(tuple,Tuple(2));").0,b"(1,) 2\n");
}

#[test]
fn display_quotes_nested_strings_but_preserves_top_level_primitives() {
    let source="bind s=\"नमस्ते 🌍\\n\\r\\t\\\\\\\"\0\u{7f}\"; print((s, -0.0, (true,)),s); eprint((\"err\",)); write((1,)); ewrite((false,));";
    let (out, err) = run(source);
    assert_eq!(
        out,
        "(\"नमस्ते 🌍\\n\\r\\t\\\\\\\"\\0\\u{7f}\", -0.0, (true,)) नमस्ते 🌍\n\r\t\\\"\0\u{7f}\n(1,)"
            .as_bytes()
    );
    assert_eq!(err, b"(\"err\",)\n(false,)");
}

#[test]
fn inferred_ids_are_canonical_inside_one_registry_and_shapes_are_portable() {
    let source="bind a=(1,\"x\"); bind b=(2,\"y\"); pure funcra f(x:tuple<integer,string>)->tuple<integer,string>{return x;} print(f(a));";
    let program = scarp::compile(source).unwrap();
    assert_eq!(program.binding_type_ids()[0], program.binding_type_ids()[1]);
    let tuple = program.binding_type_ids()[0];
    assert!(
        matches!(program.type_registry().get(tuple),Some(TypeKind::Tuple(children)) if children.as_ref()==[P::Integer.id(),P::String.id()])
    );
    assert!(
        matches!(program.type_registry().get(program.function_type_id(0).unwrap()),Some(TypeKind::Function{parameters,result,effect:FunctionEffect::Pure}) if parameters.as_ref()==[tuple] && *result==tuple)
    );
    for expr in program.typed_expressions() {
        for node in expr.nodes() {
            assert!(program.type_registry().get(node.kind).is_some());
        }
    }
    let other = scarp::compile(source).unwrap();
    assert_eq!(program.binding_types(), other.binding_types());
    assert!(program
        .type_registry()
        .get(other.binding_type_ids()[0])
        .is_none());
    assert_eq!(
        program.functions()[0].result.to_string(),
        "tuple<integer, string>"
    );
}

#[test]
fn zero_and_exact_allocation_limits_preserve_sharing_and_atomic_output() {
    let source = "print((1,));";
    let minimum = (0..4096)
        .find(|&limit| {
            scarp::check_with_options(
                source,
                ExecutionOptions {
                    max_tuple_bytes: limit,
                    ..Default::default()
                },
            )
            .is_ok()
        })
        .unwrap();
    assert!(minimum > 0);
    failure(
        source,
        ExecutionOptions {
            max_tuple_bytes: minimum - 1,
            ..Default::default()
        },
        Kind::TupleAllocation,
    );
    let options = ExecutionOptions {
        max_tuple_bytes: minimum,
        ..Default::default()
    };
    scarp::check_with_options(
        "bind tuple=(1,); repeat 5 as i {bind shared=tuple; print(shared);}",
        options,
    )
    .unwrap();
    failure(
        "repeat 2 as i {print((1,));}",
        options,
        Kind::TupleAllocation,
    );
    failure(
        "print(\"before\"); eprint(\"before\"); print((1,));",
        ExecutionOptions {
            max_tuple_bytes: 0,
            ..Default::default()
        },
        Kind::TupleAllocation,
    );
}

#[test]
fn output_and_equality_prepaid_work_and_snapshot_budgets_match_check() {
    let source = "bind value=(\"hello\",(1,true)); print(value,value==value);";
    let minimum = (0..4096)
        .find(|&steps| {
            let mut options = ExecutionOptions::default();
            options.limits.max_steps = steps;
            scarp::check_with_options(source, options).is_ok()
        })
        .unwrap();
    let mut options = ExecutionOptions::default();
    options.limits.max_steps = minimum - 1;
    failure(source, options, Kind::ExecutionLimit);
    options.limits.max_steps = minimum;
    scarp::run_with_options(source, &mut Vec::new(), options).unwrap();
    failure(
        source,
        ExecutionOptions {
            max_capture_bytes: 1,
            ..Default::default()
        },
        Kind::CaptureLimit,
    );
    let mut low = ExecutionOptions::default();
    low.limits.max_steps = 100;
    failure(
        "bind value=(text_repeat(\"x\",100),); print(value==value);",
        low,
        Kind::ExecutionLimit,
    );
}

#[test]
fn arity_depth_type_complexity_and_host_construction_have_real_bounds() {
    assert!(TupleValue::new(vec![]).is_none());
    assert!(TupleValue::new(vec![Value::Unit]).is_none());
    assert!(TupleValue::new(vec![Value::Integer(1); MAX_TUPLE_ARITY + 1]).is_none());
    for arity in [1, MAX_TUPLE_ARITY] {
        let elements = std::iter::repeat("1")
            .take(arity)
            .collect::<Vec<_>>()
            .join(",");
        assert!(scarp::compile(&format!("bind x=({elements},);")).is_ok());
    }
    let bad = std::iter::repeat("1")
        .take(MAX_TUPLE_ARITY + 1)
        .collect::<Vec<_>>()
        .join(",");
    failure(
        &format!("print(({bad},));"),
        Default::default(),
        Kind::TupleLimit,
    );
    let mut value = Value::Integer(1);
    for _ in 0..MAX_TUPLE_DEPTH {
        value = Value::Tuple(TupleValue::new(vec![value]).unwrap());
    }
    assert!(TupleValue::new(vec![value]).is_none());
    let mut shape = SourceType::from(P::Integer);
    for _ in 0..MAX_TUPLE_DEPTH {
        shape = SourceType::tuple(vec![shape]).unwrap();
    }
    assert!(SourceType::tuple(vec![shape]).is_none());
    let mut wide = "(1,)".to_string();
    for _ in 0..3 {
        wide = format!(
            "({})",
            std::iter::repeat(wide)
                .take(32)
                .collect::<Vec<_>>()
                .join(",")
        );
    }
    failure(
        &format!("bind x={wide};"),
        Default::default(),
        Kind::TupleLimit,
    );
}

#[test]
fn legal_nesting_executes_on_a_small_host_stack_without_recursive_interpreter_frames() {
    std::thread::Builder::new()
        .stack_size(256 * 1024)
        .spawn(|| {
            let mut ty = "integer".to_string();
            let mut value = "1".to_string();
            for _ in 0..MAX_TUPLE_DEPTH {
                ty = format!("tuple<{ty}>");
                value = format!("({value},)");
            }
            let source = format!("bind value:{ty}={value}; print(value==value);");
            assert_eq!(run(&source).0, b"true\n");
        })
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn tuple_owners_survive_retirement_return_and_concurrent_reuse() {
    let program=scarp::compile("pure funcra f()->tuple<integer,string>{{bind local=(7,\"kept\");return local;}} print(f());").unwrap();
    std::thread::scope(|scope| {
        for _ in 0..8 {
            let program = &program;
            scope.spawn(move || {
                for _ in 0..10 {
                    let mut out = Vec::new();
                    interpreter::execute(program, &mut out).unwrap();
                    assert_eq!(out, b"(7, \"kept\")\n");
                }
            });
        }
    });
}

#[test]
fn old_loops_vectors_bit_operations_and_capture_snapshots_are_compatible() {
    let source="var sum=0; repeat 6 as i {if i==1{continue;} if i==5{break;} sum+=i;} var n=0;while n<2{n+=1;} print((sum,n),[1.0,2.0][1],[integer:3][0],8>>1);";
    assert_eq!(run(source).0, b"(9, 2) 2.0 3 4\n");
}

#[test]
fn all_native_tuple_forms_are_explicitly_rejected_before_artifact_creation() {
    for source in [
        "bind value=(1,);",
        "bind value=(1,)[0];",
        "pure funcra unused(x:tuple<integer>)->integer{return x[0];} bind value=1;",
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

#[test]
fn seeded_heterogeneous_tuple_properties_match_independent_primitive_oracles() {
    let mut seed = 0xa55a_2026_1110_u64;
    for _ in 0..256 {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        let n = (seed % 20001) as i64 - 10000;
        let flag = seed & 1 != 0;
        let text = if flag { "é🌍" } else { "नमस्ते" };
        let source=format!("bind t=({n},\"{text}\",{flag}); print(t,t[0]=={n},t[1]==\"{text}\",t[2]=={flag},t==({n},\"{text}\",{flag}));");
        let expected = format!("({n}, \"{text}\", {flag}) true true true true\n");
        assert_eq!(run(&source).0, expected.as_bytes());
    }
}

#[test]
fn tuple_nodes_keep_original_unicode_spans_and_postorder_children() {
    let source = "// 🌍\r\nbind x=(1,(\"é\",true)); print(x[1][0]);";
    let parsed = scarp::parse(source).unwrap();
    let Statement::Declaration { initializer, .. } = &parsed.statements[0] else {
        panic!()
    };
    for (index, node) in initializer.nodes().iter().enumerate() {
        if let E::Tuple { elements } = &node.kind {
            assert!(elements.iter().all(|&child| child < index));
            assert_eq!(&source[node.span.start..node.span.start + 1], "(");
            assert_eq!(&source[node.span.end - 1..node.span.end], ")");
        }
    }
    let invalid = "// 🌍\r\nprint((1,)[2]);";
    let error = failure(invalid, Default::default(), Kind::TupleIndex);
    assert_eq!(
        error.span,
        Span::new(invalid.find('2').unwrap(), invalid.find('2').unwrap() + 1)
    );
}

#[test]
fn all_five_output_forms_accept_tuple_values_with_their_existing_stream_rules() {
    for (form, newline, stderr) in [
        ("print", true, false),
        ("println", true, false),
        ("write", false, false),
        ("eprint", true, true),
        ("ewrite", false, true),
    ] {
        let (out, err) = run(&format!("{form}((1,2));"));
        let expected = if newline {
            b"(1, 2)\n".as_slice()
        } else {
            b"(1, 2)".as_slice()
        };
        assert_eq!(
            if stderr {
                err.as_slice()
            } else {
                out.as_slice()
            },
            expected
        );
        assert!(if stderr {
            out.is_empty()
        } else {
            err.is_empty()
        });
    }
}

#[test]
fn shared_nested_tuple_payload_releases_when_its_last_projection_drops() {
    use std::sync::Arc;
    let text: Arc<str> = Arc::from("retained");
    let inner = Value::Tuple(TupleValue::new(vec![Value::String(text.clone())]).unwrap());
    let outer = TupleValue::new(vec![inner]).unwrap();
    assert_eq!(Arc::strong_count(&text), 2);
    let alias = outer.clone();
    let projected = alias.elements()[0].clone();
    assert_eq!(Arc::strong_count(&text), 2);
    drop(outer);
    drop(alias);
    assert_eq!(Arc::strong_count(&text), 2);
    assert_eq!(projected.to_string(), "(\"retained\",)");
    drop(projected);
    assert_eq!(Arc::strong_count(&text), 1);
}
