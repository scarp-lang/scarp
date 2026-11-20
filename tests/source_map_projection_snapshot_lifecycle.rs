use scarp::diagnostics::DiagnosticKind;

const SOURCE: &str = include_str!("../examples/map_projection_snapshot_lifecycle.srp");

#[test]
fn projections_preserve_original_snapshot_through_replacement_and_removal() {
    let expected = concat!(
        "[list: \"alpha\", \"mu\", \"zeta\"]\n",
        "[list: 1, 3, 6]\n",
        "[list: \"alpha\", \"mu\", \"zeta\"]\n",
        "[list: 10, 3, 6]\n",
        "[list: \"alpha\", \"zeta\"]\n",
        "[list: 10, 6]\n",
        "3 3 2\n",
        "1 10\n",
        "true false\n",
    );
    for _ in 0..8 {
        let mut output = Vec::new();
        scarp::run(SOURCE, &mut output).expect("source map lifecycle should execute");
        assert_eq!(output, expected.as_bytes());
    }
}

#[test]
fn native_backend_explicitly_rejects_map_projection_lifecycle() {
    let program = scarp::compile(SOURCE).expect("source should compile for interpreter");
    let error = scarp::c_backend::emit_program(&program).unwrap_err();
    assert_eq!(error.kind, DiagnosticKind::NativeUnsupported);
}

#[test]
fn empty_map_projections_are_typed_and_deterministic() {
    let source = r#"
        bind empty: map<string, integer> = [map: <string, integer>];
        bind keys: list<string> = map_keys(empty);
        bind values: list<integer> = map_values(empty);
        print(keys);
        print(values);
        print(map_len(empty));
    "#;
    let mut output = Vec::new();
    scarp::run(source, &mut output).expect("empty typed map projections should execute");
    assert_eq!(output, b"[list: <string>]\n[list: <integer>]\n0\n");
}
