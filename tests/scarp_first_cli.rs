use std::{
    fs,
    path::PathBuf,
    process::{Command, Output},
    sync::atomic::{AtomicUsize, Ordering},
};

static NEXT_TEMP: AtomicUsize = AtomicUsize::new(0);

fn scarp(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_scarp"))
        .args(args)
        .output()
        .expect("Scarp executable should start")
}

fn example(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("examples")
        .join(name)
}

fn file_output(args: &[&str], path: &std::path::Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_scarp"))
        .args(args)
        .arg(path)
        .output()
        .expect("Scarp executable should start")
}

#[test]
fn scarp_first_help_and_guides_are_real_and_contextual() {
    for args in [vec![], vec!["guide"], vec!["--help"], vec!["-h"]] {
        let result = scarp(&args);
        assert!(result.status.success(), "{args:?}: {result:?}");
        assert!(result.stderr.is_empty());
        let text = String::from_utf8(result.stdout).unwrap();
        assert!(text.contains("scarp exec"), "{text}");
        assert!(text.contains("scarp inspect"), "{text}");
        assert!(!text.contains("scarp compile <"), "{text}");
    }
    for (args, expected) in [
        (vec!["guide", "exec"], "Usage: scarp exec"),
        (vec!["guide", "verify"], "Usage: scarp verify"),
        (vec!["guide", "inspect"], "scarp inspect modules"),
        (
            vec!["guide", "inspect", "tokens"],
            "Usage: scarp inspect tokens",
        ),
        (
            vec!["guide", "inspect", "modules"],
            "Usage: scarp inspect modules",
        ),
        (vec!["guide", "explain"], "Usage: scarp explain"),
        (vec!["guide", "version"], "Usage: scarp version"),
        (vec!["inspect", "--help"], "scarp inspect modules"),
        (vec!["inspect", "tokens", "--help"], "scarp inspect tokens"),
        (
            vec!["inspect", "modules", "--help"],
            "scarp inspect modules",
        ),
        (vec!["exec", "--help"], "Usage: scarp exec"),
        (vec!["verify", "--help"], "Usage: scarp verify"),
        (vec!["version", "--help"], "Usage: scarp version"),
    ] {
        let result = scarp(&args);
        assert!(result.status.success(), "{args:?}: {result:?}");
        assert!(String::from_utf8_lossy(&result.stdout).contains(expected));
        assert!(result.stderr.is_empty());
    }
}

#[test]
fn version_is_same_as_existing_version_flag() {
    let canonical = scarp(&["version"]);
    assert!(canonical.status.success());
    assert!(canonical.stderr.is_empty());
    assert_eq!(canonical.stdout, scarp(&["--version"]).stdout);
    assert_eq!(canonical.stdout, scarp(&["-V"]).stdout);
}

#[test]
fn exec_uses_the_existing_interpreter_not_an_external_process() {
    let path = example("hello.srp");
    let canonical = file_output(&["exec"], &path);
    let legacy = file_output(&["run"], &path);
    assert!(canonical.status.success(), "{canonical:?}");
    assert_eq!(canonical.stdout, legacy.stdout);
    assert_eq!(canonical.stderr, legacy.stderr);
    assert!(!canonical.stdout.is_empty());

    let canonical = file_output(&["exec", "--max-steps", "0"], &path);
    let legacy = file_output(&["run", "--max-steps", "0"], &path);
    assert_eq!(canonical.status.code(), legacy.status.code());
    assert_eq!(canonical.stdout, legacy.stdout);
    assert_eq!(canonical.stderr, legacy.stderr);

    // The canonical command forwards real execution limits added to main.
    let text_program = example("text_construction.srp");
    let canonical = file_output(&["exec", "--max-text-bytes", "1"], &text_program);
    let legacy = file_output(&["run", "--max-text-bytes", "1"], &text_program);
    assert_eq!(canonical.status.code(), Some(1));
    assert_eq!(canonical.stdout, legacy.stdout);
    assert_eq!(canonical.stderr, legacy.stderr);
    assert!(String::from_utf8_lossy(&canonical.stderr).contains("E0048"));
}

#[test]
fn inspect_subcommands_preserve_backends_and_json() {
    let path = example("module_inspection.srp");
    for (canonical, legacy) in [
        (vec!["inspect", "tokens"], vec!["tokens"]),
        (vec!["inspect", "modules"], vec!["modules"]),
        (
            vec!["inspect", "modules", "--json"],
            vec!["modules", "--json"],
        ),
    ] {
        let new_result = file_output(&canonical, &path);
        let old_result = file_output(&legacy, &path);
        assert!(new_result.status.success(), "{canonical:?}: {new_result:?}");
        assert_eq!(new_result.stdout, old_result.stdout);
        assert_eq!(new_result.stderr, old_result.stderr);
        assert!(!new_result.stdout.is_empty());
    }
}

