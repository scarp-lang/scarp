use std::{
    fs,
    io::Write,
    path::PathBuf,
    process::{Command, Stdio},
    sync::atomic::{AtomicUsize, Ordering},
};

static NEXT: AtomicUsize = AtomicUsize::new(0);

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "scarp-ask-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    fn file(&self, name: &str, contents: &str) -> PathBuf {
        let path = self.0.join(name);
        fs::write(&path, contents).unwrap();
        path
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn cli(
    verb: &str,
    path: &std::path::Path,
    bytes: &[u8],
    interactive: bool,
) -> std::process::Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_scarp"));
    command.arg(verb);
    if interactive {
        command.arg("--interactive");
    }
    let mut child = command
        .arg(path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    stdin.write_all(bytes).unwrap();
    drop(stdin);
    child.wait_with_output().unwrap()
}

#[test]
fn interactive_flag_is_required_and_forwards_through_exec_and_run() {
    let dir = Fixture::new();
    let path = dir.file(
        "greeting with space.srp",
        r#"bind name = ask("Name: "); print("Hello", name);"#,
    );
    for verb in ["exec", "run"] {
        let result = cli(verb, &path, b"Ada\n", true);
        assert!(result.status.success(), "{result:?}");
        assert_eq!(result.stdout, b"Name: Hello Ada\n");
        assert!(result.stderr.is_empty());
        let rejected = cli(verb, &path, b"Ada\n", false);
        assert_eq!(rejected.status.code(), Some(1));
        assert!(rejected.stdout.is_empty());
        assert!(String::from_utf8_lossy(&rejected.stderr).contains("E0054"));
    }
}

#[test]
fn interactive_launch_uses_project_entrypoint_and_checks_prompt_errors() {
    let dir = Fixture::new();
    let root = dir.0.join("app");
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("scarp.project"),
        "project \"interactive-demo\" { version: \"0.1.0\" entry: \"src/main.srp\" type: \"application\" dependencies {} }",
    ).unwrap();
    fs::write(root.join("src/main.srp"), r#"print(ask("Project> "));"#).unwrap();
    for verb in ["run", "launch"] {
        let result = cli(verb, &root, b"okay\n", true);
        assert!(result.status.success(), "{result:?}");
        assert_eq!(result.stdout, b"Project> okay\n");
        assert!(result.stderr.is_empty());
    }
    let failure = cli("launch", &root, b"", true);
    assert_eq!(failure.status.code(), Some(1));
    assert_eq!(failure.stdout, b"Project> ");
    assert!(String::from_utf8_lossy(&failure.stderr).contains("E0055"));
}

#[test]
fn static_modes_and_duplicate_flags_are_rejected() {
    let dir = Fixture::new();
    let path = dir.file("app.srp", r#"print(ask("Name: "));"#);
    for args in [
        vec!["verify", "--interactive"],
        vec!["check", "--interactive"],
        vec!["run", "--interactive", "--interactive"],
    ] {
        let result = Command::new(env!("CARGO_BIN_EXE_scarp"))
            .args(&args)
            .arg(&path)
            .output()
            .unwrap();
        assert_eq!(result.status.code(), Some(2), "{result:?}");
    }
    let result = Command::new(env!("CARGO_BIN_EXE_scarp"))
        .arg("verify")
        .arg(&path)
        .output()
        .unwrap();
    assert!(result.status.success(), "{result:?}");
}
