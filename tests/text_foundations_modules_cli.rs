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
fn exported_functions_use_conversion_and_string_types_across_modules() {
    let mut loader = MemorySourceLoader::new();
    loader
        .insert(
            "main.srp",
            "import \"lib.srp\" as lib; print(lib.format(\" 42 \"));",
        )
        .unwrap();
    loader
        .insert(
            "lib.srp",
            r#"
        bind suffix="!";
        pub pure funcra format(text:string)->string {
            return text_concat(integer_to_text(text_to_integer(text_trim(text))),suffix);
        }
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
    assert_eq!(out, b"42!\n");
    assert!(err.is_empty());
}

#[test]
fn constant_calls_remain_rejected_and_runtime_error_sources_stay_owned() {
    let mut loader = MemorySourceLoader::new();
    loader
        .insert("main.srp", "import \"lib.srp\" as lib; print(lib.value);")
        .unwrap();
    loader
        .insert("lib.srp", "pub bind value=text_to_integer(\"42\");")
        .unwrap();
    assert_eq!(
        modules::compile_with_loader("main.srp", &loader, ModuleLimits::default())
            .unwrap_err()
            .diagnostic
            .kind,
        Kind::ModuleSyntax
    );
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
            "// café 🌍\npub pure funcra bad()->integer{return text_to_integer(\"4é\");}",
        )
        .unwrap();
    let program =
        modules::compile_with_loader("main.srp", &loader, ModuleLimits::default()).unwrap();
    let error = program.check(ExecutionOptions::default()).unwrap_err();
    assert_eq!(error.diagnostic.kind, Kind::NumericConversion);
    let rendered = error.render();
    assert!(
        rendered.contains("lib.srp:2:") && rendered.contains("main.srp:1:"),
        "{rendered}"
    );
}

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture(std::path::PathBuf);
impl Fixture {
    fn new(text: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "scarp-core-text-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        let file = path.join("text with spaces.srp");
        fs::write(&file, text).unwrap();
        Self(file)
    }
    fn run(&self, args: &[&str]) -> std::process::Output {
        Command::new(env!("CARGO_BIN_EXE_scarp"))
            .args(args)
            .arg(&self.0)
            .output()
            .unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(self.0.parent().unwrap());
    }
}

#[test]
fn cli_static_verify_and_runtime_check_have_distinct_decimal_validation() {
    let input = Fixture::new("print(text_to_integer(\"not a number\"));");
    let result = input.run(&["verify"]);
    assert!(result.status.success() && result.stdout.is_empty() && result.stderr.is_empty());
    for command in ["exec", "run", "check"] {
        let result = input.run(&[command, "--diagnostic-format=json"]);
        assert_eq!(result.status.code(), Some(1));
        assert!(result.stdout.is_empty());
        assert!(String::from_utf8_lossy(&result.stderr).contains("E0044"));
    }
    let input = Fixture::new(
        "print(text_replace(integer_to_text(text_to_integer(text_trim(\" 42 \"))),\"2\",\"3\"));",
    );
    let result = input.run(&["exec"]);
    assert_eq!(result.stdout, b"43\n");
    assert!(result.status.success() && result.stderr.is_empty());
    let result = input.run(&["check"]);
    assert!(result.status.success() && result.stdout.is_empty() && result.stderr.is_empty());
}

#[test]
fn cli_budget_errors_preserve_program_output_and_invalid_arity_is_static() {
    let input =
        Fixture::new("print(\"pending\"); eprint(\"pending\"); print(integer_to_text(123));");
    for command in ["exec", "check"] {
        for (flag, value, code) in [
            ("--max-text-bytes", "2", "E0048"),
            ("--max-steps", "20", "E0017"),
        ] {
            let result = input.run(&[command, flag, value]);
            assert_eq!(result.status.code(), Some(1));
            assert!(result.stdout.is_empty());
            let error = String::from_utf8_lossy(&result.stderr);
            assert!(
                error.contains(code) && !error.contains("\npending\n"),
                "{error}"
            );
        }
    }
    let bad = Fixture::new("print(text_replace(\"a\",\"b\"));");
    let result = bad.run(&["verify"]);
    assert_eq!(result.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&result.stderr).contains("E0024"));
}
