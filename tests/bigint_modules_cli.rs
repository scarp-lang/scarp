use scarp::{
    diagnostics::DiagnosticKind as Kind,
    modules::{self, MemorySourceLoader, ModuleLimits},
    ExecutionOptions,
};
use std::{
    fs,
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
};

#[test]
fn exported_bigint_functions_share_typed_module_and_constant_boundaries() {
    let mut loader = MemorySourceLoader::new();
    loader
        .insert(
            "main.srp",
            "import \"lib.srp\" as lib; bind n:bigint=lib.large(); print(lib.twice(n));",
        )
        .unwrap();
    loader
        .insert(
            "lib.srp",
            r#"
        bind factor=2;
        pub pure funcra large()->bigint { return bigint("123456789012345678901234567890"); }
        pub pure funcra twice(n:bigint)->bigint { return n*bigint_from_integer(factor); }
    "#,
        )
        .unwrap();
    let program =
        modules::compile_with_loader("main.srp", &loader, ModuleLimits::default()).unwrap();
    program.check(ExecutionOptions::default()).unwrap();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    program
        .execute(&mut out, &mut err, ExecutionOptions::default())
        .unwrap();
    assert_eq!(out, b"246913578024691357802469135780\n");
    assert!(err.is_empty());
    // Constant calls remain forbidden. BigInt constant folding is a separate
    // compiler-budget design, never an accidental runtime-budget bypass.
    let mut loader = MemorySourceLoader::new();
    loader
        .insert("main.srp", "import \"lib.srp\" as lib; print(lib.n);")
        .unwrap();
    loader
        .insert(
            "lib.srp",
            "pub bind n:bigint=bigint(\"12345678901234567890\");",
        )
        .unwrap();
    assert_eq!(
        modules::compile_with_loader("main.srp", &loader, ModuleLimits::default())
            .unwrap_err()
            .diagnostic
            .kind,
        Kind::ModuleSyntax
    );
}

#[test]
fn module_budget_diagnostics_keep_the_owning_unicode_source() {
    let mut loader = MemorySourceLoader::new();
    loader
        .insert(
            "main.srp",
            "import \"lib.srp\" as lib; print(\"pending\"); print(lib.bad());",
        )
        .unwrap();
    loader
        .insert(
            "lib.srp",
            "// café 🌍\npub pure funcra bad()->bigint { return bigint(\"12é\"); }",
        )
        .unwrap();
    let program =
        modules::compile_with_loader("main.srp", &loader, ModuleLimits::default()).unwrap();
    let error = program.check(ExecutionOptions::default()).unwrap_err();
    assert_eq!(error.diagnostic.kind, Kind::InvalidBigInt);
    let text = error.render();
    assert!(
        text.contains("lib.srp:2:") && text.contains("main.srp:1:"),
        "{text}"
    );
}

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Source(std::path::PathBuf);
impl Source {
    fn new(text: &str) -> Self {
        let directory = std::env::temp_dir().join(format!(
            "scarp-bigint-cli-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&directory).unwrap();
        let file = directory.join("big with spaces.srp");
        fs::write(&file, text).unwrap();
        Self(file)
    }
    fn command(&self, args: &[&str]) -> std::process::Output {
        Command::new(env!("CARGO_BIN_EXE_scarp"))
            .args(args)
            .arg(&self.0)
            .output()
            .unwrap()
    }
}
impl Drop for Source {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(self.0.parent().unwrap());
    }
}

#[test]
fn cli_exec_check_and_static_verify_have_their_documented_roles() {
    let source = Source::new("print(bigint(\"12345678901234567890\")+bigint(\"1\"));");
    for mode in ["exec", "run", "check"] {
        let result = source.command(&[
            mode,
            "--max-bigint-digits",
            "20",
            "--max-bigint-bytes",
            "64",
        ]);
        assert_eq!(result.status.code(), Some(0), "{result:?}");
        assert_eq!(
            result.stdout,
            if mode == "check" {
                b"".as_slice()
            } else {
                b"12345678901234567891\n"
            }
        );
        assert!(result.stderr.is_empty());
    }
    let invalid = Source::new("print(bigint(\"invalid\"));");
    let result = invalid.command(&["verify"]);
    assert!(result.status.success() && result.stdout.is_empty() && result.stderr.is_empty());
    for mode in ["exec", "check"] {
        let result = invalid.command(&[mode]);
        assert_eq!(result.status.code(), Some(1));
        assert!(result.stdout.is_empty());
        assert!(String::from_utf8_lossy(&result.stderr).contains("E0051"));
    }
}

#[test]
fn cli_digit_allocation_and_work_failures_are_atomic_and_explainable() {
    let source=Source::new("print(\"pending\"); eprint(\"pending\"); print(bigint(\"12345678901234567890\")+bigint(\"1\"));");
    for mode in ["exec", "check"] {
        for (flag, value, code) in [
            ("--max-bigint-digits", "19", "E0052"),
            ("--max-bigint-bytes", "0", "E0052"),
            ("--max-steps", "20", "E0017"),
        ] {
            let result = source.command(&[mode, flag, value]);
            assert_eq!(result.status.code(), Some(1), "{result:?}");
            assert!(result.stdout.is_empty());
            let error = String::from_utf8_lossy(&result.stderr);
            assert!(
                error.contains(code) && !error.contains("\npending\n"),
                "{error}"
            );
        }
    }
    let result = source.command(&[
        "check",
        "--diagnostic-format=json",
        "--max-bigint-digits",
        "19",
    ]);
    assert_eq!(result.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&result.stderr).contains("E0052"));
    for code in ["E0051", "E0052", "E0053"] {
        let result = Command::new(env!("CARGO_BIN_EXE_scarp"))
            .args(["explain", code])
            .output()
            .unwrap();
        assert!(result.status.success() && result.stderr.is_empty());
    }
}

#[test]
fn cli_rejects_invalid_duplicate_and_unsupported_limit_flags() {
    let source = Source::new("print(bigint(\"1\"));");
    for args in [
        vec!["exec", "--max-bigint-digits", "1000001"],
        vec!["exec", "--max-bigint-bytes", "-1"],
        vec![
            "exec",
            "--max-bigint-digits",
            "20",
            "--max-bigint-digits",
            "20",
        ],
        vec!["verify", "--max-bigint-digits", "20"],
        vec!["exec", "--max-bigint-bytes", "18446744073709551616"],
    ] {
        let result = source.command(&args);
        assert_eq!(result.status.code(), Some(2), "{result:?}");
        assert!(result.stdout.is_empty());
    }
}
