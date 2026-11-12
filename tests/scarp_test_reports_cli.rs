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
