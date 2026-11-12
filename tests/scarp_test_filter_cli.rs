//! End-to-end filename-filter contracts for the unreleased project test runner.
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
        let root =
            std::env::temp_dir().join(format!("scarp-test-filter-{}-{serial}", std::process::id()));
        fs::create_dir(&root).unwrap();
        Self(root)
    }

    fn project(&self) -> PathBuf {
        let root = self.0.join("app");
        let output = Command::new(env!("CARGO_BIN_EXE_scarp"))
            .arg("init")
            .arg(&root)
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        root
    }
}

impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn run(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_scarp"))
        .args(args)
        .current_dir(root)
        .output()
        .unwrap()
}

#[test]
fn literal_filter_runs_only_matching_sources_and_keeps_json_schema() {
    let ws = Workspace::new();
    let root = ws.project();
    fs::write(root.join("tests/math-add.srp"), "assert(2 + 3 == 5);").unwrap();
    fs::write(root.join("tests/math-mul.srp"), "assert(2 * 3 == 6);").unwrap();
    fs::write(root.join("tests/other.srp"), "print(1 / 0);").unwrap();

    let selected = run(
        &root,
        &["test", "--filter", "math", "--json", "--require-tests"],
    );
    assert!(selected.status.success(), "{selected:?}");
    assert!(selected.stderr.is_empty(), "{selected:?}");
    let json = String::from_utf8(selected.stdout).unwrap();
    assert_eq!(
        json,
        "{\"schema_version\":1,\"mode\":\"run\",\"discovered\":2,\"passed\":2,\"failed\":0,\"tests\":[{\"path\":\"tests/math-add.srp\",\"status\":\"passed\"},{\"path\":\"tests/math-mul.srp\",\"status\":\"passed\"}]}\n"
    );

    let all = run(&root, &["test", "--json"]);
    assert_eq!(all.status.code(), Some(1), "{all:?}");
    assert!(String::from_utf8_lossy(&all.stdout).contains("\"failed\":1"));
}

#[test]
fn filtering_is_case_sensitive_and_does_not_match_tests_prefix() {
    let ws = Workspace::new();
    let root = ws.project();
    fs::write(root.join("tests/Math.srp"), "print(1);").unwrap();
    fs::write(root.join("tests/math.srp"), "print(2);").unwrap();
    fs::write(root.join("tests/other.srp"), "print(3);").unwrap();

    let uppercase = run(&root, &["test", "--list", "--filter", "Math"]);
    assert!(uppercase.status.success(), "{uppercase:?}");
    assert_eq!(uppercase.stdout, b"tests/Math.srp\n1 tests discovered\n");

    let lower = run(&root, &["test", "--filter", "math", "--list"]);
    assert!(lower.status.success(), "{lower:?}");
    assert_eq!(lower.stdout, b"tests/math.srp\n1 tests discovered\n");

    // The relative directory label is never searched.
    let prefix = run(&root, &["test", "--filter", "tests/", "--list"]);
    assert_eq!(prefix.status.code(), Some(2), "{prefix:?}");
}

#[test]
fn unicode_filename_substrings_work_in_run_and_read_only_list_modes() {
    let ws = Workspace::new();
    let root = ws.project();
    fs::write(root.join("tests/हिंदी-योग.srp"), "assert(true);").unwrap();
    fs::write(root.join("tests/ordinary.srp"), "print(1 / 0);").unwrap();
    for args in [
        &["test", "--filter", "योग", "--json"][..],
        &["test", "--json", "--list", "--filter", "योग"][..],
    ] {
        let output = run(&root, args);
        assert!(output.status.success(), "{args:?}: {output:?}");
        assert!(output.stderr.is_empty(), "{output:?}");
        let text = String::from_utf8(output.stdout).unwrap();
        assert!(text.contains("\"path\":\"tests/हिंदी-योग.srp\""), "{text}");
        assert!(text.contains("\"discovered\":1"), "{text}");
        assert!(!text.contains("ordinary"), "{text}");
    }
}

#[test]
fn empty_selection_is_success_by_default_and_failure_under_strict_guard() {
    let ws = Workspace::new();
    let root = ws.project();
    fs::write(root.join("tests/exists.srp"), "print(99);").unwrap();
    let normal = run(&root, &["test", "--json", "--filter", "not-found"]);
    assert!(normal.status.success(), "{normal:?}");
    let required = run(
        &root,
        &["test", "--json", "--filter", "not-found", "--require-tests"],
    );
    assert_eq!(required.status.code(), Some(1), "{required:?}");
    assert_eq!(required.stdout, normal.stdout);
    assert!(required.stderr.is_empty());
    assert!(String::from_utf8_lossy(&normal.stdout).contains("\"discovered\":0"));
    let listed = run(
        &root,
        &["test", "--filter", "not-found", "--list", "--require-tests"],
    );
    assert_eq!(listed.status.code(), Some(1), "{listed:?}");
    assert_eq!(
        listed.stdout,
        b"No .srp test files found in tests/ (0 discovered)\n"
    );
}

#[test]
fn bad_filter_usage_is_reported_without_running_project_code() {
    let ws = Workspace::new();
    let root = ws.project();
    let oversized = "a".repeat(129);
    let bads: Vec<Vec<&str>> = vec![
        vec!["test", "--filter"],
        vec!["test", "--filter", ""],
        vec!["test", "--filter", "a/b"],
        vec!["test", "--filter", "a\\b"],
        vec!["test", "--filter", "-help"],
        vec!["test", "--filter", "a", "--filter", "b"],
        vec!["test", "--filter", "--json"],
        vec!["test", "--filter", &oversized],
    ];
    for args in bads {
        let out = run(&root, &args);
        assert_eq!(out.status.code(), Some(2), "{args:?}: {out:?}");
        assert!(out.stdout.is_empty());
        assert!(String::from_utf8_lossy(&out.stderr).contains("Usage: scarp test"));
    }
}

#[test]
fn filter_keeps_global_test_limit_and_help_contracts() {
    let ws = Workspace::new();
    let root = ws.project();
    for n in 0..65 {
        fs::write(root.join(format!("tests/case-{n:03}.srp")), "print(1);").unwrap();
    }
    let out = run(&root, &["test", "--filter", "case-000", "--list", "--json"]);
    assert_eq!(out.status.code(), Some(1), "{out:?}");
    assert!(out.stdout.is_empty());
    assert!(String::from_utf8_lossy(&out.stderr).contains("limit 64"));
    for args in [&["test", "--help"][..], &["guide", "test"][..]] {
        let help = run(&root, args);
        assert!(help.status.success(), "{help:?}");
        assert!(String::from_utf8_lossy(&help.stdout).contains("--filter SUBSTRING"));
    }
}

#[cfg(unix)]
#[test]
fn unmatched_symlink_still_fails_discovery_without_running_sources() {
    use std::os::unix::fs::symlink;
    let ws = Workspace::new();
    let root = ws.project();
    let outside = ws.0.join("outside.srp");
    fs::write(&outside, "print(1);").unwrap();
    fs::write(root.join("tests/selected.srp"), "print(1);").unwrap();
    symlink(&outside, root.join("tests/unmatched.srp")).unwrap();
    let output = run(&root, &["test", "--list", "--filter", "selected", "--json"]);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(output.stdout.is_empty());
}
