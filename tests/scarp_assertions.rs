//! End-to-end contracts for the real, checked Scarp assertion statement.
//! Assertions must use the same compiler, source maps and interpreter as other
//! statements. No output-based or arithmetic-error assertion emulation.

use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::atomic::{AtomicUsize, Ordering},
};

static NEXT: AtomicUsize = AtomicUsize::new(0);

struct Workspace(PathBuf);

impl Workspace {
    fn new() -> Self {
        let serial = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "scarp-native-assert-tests-{}-{serial}",
            std::process::id()
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn source(&self, name: &str, code: &str) -> PathBuf {
        let file = self.0.join(name);
        fs::write(&file, code).unwrap();
        file
    }

    fn project(&self, name: &str) -> PathBuf {
        let directory = self.0.join(name);
        let result = Command::new(env!("CARGO_BIN_EXE_scarp"))
            .arg("init")
            .arg(&directory)
            .output()
            .unwrap();
        assert!(result.status.success(), "{result:?}");
        directory
    }
}

impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn cli(args: &[&str], cwd: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_scarp"))
        .args(args)
        .current_dir(cwd)
        .output()
        .unwrap()
}

fn file_cli(command: &str, file: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_scarp"))
        .arg(command)
        .arg(file)
        .output()
        .unwrap()
}

#[test]
fn assertions_pass_in_root_program_loops_and_pure_functions() {
    let ws = Workspace::new();
    let source = ws.source(
        "pass.srp",
        r#"pure funcra positive(value: integer) -> integer {
    assert(value > 0, "positive integer required");
    return value;
}
var total = 0;
repeat 3 as i {
    assert(i >= 0 and i < 3);
    total += positive(1);
}
assert(total == 3);
print(total);
"#,
    );
    let result = file_cli("exec", &source);
    assert!(result.status.success(), "{result:?}");
    assert_eq!(result.stdout, b"3\n");
    assert!(result.stderr.is_empty());
    assert!(file_cli("verify", &source).status.success());
}

#[test]
fn failing_assertion_is_a_dedicated_diagnostic_without_program_stdout() {
    let ws = Workspace::new();
    let source = ws.source(
        "failed.srp",
        "print(\"before\");\nassert(1 == 2, \"a useful explanation\");\nprint(\"after\");\n",
    );
    let result = file_cli("exec", &source);
    assert_eq!(result.status.code(), Some(1), "{result:?}");
    assert!(result.stdout.is_empty(), "{result:?}");
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(stderr.contains("E0062"), "{stderr}");
    assert!(stderr.contains("a useful explanation"), "{stderr}");
    assert!(stderr.contains("failed.srp"), "{stderr}");

    // Static validation must not evaluate dynamic boolean predicates.
    let verified = file_cli("verify", &source);
    assert!(verified.status.success(), "{verified:?}");
    assert!(verified.stdout.is_empty());
}

#[test]
fn failed_nested_assertions_preserve_runtime_source_location() {
    let ws = Workspace::new();
    let root = ws.project("multimodule");
    fs::write(
        root.join("src/math.srp"),
        "pub pure funcra check(value: integer) -> integer {\n    assert(value > 0, \"must be positive\");\n    return value;\n}\n",
    )
    .unwrap();
    fs::write(
        root.join("src/main.srp"),
        "import \"math.srp\" as math;\nprint(math.check(-1));\n",
    )
    .unwrap();
    let result = cli(&["run"], &root);
    assert_eq!(result.status.code(), Some(1), "{result:?}");
    assert!(result.stdout.is_empty());
    let diagnostic = String::from_utf8_lossy(&result.stderr);
    assert!(diagnostic.contains("E0062"), "{diagnostic}");
    assert!(diagnostic.contains("math.srp"), "{diagnostic}");
    assert!(diagnostic.contains("must be positive"), "{diagnostic}");
}

#[test]
fn assert_conditions_are_statically_boolean_even_in_unreachable_code() {
    let ws = Workspace::new();
    for (index, code) in [
        "assert(1);\n",
        "assert(\"not a boolean\");\n",
        "if false { assert(42); }\n",
        "pure funcra bad() -> unit { assert(3); return; }\n",
    ]
    .iter()
    .enumerate()
    {
        let source = ws.source(&format!("wrong-type-{index}.srp"), code);
        let verified = file_cli("verify", &source);
        assert_eq!(verified.status.code(), Some(1), "{code}: {verified:?}");
        assert!(verified.stdout.is_empty());
        assert!(
            String::from_utf8_lossy(&verified.stderr).contains("E0015"),
            "{code}: {verified:?}"
        );
    }
}

#[test]
fn assertion_syntax_requires_one_predicate_and_literal_optional_message() {
    let ws = Workspace::new();
    for (index, source) in [
        "assert();",
        "assert(true, 123);",
        "assert(true, false);",
        "assert(true, \"one\", \"two\");",
        "assert(true)",
        "assert true;",
        "assert(true, \"unterminated);",
    ]
    .iter()
    .enumerate()
    {
        let file = ws.source(&format!("malformed-{index}.srp"), source);
        let verified = file_cli("verify", &file);
        assert_eq!(verified.status.code(), Some(1), "{source}: {verified:?}");
        assert!(verified.stdout.is_empty());
    }
}

