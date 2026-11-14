//! First source-language bridge to the experimental composite data engine:
 //! flat tuple/fixed-array destructuring with exact types, lexical scoping,
 //! unique immutable or mutable slots and one-time initializer evaluation.
use scarp::{
    ast::Statement,
    diagnostics::{Diagnostic, DiagnosticKind as Kind},
    interpreter, ExecutionOptions, RunError,
};

fn run(source: &str) -> Vec<u8> {
    let mut out = Vec::new();
    scarp::run(source, &mut out).unwrap();
    out
}
fn check_error(source: &str, expected: Kind) -> Diagnostic {
    let error = scarp::check(source).unwrap_err();
    let mut output = Vec::new();
    let RunError::Diagnostic(runtime) = scarp::run(source, &mut output).unwrap_err() else {
        panic!("expected a compilation diagnostic: {source}");
    };
    assert_eq!(error.kind, expected, "unexpected error: {error:?}");
    assert_eq!(runtime.kind, error.kind);
    assert_eq!(runtime.span, error.span);
    assert!(output.is_empty(), "no output after a rejected program");
    error
}

#[test]
fn source_tuple_destructuring_runs_and_preserves_heterogeneous_types() {
    assert_eq!(
        run(r#"bind (number, message, active) = (12, "hello", true);
               print(number, message, active);"#),
        b"12 hello true\n"
    );
}
#[test]
fn source_array_destructuring_runs_with_homogeneous_values() {
    assert_eq!(
        run(r#"bind [first, second, third] = [array: 4, 5, 6];
               print(first + second, third);"#),
        b"9 6\n"
    );
}
#[test]
fn source_singleton_tuple_requires_a_comma() {
    assert_eq!(
        run(r#"bind (item,) = (42,); print(item);"#),
        b"42\n"
    );
    assert_eq!(
        check_error("bind (item) = (42,);", Kind::UnexpectedToken).kind,
        Kind::UnexpectedToken
    );
}
#[test]
fn source_singleton_array_is_not_a_grouped_tuple() {
    assert_eq!(run("bind [item] = [array: 42]; print(item);"), b"42\n");
}
#[test]
fn mutable_destructured_slots_follow_existing_assignment_rules() {
    assert_eq!(
        run(r#"var (number, word) = (2, "yes");
               number += 3; word = "changed"; print(number, word);
               var [a, b] = [array: 3, 7];
               a += b; print(a, b);"#),
        b"5 changed\n10 7\n"
    );
    check_error("bind (first, second) = (1, 2); first = 4;", Kind::ImmutableAssignment);
}
#[test]
fn wildcard_discards_values_without_binding_or_hidden_slots() {
    let compiled = scarp::compile(
        r#"bind (_, second, _) = (true, 9, "unused"); print(second);"#
    ).unwrap();
    assert_eq!(compiled.binding_count(), 1);
    let mut out = Vec::new();
    interpreter::execute(&compiled, &mut out).unwrap();
    assert_eq!(out, b"9\n");
    check_error("bind (x, _) = (1, 2); print(_);", Kind::UnknownName);
}
#[test]
fn multiple_wildcards_still_require_exact_arity() {
    assert_eq!(run("bind (_, _) = (false, true); print(1);"), b"1\n");
    check_error("bind (_, _) = (1, 2, 3);", Kind::TypeMismatch);
}
#[test]
fn tuple_pattern_rejects_fixed_array_initializer_and_vice_versa() {
    check_error("bind (x, y) = [array: 1, 2];", Kind::TypeMismatch);
    check_error("bind [x, y] = (1, 2);", Kind::TypeMismatch);
}
#[test]
fn fixed_integer_vector_is_not_a_typed_fixed_array() {
    check_error("bind [x, y] = [integer: 1, 2];", Kind::TypeMismatch);
}
#[test]
fn wrong_tuple_and_array_lengths_fail_before_output() {
    check_error("bind (x, y, z) = (1, 2);", Kind::TypeMismatch);
    check_error("bind [x, y] = [array: 1];", Kind::TypeMismatch);
    check_error("print(99); bind (x, y) = (1,);", Kind::TypeMismatch);
}
#[test]
fn nested_tuple_values_remain_first_class_after_flat_unpacking() {
    assert_eq!(
        run(r#"bind (pair, message) = ((1, true), "ok");
               print(pair[0], pair[1], message);"#),
        b"1 true ok\n"
    );
}
#[test]
fn fixed_arrays_can_contain_tuple_elements() {
    assert_eq!(
        run(r#"bind [first, second] = [array: (1, true), (2, false)];
               print(first[0], second[1]);"#),
        b"1 false\n"
    );
}
#[test]
fn initializer_executes_only_once_even_with_several_bindings() {
    let source = r#"
        funcra create() -> tuple<integer, integer> {
            print("created");
            return (5, 7);
        }
        bind (left, right) = create();
        print(left + right);
    "#;
    assert_eq!(run(source), b"created\n12\n");
}
#[test]
fn function_arguments_can_be_unpacked_locally() {
    let source = r#"
        pure funcra combine(item: tuple<integer, integer>) -> integer {
            bind (left, right) = item;
            return left * 10 + right;
        }
        print(combine((3, 4)));
    "#;
    assert_eq!(run(source), b"34\n");
}
#[test]
fn function_call_graph_discovers_calls_inside_destructure_initializers() {
    let source = r#"
        pure funcra pair() -> tuple<integer, integer> {
            return (1, 2);
        }
        pure funcra consume() -> integer {
            bind (a, b) = pair();
            return a + b;
        }
        print(consume());
    "#;
    let program = scarp::compile(source).unwrap();
    let consume = program.functions().iter().find(|f| f.name == "consume").unwrap();
    assert!(consume.callees.contains(&"pair".to_owned()));
    assert_eq!(run(source), b"3\n");
}
#[test]
fn repeated_execution_of_precompiled_program_uses_fresh_storage() {
    let compiled = scarp::compile(
        "var [x, y] = [array: 2, 3]; x += y; print(x, y);"
    ).unwrap();
    for _ in 0..4 {
        let mut out = Vec::new();
        interpreter::execute(&compiled, &mut out).unwrap();
        assert_eq!(out, b"5 3\n");
    }
}
#[test]
fn destructured_bindings_shadow_outer_names_only_in_inner_scope() {
    assert_eq!(
        run(r#"bind x = 9; {
            bind (x, y) = (1, 2);
            print(x, y);
        }
        print(x);"#),
        b"1 2\n9\n"
    );
}
#[test]
fn nested_block_scope_retirement_discards_all_destructured_slots() {
    let compiled = scarp::compile(
        "bind root=1; { bind (x,y)=(2,3); print(x+y); } print(root);"
    ).unwrap();
    let mut out = Vec::new();
    interpreter::execute(&compiled, &mut out).unwrap();
    assert_eq!(out, b"5\n1\n");
    assert_eq!(compiled.binding_count(), 3);
}
#[test]
fn duplicate_names_in_same_tuple_or_array_pattern_are_rejected() {
    for source in [
        "bind (x, x) = (1, 2);",
        "var [x, y, x] = [array: 1, 2, 3];",
    ] {
        let err = check_error(source, Kind::DuplicateBinding);
        assert!(err.message.contains("more than once"));
    }
}
#[test]
fn duplicate_names_in_current_scope_are_rejected() {
    for source in [
        "bind x=1; bind (x, y) = (2, 3);",
        "bind y=3; var [x, y] = [array: 1, 2];",
    ] {
        check_error(source, Kind::DuplicateBinding);
    }
}
#[test]
fn reserved_binding_names_fail_exactly_as_normal_declarations() {
    for source in [
        "bind (true, item) = (1, 2);",
        "var [if, item] = [array: 1, 2];",
        "bind (print, name) = (1, 2);",
    ] {
        check_error(source, Kind::ReservedName);
    }
}
#[test]
fn initializer_cannot_refer_to_a_later_pattern_name() {
    check_error("bind (a, b) = (a, 2);", Kind::UnknownName);
    check_error("bind [a, b] = [array: a, 2];", Kind::UnknownName);
}
#[test]
fn inner_scope_initializer_can_see_existing_outer_binding() {
    assert_eq!(
        run("bind a=4; { bind (a,b)=(a+1,2); print(a,b); } print(a);"),
        b"5 2\n4\n"
    );
}
#[test]
fn missing_pattern_element_or_brace_reports_a_syntax_error() {
    for source in [
        "bind (,x) = (1, 2);",
        "bind (x,,y) = (1, 2);",
        "bind [x,] = [array: 1, 2];",
        "bind (x,y) (1,2);",
        "var [x,y] = [array: 1, 2]",
    ] {
        assert!(scarp::compile(source).is_err(), "{source}");
    }
}
#[test]
fn empty_patterns_are_rejected_instead_of_accepting_untyped_empty_tuples() {
    check_error("bind () = (1,);", Kind::UnexpectedToken);
    check_error("bind [] = [array: <integer>];", Kind::UnexpectedToken);
}
#[test]
fn ast_retains_original_member_spans_and_mutability() {
    let source = "bind (alpha, _, gamma) = (1, true, 3);";
    let parsed = scarp::parse(source).unwrap();
    let Statement::Destructure {
        bindings,
        array,
        mutable,
        initializer,
        span,
    } = &parsed.statements[0] else {
        panic!("expected destructuring statement");
    };
    assert!(!array);
    assert!(!mutable);
    assert_eq!(bindings.len(), 3);
    for (name, original) in bindings {
        assert_eq!(&source[original.start..original.end], name);
    }
    assert_eq!(&source[span.start..span.end], source);
    assert!(!initializer.nodes().is_empty());
}
#[test]
fn array_pattern_parses_and_preserves_mutable_flag() {
    let parsed = scarp::parse("var [x, _, y] = [array: 1, 2, 3];").unwrap();
    let Statement::Destructure {
        bindings,
        array,
        mutable,
        ..
    } = &parsed.statements[0] else {
        panic!("expected array destructure");
    };
    assert!(*array && *mutable);
    assert_eq!(bindings.iter().map(|(name,_)|name.as_str()).collect::<Vec<_>>(),
        vec!["x","_","y"]);
}
#[test]
fn max_array_binding_count_is_enforced_in_parser() {
    let names = (0..129).map(|n|format!("value{n}")).collect::<Vec<_>>().join(",");
    let source = format!("bind [{names}] = 1;");
    assert_eq!(scarp::parse(&source).unwrap_err().kind, Kind::ArrayLimit);
}
#[test]
fn max_tuple_binding_count_is_enforced_in_parser() {
    let names = (0..33).map(|n|format!("value{n}")).collect::<Vec<_>>().join(",");
    let source = format!("bind ({names}) = 1;");
    assert_eq!(scarp::parse(&source).unwrap_err().kind, Kind::TupleLimit);
}
#[test]
fn typed_array_pattern_supports_whole_source_parameter() {
    let source = r#"
        pure funcra dot(a: array<integer,2>) -> integer {
            bind [first, second] = a;
            return first * second;
        }
        print(dot([array: 3, 4]));
    "#;
    assert_eq!(run(source), b"12\n");
}
#[test]
fn pattern_declaration_count_is_one_source_statement() {
    let p = scarp::compile("bind (a, b) = (1, 2); print(a,b);").unwrap();
    assert_eq!(p.statement_count(), 2);
    assert_eq!(p.binding_count(), 2);
}
#[test]
fn destructure_does_not_change_unsupported_native_backend_contract() {
    let program = scarp::compile("bind (a, b) = (1, 2); print(a+b);").unwrap();
    assert!(scarp::c_backend::emit_program(&program).is_err());
    assert!(scarp::native_object::emit_elf64_integer_kernel("bind (a,b)=(1,2); print(a);").is_err());
}
#[test]
fn check_and_run_share_the_same_compiled_destructuring_semantics() {
    let source = r#"bind [a, b] = [array: 4, 6]; print(a+b);"#;
    scarp::check(source).unwrap();
    assert_eq!(run(source), b"10\n");
}
#[test]
fn standard_budget_prevents_unbounded_destructuring_computation() {
    let source = r#"bind (a,b,c,d) = (1,2,3,4); print(a+b+c+d);"#;
    let options = ExecutionOptions {
        limits: scarp::ExecutionLimits {
            max_steps: 2,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut out = Vec::new();
    let result = scarp::run_with_options(source, &mut out, options);
    assert!(result.is_err());
    assert!(out.is_empty());
}
