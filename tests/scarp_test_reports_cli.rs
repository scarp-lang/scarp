//! End-to-end contracts for machine-readable test reports and discovery.
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
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("scarp-test-report-cli-{}-{n}", std::process::id()));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn child(&self, label: &str) -> PathBuf {
        self.0.join(label)
    }
}
impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn cli(args: &[&str], directory: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_scarp"))
        .args(args)
        .current_dir(directory)
        .output()
        .unwrap()
}

fn project(workspace: &Workspace, label: &str) -> PathBuf {
    let project = workspace.child(label);
    let output = Command::new(env!("CARGO_BIN_EXE_scarp"))
        .arg("init")
        .arg(&project)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    project
}

#[test]
fn machine_report_has_sorted_results_with_bounded_failure_details() {
    let ws = Workspace::new();
    let root = project(&ws, "structured");
    fs::write(root.join("tests/c.srp"), "print(1 / 0);").unwrap();
    fs::write(root.join("tests/a.srp"), "print(42);").unwrap();
    fs::write(root.join("tests/b.srp"), "print(missing_name);").unwrap();
    let result = cli(&["test", "--json"], &root);
    assert_eq!(result.status.code(), Some(1), "{result:?}");
    assert!(result.stderr.is_empty(), "{result:?}");
    let text = String::from_utf8(result.stdout).unwrap();
    assert!(
        text.starts_with(
            "{\"schema_version\":1,\"mode\":\"run\",\"discovered\":3,\"passed\":1,\"failed\":2,\"tests\":["
        ),
        "{text}"
    );
    let a = text.find("\"path\":\"tests/a.srp\"").unwrap();
    let b = text.find("\"path\":\"tests/b.srp\"").unwrap();
    let c = text.find("\"path\":\"tests/c.srp\"").unwrap();
    assert!(a < b && b < c, "{text}");
    assert!(text.contains("\"status\":\"passed\""));
    assert_eq!(text.matches("\"status\":\"failed\"").count(), 2);
    assert!(text.contains("\"error\":"));
    assert!(!text.contains("42\n"));
    assert!(text.ends_with("]}\n"), "{text}");
}

