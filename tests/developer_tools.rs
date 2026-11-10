use scarp::{
    diagnostics::DiagnosticKind as Kind,
    modules::{self, MemorySourceLoader, ModuleLimits, SourceLoadError, SourceLoader},
};
use std::process::Command;

fn compile(files: &[(&str, &str)]) -> modules::ModuleProgram {
    let mut loader = MemorySourceLoader::new();
    for (name, source) in files {
        loader.insert(*name, *source).unwrap();
    }
    modules::compile_with_loader("main.srp", &loader, ModuleLimits::default()).unwrap()
}
fn json(program: &modules::ModuleProgram) -> Vec<u8> {
    let mut bytes = Vec::new();
    program.write_inspection_json(&mut bytes).unwrap();
    bytes
}

#[test]
fn every_diagnostic_code_has_a_unique_stable_lookup_and_explanation() {
    let mut codes = std::collections::HashSet::new();
    assert_eq!(Kind::ALL.len(), 58);
    // E0051-E0053 are reserved for the independent BigInt development slice.
    for (&kind, number) in Kind::ALL.iter().zip((1..=50).chain(54..=61)) {
        assert_eq!(kind.code(), format!("E{number:04}"));
        assert!(codes.insert(kind.code()));
        assert_eq!(Kind::from_code(kind.code()), Some(kind));
        assert!(!kind.explanation().is_empty());
    }
    for invalid in ["E0000", "E0051", "E9999", "e0007", "E00007", "7", ""] {
        assert_eq!(Kind::from_code(invalid), None);
    }
    const OVERFLOW: &str = Kind::ArithmeticOverflow.code();
    assert_eq!(OVERFLOW, "E0007");
}

#[test]
fn json_snapshot_is_exact_and_does_not_execute_runtime_calls() {
    let source = include_str!("../examples/inspection_json.srp");
    let mut loader = MemorySourceLoader::new();
    loader.insert("inspection_json.srp", source).unwrap();
    let program =
        modules::compile_with_loader("inspection_json.srp", &loader, ModuleLimits::default())
            .unwrap();
    assert_eq!(
        json(&program),
        include_bytes!("../examples/inspection_json.modules.json")
    );
    let static_program=compile(&[("main.srp","pub bind secret=\"sensitive-secret\"; pure funcra f(n: integer=1/0)->integer needs false; {return n;} print(f());")]);
    let text = String::from_utf8(json(&static_program)).unwrap();
    assert!(text.contains("\"has_default\":true"));
    assert!(!text.contains("sensitive-secret") && !text.contains("1/0"));
}

#[test]
fn json_graph_preserves_deduplicated_dependencies_imports_and_reference_owners() {
    let program=compile(&[("main.srp","import \"a.srp\" as a; import \"a.srp\" as again; import \"b.srp\" as b; pub bind answer=a.value+b.value;"),
        ("a.srp","import \"common.srp\" as c; pub bind value=c.base+1;"),("b.srp","import \"common.srp\" as c; pub bind value=c.base*2;"),("common.srp","pub bind base=10;")]);
    let text = String::from_utf8(json(&program)).unwrap();
    assert!(text.contains("\"dependencies\":[1,3]"));
    assert!(text.contains("\"dependents\":[1,3]"));
    assert_eq!(text.matches("\"target_declaration\":").count(), 4);
    assert_eq!(text.matches("\"path_span\":").count(), 5);
    assert!(text.contains("\"source\":0,\"declaration\":"));
    assert_eq!(
        program
            .sources()
            .locate_span(program.constant_dependencies()[0].span)
            .unwrap()
            .source,
        0
    );
}

struct Labels {
    loader: MemorySourceLoader,
    label: String,
}
impl SourceLoader for Labels {
    fn resolve(&self, from: Option<&str>, path: &str) -> Result<String, SourceLoadError> {
        self.loader.resolve(from, path)
    }
    fn load(&self, path: &str, max: usize) -> Result<String, SourceLoadError> {
        self.loader.load(path, max)
    }
    fn display_name(&self, _: &str) -> String {
        self.label.clone()
    }
}

#[test]
fn json_escapes_controls_quotes_backslashes_and_keeps_unicode() {
    let mut loader = MemorySourceLoader::new();
    loader
        .insert("main.srp", "import \"lib.srp\" as lib;")
        .unwrap();
    loader.insert("lib.srp", "").unwrap();
    let label = format!(
        "café🌍\"\\{}",
        (0u8..=31).map(char::from).collect::<String>()
    );
    let program = modules::compile_with_loader(
        "main.srp",
        &Labels { loader, label },
        ModuleLimits::default(),
    )
    .unwrap();
    let text = String::from_utf8(json(&program)).unwrap();
    assert!(text.contains("café🌍\\\"\\\\"));
    assert!(
        text.contains("\\u0000")
            && text.contains("\\u001b")
            && text.contains("\\n")
            && text.contains("\\r")
            && text.contains("\\t")
    );
    assert!(text
        .trim_end_matches('\n')
        .chars()
        .all(|ch| !ch.is_control()));
}

