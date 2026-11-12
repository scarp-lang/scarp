//! Optional nonempty-suite guard for CI consumers; default behavior is stable.
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
            "scarp-test-require-nonempty-{}-{serial}",
            std::process::id()
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn project(&self, name: &str) -> PathBuf {
        let root = self.0.join(name);
        let result = Command::new(env!("CARGO_BIN_EXE_scarp"))
            .arg("init")
            .arg(&root)
            .output()
            .unwrap();
        assert!(result.status.success(), "{result:?}");
        root
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

#[test]
fn empty_project_is_success_by_default_and_failure_only_with_guard() {
    let workspace = Workspace::new();
    let root = workspace.project("empty");

    let plain = cli(&["test"], &root);
    assert!(plain.status.success(), "{plain:?}");
    assert_eq!(
        std::str::from_utf8(&plain.stdout).unwrap(),
        "No .srp test files found in tests/ (0 passed, 0 failed)\n"
    );

    let required = cli(&["test", "--require-tests"], &root);
    assert_eq!(required.status.code(), Some(1), "{required:?}");
    assert_eq!(required.stdout, plain.stdout);
    assert!(required.stderr.is_empty());
}

#[test]
fn json_reports_are_complete_even_when_empty_suite_is_rejected() {
    let workspace = Workspace::new();
    let root = workspace.project("empty-json");
    let regular = cli(&["test", "--json"], &root);
    let required = cli(&["test", "--require-tests", "--json"], &root);
    assert!(regular.status.success(), "{regular:?}");
    assert_eq!(required.status.code(), Some(1), "{required:?}");
    assert_eq!(required.stdout, regular.stdout);
    assert!(required.stderr.is_empty());
    assert_eq!(
        std::str::from_utf8(&required.stdout).unwrap(),
        "{\"schema_version\":1,\"mode\":\"run\",\"discovered\":0,\"passed\":0,\"failed\":0,\"tests\":[]}\n"
    );
}

#[test]
fn read_only_listing_remains_read_only_with_required_nonempty_guard() {
    let workspace = Workspace::new();
    let root = workspace.project("empty-list");

    let plain = cli(&["test", "--require-tests", "--list"], &root);
    assert_eq!(plain.status.code(), Some(1), "{plain:?}");
    assert_eq!(
        std::str::from_utf8(&plain.stdout).unwrap(),
        "No .srp test files found in tests/ (0 discovered)\n"
    );
    assert!(plain.stderr.is_empty());

    let json = cli(&["test", "--list", "--json", "--require-tests"], &root);
    assert_eq!(json.status.code(), Some(1), "{json:?}");
    assert_eq!(
        std::str::from_utf8(&json.stdout).unwrap(),
        "{\"schema_version\":1,\"mode\":\"list\",\"discovered\":0,\"passed\":0,\"failed\":0,\"tests\":[]}\n"
    );
    assert!(json.stderr.is_empty());
}

#[test]
fn nonempty_suite_succeeds_with_guard_across_run_list_and_json() {
    let workspace = Workspace::new();
    let root = workspace.project("nonempty");
    fs::write(root.join("tests/ok.srp"), "print(42);").unwrap();

    for args in [
        &["test", "--require-tests"][..],
        &["test", "--require-tests", "--list"][..],
        &["test", "--require-tests", "--json"][..],
        &["test", "--require-tests", "--json", "--list"][..],
    ] {
        let output = cli(args, &root);
        assert!(output.status.success(), "{args:?}: {output:?}");
        assert!(output.stderr.is_empty());
        assert!(!output.stdout.is_empty());
    }

    fs::write(root.join("tests/failed.srp"), "print(1 / 0);").unwrap();
    let failed = cli(&["test", "--require-tests", "--json"], &root);
    assert_eq!(failed.status.code(), Some(1), "{failed:?}");
    assert!(String::from_utf8_lossy(&failed.stdout).contains("\"failed\":1"));
}

#[test]
fn malformed_projects_and_duplicate_flag_do_not_report_success() {
    let workspace = Workspace::new();
    let root = workspace.project("invalid");
    let duplicated = cli(&["test", "--require-tests", "--require-tests"], &root);
    assert_eq!(duplicated.status.code(), Some(2), "{duplicated:?}");
    assert!(duplicated.stdout.is_empty());
    assert!(String::from_utf8_lossy(&duplicated.stderr).contains("Usage:"));

    fs::remove_file(root.join("scarp.project")).unwrap();
    let malformed = cli(&["test", "--require-tests", "--json"], &root);
    assert_eq!(malformed.status.code(), Some(1), "{malformed:?}");
    assert!(malformed.stdout.is_empty());
}

#[test]
fn help_and_guide_document_the_new_flag() {
    let workspace = Workspace::new();
    for args in [&["test", "--help"][..], &["guide", "test"][..]] {
        let result = cli(args, &workspace.0);
        assert!(result.status.success(), "{args:?}: {result:?}");
        assert!(String::from_utf8_lossy(&result.stdout).contains("--require-tests"));
    }
}
