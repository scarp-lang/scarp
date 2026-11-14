//! Cross-feature tests: EOF fallback, file-backed stdin and immediate prompts.
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    sync::atomic::{AtomicUsize, Ordering},
};

static NEXT: AtomicUsize = AtomicUsize::new(0);

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!(
            "scarp-io-integration-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    fn write(&self, filename: &str, bytes: &[u8]) -> PathBuf {
        let path = self.0.join(filename);
        fs::write(&path, bytes).unwrap();
        path
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn cli(command: &str, flags: &[&str], source: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_scarp"))
        .arg(command)
        .args(flags)
        .arg(source)
        .stdin(Stdio::null())
        .output()
        .expect("run Scarp test program")
}

#[test]
fn input_file_and_immediate_ask_combine_with_eof_fallback() {
    let fixture = Fixture::new();
    let source = fixture.write(
        "greeter.srp",
        br#"print("buffered"); bind name = ask("Name: "); bind tail = read_line_or("EOF"); print(name, tail);"#,
    );
    let path = fixture.write("input with spaces.txt", "Ada\n".as_bytes());
    let file = path.to_str().unwrap();

    for verb in ["exec", "run"] {
        for flags in [
            vec!["--interactive", "--input-file", file],
            vec!["--input-file", file, "--interactive"],
        ] {
            let result = cli(verb, &flags, &source);
            assert!(result.status.success(), "{result:?}");
            assert_eq!(result.stdout, b"Name: buffered\nAda EOF\n");
            assert!(result.stderr.is_empty());
        }
    }
}

#[test]
fn explicit_file_input_without_prompt_capability_remains_noninteractive() {
    let fixture = Fixture::new();
    let source = fixture.write("app.srp", br#"print(read_line_or("fallback"));"#);
    let input = fixture.write("empty.txt", b"");
    let result = cli("exec", &["--input-file", input.to_str().unwrap()], &source);
    assert!(result.status.success(), "{result:?}");
    assert_eq!(result.stdout, b"fallback\n");

    let source = fixture.write("ask.srp", br#"print(ask("Name: "));"#);
    let result = cli("exec", &["--input-file", input.to_str().unwrap()], &source);
    assert_eq!(result.status.code(), Some(1));
    assert!(result.stdout.is_empty());
    assert!(String::from_utf8_lossy(&result.stderr).contains("E0054"));
}

#[test]
fn prompt_before_invalid_utf8_stays_visible_and_buffered_output_is_discarded() {
    let fixture = Fixture::new();
    let source = fixture.write("app.srp", br#"print("buffered"); print(ask("Name: "));"#);
    let input = fixture.write("invalid.txt", b"\xff\n");
    let result = cli(
        "exec",
        &["--interactive", "--input-file", input.to_str().unwrap()],
        &source,
    );
    assert_eq!(result.status.code(), Some(1));
    assert_eq!(result.stdout, b"Name: ");
    assert!(String::from_utf8_lossy(&result.stderr).contains("E0056"));
}

#[test]
fn read_limit_preflight_does_not_expose_prompt() {
    let fixture = Fixture::new();
    let source = fixture.write("app.srp", br#"print(ask("Name: "));"#);
    let input = fixture.write("input.txt", b"Ada\n");
    let result = cli(
        "exec",
        &[
            "--interactive",
            "--max-input-reads",
            "0",
            "--input-file",
            input.to_str().unwrap(),
        ],
        &source,
    );
    assert_eq!(result.status.code(), Some(1));
    assert!(result.stdout.is_empty());
    assert!(String::from_utf8_lossy(&result.stderr).contains("E0058"));
}

#[test]
fn empty_line_is_not_eof_even_with_interactive_file_input() {
    let fixture = Fixture::new();
    let source = fixture.write(
        "empty.srp",
        br#"bind value = ask("First: "); bind other = read_line_or("fallback"); print(value, other);"#,
    );
    let input = fixture.write("two-lines.txt", b"\nZ\n");
    let result = cli(
        "run",
        &["--interactive", "--input-file", input.to_str().unwrap()],
        &source,
    );
    assert!(result.status.success(), "{result:?}");
    assert_eq!(result.stdout, b"First:  Z\n");
}

#[test]
fn failed_input_file_open_does_not_expose_prompt() {
    let fixture = Fixture::new();
    let source = fixture.write("app.srp", br#"print(ask("Name: "));"#);
    let missing = fixture.0.join("does-not-exist.txt");
    let result = cli(
        "exec",
        &["--interactive", "--input-file", missing.to_str().unwrap()],
        &source,
    );
    assert_eq!(result.status.code(), Some(1));
    assert!(result.stdout.is_empty());
    assert!(String::from_utf8_lossy(&result.stderr).contains("could not open input file"));
}
