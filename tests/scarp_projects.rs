//! End-to-end project workflow checks use only the real Scarp binary.
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
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("scarp-project-test-{}-{id}", std::process::id()));
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

fn cli(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_scarp"))
        .args(args)
        .output()
        .expect("Scarp binary must start")
}

fn with_path(args: &[&str], path: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_scarp"))
        .args(args)
        .arg(path)
        .output()
        .expect("Scarp binary must start")
}

#[test]
fn create_verify_and_launch_use_a_real_compiler_entrypoint() {
    let workspace = Workspace::new();
    let project = workspace.child("Hello नमस्ते");
    let result = with_path(&["create"], &project);
    assert!(result.status.success(), "{result:?}");
    assert!(result.stderr.is_empty());
    let manifest = fs::read_to_string(project.join("Scarp.project")).unwrap();
    assert_eq!(
        manifest,
        "scarp-project: 1\nname: Hello नमस्ते\nentry: src/main.srp\n"
    );
    let entry = project.join("src/main.srp");
    assert_eq!(
        fs::read_to_string(&entry).unwrap(),
        "print(\"Hello from Scarp!\");\n"
    );
    let verify = with_path(&["verify"], &entry);
    assert!(verify.status.success(), "{verify:?}");
    assert!(verify.stdout.is_empty());
    let project_verify = with_path(&["verify"], &project);
    assert!(project_verify.status.success(), "{project_verify:?}");
    assert!(project_verify.stdout.is_empty());
    assert!(project_verify.stderr.is_empty());
    let run = with_path(&["launch"], &project);
    assert!(run.status.success(), "{run:?}");
    assert_eq!(run.stdout, b"Hello from Scarp!\n");
    assert!(run.stderr.is_empty());
    let exec = with_path(&["exec"], &entry);
    assert_eq!(run.stdout, exec.stdout);
    assert_eq!(run.stderr, exec.stderr);
}

#[test]
fn verify_project_directory_is_static_and_uses_declared_entry() {
    let workspace = Workspace::new();
    let project = workspace.child("verified project नमस्ते");
    assert!(with_path(&["create"], &project).status.success());
    let entry = project.join("src/other.srp");
    fs::write(&entry, "print(1 / 0);").unwrap();
    fs::write(
        project.join("Scarp.project"),
        "scarp-project: 1\nname: verified project नमस्ते\nentry: src/other.srp\n",
    )
    .unwrap();

    let verified = with_path(&["verify"], &project);
    assert!(verified.status.success(), "{verified:?}");
    assert!(verified.stdout.is_empty());
    assert!(verified.stderr.is_empty());

    // This code is type-correct but fails during execution; verify must not run it.
    let launched = with_path(&["launch"], &project);
    assert_eq!(launched.status.code(), Some(1), "{launched:?}");
    assert!(launched.stdout.is_empty());

    // A declared-entry error must match the existing source-file verifier.
    fs::write(&entry, "print(unknown_symbol);").unwrap();
    let canonical = fs::canonicalize(&entry).unwrap();
    let project_json = with_path(&["verify", "--diagnostic-format=json"], &project);
    let source_json = with_path(&["verify", "--diagnostic-format=json"], &canonical);
    assert_eq!(project_json.status.code(), Some(1), "{project_json:?}");
    assert_eq!(project_json.stdout, source_json.stdout);
    assert_eq!(project_json.stderr, source_json.stderr);
    assert!(project_json.stdout.is_empty());
    assert!(
        String::from_utf8_lossy(&project_json.stderr).contains("\"schema_version\":1"),
        "{project_json:?}"
    );
}

#[test]
fn verify_project_directory_rejects_invalid_manifests_with_json_errors() {
    let workspace = Workspace::new();
    let project = workspace.child("guarded");
    assert!(with_path(&["create"], &project).status.success());
    for manifest in [
        "scarp-project: 2\nname: guarded\nentry: src/main.srp\n",
        "scarp-project: 1\nname: guarded\nentry: ../outside.srp\n",
        "scarp-project: 1\nname: guarded\nentry: src/main.srp\nunknown: value\n",
    ] {
        fs::write(project.join("Scarp.project"), manifest).unwrap();
        let verified = with_path(&["verify", "--diagnostic-format=json"], &project);
        assert_eq!(verified.status.code(), Some(1), "{verified:?}");
        assert!(verified.stdout.is_empty());
        assert!(
            String::from_utf8_lossy(&verified.stderr).contains("\"schema_version\":1"),
            "{verified:?}"
        );
    }
    fs::remove_file(project.join("Scarp.project")).unwrap();
    let missing = with_path(&["verify"], &project);
    assert_eq!(missing.status.code(), Some(1), "{missing:?}");
    assert!(missing.stdout.is_empty());
}

