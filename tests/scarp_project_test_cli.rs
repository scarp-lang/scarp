//! Cross-platform CLI contracts for project test execution.
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
            "scarp-cli-test-project-{}-{serial}",
            std::process::id()
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn child(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn command(args: &[&str], dir: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_scarp"))
        .args(args)
        .current_dir(dir)
        .output()
        .expect("Scarp CLI should start")
}

fn project(ws: &Workspace, name: &str) -> PathBuf {
    let path = ws.child(name);
    let output = Command::new(env!("CARGO_BIN_EXE_scarp"))
        .arg("init")
        .arg(&path)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    path
}

#[test]
fn empty_suite_is_reported_explicitly_not_silently() {
    let ws = Workspace::new();
    let root = project(&ws, "empty नमस्ते");
    let result = command(&["test"], &root);
    assert!(result.status.success(), "{result:?}");
    assert!(result.stdout.starts_with(b"No .srp test files found"));
    assert!(result.stderr.is_empty());
}

#[test]
fn tests_run_in_deterministic_filename_order_and_can_import_project_sources() {
    let ws = Workspace::new();
    let root = project(&ws, "cross-module");
    fs::write(root.join("src/math.srp"), "pub bind answer = 42;\n").unwrap();
    fs::write(root.join("tests/zeta.srp"), "print(2 + 2);\n").unwrap();
    fs::write(
        root.join("tests/alpha.srp"),
        "import \"../src/math.srp\" as math;\nprint(math.answer);\n",
    ).unwrap();
    // Non-.srp files and nested directories are not recursively executed.
    fs::write(root.join("tests/notes.txt"), "print(1 / 0);\n").unwrap();
    fs::create_dir(root.join("tests/nested")).unwrap();
    fs::write(root.join("tests/nested/ignored.srp"), "print(1 / 0);\n").unwrap();
    let result = command(&["test"], &root);
    assert!(result.status.success(), "{result:?}");
    assert_eq!(
        String::from_utf8(result.stdout).unwrap(),
        "PASS tests/alpha.srp\nPASS tests/zeta.srp\n2 passed, 0 failed\n"
    );
    assert!(result.stderr.is_empty());
    let explicit = Command::new(env!("CARGO_BIN_EXE_scarp"))
        .arg("test")
        .arg(&root)
        .output()
        .unwrap();
    assert_eq!(explicit.stdout, result.stdout);
}

#[test]
fn compilation_and_runtime_failures_both_fail_suite_without_leaking_program_output() {
    let ws = Workspace::new();
    let root = project(&ws, "failure");
    fs::write(root.join("tests/1-pass.srp"), "print(\"hello\");").unwrap();
    fs::write(root.join("tests/2-error.srp"), "print(1 / 0);").unwrap();
    fs::write(root.join("tests/3-unknown.srp"), "print(missing_symbol);").unwrap();
    let result = command(&["test"], &root);
    assert_eq!(result.status.code(), Some(1), "{result:?}");
    assert_eq!(
        String::from_utf8(result.stdout).unwrap(),
        "PASS tests/1-pass.srp\nFAIL tests/2-error.srp\nFAIL tests/3-unknown.srp\n1 passed, 2 failed\n"
    );
    let details = String::from_utf8_lossy(&result.stderr);
    assert!(details.contains("tests/2-error.srp:"), "{details}");
    assert!(details.contains("tests/3-unknown.srp:"), "{details}");
    assert!(!result.stdout.windows(5).any(|w| w == b"hello"));
}

#[test]
fn requires_valid_manifest_and_test_directory() {
    let ws = Workspace::new();
    let root = project(&ws, "guards");
    fs::remove_file(root.join("scarp.project")).unwrap();
    let result = command(&["test"], &root);
    assert_eq!(result.status.code(), Some(1), "{result:?}");
    assert!(result.stdout.is_empty());
    fs::write(
        root.join("scarp.project"),
        "project \"guards\" { version: \"0.1.0\" entry: \"../outside.srp\" type: \"application\" }\n",
    ).unwrap();
    let invalid = command(&["test"], &root);
    assert_eq!(invalid.status.code(), Some(1), "{invalid:?}");
    assert!(invalid.stdout.is_empty());

    let other = project(&ws, "missing-tests");
    fs::remove_dir(other.join("tests")).unwrap();
    let missing = command(&["test"], &other);
    assert_eq!(missing.status.code(), Some(1), "{missing:?}");
    assert!(missing.stdout.is_empty());
}

#[test]
fn tests_reject_bad_usage_and_accept_help() {
    let ws = Workspace::new();
    for args in [
        &["test", "--unknown"][..],
        &["test", "one", "two"][..],
        &["test", "--diagnostic-format=json", "."][..],
    ] {
        let result = command(args, &ws.0);
        assert_eq!(result.status.code(), Some(2), "{args:?}: {result:?}");
        assert!(result.stdout.is_empty());
        assert!(String::from_utf8_lossy(&result.stderr).contains("Usage: scarp test"));
    }
    for args in [&["test", "--help"][..], &["guide", "test"][..]] {
        let result = command(args, &ws.0);
        assert!(result.status.success(), "{args:?}: {result:?}");
        assert!(result.stderr.is_empty());
        assert!(result.stdout.starts_with(b"Usage: scarp test"));
    }
}

#[test]
fn discovery_limit_is_enforced_before_running_user_tests() {
    let ws = Workspace::new();
    let root = project(&ws, "too-many");
    for i in 0..65 {
        fs::write(root.join(format!("tests/{i:03}.srp")), "print(1 / 0);").unwrap();
    }
    let result = command(&["test"], &root);
    assert_eq!(result.status.code(), Some(1), "{result:?}");
    assert!(result.stdout.is_empty());
    assert!(String::from_utf8_lossy(&result.stderr).contains("limit 64"));
}

#[test]
fn legacy_project_can_run_project_tests_without_migration() {
    let ws = Workspace::new();
    let root = ws.child("legacy");
    let output = Command::new(env!("CARGO_BIN_EXE_scarp"))
        .arg("create")
        .arg(&root)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    fs::create_dir(root.join("tests")).unwrap();
    fs::write(root.join("tests/test.srp"), "print(42);").unwrap();
    let result = command(&["test"], &root);
    assert!(result.status.success(), "{result:?}");
    assert!(String::from_utf8_lossy(&result.stdout).contains("1 passed, 0 failed"));
}

#[cfg(unix)]
#[test]
fn symlinked_test_directory_or_file_is_rejected() {
    use std::os::unix::fs::symlink;
    let ws = Workspace::new();
    let root = project(&ws, "symlinked");
    let outside = ws.child("outside.srp");
    fs::write(&outside, "print(42);").unwrap();
    symlink(&outside, root.join("tests/evil.srp")).unwrap();
    let result = command(&["test"], &root);
    assert_eq!(result.status.code(), Some(1), "{result:?}");
    assert!(result.stdout.is_empty());
    fs::remove_file(root.join("tests/evil.srp")).unwrap();
    fs::remove_dir(root.join("tests")).unwrap();
    let external_dir = ws.child("external-tests");
    fs::create_dir(&external_dir).unwrap();
    symlink(&external_dir, root.join("tests")).unwrap();
    let result = command(&["test"], &root);
    assert_eq!(result.status.code(), Some(1), "{result:?}");
    assert!(result.stdout.is_empty());
}
