use std::{
    fs,
    path::Path,
    process::{Command, Output},
    sync::atomic::{AtomicUsize, Ordering},
};

static NEXT: AtomicUsize = AtomicUsize::new(0);

fn cli(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_scarp"))
        .args(args)
        .output()
        .expect("Scarp CLI must start")
}

fn with_file(args: &[&str], file: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_scarp"))
        .args(args)
        .arg(file)
        .output()
        .expect("Scarp CLI must start")
}

#[test]
fn inspect_ast_outputs_the_actual_parser_tree_without_executing() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("examples")
        .join("hello.srp");
    let source = fs::read_to_string(&path).unwrap();
    let parsed = scarp::parse(&source).unwrap();
    let output = with_file(&["inspect", "ast"], &path);
    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        format!("{parsed:#?}\n")
    );
}

#[test]
fn inspect_ast_reads_only_entry_source_and_does_not_run_program() {
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    let folder =
        std::env::temp_dir().join(format!("scarp-inspect-ast-{}-{id}", std::process::id()));
    fs::create_dir(&folder).unwrap();
    let file = folder.join("AST नमस्ते.srp");
    // Missing import would make verify fail; runtime arithmetic would fail
    // during execution. The frontend AST parser deliberately does neither.
    fs::write(&file, "import \"missing.srp\" as missing; print(1 / 0);").unwrap();
    let output = with_file(&["inspect", "ast"], &file);
    let verified = with_file(&["verify"], &file);
    let _ = fs::remove_dir_all(folder);
    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty());
    let result = String::from_utf8(output.stdout).unwrap();
    assert!(result.contains("Import"), "{result}");
    assert!(
        result.contains("Println") || result.contains("Output"),
        "{result}"
    );
    assert_eq!(verified.status.code(), Some(1), "{verified:?}");
}

#[test]
fn inspect_ast_rejects_unavailable_success_json_and_top_level_alias() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("examples")
        .join("hello.srp");
    for args in [
        vec!["inspect", "ast", "--json"],
        vec!["inspect", "ast", "--max-steps", "1"],
        vec!["inspect", "ast", "--bogus"],
        vec!["ast"],
    ] {
        let output = with_file(&args, &path);
        assert_eq!(output.status.code(), Some(2), "{args:?}: {output:?}");
        assert!(output.stdout.is_empty(), "{args:?}: {output:?}");
        assert!(!output.stderr.is_empty(), "{args:?}");
    }
    for args in [
        vec!["guide", "inspect", "ast"],
        vec!["inspect", "ast", "--help"],
        vec!["inspect", "ast", "-h"],
    ] {
        let output = cli(&args);
        assert!(output.status.success(), "{args:?}: {output:?}");
        assert!(output.stderr.is_empty());
        assert!(String::from_utf8_lossy(&output.stdout).contains("Usage: scarp inspect ast"));
    }
}

#[test]
fn inspect_ast_preserves_structured_parser_diagnostics() {
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    let folder =
        std::env::temp_dir().join(format!("scarp-inspect-error-{}-{id}", std::process::id()));
    fs::create_dir(&folder).unwrap();
    let file = folder.join("invalid space.srp");
    fs::write(&file, "print(1 + );").unwrap();
    let output = with_file(&["inspect", "ast", "--diagnostic-format=json"], &file);
    let text_error = with_file(&["inspect", "ast"], &file);
    let _ = fs::remove_dir_all(folder);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(output.stdout.is_empty());
    let diagnostic = String::from_utf8(output.stderr).unwrap();
    assert!(diagnostic.contains("\"schema_version\":1"), "{diagnostic}");
    assert!(
        diagnostic.contains("\"severity\":\"error\""),
        "{diagnostic}"
    );
    assert_eq!(text_error.status.code(), Some(1), "{text_error:?}");
    assert!(text_error.stdout.is_empty());
    assert!(!text_error.stderr.is_empty());
}

#[test]
fn inspect_ast_accepts_explicit_separator_for_dash_leading_filename() {
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    let folder = std::env::temp_dir().join(format!(
        "scarp-inspect-separator-{}-{id}",
        std::process::id()
    ));
    fs::create_dir(&folder).unwrap();
    fs::write(folder.join("-source.srp"), "print(12);").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_scarp"))
        .args(["inspect", "ast", "--", "-source.srp"])
        .current_dir(&folder)
        .output()
        .unwrap();
    let _ = fs::remove_dir_all(folder);
    assert!(output.status.success(), "{output:?}");
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("Program"),
        "{output:?}"
    );
    assert!(output.stderr.is_empty());
}
