use scarp::{
    diagnostics::DiagnosticKind,
    types::{SourceType, TypeKind, MAX_LIST_LENGTH},
    value::{ListValue, Value},
    ExecutionOptions,
};

fn run(source: &str) -> String {
    let mut output = Vec::new();
    scarp::run(source, &mut output).unwrap();
    String::from_utf8(output).unwrap()
}

#[test]
fn typed_dynamic_lists_example_is_executable() {
    assert_eq!(
        run(include_str!("../examples/typed_dynamic_lists.srp")).as_bytes(),
        include_bytes!("../examples/typed_dynamic_lists.stdout")
    );
}

#[test]
fn list_type_has_no_fixed_length_but_keeps_exact_element_identity() {
    let program = scarp::compile(
        "bind a: list<integer> = [list: 1];          bind b: list<integer> = [list: 1, 2, 3];          bind c: list<string> = [list: \"one\"];",
    )
    .unwrap();
    let bindings = program.binding_types();
    assert_eq!(bindings[0], bindings[1]);
    assert_ne!(bindings[0], bindings[2]);
}

#[test]
fn nested_lists_and_empty_snapshots_have_deterministic_equality_and_display() {
    let code = r#"
        bind empty: list<string> = [list: <string>];
        bind other: list<string> = [list: <string>];
        bind full = [list: "A", "B"];
        print(empty == other);
        print(empty != full);
        print(full);
        bind nested = [list: [list: 1], [list: 2]];
        print(nested[1][0]);
    "#;
    assert_eq!(
        run(code),
        "true\ntrue\n[list: \"A\", \"B\"]\n2\n"
    );
}

#[test]
fn pure_functions_accept_and_return_immutable_list_snapshots() {
    let code = r#"
        pure funcra transform(input: list<integer>) -> list<integer> {
            return [list: input[0], input[0] + 1];
        }
        bind previous = [list: 5];
        bind updated = transform(previous);
        print(previous[0], updated[0], updated[1]);
    "#;
    assert_eq!(run(code), "5 5 6\n");
}

#[test]
fn nonuniform_list_literals_are_rejected_at_the_mismatching_element() {
    let source = "bind values = [list: 1, \"text\"];";
    let error = scarp::compile(source).unwrap_err();
    assert_eq!(error.kind, DiagnosticKind::TypeMismatch);
    assert_eq!(&source[error.span.start..error.span.end], "\"text\"");
}

#[test]
fn empty_list_requires_explicit_element_type() {
    let err = scarp::compile("bind empty = [list: ];").unwrap_err();
    assert_eq!(err.kind, DiagnosticKind::UnexpectedToken);
    let err = scarp::compile("bind empty: list<unit> = [list: <unit>];").unwrap_err();
    assert!(matches!(err.kind, DiagnosticKind::FunctionDefinition | DiagnosticKind::ListLimit));
}

#[test]
fn dynamic_list_index_checks_negative_and_out_of_range() {
    for index in ["-1", "2"] {
        let source = format!("bind values = [list: 7, 8]; print(values[{index}]);");
        let mut output = Vec::new();
        let error = scarp::run(&source, &mut output).unwrap_err();
        assert!(format!("{error:?}").contains("ListIndex"));
        assert!(output.is_empty());
    }
}

#[test]
fn list_allocation_shares_the_fixed_array_byte_budget() {
    let source = "print(\"begin\"); bind xs = [list: 1, 2, 3]; print(xs);";
    let mut output = Vec::new();
    let error = scarp::run_with_options(
        source,
        &mut output,
        ExecutionOptions {
            max_array_bytes: 0,
            ..ExecutionOptions::default()
        },
    )
    .unwrap_err();
    assert!(format!("{error:?}").contains("ListAllocation"));
    assert!(output.is_empty());
}

#[test]
fn host_list_constructor_requires_matching_type_and_length_limit() {
    let integer = SourceType::from(scarp::value::ValueType::Integer);
    assert!(ListValue::new(integer.clone(), vec![Value::Integer(1)]).is_some());
    assert!(ListValue::new(integer.clone(), vec![Value::from("text")]).is_none());
    assert!(ListValue::new(integer, vec![Value::Integer(0); MAX_LIST_LENGTH + 1]).is_none());
}

#[test]
fn dynamic_list_type_interns_as_unbounded_sequence() {
    let program = scarp::compile("bind values = [list: 10, 20];").unwrap();
    let ty = &program.binding_types()[0];
    let id = program
        .type_registry()
        .iter()
        .find(|(_, kind)| {
            matches!(kind, TypeKind::Sequence { length: None, .. })
        })
        .map(|(id, _)| id);
    assert!(id.is_some(), "list must have unbounded structural sequence type");
    assert_eq!(ty.as_list().unwrap().element(), &SourceType::from(scarp::value::ValueType::Integer));
}
