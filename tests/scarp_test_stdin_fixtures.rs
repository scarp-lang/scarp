//! Optional bounded .stdin fixtures for real Scarp project tests.
//! The test runner does not read the CLI process's stdin.
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
        let dir = std::env::temp_dir().join(format!(
            "scarp-test-stdin-fixture-{}-{serial}",
            std::process::id()
        ));
        fs::create_dir(&dir).unwrap();
        Self(dir)
    }

    fn project(&self) -> PathBuf {
        let dir = self.0.join("app");
        let output = Command::new(env!("CARGO_BIN_EXE_scarp"))
            .arg("init")
            .arg(&dir)
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        dir
    }
}

impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn cli(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_scarp"))
        .args(args)
        .current_dir(root)
        .output()
        .unwrap()
}

#[test]
fn matching_fixture_supplies_unicode_input_only_to_its_test() {
    let ws = Workspace::new();
    let root = ws.project();
    fs::write(
        root.join("tests/greeting.srp"),
        r#"assert(read_line() == "नमस्ते", "expected provided fixture");"#,
    )
    .unwrap();
    fs::write(root.join("tests/greeting.stdin"), "नमस्ते\r\n").unwrap();
    fs::write(root.join("tests/other.srp"), "print(42);").unwrap();
    let result = cli(&root, &["test", "--json", "--require-tests"]);
    assert!(result.status.success(), "{result:?}");
    assert!(result.stderr.is_empty());
    let report = String::from_utf8(result.stdout).unwrap();
    assert!(
        report.contains(r#""discovered":2,"passed":2,"failed":0"#),
        "{report}"
    );
    assert!(report.contains(r#""path":"tests/greeting.srp","status":"passed""#));
    assert!(report.contains(r#""path":"tests/other.srp","status":"passed""#));
    assert!(!report.contains("नमस्ते"), "{report}");
}

#[test]
fn no_fixture_preserves_existing_input_unavailable_diagnostic() {
    let ws = Workspace::new();
    let root = ws.project();
    fs::write(root.join("tests/no-fixture.srp"), "print(read_line());").unwrap();
    let result = cli(&root, &["test", "--json"]);
    assert_eq!(result.status.code(), Some(1), "{result:?}");
    assert!(result.stderr.is_empty());
    let json = String::from_utf8(result.stdout).unwrap();
    assert!(json.contains(r#""failed":1"#), "{json}");
    assert!(
        json.contains("read_line requires an explicit input reader"),
        "{json}"
    );
}

#[test]
fn oversized_fixture_fails_only_its_case_without_partial_json() {
    let ws = Workspace::new();
    let root = ws.project();
    fs::write(root.join("tests/a-good.srp"), "assert(true);").unwrap();
    fs::write(root.join("tests/z-input.srp"), "print(read_line());").unwrap();
    fs::write(
        root.join("tests/z-input.stdin"),
        vec![b'x'; 1024 * 1024 + 1],
    )
    .unwrap();
    let result = cli(&root, &["test", "--json"]);
    assert_eq!(result.status.code(), Some(1), "{result:?}");
    assert!(result.stderr.is_empty());
    let json = String::from_utf8(result.stdout).unwrap();
    assert!(
        json.starts_with(
            r#"{"schema_version":1,"mode":"run","discovered":2,"passed":1,"failed":1"#
        ),
        "{json}"
    );
    assert!(
        json.contains("fixture exceeds the 1 MiB input limit"),
        "{json}"
    );
    assert!(json.ends_with("]}\n"), "{json}");
}

#[test]
fn line_size_limits_remain_enforced_by_the_interpreter() {
    let ws = Workspace::new();
    let root = ws.project();
    fs::write(root.join("tests/input.srp"), "print(read_line());").unwrap();
    fs::write(root.join("tests/input.stdin"), vec![b'a'; 64 * 1024 + 1]).unwrap();
    let result = cli(&root, &["test", "--json"]);
    assert_eq!(result.status.code(), Some(1), "{result:?}");
    let json = String::from_utf8(result.stdout).unwrap();
    assert!(
        json.contains("input line or cumulative byte quota exceeded"),
        "{json}"
    );
}

#[test]
fn read_only_listing_never_reads_or_validates_stdin_fixtures() {
    let ws = Workspace::new();
    let root = ws.project();
    fs::write(root.join("tests/input.srp"), "print(read_line());").unwrap();
    fs::create_dir(root.join("tests/input.stdin")).unwrap();
    let listed = cli(&root, &["test", "--json", "--list"]);
    assert!(listed.status.success(), "{listed:?}");
    assert!(String::from_utf8_lossy(&listed.stdout).contains(r#""status":"discovered""#));
    let executed = cli(&root, &["test", "--json"]);
    assert_eq!(executed.status.code(), Some(1), "{executed:?}");
    assert!(String::from_utf8_lossy(&executed.stdout).contains("not a symlink or directory"));
}

#[test]
fn filtered_execution_does_not_read_unselected_bad_fixtures() {
    let ws = Workspace::new();
    let root = ws.project();
    fs::write(
        root.join("tests/selected.srp"),
        "assert(read_line() == \"yes\");",
    )
    .unwrap();
    fs::write(root.join("tests/selected.stdin"), b"yes\n").unwrap();
    fs::write(root.join("tests/ignored.srp"), "assert(true);").unwrap();
    fs::create_dir(root.join("tests/ignored.stdin")).unwrap();
    let filtered = cli(&root, &["test", "--json", "--filter", "selected.srp"]);
    assert!(filtered.status.success(), "{filtered:?}");
    let json = String::from_utf8(filtered.stdout).unwrap();
    assert!(
        json.contains(r#""discovered":1,"passed":1,"failed":0"#),
        "{json}"
    );
    let all = cli(&root, &["test", "--json"]);
    assert_eq!(all.status.code(), Some(1), "{all:?}");
}

#[cfg(unix)]
#[test]
fn symlinked_input_fixture_is_rejected_but_listing_is_safe() {
    use std::os::unix::fs::symlink;
    let ws = Workspace::new();
    let root = ws.project();
    fs::write(root.join("tests/linked.srp"), "print(read_line());").unwrap();
    let target = ws.0.join("outside.stdin");
    fs::write(&target, b"secret\n").unwrap();
    symlink(&target, root.join("tests/linked.stdin")).unwrap();

    let listed = cli(&root, &["test", "--list", "--json"]);
    assert!(listed.status.success(), "{listed:?}");

    let executed = cli(&root, &["test", "--json"]);
    assert_eq!(executed.status.code(), Some(1), "{executed:?}");
    assert!(executed.stderr.is_empty());
    let json = String::from_utf8(executed.stdout).unwrap();
    assert!(json.contains("fixture must be an ordinary file"), "{json}");
    assert!(!json.contains("secret"), "{json}");
}

#[test]
fn stdout_and_stderr_fixtures_are_exact_per_test_and_absent_from_reports() {
    let ws = Workspace::new();
    let root = ws.project();
    fs::write(
        root.join("tests/complete.srp"),
        r#"bind line = try_read_line(); assert(line[0]); write(line[1]); print("!"); eprint("résumé"); flush_stderr();"#,
    ).unwrap();
    fs::write(root.join("tests/complete.stdin"), "नमस्ते 🌍\r\n").unwrap();
    fs::write(root.join("tests/complete.stdout"), "नमस्ते 🌍!\n").unwrap();
    fs::write(root.join("tests/complete.stderr"), "résumé\n").unwrap();
    fs::write(
        root.join("tests/no-fixtures.srp"),
        r#"print("unreported");"#,
    )
    .unwrap();
    for mode in ["--json", "--json=v2"] {
        let result = cli(&root, &["test", mode, "--require-tests"]);
        assert!(result.status.success(), "{result:?}");
        assert!(result.stderr.is_empty());
        let json = String::from_utf8(result.stdout).unwrap();
        assert!(
            json.contains(r#""discovered":2,"passed":2,"failed":0"#),
            "{json}"
        );
        for data in ["नमस्ते", "résumé", "unreported"] {
            assert!(!json.contains(data), "{json}");
        }
    }
}

#[test]
fn output_mismatches_distinguish_first_difference_truncation_and_excess() {
    let ws = Workspace::new();
    let root = ws.project();
    let source = root.join("tests/compare.srp");
    for (stream, program, bytes, expected) in [
        (
            "stdout",
            r#"write("café");"#,
            "café!\n",
            "byte 5 (expected 7 bytes, received 5)",
        ),
        (
            "stdout",
            r#"print("secret");"#,
            "",
            "byte 0 (expected 0 bytes, received 7)",
        ),
        (
            "stderr",
            r#"ewrite("é"); eprint("z");"#,
            "êz\n",
            "byte 1 (expected 4 bytes, received 4)",
        ),
        (
            "stderr",
            r#"ewrite("one"); ewrite("twice");"#,
            "onet",
            "byte 4 (expected 4 bytes, received 8)",
        ),
    ] {
        fs::write(&source, program).unwrap();
        let fixture = source.with_extension(stream);
        fs::write(&fixture, bytes).unwrap();
        let result = cli(&root, &["test", "--json=v2"]);
        assert_eq!(result.status.code(), Some(1), "{result:?}");
        assert!(result.stderr.is_empty());
        let json = String::from_utf8(result.stdout).unwrap();
        assert!(json.contains(r#""phase":"runtime""#), "{json}");
        assert!(
            json.contains(&format!("{stream} differs from .{stream} at {expected}")),
            "{json}"
        );
        assert!(!json.contains("secret"), "{json}");
        fs::remove_file(fixture).unwrap();
    }
}

#[test]
fn output_fixtures_are_bounded_utf8_files_and_listing_skips_them() {
    let ws = Workspace::new();
    let root = ws.project();
    fs::write(root.join("tests/simple.srp"), "assert(true);").unwrap();
    fs::write(root.join("tests/simple.stdout"), b"\xff").unwrap();
    fs::write(
        root.join("tests/simple.stderr"),
        vec![b'x'; 1024 * 1024 + 1],
    )
    .unwrap();
    let listed = cli(&root, &["test", "--list", "--json=v2"]);
    assert!(listed.status.success(), "{listed:?}");
    let first = cli(&root, &["test", "--json=v2"]);
    let json = String::from_utf8(first.stdout).unwrap();
    assert_eq!(first.status.code(), Some(1));
    assert!(json.contains(r#""phase":"fixture""#), "{json}");
    assert!(
        json.contains(".stdout fixture must contain valid UTF-8"),
        "{json}"
    );
    fs::remove_file(root.join("tests/simple.stdout")).unwrap();
    let second = cli(&root, &["test", "--json=v2"]);
    let json = String::from_utf8(second.stdout).unwrap();
    assert_eq!(second.status.code(), Some(1));
    assert!(
        json.contains(".stderr fixture exceeds the 1 MiB limit"),
        "{json}"
    );
}