#[test]
fn json_writer_failures_keep_io_kind_and_can_follow_partial_output() {
    struct Limited(usize);
    impl std::io::Write for Limited {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            if self.0 == 0 {
                return Err(std::io::ErrorKind::PermissionDenied.into());
            }
            let n = self.0.min(bytes.len());
            self.0 -= n;
            Ok(n)
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let program = compile(&[("main.srp", "pub bind value=1; print(1/0);")]);
    assert_eq!(
        program
            .write_inspection_json(&mut Limited(10))
            .unwrap_err()
            .kind(),
        std::io::ErrorKind::PermissionDenied
    );
}

#[test]
fn concurrent_json_reuses_immutable_metadata_and_preserves_original_function_names() {
    let program = std::sync::Arc::new(compile(&[
        ("main.srp", "import \"lib.srp\" as lib;"),
        (
            "lib.srp",
            "pub pure funcra f(n: integer=2)->integer{return n;}",
        ),
    ]));
    let expected = json(&program);
    let mut threads = Vec::new();
    for _ in 0..4 {
        let program = program.clone();
        threads.push(std::thread::spawn(move || json(&program)));
    }
    for thread in threads {
        assert_eq!(thread.join().unwrap(), expected);
    }
    let text = String::from_utf8(expected).unwrap();
    assert!(text.contains("\"name\":\"f\""));
    assert!(!text.contains("lib.srp::f"));
}

#[test]
fn json_cli_rejects_bad_flags_before_io_and_source_errors_before_inventory() {
    for args in [
        vec!["modules", "--json"],
        vec!["modules", "--json", "--json", "missing.srp"],
        vec!["modules", "--json", "--max-steps", "1", "missing.srp"],
        vec!["modules", "missing.srp", "--json"],
        vec!["run", "--json", "missing.srp"],
    ] {
        let result = Command::new(env!("CARGO_BIN_EXE_scarp"))
            .args(args)
            .output()
            .unwrap();
        assert_eq!(result.status.code(), Some(2));
        assert!(result.stdout.is_empty());
    }
    for filename in [
        "tests/fixtures/constants/division.srp",
        "tests/fixtures/constants/call.srp",
    ] {
        let result = Command::new(env!("CARGO_BIN_EXE_scarp"))
            .current_dir(env!("CARGO_MANIFEST_DIR"))
            .args(["modules", "--json", filename])
            .output()
            .unwrap();
        assert_eq!(result.status.code(), Some(1));
        assert!(result.stdout.is_empty());
        assert!(String::from_utf8(result.stderr)
            .unwrap()
            .starts_with("error[E00"));
    }
}

#[test]
fn explain_help_unknown_codes_and_extra_arguments_are_usage_safe() {
    for args in [
        vec!["explain"],
        vec!["explain", "E9999"],
        vec!["explain", "e0007"],
        vec!["explain", "missing.srp"],
        vec!["explain", "E0007", "extra"],
    ] {
        let result = Command::new(env!("CARGO_BIN_EXE_scarp"))
            .args(args)
            .output()
            .unwrap();
        assert_eq!(result.status.code(), Some(2));
        assert!(result.stdout.is_empty());
        assert!(String::from_utf8(result.stderr)
            .unwrap()
            .contains("Usage: scarp explain"));
    }
    let result = Command::new(env!("CARGO_BIN_EXE_scarp"))
        .args(["explain", "--help"])
        .output()
        .unwrap();
    assert!(result.status.success());
    assert!(result.stderr.is_empty());
    let result = Command::new(env!("CARGO_BIN_EXE_scarp"))
        .args(["explain", "E0045"])
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8(result.stdout).unwrap(),
        format!("E0045: {}\n", Kind::TextIndex.explanation())
    );
}

#[test]
fn module_json_flag_emits_a_checked_inventory() {
    let output = Command::new(env!("CARGO_BIN_EXE_scarp"))
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .args(["modules", "--json", "examples/module_inspection.srp"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    assert!(String::from_utf8(output.stdout)
        .unwrap()
        .starts_with("{\"schema_version\":1,"));
}

#[test]
fn explain_codes_needs_no_source_file() {
    let shift = Command::new(env!("CARGO_BIN_EXE_scarp"))
        .args(["explain", "E0046"])
        .output()
        .unwrap();
    assert!(shift.status.success() && shift.stderr.is_empty());
    assert!(String::from_utf8(shift.stdout)
        .unwrap()
        .contains("0 through 63"));
    let output = Command::new(env!("CARGO_BIN_EXE_scarp"))
        .args(["explain", "E0007"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    assert!(String::from_utf8(output.stdout)
        .unwrap()
        .contains("overflow"));
}
