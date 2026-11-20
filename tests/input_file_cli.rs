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
            "scarp-input-file-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn file(&self, name: &str, bytes: &[u8]) -> PathBuf {
        let path = self.0.join(name);
        fs::write(&path, bytes).unwrap();
        path
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn explicit_input_file_overrides_stdin_and_preserves_unicode() {
    let fixture = Fixture::new();
    let source = fixture.file("app with space.srp", b"print(read_line());");
    let input = fixture.file("input with space.txt", "नमस्ते 🌍\r\n".as_bytes());
    for command in ["exec", "run"] {
        let mut child = Command::new(env!("CARGO_BIN_EXE_scarp"))
            .arg(command)
            .arg("--input-file")
            .arg(&input)
            .arg(&source)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(b"wrong\n").ok();
        let result = child.wait_with_output().unwrap();
        assert!(result.status.success(), "{result:?}");
        assert_eq!(result.stdout, "नमस्ते 🌍\n".as_bytes());
        assert!(result.stderr.is_empty());
    }
}

#[test]
fn file_errors_and_runtime_limits_produce_no_program_output() {
    let fixture = Fixture::new();
    let source = fixture.file("app.srp", b"print(\"before\"); print(read_line());");
    let invalid = fixture.file("invalid.txt", b"\xff\n");
    let valid = fixture.file("valid.txt", b"text\n");
    let missing = fixture.0.join("missing.txt");
    for (path, flag, expected, status) in [
        (&invalid, None, "E0056", 1),
        (&valid, Some("--max-input-reads"), "E0058", 1),
        (&missing, None, "could not open input file", 1),
    ] {
        let mut command = Command::new(env!("CARGO_BIN_EXE_scarp"));
        command.arg("exec").arg("--input-file").arg(path);
        if let Some(flag) = flag {
            command.arg(flag).arg("0");
        }
        let result = command.arg(&source).output().unwrap();
        assert_eq!(result.status.code(), Some(status));
        assert!(result.stdout.is_empty());
        assert!(
            String::from_utf8_lossy(&result.stderr).contains(expected),
            "{result:?}"
        );
    }

    let result = Command::new(env!("CARGO_BIN_EXE_scarp"))
        .args(["exec", "--diagnostic-format=json", "--input-file"])
        .arg(&missing)
        .arg(&source)
        .output()
        .unwrap();
    assert_eq!(result.status.code(), Some(1));
    assert!(result.stdout.is_empty());
    assert!(String::from_utf8_lossy(&result.stderr).contains("could not open input file"));
}

#[test]
fn static_commands_duplicates_and_missing_paths_reject_input_file() {
    let fixture = Fixture::new();
    let source = fixture.file("app.srp", b"print(read_line());");
    let input = fixture.file("input.txt", b"text\n");
    for args in [
        vec!["verify", "--input-file", "input.txt"],
        vec!["check", "--input-file", "input.txt"],
        vec!["exec", "--input-file"],
        vec![
            "exec",
            "--input-file",
            "input.txt",
            "--input-file",
            "again.txt",
        ],
    ] {
        let result = Command::new(env!("CARGO_BIN_EXE_scarp"))
            .args(args)
            .arg(&source)
            .output()
            .unwrap();
        assert_eq!(result.status.code(), Some(2), "{result:?}");
    }
    let result = Command::new(env!("CARGO_BIN_EXE_scarp"))
        .arg("exec")
        .arg("--input-file")
        .arg(&input)
        .arg(&source)
        .output()
        .unwrap();
    assert!(result.status.success(), "{result:?}");
    assert_eq!(result.stdout, b"text\n");
}

#[test]
fn project_execution_accepts_the_same_explicit_input_file() {
    let fixture = Fixture::new();
    let project = fixture.0.join("project");
    fs::create_dir_all(project.join("src")).unwrap();
    fs::write(
        project.join("scarp.project"),
        "project \"input-demo\" { version: \"0.1.0\" entry: \"src/main.srp\" type: \"application\" dependencies {} }",
    )
    .unwrap();
    fs::write(project.join("src/main.srp"), "print(read_line());").unwrap();
    let input = fixture.file("project-data.txt", b"project input\n");
    for verb in ["run", "launch"] {
        let result = Command::new(env!("CARGO_BIN_EXE_scarp"))
            .arg(verb)
            .arg("--input-file")
            .arg(&input)
            .arg(&project)
            .output()
            .unwrap();
        assert!(result.status.success(), "{result:?}");
        assert_eq!(result.stdout, b"project input\n");
        assert!(result.stderr.is_empty());
    }
}