#[test]
fn existing_destinations_are_never_overwritten() {
    let workspace = Workspace::new();
    let project = workspace.child("occupied");
    fs::create_dir(&project).unwrap();
    fs::write(project.join("sentinel.txt"), b"do not touch").unwrap();
    let result = with_path(&["create"], &project);
    assert_eq!(result.status.code(), Some(1), "{result:?}");
    assert!(result.stdout.is_empty());
    assert_eq!(
        fs::read(project.join("sentinel.txt")).unwrap(),
        b"do not touch"
    );
    assert!(!project.join("Scarp.project").exists());
    let missing_parent = with_path(&["create"], &project.join("nested").join("app"));
    assert_eq!(missing_parent.status.code(), Some(1), "{missing_parent:?}");
    assert!(!project.join("nested").exists());
}

#[test]
fn launch_uses_declared_entry_and_preserves_compiler_diagnostics_and_limits() {
    let workspace = Workspace::new();
    let project = workspace.child("calculator");
    assert!(with_path(&["create"], &project).status.success());
    fs::write(project.join("src/other.srp"), "print(6 * 7);").unwrap();
    fs::write(
        project.join("Scarp.project"),
        "scarp-project: 1\nname: calculator\nentry: src/other.srp\n",
    )
    .unwrap();
    let output = with_path(&["launch"], &project);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(output.stdout, b"42\n");

    fs::write(project.join("src/other.srp"), "print(1 / 0);").unwrap();
    let launch = with_path(&["launch", "--diagnostic-format=json"], &project);
    let exec = with_path(
        &["exec", "--diagnostic-format=json"],
        &fs::canonicalize(project.join("src/other.srp")).unwrap(),
    );
    assert_eq!(launch.status.code(), Some(1), "{launch:?}");
    assert_eq!(launch.stdout, exec.stdout);
    assert_eq!(launch.stderr, exec.stderr);
    assert!(launch.stdout.is_empty());
    assert!(String::from_utf8_lossy(&launch.stderr).contains("\"schema_version\":1"));
    fs::write(project.join("src/other.srp"), "print(2);").unwrap();
    let limited = with_path(&["launch", "--max-steps", "0"], &project);
    assert_eq!(limited.status.code(), Some(1), "{limited:?}");
    assert!(limited.stdout.is_empty());
}

#[test]
fn manifest_validation_rejects_bad_version_unknown_keys_traversal_and_size() {
    let workspace = Workspace::new();
    let project = workspace.child("invalid");
    assert!(with_path(&["create"], &project).status.success());
    let manifest = project.join("Scarp.project");
    for contents in [
        "scarp-project: 2\nname: invalid\nentry: src/main.srp\n".to_owned(),
        "scarp-project: 1\nname: invalid\nentry: src/main.srp\nextra: ignored\n".to_owned(),
        "scarp-project: 1\nname: invalid\nentry: ../other.srp\n".to_owned(),
        "scarp-project: 1\nname: invalid\nentry: /tmp/file.srp\n".to_owned(),
        "scarp-project: 1\nname: invalid\nentry: src/./main.srp\n".to_owned(),
        "scarp-project: 1\nname: invalid\nentry: src\\main.srp\n".to_owned(),
        "scarp-project: 1\nname: invalid\nentry: src/main.srp\n".repeat(120),
    ] {
        fs::write(&manifest, contents).unwrap();
        let failure = with_path(&["launch", "--diagnostic-format=json"], &project);
        assert_eq!(failure.status.code(), Some(1), "{failure:?}");
        assert!(failure.stdout.is_empty());
        assert!(
            String::from_utf8_lossy(&failure.stderr).contains("\"schema_version\":1"),
            "{failure:?}"
        );
    }
    fs::remove_file(&manifest).unwrap();
    let missing = with_path(&["launch"], &project);
    assert_eq!(missing.status.code(), Some(1), "{missing:?}");
}

#[test]
fn project_commands_validate_usage_and_offer_real_guides() {
    for args in [
        vec!["create"],
        vec!["launch"],
        vec!["create", "--bad"],
        vec!["launch", "--bogus", "missing"],
        vec!["create", "one", "two"],
        vec!["launch", "one", "two"],
    ] {
        let output = cli(&args);
        assert_eq!(output.status.code(), Some(2), "{args:?}: {output:?}");
        assert!(output.stdout.is_empty());
    }
    for args in [
        vec!["create", "--help"],
        vec!["launch", "--help"],
        vec!["guide", "create"],
        vec!["guide", "launch"],
    ] {
        let output = cli(&args);
        assert!(output.status.success(), "{args:?}: {output:?}");
        assert!(output.stderr.is_empty());
        assert!(String::from_utf8_lossy(&output.stdout).contains("Usage: scarp"));
    }
    let report = cli(&["diagnose", "--json"]);
    assert!(report.status.success());
    assert!(String::from_utf8_lossy(&report.stdout).contains("\"project_workflow\":true"));
}

