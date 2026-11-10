use scarp::{
    diagnostics::DiagnosticKind as Kind,
    modules::{self, MemorySourceLoader, ModuleLimits, ModuleRunError},
};
use std::{
    fs,
    io::{self, Cursor, Write},
    path::PathBuf,
    process::{Child, Command, Output, Stdio},
    sync::atomic::{AtomicUsize, Ordering},
    time::{Duration, Instant},
};

static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "scarp-input-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    fn source(&self, name: &str, text: &str) -> PathBuf {
        let path = self.0.join(name);
        fs::write(&path, text).unwrap();
        path
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn wait(mut child: Child) -> Output {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if child.try_wait().unwrap().is_some() {
            return child.wait_with_output().unwrap();
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            let output = child.wait_with_output().unwrap();
            panic!("Scarp unexpectedly blocked on stdin: {output:?}");
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn cli(args: &[&str], path: &std::path::Path, bytes: &[u8]) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_scarp"))
        .args(args)
        .arg(path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    if let Err(error) = input.write_all(bytes) {
        // A rejected invocation/zero quota may exit before the parent writes.
        assert_eq!(error.kind(), io::ErrorKind::BrokenPipe);
    }
    drop(input);
    wait(child)
}

#[test]
fn input_fixture_runs_through_exec_run_embedding_and_static_verify() {
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/program_input.srp");
    for verb in ["exec", "run"] {
        let result = cli(
            &[verb],
            &source,
            include_bytes!("../examples/program_input.stdin"),
        );
        assert!(result.status.success(), "{result:?}");
        assert_eq!(
            result.stdout,
            include_bytes!("../examples/program_input.stdout")
        );
        assert!(result.stderr.is_empty());
    }
    let result = cli(&["verify"], &source, b"");
    assert!(result.status.success());
    assert!(result.stdout.is_empty() && result.stderr.is_empty());
}

#[test]
fn static_commands_and_legacy_check_finish_with_stdin_still_open() {
    let temp = Temp::new();
    let path = temp.source("input.srp", "print(read_line());");
    for args in [
        vec!["verify"],
        vec!["inspect", "modules"],
        vec!["inspect", "symbols", "--json"],
        vec!["inspect", "references", "--json"],
        vec!["check"],
    ] {
        let mut child = Command::new(env!("CARGO_BIN_EXE_scarp"))
            .args(&args)
            .arg(&path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let still_open = child.stdin.take().unwrap();
        let result = wait(child);
        drop(still_open);
        if args[0] == "check" {
            assert_eq!(result.status.code(), Some(1));
            assert!(result.stdout.is_empty());
            assert!(String::from_utf8_lossy(&result.stderr).contains("E0054"));
        } else {
            assert!(result.status.success(), "{args:?}: {result:?}");
            assert!(result.stderr.is_empty());
        }
    }
}

#[test]
fn cli_input_limits_json_errors_and_raw_unicode_bytes_are_real() {
    let temp = Temp::new();
    let path = temp.source(
        "unicode.srp",
        "// 🌍\nprint(\"before\"); print(read_line());",
    );
    for (flags, bytes, code) in [
        (vec![], b"".as_slice(), "E0055"),
        (vec![], b"\xff\n".as_slice(), "E0056"),
        (
            vec!["--max-input-line-bytes", "2"],
            b"abc\n".as_slice(),
            "E0058",
        ),
        (vec!["--max-input-bytes", "2"], b"abc\n".as_slice(), "E0058"),
        (vec!["--max-input-reads", "0"], b"abc\n".as_slice(), "E0058"),
    ] {
        let mut args = vec!["exec", "--diagnostic-format=json"];
        args.extend(flags);
        let result = cli(&args, &path, bytes);
        assert_eq!(result.status.code(), Some(1), "{result:?}");
        assert!(result.stdout.is_empty());
        let json = String::from_utf8(result.stderr).unwrap();
        assert!(
            json.contains(code) && json.contains("unicode.srp"),
            "{json}"
        );
        assert!(json.trim_start().starts_with('{'));
    }
    let result = cli(
        &[
            "exec",
            "--max-input-line-bytes",
            "4",
            "--max-input-bytes",
            "4",
            "--max-input-reads",
            "1",
        ],
        &path,
        "é\r\n".as_bytes(),
    );
    assert!(result.status.success(), "{result:?}");
    assert_eq!(result.stdout, "before\né\n".as_bytes());
    assert!(result.stderr.is_empty());
    for flags in [
        vec!["--max-input-reads", "-1"],
        vec!["--max-input-bytes", "1x"],
        vec!["--max-input-reads", "1", "--max-input-reads", "2"],
    ] {
        let mut args = vec!["exec"];
        args.extend(flags);
        assert_eq!(cli(&args, &path, b"").status.code(), Some(2));
    }
}

#[test]
fn project_launch_supports_input_and_verify_remains_static() {
    let temp = Temp::new();
    temp.source(
        "Scarp.project",
        "scarp-project: 1\nname: input-test\nentry: main.srp\n",
    );
    temp.source("main.srp", "print(read_line());");
    let result = cli(&["launch", "--max-input-reads", "1"], &temp.0, b"project\n");
    assert!(result.status.success(), "{result:?}");
    assert_eq!(result.stdout, b"project\n");
    let result = cli(&["verify"], &temp.0, b"");
    assert!(result.status.success() && result.stdout.is_empty() && result.stderr.is_empty());
}

#[test]
fn module_input_navigation_signatures_and_failures_preserve_original_sources() {
    let mut loader = MemorySourceLoader::new();
    loader
        .insert("main.srp", "import \"lib.srp\" as lib; print(lib.line());")
        .unwrap();
    loader
        .insert(
            "lib.srp",
            "// 🌍\npub funcra line() -> string { return read_line(); }",
        )
        .unwrap();
    let indexed = modules::compile_indexed_with_loader(
        "main.srp",
        &loader,
        ModuleLimits::default(),
        Default::default(),
    )
    .unwrap();
    let mut symbols = Vec::new();
    indexed.index().write_symbols_json(&mut symbols).unwrap();
    assert!(String::from_utf8(symbols).unwrap().contains("read_line"));
    let program =
        modules::compile_with_loader("main.srp", &loader, ModuleLimits::default()).unwrap();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    program
        .execute_with_input(
            &mut Cursor::new(b"module\n"),
            &mut out,
            &mut err,
            Default::default(),
            Default::default(),
        )
        .unwrap();
    assert_eq!(out, b"module\n");
    out.clear();
    let ModuleRunError::Diagnostic(error) = program
        .execute_with_input(
            &mut Cursor::new(b""),
            &mut out,
            &mut err,
            Default::default(),
            Default::default(),
        )
        .unwrap_err()
    else {
        panic!()
    };
    assert_eq!(error.diagnostic.kind, Kind::InputEof);
    let rendered = error.render();
    assert!(
        rendered.contains("lib.srp:2:") && rendered.contains("main.srp:1:"),
        "{rendered}"
    );
    assert!(out.is_empty() && err.is_empty());
}

struct Broken;
impl Write for Broken {
    fn write(&mut self, _: &[u8]) -> io::Result<usize> {
        Err(io::Error::new(io::ErrorKind::BrokenPipe, "closed"))
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[test]
fn input_consumption_cannot_be_rolled_back_after_output_failure() {
    let mut input = Cursor::new(b"read\nnext\n");
    let error = scarp::run_with_input(
        "print(read_line());",
        &mut input,
        &mut Broken,
        &mut Vec::new(),
    )
    .unwrap_err();
    assert!(matches!(error, scarp::RunError::Output(e) if e.kind()==io::ErrorKind::BrokenPipe));
    assert_eq!(input.position(), 5);
}
