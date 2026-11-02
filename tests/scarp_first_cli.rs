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
    for args in [
        vec![],
        vec!["guide"],
        vec!["--help"],
        vec!["-h"],
    ] {
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
        (vec!["inspect", "modules", "--help"], "scarp inspect modules"),
        (vec!["exec", "--help"], "Usage: scarp exec"),
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
}

#[test]
fn inspect_subcommands_preserve_backends_and_json() {
    let path = example("module_inspection.srp");
    for (canonical, legacy) in [
        (vec!["inspect", "tokens"], vec!["tokens"]),
        (vec!["inspect", "modules"], vec!["modules"]),
        (vec!["inspect", "modules", "--json"], vec!["modules", "--json"]),
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
        vec!["verify", "hello.srp"],
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
    let dir = std::env::temp_dir().join(format!(
        "scarp-cli-identity-{}-{id}",
        std::process::id()
    ));
    fs::create_dir(&dir).unwrap();
    let path = dir.join("नमस्ते file.srp");
    fs::write(&path, "print(\"Scarp\");").unwrap();
    let result = file_output(&["exec"], &path);
    let _ = fs::remove_dir_all(dir);
    assert!(result.status.success(), "{result:?}");
    assert_eq!(result.stdout, b"Scarp\n");
}
