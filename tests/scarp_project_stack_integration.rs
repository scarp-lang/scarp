//! Cross-feature integration: native assertions, project test runner and the
//! opt-in CI nonempty-suite gate must preserve structured JSON semantics.
use std::{
    fs,
    path::PathBuf,
    process::{Command, Output},
    sync::atomic::{AtomicUsize, Ordering},
};

static NEXT: AtomicUsize = AtomicUsize::new(0);

struct Workspace(PathBuf);

impl Workspace {
    fn new() -> Self {
        let serial = NEXT.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "scarp-project-integration-{}-{serial}",
            std::process::id()
        ));
        fs::create_dir(&root).unwrap();
        Self(root)
    }

    fn project(&self) -> PathBuf {
        let project = self.0.join("combined");
        let output = Command::new(env!("CARGO_BIN_EXE_scarp"))
            .arg("init")
            .arg(&project)
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        project
    }
}

impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn execute(project: &PathBuf, arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_scarp"))
        .args(arguments)
        .current_dir(project)
        .output()
        .unwrap()
}

#[test]
fn assertion_failure_is_reported_in_json_without_leaking_program_output() {
    let workspace = Workspace::new();
    let project = workspace.project();
    fs::write(
        project.join("tests/a-pass.srp"),
        "assert(2 + 2 == 4, \"arithmetic\");\n",
    )
    .unwrap();
    fs::write(
        project.join("tests/z-fail.srp"),
        "print(\"sensitive program output\"); assert(false, \"expected failure\");\n",
    )
    .unwrap();

    let report = execute(&project, &["test", "--json", "--require-tests"]);
    assert_eq!(report.status.code(), Some(1), "{report:?}");
    assert!(report.stderr.is_empty(), "{report:?}");
    let json = String::from_utf8(report.stdout).unwrap();
    assert!(
        json.starts_with("{\"schema_version\":1,\"mode\":\"run\""),
        "{json}"
    );
    assert!(
        json.contains("\"discovered\":2,\"passed\":1,\"failed\":1"),
        "{json}"
    );
    assert!(
        json.contains("\"path\":\"tests/a-pass.srp\",\"status\":\"passed\""),
        "{json}"
    );
    assert!(
        json.contains("\"path\":\"tests/z-fail.srp\",\"status\":\"failed\""),
        "{json}"
    );
    assert!(json.contains("E0065"), "{json}");
    assert!(json.contains("expected failure"), "{json}");
    assert!(!json.contains("sensitive program output"), "{json}");

    let listing = execute(&project, &["test", "--list", "--json", "--require-tests"]);
    assert!(listing.status.success(), "{listing:?}");
    let listing = String::from_utf8(listing.stdout).unwrap();
    assert!(listing.contains("\"mode\":\"list\""), "{listing}");
    assert!(
        listing.contains("\"discovered\":2,\"passed\":0,\"failed\":0"),
        "{listing}"
    );
    assert!(!listing.contains("E0065"), "{listing}");
}

#[test]
fn optional_nonempty_suite_gate_preserves_default_compatibility() {
    let workspace = Workspace::new();
    let project = workspace.project();
    let normal = execute(&project, &["test", "--json"]);
    assert!(normal.status.success(), "{normal:?}");
    let strict = execute(&project, &["test", "--require-tests", "--json"]);
    assert_eq!(strict.status.code(), Some(1), "{strict:?}");
    assert_eq!(normal.stdout, strict.stdout);
    assert!(strict.stderr.is_empty(), "{strict:?}");
}
