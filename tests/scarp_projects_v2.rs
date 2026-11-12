//! CLI contract tests for the declarative scarp.project workflow.
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
        let path = std::env::temp_dir().join(format!(
            "scarp-project-v2-test-{}-{id}",
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

fn execute(args: &[&str], project: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_scarp"))
        .args(args)
        .arg(project)
        .output()
        .expect("Scarp binary should start")
}

fn inside(args: &[&str], cwd: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_scarp"))
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("Scarp binary should start")
}

fn init(path: &Path) {
    let output = execute(&["init"], path);
    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty());
}

#[test]
fn initialized_project_is_runnable_and_statically_verifiable() {
    let workspace = Workspace::new();
    let project = workspace.child("hello नमस्ते");
    init(&project);
    let manifest = fs::read_to_string(project.join("scarp.project")).unwrap();
    assert_eq!(manifest, "project \"hello नमस्ते\" {\n    version: \"0.1.0\"\n    entry: \"src/main.srp\"\n    type: \"application\"\n    dependencies {}\n}\n");
    assert_eq!(
        fs::read_to_string(project.join("src/main.srp")).unwrap(),
        "print(\"Hello from Scarp!\");\n"
    );
    assert!(project.join("tests").is_dir());
    assert!(project.join("examples").is_dir());
    assert!(project.join("README.md").is_file());
    assert!(project.join(".gitignore").is_file());
    let verified = execute(&["verify"], &project);
    assert!(verified.status.success(), "{verified:?}");
    assert!(verified.stdout.is_empty());
    let run = execute(&["run"], &project);
    assert!(run.status.success(), "{run:?}");
    assert_eq!(run.stdout, b"Hello from Scarp!\n");
    assert!(run.stderr.is_empty());
    let launch = execute(&["launch"], &project);
    assert_eq!(launch.stdout, run.stdout);
}

#[test]
fn current_directory_project_run_keeps_existing_file_run_behavior() {
    let workspace = Workspace::new();
    let project = workspace.child("current");
    init(&project);
    let from_cwd = inside(&["run"], &project);
    assert!(from_cwd.status.success(), "{from_cwd:?}");
    assert_eq!(from_cwd.stdout, b"Hello from Scarp!\n");
    let from_file = execute(&["run"], &project.join("src/main.srp"));
    assert_eq!(from_cwd.stdout, from_file.stdout);
    let original = execute(&["exec"], &project.join("src/main.srp"));
    assert_eq!(original.stdout, from_file.stdout);
}

#[test]
fn modern_projects_load_real_imports_and_verify_without_runtime_execution() {
    let workspace = Workspace::new();
    let project = workspace.child("multi-file");
    init(&project);
    fs::write(project.join("src/math.srp"), "pub bind answer = 42;\n").unwrap();
    fs::write(
        project.join("src/main.srp"),
        "import \"math.srp\" as math;\nprint(math.answer);\n",
    )
    .unwrap();
    let verified = execute(&["verify"], &project);
    assert!(verified.status.success(), "{verified:?}");
    let run = execute(&["run"], &project);
    assert!(run.status.success(), "{run:?}");
    assert_eq!(run.stdout, b"42\n");

    fs::write(project.join("src/main.srp"), "print(1 / 0);").unwrap();
    let static_check = execute(&["verify"], &project);
    assert!(static_check.status.success(), "{static_check:?}");
    let failing_run = execute(&["run"], &project);
    assert_eq!(failing_run.status.code(), Some(1), "{failing_run:?}");
    assert!(failing_run.stdout.is_empty());
}

#[test]
fn legacy_manifest_and_commands_remain_compatible() {
    let workspace = Workspace::new();
    let project = workspace.child("legacy");
    let created = execute(&["create"], &project);
    assert!(created.status.success(), "{created:?}");
    assert!(project.join("Scarp.project").is_file());
    assert!(!project.join("scarp.project").is_file() || cfg!(windows));
    let run = execute(&["run"], &project);
    assert!(run.status.success(), "{run:?}");
    assert_eq!(run.stdout, b"Hello from Scarp!\n");
    assert!(execute(&["launch"], &project).status.success());
    assert!(execute(&["verify"], &project).status.success());
}

#[test]
fn malformed_manifest_is_rejected_with_no_application_stdout() {
    let workspace = Workspace::new();
    let project = workspace.child("bad-format");
    init(&project);
    let manifest = project.join("scarp.project");
    let good = fs::read_to_string(&manifest).unwrap();
    for broken in [
        good.replace("0.1.0", "0.01.0"),
        good.replace("src/main.srp", "../escape.srp"),
        good.replace("src/main.srp", "src/./main.srp"),
        good.replace("version: \"0.1.0\"", "version: \"0.1.0\"\n version: \"0.2.0\""),
        good.replace("dependencies {}", "dependencies { remote: \"script\" }"),
        good.replace("type: \"application\"", "type: \"unknown\""),
        good.replace("type: \"application\"", "unknown: \"value\""),
        good.replace("type: \"application\"", ""),
        "project \"oops\" {".to_owned(),
    ] {
        fs::write(&manifest, &broken).unwrap();
        for command in ["run", "verify"] {
            let output = execute(&[command, "--diagnostic-format=json"], &project);
            assert_eq!(output.status.code(), Some(1), "{command}: {output:?}");
            assert!(output.stdout.is_empty());
            assert!(
                String::from_utf8_lossy(&output.stderr).contains("\"schema_version\":1"),
                "{command}: {output:?}"
            );
        }
    }
}

#[test]
fn invalid_utf8_and_missing_entry_are_rejected() {
    let workspace = Workspace::new();
    let project = workspace.child("missing");
    init(&project);
    fs::write(project.join("scarp.project"), [0xff_u8, 0xfe]).unwrap();
    let malformed = execute(&["verify"], &project);
    assert_eq!(malformed.status.code(), Some(1), "{malformed:?}");
    assert!(malformed.stdout.is_empty());

    fs::write(
        project.join("scarp.project"),
        "project \"missing\" { version: \"0.1.0\" entry: \"src/not-found.srp\" type: \"application\" }\n",
    ).unwrap();
    let missing = execute(&["run"], &project);
    assert_eq!(missing.status.code(), Some(1), "{missing:?}");
    assert!(missing.stdout.is_empty());
}

#[test]
fn init_never_adopts_existing_directory_or_modifies_its_files() {
    let workspace = Workspace::new();
    let project = workspace.child("occupied");
    fs::create_dir(&project).unwrap();
    fs::write(project.join("sentinel"), b"keep").unwrap();
    let result = execute(&["init"], &project);
    assert_eq!(result.status.code(), Some(1), "{result:?}");
    assert_eq!(fs::read(project.join("sentinel")).unwrap(), b"keep");
    assert!(!project.join("scarp.project").exists());
}

#[test]
fn init_rejects_invalid_names_and_bad_cli_arguments() {
    let workspace = Workspace::new();
    for name in [" name", "end ", "a:b"] {
        let output = execute(&["init"], &workspace.child(name));
        assert_eq!(output.status.code(), Some(1), "{name}: {output:?}");
    }
    for arguments in [&["init"][..], &["init", "a", "b"][..], &["init", "--oops"][..]] {
        let output = inside(arguments, &workspace.0);
        assert_eq!(output.status.code(), Some(2), "{arguments:?}: {output:?}");
    }
    assert!(inside(&["init", "--help"], &workspace.0).status.success());
    assert!(inside(&["guide", "init"], &workspace.0).status.success());
}

#[cfg(unix)]
#[test]
fn modern_manifest_entry_symlink_cannot_escape_project_root() {
    use std::os::unix::fs::symlink;
    let workspace = Workspace::new();
    let project = workspace.child("inside");
    init(&project);
    let outside = workspace.child("outside.srp");
    fs::write(&outside, "print(100);").unwrap();
    fs::remove_file(project.join("src/main.srp")).unwrap();
    symlink(outside, project.join("src/main.srp")).unwrap();
    let output = execute(&["run"], &project);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(output.stdout.is_empty());
    let verified = execute(&["verify"], &project);
    assert_eq!(verified.status.code(), Some(1), "{verified:?}");
}
