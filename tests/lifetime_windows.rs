use scarp::{diagnostics::DiagnosticKind, ExecutionOptions};

fn output(source: &str) -> String {
    let mut result = Vec::new();
    scarp::run(source, &mut result).unwrap();
    scarp::check(source).unwrap();
    String::from_utf8(result).unwrap()
}

#[test]
fn empty_vector_windows_preserve_source_semantics() {
    let source = r#"
        bind original = [1.0, 2.0, 3.0];
        bind beginning = window(original, 0, 0);
        bind end = window(original, 3, 0);
        bind middle = window(original, 1, 2);
        bind nested = window(middle, 2, 0);
        print(beginning, end, nested);
        print(middle);
    "#;
    assert_eq!(output(source), "[] [] []\n[2.0, 3.0]\n");
}

#[test]
fn nonempty_window_survives_binding_reassignment_and_scope_exit() {
    let source = r#"
        var values = [1.0, 2.0, 3.0];
        bind selected = window(values, 1, 2);
        {
            bind empty = window(values, 1, 0);
            print(empty);
        }
        values = [9.0, 9.0, 9.0];
        print(selected);
        print(values);
    "#;
    assert_eq!(output(source), "[]\n[2.0, 3.0]\n[9.0, 9.0, 9.0]\n");
}

#[test]
fn creating_empty_windows_does_not_recharge_vector_payload_quota() {
    let source = r#"
        bind values = [1.0, 2.0, 3.0];
        repeat 12 as i {
            bind empty = window(values, i, 0);
            print(len(empty));
        }
    "#;
    let options = ExecutionOptions {
        max_vector_bytes: 3 * 8,
        ..ExecutionOptions::default()
    };
    scarp::check_with_options(source, options).unwrap();
    let insufficient = ExecutionOptions {
        max_vector_bytes: 3 * 8 - 1,
        ..ExecutionOptions::default()
    };
    assert_eq!(
        scarp::check_with_options(source, insufficient)
            .unwrap_err()
            .kind,
        DiagnosticKind::VectorBudget
    );
}