#[test]
fn assertion_messages_are_bounded_and_reserved_names_rejected() {
    let ws = Workspace::new();
    let max_message = "x".repeat(256);
    let accepted = ws.source(
        "max-message.srp",
        &format!("assert(true, \"{max_message}\");"),
    );
    assert!(file_cli("verify", &accepted).status.success());
    let long = ws.source(
        "too-long.srp",
        &format!("assert(true, \"{}\");", "x".repeat(257)),
    );
    let rejected = file_cli("verify", &long);
    assert_eq!(rejected.status.code(), Some(1), "{rejected:?}");
    assert!(String::from_utf8_lossy(&rejected.stderr).contains("256"));
    for code in [
        "bind assert = true;",
        "funcra assert() -> unit { return; }",
    ] {
        let source = ws.source("reserved.srp", code);
        let result = file_cli("verify", &source);
        assert_eq!(result.status.code(), Some(1), "{code}: {result:?}");
        assert!(String::from_utf8_lossy(&result.stderr).contains("E0012"));
    }
}

#[test]
fn short_circuit_and_execution_budgets_remain_enforced() {
    let ws = Workspace::new();
    let pass = ws.source(
        "short-circuit-pass.srp",
        "assert(true or (1 / 0 == 0));\n",
    );
    assert!(file_cli("exec", &pass).status.success());
    let fail = ws.source(
        "short-circuit-fail.srp",
        "assert(false and (1 / 0 == 0));\n",
    );
    let result = file_cli("exec", &fail);
    assert_eq!(result.status.code(), Some(1), "{result:?}");
    let diagnostic = String::from_utf8_lossy(&result.stderr);
    assert!(diagnostic.contains("E0062"), "{diagnostic}");
    assert!(!diagnostic.contains("E0008"), "{diagnostic}");

    let limited = Command::new(env!("CARGO_BIN_EXE_scarp"))
        .args(["exec", "--max-steps", "0"])
        .arg(&pass)
        .output()
        .unwrap();
    assert_eq!(limited.status.code(), Some(1), "{limited:?}");
    assert!(String::from_utf8_lossy(&limited.stderr).contains("E0017"));
}

#[test]
fn project_test_reports_include_assertion_diagnostics() {
    let ws = Workspace::new();
    let root = ws.project("suite");
    fs::write(root.join("tests/ok.srp"), "assert(2 + 2 == 4);").unwrap();
    fs::write(
        root.join("tests/fail.srp"),
        "assert(false, \"important test failure\");",
    )
    .unwrap();
    let result = cli(&["test", "--json"], &root);
    assert_eq!(result.status.code(), Some(1), "{result:?}");
    assert!(result.stderr.is_empty(), "{result:?}");
    let report = String::from_utf8_lossy(&result.stdout);
    assert!(report.contains("\"discovered\":2"), "{report}");
    assert!(report.contains("\"passed\":1,\"failed\":1"), "{report}");
    assert!(report.contains("E0062"), "{report}");
    assert!(report.contains("important test failure"), "{report}");
}

#[test]
fn assertion_diagnostic_is_explained_by_cli() {
    let ws = Workspace::new();
    let result = cli(&["explain", "E0062"], &ws.0);
    assert!(result.status.success(), "{result:?}");
    assert!(String::from_utf8_lossy(&result.stdout).contains("assert"));
}

#[test]
fn escaped_failure_message_cannot_insert_extra_diagnostic_lines() {
    let ws = Workspace::new();
    let source = ws.source(
        "escaped-message.srp",
        "assert(false, \"line\\nwith\\ttabs\");\n",
    );
    let result = file_cli("exec", &source);
    assert_eq!(result.status.code(), Some(1), "{result:?}");
    let text = String::from_utf8_lossy(&result.stderr);
    assert!(text.contains("E0062"), "{text}");
    assert!(text.contains("line\\nwith\\ttabs"), "{text}");
    assert!(!text.contains("line\nwith\ttabs"), "{text}");
}

#[test]
fn experimental_c_backend_explicitly_rejects_assertion_statements() {
    let failure = scarp::c_backend::emit_c("assert(true);").unwrap_err();
    assert_eq!(
        failure.kind,
        scarp::diagnostics::DiagnosticKind::NativeUnsupported
    );
}

#[test]
fn parser_and_source_rebasing_retain_real_assertion_spans() {
    let source = "print(\"नमस्ते\");\nassert(false, \"check\");\n";
    let parsed = scarp::parse(source).unwrap();
    assert_eq!(parsed.statements.len(), 2);
    let span = parsed.statements[1].span();
    let location = scarp::diagnostics::location(source, span.start);
    assert_eq!(location.line, 2);
    assert_eq!(location.column, 1);
    assert_eq!(&source[span.start..span.start + 6], "assert");
}
