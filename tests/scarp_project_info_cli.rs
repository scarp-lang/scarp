//! End-to-end contracts for read-only Scarp project metadata inspection.
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
            "scarp-project-info-cli-{}-{serial}",
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

fn cli(args: &[&str], cwd: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_scarp"))
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("Scarp CLI must start")
}

fn init(ws: &Workspace, name: &str) -> PathBuf {
    let path = ws.child(name);
    let result = Command::new(env!("CARGO_BIN_EXE_scarp"))
        .arg("init")
        .arg(&path)
        .output()
        .unwrap();
    assert!(result.status.success(), "{result:?}");
    path
}

#[test]
fn modern_project_reports_stable_json_and_text_without_running_source() {
    let ws = Workspace::new();
    let root = init(&ws, "unicode नमस्ते");
    fs::write(root.join("src/main.srp"), "print(1 / 0);\n").unwrap();
    let from_cwd = cli(&["project", "info", "--json"], &root);
    assert!(from_cwd.status.success(), "{from_cwd:?}");
    assert!(from_cwd.stderr.is_empty());
    assert_eq!(
        std::str::from_utf8(&from_cwd.stdout).unwrap(),
        "{\"schema_version\":1,\"name\":\"unicode नमस्ते\",\"version\":\"0.1.0\",\"type\":\"application\",\"entry\":\"src/main.srp\",\"manifest\":\"scarp.project\",\"format\":\"v2\"}\n"
    );

    let from_path = Command::new(env!("CARGO_BIN_EXE_scarp"))
        .arg("project")
        .arg("info")
        .arg("--json")
        .arg(&root)
        .output()
        .unwrap();
    assert_eq!(from_path.stdout, from_cwd.stdout);

    let text = cli(&["project", "info"], &root);
    assert!(text.status.success(), "{text:?}");
    assert_eq!(
        std::str::from_utf8(&text.stdout).unwrap(),
        "Name: unicode नमस्ते\nVersion: 0.1.0\nType: application\nManifest: scarp.project (v2)\nEntry: src/main.srp\n"
    );
    // A runtime error in the application's entry does not affect read-only
    // project inspection. The old run command still executes source.
    assert_eq!(cli(&["run"], &root).status.code(), Some(1));
}

#[test]
fn legacy_v1_manifest_has_unspecified_version_and_same_entry() {
    let ws = Workspace::new();
    let path = ws.child("legacy");
    let created = Command::new(env!("CARGO_BIN_EXE_scarp"))
        .arg("create")
        .arg(&path)
        .output()
        .unwrap();
    assert!(created.status.success(), "{created:?}");
    let result = cli(&["project", "info", "--json"], &path);
    assert!(result.status.success(), "{result:?}");
    assert_eq!(
        std::str::from_utf8(&result.stdout).unwrap(),
        "{\"schema_version\":1,\"name\":\"legacy\",\"version\":null,\"type\":\"application\",\"entry\":\"src/main.srp\",\"manifest\":\"Scarp.project\",\"format\":\"legacy-v1\"}\n"
    );
    let text = cli(&["project", "info"], &path);
    assert!(text.status.success(), "{text:?}");
    assert!(String::from_utf8_lossy(&text.stdout).contains("Version: unspecified\n"));
}

#[test]
fn JSON_metadata_escapes_quotes_and_keeps_unicode_readable() {
    let ws = Workspace::new();
    let root = init(&ws, "quoted");
    fs::write(
        root.join("scarp.project"),
        "project \"a \\\"quoted\\\" हिंदी\" {\n version: \"2.3.4-rc.1+git.9\"\n entry: \"src/main.srp\"\n type: \"application\"\n}\n",
    )
    .unwrap();
    let result = cli(&["project", "info", "--json"], &root);
    assert!(result.status.success(), "{result:?}");
    let json = std::str::from_utf8(&result.stdout).unwrap();
    assert!(
        json.contains("\"name\":\"a \\\"quoted\\\" हिंदी\""),
        "{json}"
    );
    assert!(json.contains("\"version\":\"2.3.4-rc.1+git.9\""), "{json}");
}

#[test]
fn invalid_manifest_or_entry_reports_failure_without_success_json() {
    let ws = Workspace::new();
    let root = init(&ws, "broken");
    let manifest_path = root.join("scarp.project");
    let good = fs::read_to_string(&manifest_path).unwrap();
    for source in [
        good.replace("0.1.0", "0.01.0").into_bytes(),
        good.replace("src/main.srp", "../escape.srp").into_bytes(),
        good.replace("application", "library").into_bytes(),
        good.replace(
            "version: \"0.1.0\"",
            "version: \"0.1.0\"\n version: \"0.2.0\"",
        )
        .into_bytes(),
        vec![0xff, 0xfe],
        vec![b'x'; 4097],
    ] {
        fs::write(&manifest_path, source).unwrap();
        let result = cli(&["project", "info", "--json"], &root);
        assert_eq!(result.status.code(), Some(1), "{result:?}");
        assert!(result.stdout.is_empty(), "{result:?}");
        assert!(String::from_utf8_lossy(&result.stderr).starts_with("error: "));
    }
    fs::write(&manifest_path, good).unwrap();
    fs::remove_file(root.join("src/main.srp")).unwrap();
    let missing = cli(&["project", "info"], &root);
    assert_eq!(missing.status.code(), Some(1), "{missing:?}");
    assert!(missing.stdout.is_empty());
}

#[test]
fn usage_errors_do_not_open_or_execute_programs() {
    let ws = Workspace::new();
    let root = init(&ws, "usage");
    for args in [
        &["project"][..],
        &["project", "unknown"][..],
        &["project", "info", "a", "b"][..],
        &["project", "info", "--json", "--json"][..],
        &["project", "info", "--unknown"][..],
    ] {
        let result = cli(args, &root);
        assert_eq!(result.status.code(), Some(2), "{args:?}: {result:?}");
        assert!(result.stdout.is_empty());
        assert!(String::from_utf8_lossy(&result.stderr).contains("Usage:"));
    }
    for args in [
        &["project", "--help"][..],
        &["project", "info", "--help"][..],
        &["guide", "project"][..],
    ] {
        let result = cli(args, &root);
        assert!(result.status.success(), "{args:?}: {result:?}");
        assert!(result.stdout.starts_with(b"Usage:"));
        assert!(result.stderr.is_empty());
    }
    assert!(cli(&["project", "info", "--"], &root).status.success());
}

#[cfg(unix)]
#[test]
fn rejects_symlinked_manifests_and_outside_entrypoints() {
    use std::os::unix::fs::symlink;
    let ws = Workspace::new();
    let root = init(&ws, "linked");
    let manifest_path = root.join("scarp.project");
    let external = ws.child("outside.srp");
    fs::write(&external, "print(42);").unwrap();

    fs::remove_file(root.join("src/main.srp")).unwrap();
    symlink(&external, root.join("src/main.srp")).unwrap();
    let outside = cli(&["project", "info", "--json"], &root);
    assert_eq!(outside.status.code(), Some(1), "{outside:?}");
    assert!(outside.stdout.is_empty());

    fs::remove_file(&manifest_path).unwrap();
    let external_manifest = ws.child("external-manifest");
    fs::write(
        &external_manifest,
        "project \"linked\" { version: \"1.0.0\" entry: \"src/main.srp\" type: \"application\" }",
    )
    .unwrap();
    symlink(&external_manifest, &manifest_path).unwrap();
    let linked = cli(&["project", "info"], &root);
    assert_eq!(linked.status.code(), Some(1), "{linked:?}");
    assert!(linked.stdout.is_empty());
}