#[test]
fn v2_report_classifies_failures_and_keeps_unicode_source_coordinates() {
    let ws = Workspace::new();
    let root = project(&ws, "structured हिंदी");
    let statement = r#"assert(false, "line\nnext");"#;
    let line_prefix = "print(\"नमस्ते\"); ";
    let source = format!("// heading\r\n{line_prefix}{statement}\r\n");
    fs::write(root.join("tests/a हिंदी.srp"), &source).unwrap();
    fs::write(root.join("tests/b.srp"), "print(missing_name);\n").unwrap();
    fs::create_dir(root.join("tests/b.stdin")).unwrap();
    fs::write(root.join("tests/c.srp"), "assert(true);\n").unwrap();
    fs::create_dir(root.join("tests/c.stdin")).unwrap();
    fs::write(root.join("tests/d.srp"), "print(\"separate output\");\n").unwrap();

    let args = ["test", "--json=v2", "--require-tests"];
    let result = cli(&args, &root);
    assert_eq!(result.status.code(), Some(1), "{result:?}");
    assert!(result.stderr.is_empty(), "{result:?}");
    assert_eq!(result.stdout, cli(&args, &root).stdout);
    let text = String::from_utf8(result.stdout).unwrap();
    assert!(
        text.starts_with(
            "{\"schema_version\":2,\"mode\":\"run\",\"discovered\":4,\"passed\":1,\"failed\":3,"
        ),
        "{text}"
    );
    assert_eq!(text.lines().count(), 1);
    assert!(!text.contains("separate output") && !text.contains("नमस्ते"));
    assert!(!text.contains("\"error\":"));
    assert!(text.contains("\"path\":\"tests/a हिंदी.srp\",\"status\":\"failed\",\"phase\":\"runtime\",\"diagnostic\":{\"schema_version\":1,\"severity\":\"error\",\"code\":\"E0065\""), "{text}");
    assert!(text.contains(r#"line\\nnext"#), "{text}");
    let start = source.find(statement).unwrap();
    let column = line_prefix.chars().count() + 1;
    assert!(text.contains(&format!(
        "\"primary\":{{\"path\":\"tests/a हिंदी.srp\",\"span\":[{start},{}],\"start\":{{\"line\":2,\"column\":{column}}}",
        start + statement.len(),
    )), "{text}");
    assert!(text.contains("\"path\":\"tests/b.srp\",\"status\":\"failed\",\"phase\":\"compile\""));
    assert!(text.contains("\"code\":\"E0010\""));
    assert!(text.contains("\"path\":\"tests/c.srp\",\"status\":\"failed\",\"phase\":\"fixture\",\"diagnostic\":{\"schema_version\":1,\"severity\":\"error\",\"code\":null"));
    assert!(text.contains("\"primary\":null,\"related\":[],\"help\":null"));
    assert!(text.ends_with("{\"path\":\"tests/d.srp\",\"status\":\"passed\"}]}\n"));
}

#[test]
fn v2_report_keeps_imported_primary_and_related_locations_project_relative() {
    let ws = Workspace::new();
    let root = project(&ws, "import notes");
    fs::write(
        root.join("tests/cycle.srp"),
        "import \"../src/one.srp\" as one;\n",
    )
    .unwrap();
    fs::write(root.join("src/one.srp"), "import \"two.srp\" as two;\n").unwrap();
    fs::write(root.join("src/two.srp"), "import \"one.srp\" as one;\n").unwrap();
    let result = cli(&["test", "--json=v2"], &root);
    assert_eq!(result.status.code(), Some(1), "{result:?}");
    assert!(result.stderr.is_empty());
    let text = String::from_utf8(result.stdout).unwrap();
    assert!(
        text.contains("\"primary\":{\"path\":\"src/two.srp\""),
        "{text}"
    );
    assert!(text.contains("\"related\":[{\"message\":\"this import continues the cycle\",\"location\":{\"path\":\"src/one.srp\""), "{text}");
    assert!(
        text.contains(r#"circular import reaches \"src/one.srp\""#),
        "{text}"
    );
    assert!(!text.contains("import notes"), "{text}");
}

#[test]
fn v2_report_uses_contained_paths_for_source_loading_failures() {
    let ws = Workspace::new();
    let root = project(&ws, "PRIVATE working directory");
    fs::write(
        root.join("tests/missing.srp"),
        "import \"../src/लापता.srp\" as missing;",
    )
    .unwrap();
    fs::write(
        root.join("tests/outside.srp"),
        "import \"../../outside.srp\" as outside;",
    )
    .unwrap();
    fs::write(ws.child("outside.srp"), "print(\"outside source\");").unwrap();

    let result = cli(&["test", "--json=v2"], &root);
    assert_eq!(result.status.code(), Some(1), "{result:?}");
    assert!(result.stderr.is_empty());
    let text = String::from_utf8(result.stdout).unwrap();
    assert_eq!(text.matches("\"phase\":\"compile\"").count(), 2, "{text}");
    assert!(text.contains("could not read 'src/लापता.srp'"), "{text}");
    assert!(text.contains("\"code\":\"E0043\""), "{text}");
    assert!(!text.contains("PRIVATE working directory"), "{text}");
    assert!(!text.contains("outside source"), "{text}");
}

#[test]
fn v2_report_uses_relative_qualified_names_in_linked_diagnostics() {
    let ws = Workspace::new();
    let root = project(&ws, "PRIVATE working directory");
    fs::write(
        root.join("src/लाइब्रेरी.srp"),
        "pub pure funcra broken() -> integer { return 1 / 0; }",
    )
    .unwrap();
    fs::write(
        root.join("tests/runtime.srp"),
        "import \"../src/लाइब्रेरी.srp\" as lib; print(lib.broken());",
    )
    .unwrap();
    fs::write(
        root.join("src/incomplete.srp"),
        "pub pure funcra incomplete() -> integer {}",
    )
    .unwrap();
    fs::write(
        root.join("tests/compile.srp"),
        "import \"../src/incomplete.srp\" as lib;",
    )
    .unwrap();

    let result = cli(&["test", "--json=v2"], &root);
    assert_eq!(result.status.code(), Some(1), "{result:?}");
    assert!(result.stderr.is_empty());
    let text = String::from_utf8(result.stdout).unwrap();
    assert!(text.contains("src/incomplete.srp::incomplete"), "{text}");
    assert!(
        text.contains("called funcra src/लाइब्रेरी.srp::broken here"),
        "{text}"
    );
    assert!(
        text.contains("\"primary\":{\"path\":\"src/लाइब्रेरी.srp\""),
        "{text}"
    );
    assert!(!text.contains("PRIVATE working directory"), "{text}");
}

#[test]
fn v2_report_caps_diagnostic_messages_without_losing_code_or_location() {
    let ws = Workspace::new();
    let root = project(&ws, "bounded");
    let name = "z".repeat(8000);
    fs::write(root.join("tests/bounded.srp"), format!("print({name});")).unwrap();
    let result = cli(&["test", "--json=v2"], &root);
    assert_eq!(result.status.code(), Some(1), "{result:?}");
    assert!(result.stderr.is_empty());
    let text = String::from_utf8(result.stdout).unwrap();
    assert!(text.len() < 2400, "{}", text.len());
    assert!(text.contains("\"code\":\"E0010\""));
    assert!(text.contains("\"primary\":{\"path\":\"tests/bounded.srp\",\"span\":[6,8006]"));
}

#[test]
fn v2_discovery_is_read_only_and_preserves_empty_filter_exit_status() {
    let ws = Workspace::new();
    let root = project(&ws, "listing v2");
    fs::write(root.join("tests/unread.srp"), [0xff]).unwrap();
    fs::create_dir(root.join("tests/unread.stdin")).unwrap();
    let args = ["test", "--json=v2", "--list", "--require-tests"];
    let listed = cli(&args, &root);
    assert!(listed.status.success(), "{listed:?}");
    assert!(listed.stderr.is_empty());
    assert_eq!(String::from_utf8(listed.stdout).unwrap(), "{\"schema_version\":2,\"mode\":\"list\",\"discovered\":1,\"passed\":0,\"failed\":0,\"tests\":[{\"path\":\"tests/unread.srp\",\"status\":\"discovered\"}]}\n");
    let mut filtered = args.to_vec();
    filtered.extend(["--filter", "absent"]);
    let empty = cli(&filtered, &root);
    assert_eq!(empty.status.code(), Some(1), "{empty:?}");
    assert!(empty.stderr.is_empty());
    assert_eq!(String::from_utf8(empty.stdout).unwrap(), "{\"schema_version\":2,\"mode\":\"list\",\"discovered\":0,\"passed\":0,\"failed\":0,\"tests\":[]}\n");
}

#[test]
fn listing_does_not_compile_or_execute_test_sources() {
    let ws = Workspace::new();
    let root = project(&ws, "listing");
    fs::write(root.join("tests/z.srp"), "print(1 / 0);").unwrap();
    fs::write(root.join("tests/a.srp"), "this would not compile").unwrap();
    let listed = cli(&["test", "--list"], &root);
    assert!(listed.status.success(), "{listed:?}");
    assert!(listed.stderr.is_empty());
    assert_eq!(
        std::str::from_utf8(&listed.stdout).unwrap(),
        "tests/a.srp\ntests/z.srp\n2 tests discovered\n"
    );

    let structured = cli(&["test", "--list", "--json"], &root);
    assert!(structured.status.success(), "{structured:?}");
    assert!(structured.stderr.is_empty());
    let text = std::str::from_utf8(&structured.stdout).unwrap();
    assert_eq!(
        text,
        "{\"schema_version\":1,\"mode\":\"list\",\"discovered\":2,\"passed\":0,\"failed\":0,\"tests\":[{\"path\":\"tests/a.srp\",\"status\":\"discovered\"},{\"path\":\"tests/z.srp\",\"status\":\"discovered\"}]}\n"
    );

    let executed = cli(&["test"], &root);
    assert_eq!(executed.status.code(), Some(1), "{executed:?}");
}

#[test]
fn empty_suites_report_unambiguously_in_both_json_modes() {
    let ws = Workspace::new();
    let root = project(&ws, "empty");
    let normal = cli(&["test", "--json"], &root);
    assert!(normal.status.success(), "{normal:?}");
    assert_eq!(
        std::str::from_utf8(&normal.stdout).unwrap(),
        "{\"schema_version\":1,\"mode\":\"run\",\"discovered\":0,\"passed\":0,\"failed\":0,\"tests\":[]}\n"
    );
    let list = cli(&["test", "--json", "--list"], &root);
    assert!(list.status.success(), "{list:?}");
    assert_eq!(
        std::str::from_utf8(&list.stdout).unwrap(),
        "{\"schema_version\":1,\"mode\":\"list\",\"discovered\":0,\"passed\":0,\"failed\":0,\"tests\":[]}\n"
    );
    let list_text = cli(&["test", "--list"], &root);
    assert!(list_text.status.success(), "{list_text:?}");
    assert_eq!(
        std::str::from_utf8(&list_text.stdout).unwrap(),
        "No .srp test files found in tests/ (0 discovered)\n"
    );
}

#[test]
fn unicode_test_filenames_are_reported_on_all_platforms() {
    let ws = Workspace::new();
    let root = project(&ws, "unicode");
    fs::write(root.join("tests/हिंदी.srp"), "print(3);").unwrap();
    let info = cli(&["test", "--json", "--list"], &root);
    assert!(info.status.success(), "{info:?}");
    assert!(
        String::from_utf8_lossy(&info.stdout).contains("\"path\":\"tests/हिंदी.srp\""),
        "{info:?}"
    );
}

#[cfg(unix)]
#[test]
fn filenames_with_unicode_and_quotes_are_escaped_correctly() {
    let ws = Workspace::new();
    let root = project(&ws, "quoted");
    fs::write(root.join("tests/हिंदी \"hi\".srp"), "print(3);").unwrap();
    let info = cli(&["test", "--json", "--list"], &root);
    assert!(info.status.success(), "{info:?}");
    assert!(
        String::from_utf8_lossy(&info.stdout).contains("\"path\":\"tests/हिंदी \\\"hi\\\".srp\""),
        "{info:?}"
    );
}

#[test]
fn explicit_path_and_legacy_projects_retain_report_compatibility() {
    let ws = Workspace::new();
    let root = ws.child("legacy");
    let created = Command::new(env!("CARGO_BIN_EXE_scarp"))
        .arg("create")
        .arg(&root)
        .output()
        .unwrap();
    assert!(created.status.success(), "{created:?}");
    fs::create_dir(root.join("tests")).unwrap();
    fs::write(root.join("tests/test.srp"), "print(42);").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_scarp"))
        .arg("test")
        .arg("--json")
        .arg(&root)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    assert!(String::from_utf8_lossy(&output.stdout).contains("\"passed\":1,\"failed\":0"));
    let text = cli(&["test"], &root);
    assert!(text.status.success(), "{text:?}");
    assert_eq!(
        std::str::from_utf8(&text.stdout).unwrap(),
        "PASS tests/test.srp\n1 passed, 0 failed\n"
    );
}

#[test]
fn invalid_project_and_unsupported_flags_do_not_emit_partial_json() {
    let ws = Workspace::new();
    let root = project(&ws, "invalid");
    for args in [
        &["test", "--json", "--json"][..],
        &["test", "--json=v2", "--json=v2"][..],
        &["test", "--json=v2", "--json"][..],
        &["test", "--json", "--json=v2"][..],
        &["test", "--json=v3"][..],
        &["test", "--list", "--list"][..],
        &["test", "--json", "--unknown"][..],
        &["test", "--list", "--max-steps", "10"][..],
        &["test", "--json", "a", "b"][..],
    ] {
        let result = cli(args, &root);
        assert_eq!(result.status.code(), Some(2), "{args:?}: {result:?}");
        assert!(result.stdout.is_empty());
        assert!(String::from_utf8_lossy(&result.stderr).contains("Usage: scarp test"));
    }
    fs::remove_file(root.join("scarp.project")).unwrap();
    let invalid = cli(&["test", "--json"], &root);
    assert_eq!(invalid.status.code(), Some(1), "{invalid:?}");
    assert!(invalid.stdout.is_empty());
}

#[cfg(unix)]
#[test]
fn listing_observes_the_same_symlink_safety_rules_as_execution() {
    use std::os::unix::fs::symlink;
    let ws = Workspace::new();
    let root = project(&ws, "linked");
    let outside = ws.child("outside.srp");
    fs::write(&outside, "print(1);").unwrap();
    symlink(&outside, root.join("tests/unsafe.srp")).unwrap();
    let listed = cli(&["test", "--list", "--json"], &root);
    assert_eq!(listed.status.code(), Some(1), "{listed:?}");
    assert!(listed.stdout.is_empty());
}
