use scarp::{
    diagnostics::DiagnosticKind,
    types::{SourceType, TypeKind},
    value::ValueType,
    ExecutionOptions,
};

fn output(source: &str) -> String {
    let mut bytes = Vec::new();
    scarp::run(source, &mut bytes).unwrap();
    String::from_utf8(bytes).unwrap()
}

#[test]
fn canonical_map_golden_is_executable() {
    assert_eq!(
        output(include_str!("../examples/typed_maps.srp")).as_bytes(),
        include_bytes!("../examples/typed_maps.stdout")
    );
}

#[test]
fn map_keys_are_sorted_independent_of_input_order_and_equality() {
    let source = r#"
        bind a = [map: 9: "z", -2: "minus", 0: "zero"];
        bind b = [map: 0: "zero", 9: "z", -2: "minus"];
        print(a == b);
        print(a);
        bind c = [map: false: 0, true: 1];
        print(c);
    "#;
    assert_eq!(
        output(source),
        "true\n[map: -2: \"minus\", 0: \"zero\", 9: \"z\"]\n[map: false: 0, true: 1]\n"
    );
}

#[test]
fn empty_typed_maps_preserve_distinct_key_and_value_types() {
    let program = scarp::compile(
        "bind a: map<integer, string> = [map: <integer, string>]; \
         bind b: map<boolean, string> = [map: <boolean, string>]; \
         bind c: map<integer, integer> = [map: <integer, integer>];",
    )
    .unwrap();
    let bindings = program.binding_types();
    assert_ne!(bindings[0], bindings[1]);
    assert_ne!(bindings[0], bindings[2]);
    let mapping_count = program
        .type_registry()
        .iter()
        .filter(|(_, kind)| matches!(kind, TypeKind::Mapping { .. }))
        .count();
    assert_eq!(mapping_count, 3);
    assert_eq!(
        bindings[0].as_map().unwrap().key(),
        &SourceType::from(ValueType::Integer)
    );
}

#[test]
fn map_values_support_nested_typed_composites_and_pure_functions() {
    let source = r#"
        record Item { title: string, done: boolean }
        pure funcra make() -> map<string, list<record Item>> {
            return [map:
                "first": [list: new Item { title: "A", done: true }],
                "second": [list: new Item { title: "B", done: false }]
            ];
        }
        bind result = make();
        print(result);
    "#;
    assert!(output(source).contains("new Item"));
}

#[test]
fn invalid_map_keys_and_inhomogeneous_entries_are_statically_rejected() {
    for source in [
        "bind bad = [map: 1.0: 2];",
        "bind bad = [map: [list: 1]: 3];",
        "bind bad: map<float, integer> = [map: <float, integer>];",
    ] {
        let error = scarp::compile(source).unwrap_err();
        assert!(
            matches!(error.kind, DiagnosticKind::MapLimit | DiagnosticKind::UnexpectedToken),
            "{source}: {error:?}"
        );
    }
    for source in [
        "bind bad = [map: 1: 10, 2: \"text\"];",
        "bind bad = [map: 1: 10, \"two\": 20];",
    ] {
        let error = scarp::compile(source).unwrap_err();
        assert_eq!(error.kind, DiagnosticKind::TypeMismatch, "{source}");
    }
}

#[test]
fn duplicate_keys_are_detected_after_expression_evaluation() {
    for source in [
        "bind m = [map: 2: \"first\", 1+1: \"second\"];",
        "bind m = [map: \"café\": 1, \"café\": 2];",
        "bind m = [map: true: 1, true: 2];",
    ] {
        let mut bytes = Vec::new();
        let error = scarp::run(source, &mut bytes).unwrap_err();
        assert!(format!("{error:?}").contains("MapKey"), "{error:?}");
        assert!(bytes.is_empty(), "no output on duplicate key");
    }
}

#[test]
fn map_allocation_consumes_the_shared_array_list_map_quota() {
    let source = "print(\"before\"); bind m = [map: 1: 2, 3: 4]; print(m);";
    let mut bytes = Vec::new();
    let error = scarp::run_with_options(
        source,
        &mut bytes,
        ExecutionOptions {
            max_array_bytes: 0,
            ..ExecutionOptions::default()
        },
    )
    .unwrap_err();
    assert!(format!("{error:?}").contains("MapAllocation"));
    assert!(bytes.is_empty());
}

#[test]
fn map_work_limit_fails_without_publishing_partial_output() {
    let source = r#"
        print("before");
        bind m = [map:
            "zzzz": 1,
            "yyyy": 2,
            "xxxx": 3,
            "wwww": 4
        ];
        print(m);
    "#;
    let mut bytes = Vec::new();
    let error = scarp::run_with_options(
        source,
        &mut bytes,
        ExecutionOptions {
            limits: scarp::ExecutionLimits {
                max_steps: 6,
                max_prints: 100,
            },
            ..ExecutionOptions::default()
        },
    );
    assert!(error.is_err());
    assert!(bytes.is_empty());
}

#[test]
fn utf8_map_keys_are_ordered_by_unicode_utf8_scalar_encoding() {
    let source = r#"
        bind m = [map: "न": 2, "a": 1, "é": 3];
        print(m);
    "#;
    assert_eq!(output(source), "[map: \"a\": 1, \"é\": 3, \"न\": 2]\n");
}

#[test]
fn malformed_empty_map_requires_key_and_value_annotations() {
    let error = scarp::compile("bind x = [map: ];").unwrap_err();
    assert_eq!(error.kind, DiagnosticKind::UnexpectedToken);
    let error = scarp::compile("bind x = [map: <integer>];").unwrap_err();
    assert_eq!(error.kind, DiagnosticKind::UnexpectedToken);
}

#[test]
fn native_c_backend_reports_unsupported_map_aggregate() {
    let program = scarp::compile("bind m = [map: 2: 3]; print(m);").unwrap();
    assert_eq!(
        scarp::c_backend::emit_program(&program).unwrap_err().kind,
        DiagnosticKind::NativeUnsupported
    );
}