#[test]
fn explicit_separator_allows_dash_leading_project_directory() {
    let workspace = Workspace::new();
    let create = Command::new(env!("CARGO_BIN_EXE_scarp"))
        .args(["create", "--", "-project"])
        .current_dir(&workspace.0)
        .output()
        .unwrap();
    assert!(create.status.success(), "{create:?}");
    let launch = Command::new(env!("CARGO_BIN_EXE_scarp"))
        .args(["launch", "--", "-project"])
        .current_dir(&workspace.0)
        .output()
        .unwrap();
    assert!(launch.status.success(), "{launch:?}");
    assert_eq!(launch.stdout, b"Hello from Scarp!\n");
    let verified = Command::new(env!("CARGO_BIN_EXE_scarp"))
        .args(["verify", "--", "-project"])
        .current_dir(&workspace.0)
        .output()
        .unwrap();
    assert!(verified.status.success(), "{verified:?}");
    assert!(verified.stdout.is_empty());
    assert!(verified.stderr.is_empty());
}

#[test]
fn project_module_inspection_matches_contained_entry_without_running_code() {
    let workspace = Workspace::new();
    let project = workspace.child("inspection नमस्ते");
    assert!(with_path(&["create"], &project).status.success());
    let entry = project.join("src/main.srp");
    fs::write(
        &entry,
        "import \"library.srp\" as library;\nprint(library.answer / 0);\n",
    )
    .unwrap();
    fs::write(project.join("src/library.srp"), "pub bind answer = 42;\n").unwrap();
    let entry = fs::canonicalize(entry).unwrap();

    for arguments in [
        &["inspect", "modules"][..],
        &["inspect", "modules", "--json"][..],
        &["modules", "--json"][..],
    ] {
        let from_project = with_path(arguments, &project);
        let from_file = with_path(arguments, &entry);
        assert!(
            from_project.status.success(),
            "{arguments:?}: {from_project:?}"
        );
        assert_eq!(from_project.status.code(), from_file.status.code());
        assert_eq!(from_project.stdout, from_file.stdout);
        assert_eq!(from_project.stderr, from_file.stderr);
        assert!(from_project.stderr.is_empty());
        let inventory = String::from_utf8_lossy(&from_project.stdout);
        assert!(inventory.contains("library.srp"), "{inventory}");
        if arguments.contains(&"--json") {
            assert!(inventory.contains("\"schema_version\":1"), "{inventory}");
        }
    }

    // The compiler can inspect this program, but executing the division fails.
    let launched = with_path(&["launch"], &project);
    assert_eq!(launched.status.code(), Some(1), "{launched:?}");
    assert!(launched.stdout.is_empty());
}

#[test]
fn project_module_inspection_rejects_invalid_manifests_before_output() {
    let workspace = Workspace::new();
    let project = workspace.child("inspect-guard");
    assert!(with_path(&["create"], &project).status.success());
    let manifest = project.join("Scarp.project");
    for contents in [
        "scarp-project: 2\nname: inspect-guard\nentry: src/main.srp\n",
        "scarp-project: 1\nname: inspect-guard\nentry: ../escape.srp\n",
        "scarp-project: 1\nname: inspect-guard\nentry: src/main.srp\nunknown: value\n",
    ] {
        fs::write(&manifest, contents).unwrap();
        let inspected = with_path(
            &["inspect", "modules", "--json", "--diagnostic-format=json"],
            &project,
        );
        assert_eq!(inspected.status.code(), Some(1), "{inspected:?}");
        assert!(inspected.stdout.is_empty(), "{inspected:?}");
        assert!(
            String::from_utf8_lossy(&inspected.stderr).contains("\"schema_version\":1"),
            "{inspected:?}"
        );
    }

    fs::remove_file(manifest).unwrap();
    let missing = with_path(&["inspect", "modules"], &project);
    assert_eq!(missing.status.code(), Some(1), "{missing:?}");
    assert!(missing.stdout.is_empty());
}

#[cfg(unix)]
#[test]
fn symlinked_entry_outside_project_is_rejected() {
    use std::os::unix::fs::symlink;
    let workspace = Workspace::new();
    let project = workspace.child("inside");
    assert!(with_path(&["create"], &project).status.success());
    let outside = workspace.child("outside.srp");
    fs::write(&outside, "print(77);").unwrap();
    fs::remove_file(project.join("src/main.srp")).unwrap();
    symlink(&outside, project.join("src/main.srp")).unwrap();
    let output = with_path(&["launch"], &project);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("outside"));
    let verified = with_path(&["verify", "--diagnostic-format=json"], &project);
    assert_eq!(verified.status.code(), Some(1), "{verified:?}");
    assert!(verified.stdout.is_empty());
    assert!(String::from_utf8_lossy(&verified.stderr).contains("outside"));

    let inspected = with_path(
        &["inspect", "modules", "--diagnostic-format=json"],
        &project,
    );
    assert_eq!(inspected.status.code(), Some(1), "{inspected:?}");
    assert!(inspected.stdout.is_empty());
    assert!(String::from_utf8_lossy(&inspected.stderr).contains("outside"));
}