#[test]
fn unknown_guides_subcommands_and_unsupported_features_fail() {
    for args in [
        vec!["guide", "compile"],
        vec!["guide", "inspect", "ast"],
        vec!["guide", "inspect", "tokens", "unexpected"],
        vec!["inspect", "ast", "hello.srp"],
        vec!["inspect", "tokens", "--json", "hello.srp"],
        vec!["version", "--bogus"],
        vec!["compile", "hello.srp"],
        vec!["verify", "--strict", "hello.srp"],
        vec!["verify", "--max-steps", "1", "hello.srp"],
        vec!["exec", "--unknown-option", "hello.srp"],
    ] {
        let result = scarp(&args);
        assert_eq!(result.status.code(), Some(2), "{args:?}: {result:?}");
        assert!(result.stdout.is_empty(), "{args:?}: {result:?}");
        assert!(!result.stderr.is_empty(), "{args:?}");
    }
}

#[test]
fn exec_handles_unicode_and_spaced_source_paths() {
    let id = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("scarp-cli-identity-{}-{id}", std::process::id()));
    fs::create_dir(&dir).unwrap();
    let path = dir.join("नमस्ते file.srp");
    fs::write(&path, "print(\"Scarp\");").unwrap();
    let result = file_output(&["exec"], &path);
    let _ = fs::remove_dir_all(dir);
    assert!(result.status.success(), "{result:?}");
    assert_eq!(result.stdout, b"Scarp\n");
}

#[test]
fn verify_compiles_without_running_program_statements() {
    let id = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("scarp-static-{}-{id}", std::process::id()));
    fs::create_dir(&dir).unwrap();
    let source = dir.join("static only.srp");
    // The expression is type-correct but fails if it is actually evaluated.
    fs::write(&source, "print(1 / 0);").unwrap();

    let verified = file_output(&["verify"], &source);
    let evaluated = file_output(&["check"], &source);
    assert!(verified.status.success(), "{verified:?}");
    assert!(verified.stdout.is_empty());
    assert!(verified.stderr.is_empty());
    assert_eq!(evaluated.status.code(), Some(1), "{evaluated:?}");
    assert!(evaluated.stdout.is_empty());

    // Static errors still fail with a meaningful source diagnostic.
    fs::write(&source, "print(unknown_identifier);").unwrap();
    let invalid = file_output(&["verify"], &source);
    let _ = fs::remove_dir_all(dir);
    assert_eq!(invalid.status.code(), Some(1), "{invalid:?}");
    assert!(invalid.stdout.is_empty());
    assert!(!invalid.stderr.is_empty());
}

#[test]
fn scarp_first_source_commands_preserve_json_diagnostics() {
    let id = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("scarp-cli-json-{}-{id}", std::process::id()));
    fs::create_dir(&dir).unwrap();
    let path = dir.join("invalid input.srp");
    fs::write(&path, "print(unknown_symbol);").unwrap();

    for (new_args, legacy_args) in [
        (
            vec!["exec", "--diagnostic-format=json"],
            vec!["run", "--diagnostic-format=json"],
        ),
        (
            vec!["verify", "--diagnostic-format=json"],
            vec!["modules", "--diagnostic-format=json"],
        ),
        (
            vec!["inspect", "tokens", "--diagnostic-format=json"],
            vec!["tokens", "--diagnostic-format=json"],
        ),
        (
            vec!["inspect", "modules", "--diagnostic-format=json"],
            vec!["modules", "--diagnostic-format=json"],
        ),
    ] {
        let current = file_output(&new_args, &path);
        let previous = file_output(&legacy_args, &path);
        if new_args[1] == "tokens" {
            // Lexical analysis intentionally accepts an unresolved name.
            assert!(current.status.success(), "{current:?}");
        } else {
            assert_eq!(current.status.code(), Some(1), "{new_args:?}: {current:?}");
        }
        assert_eq!(current.stdout, previous.stdout, "{new_args:?}");
        assert_eq!(current.stderr, previous.stderr, "{new_args:?}");
    }
    let bad = file_output(
        &["inspect", "modules", "--diagnostic-format=json", "--json"],
        &path,
    );
    assert_eq!(bad.status.code(), Some(1), "{bad:?}");
    assert!(bad.stdout.is_empty());
    assert!(String::from_utf8_lossy(&bad.stderr).contains("schema_version"));
    let _ = fs::remove_dir_all(dir);
}
