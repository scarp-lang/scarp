//! End-to-end checks for the single-command Scarp execution workflow.
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    sync::atomic::{AtomicUsize, Ordering},
};

static NEXT: AtomicUsize = AtomicUsize::new(0);

struct Workspace(PathBuf);

impl Workspace {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "scarp-unified-exec-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        Self(root)
    }

    fn write(&self, path: &str, source: &str) {
        let path = self.0.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, source).unwrap();
    }
}

impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn cli(cwd: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_scarp"))
        .current_dir(cwd)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .unwrap()
}

fn cli_with_input(cwd: &Path, args: &[&str], input: &[u8]) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_scarp"))
        .current_dir(cwd)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(input).unwrap();
    child.wait_with_output().unwrap()
}

#[test]
fn zero_argument_exec_prioritizes_manifest_declared_entrypoint() {
    let ws = Workspace::new();
    ws.write(
        "scarp.project",
        "project \"demo\" { version: \"0.1.0\" entry: \"src/real.srp\" type: \"application\" dependencies {} }",
    );
    ws.write("src/real.srp", "print(\"manifest\");");
    ws.write("src/main.srp", "print(\"src fallback\");");
    ws.write("main.srp", "print(\"root fallback\");");

    for args in [
        vec!["exec"],
        vec!["exec", "--max-steps", "10000"],
        vec!["exec", "."],
    ] {
        let output = cli(&ws.0, &args);
        assert!(output.status.success(), "{args:?}: {output:?}");
        assert_eq!(output.stdout, b"manifest\n");
        assert!(output.stderr.is_empty());
    }

    let explicit = cli(&ws.0, &["exec", "src/main.srp"]);
    assert!(explicit.status.success(), "{explicit:?}");
    assert_eq!(explicit.stdout, b"src fallback\n");
}

#[test]
fn zero_argument_exec_supports_src_then_root_main_without_manifest() {
    let ws = Workspace::new();
    ws.write("src/main.srp", "print(\"src\");");
    ws.write("main.srp", "print(\"root\");");

    let src = cli(&ws.0, &["exec"]);
    assert!(src.status.success(), "{src:?}");
    assert_eq!(src.stdout, b"src\n");

    fs::remove_file(ws.0.join("src/main.srp")).unwrap();
    let root = cli(&ws.0, &["exec"]);
    assert!(root.status.success(), "{root:?}");
    assert_eq!(root.stdout, b"root\n");

    fs::remove_file(ws.0.join("main.srp")).unwrap();
    let missing = cli(&ws.0, &["exec"]);
    assert_eq!(missing.status.code(), Some(2), "{missing:?}");
    assert!(missing.stdout.is_empty());
    assert!(String::from_utf8_lossy(&missing.stderr).contains("no Scarp entrypoint"));

    let json = cli(&ws.0, &["exec", "--diagnostic-format=json"]);
    assert_eq!(json.status.code(), Some(2), "{json:?}");
    assert!(String::from_utf8_lossy(&json.stderr).contains("schema_version"));
}

#[test]
fn one_exec_command_accepts_explicit_root_nested_and_dash_prefixed_files() {
    let ws = Workspace::new();
    ws.write("calculator.srp", "print(2 + 3);");
    ws.write("src/calculator.srp", "print(6 * 7);");
    ws.write("--private.srp", "print(9);");

    for (args, expected) in [
        (vec!["exec", "calculator.srp"], b"5\n".as_slice()),
        (vec!["exec", "src/calculator.srp"], b"42\n".as_slice()),
        (vec!["exec", "--", "--private.srp"], b"9\n".as_slice()),
    ] {
        let result = cli(&ws.0, &args);
        assert!(result.status.success(), "{args:?}: {result:?}");
        assert_eq!(result.stdout, expected);
        assert!(result.stderr.is_empty());
    }

    // A custom file does not silently become the default project entrypoint.
    let missing = cli(&ws.0, &["exec"]);
    assert_eq!(missing.status.code(), Some(2), "{missing:?}");
}

#[test]
fn exec_prompts_and_reads_typed_input_without_an_interactive_flag() {
    let ws = Workspace::new();
    ws.write(
        "main.srp",
        r#"bind number: integer = input_integer("Number: "); print(number + number);"#,
    );
    let result = cli_with_input(&ws.0, &["exec"], b"21\n");
    assert!(result.status.success(), "{result:?}");
    assert_eq!(result.stdout, b"Number: 42\n");
    assert!(result.stderr.is_empty());

    // Legacy run retains explicit interactive opt-in for existing clients.
    let rejected = cli(&ws.0, &["run", "main.srp"]);
    assert_eq!(rejected.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&rejected.stderr).contains("E0054"));
}

#[test]
fn malformed_manifest_never_falls_back_to_an_unrelated_source() {
    let ws = Workspace::new();
    ws.write("scarp.project", "this is not a valid project manifest");
    ws.write("src/main.srp", "print(\"must not execute\");");

    let failure = cli(&ws.0, &["exec"]);
    assert_eq!(failure.status.code(), Some(1), "{failure:?}");
    assert!(failure.stdout.is_empty());

    let malformed_option = cli(&ws.0, &["exec", "--max-steps"]);
    assert_eq!(malformed_option.status.code(), Some(2), "{malformed_option:?}");
}
