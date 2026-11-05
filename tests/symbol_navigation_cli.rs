use std::{
    fs,
    path::PathBuf,
    process::{Command, Output},
    sync::atomic::{AtomicUsize, Ordering},
};
static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!(
            "scarp-navigation-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&dir).unwrap();
        Self(dir)
    }
    fn file(&self, name: &str, source: &str) -> PathBuf {
        let p = self.0.join(name);
        fs::write(&p, source).unwrap();
        p
    }
    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_scarp"))
            .current_dir(&self.0)
            .args(args)
            .output()
            .unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
#[test]
fn static_commands_publish_definitions_and_references_without_running_user_code() {
    let f = Fixture::new();
    f.file("main.srp","import \"lib.srp\" as l; print(\"OUT\"); eprint(\"ERR\"); print(l.f()); while true {continue;}");
    f.file(
        "lib.srp",
        "pub pure funcra f(n: integer=1/0)->integer needs false;{return n;}",
    );
    for command in ["symbols", "references"] {
        let r = f.run(&["inspect", command, "--json", "main.srp"]);
        assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
        assert!(r.stderr.is_empty());
        let s = String::from_utf8(r.stdout).unwrap();
        assert!(s.starts_with("{\"schema_version\":1,\"kind\":\"scarp."));
        assert!(s.contains("\"kind\":\"parameter\""));
        assert!(!s.contains("OUT") && !s.contains("ERR") && !s.contains("1/0"));
        assert_eq!(s.contains("\"references\":["), command == "references");
        let text = f.run(&["inspect", command, "main.srp"]);
        assert!(text.status.success() && text.stderr.is_empty());
        assert!(text.stdout.starts_with(b"Scarp symbol index 1:"));
    }
}
#[test]
fn errors_have_existing_json_format_and_never_emit_partial_metadata() {
    let f = Fixture::new();
    f.file("bad.srp", "bind x: integer=false;");
    for command in ["symbols", "references"] {
        for flags in [
            vec!["inspect", command, "bad.srp"],
            vec![
                "inspect",
                command,
                "--diagnostic-format=json",
                "--json",
                "bad.srp",
            ],
        ] {
            let r = f.run(&flags);
            assert_eq!(r.status.code(), Some(1));
            assert!(r.stdout.is_empty());
            let s = String::from_utf8(r.stderr).unwrap();
            assert!(s.contains("E0015"));
            assert_eq!(
                s.starts_with('{'),
                flags.contains(&"--diagnostic-format=json")
            );
        }
    }
}
#[test]
fn usage_flags_help_and_top_level_aliases_do_not_trigger_file_reads() {
    let f = Fixture::new();
    for command in ["symbols", "references"] {
        for args in [
            vec!["inspect", command],
            vec!["inspect", command, "--json", "--json", "missing.srp"],
            vec!["inspect", command, "--max-steps", "0", "missing.srp"],
            vec!["inspect", command, "missing.srp", "--json"],
            vec![command, "missing.srp"],
        ] {
            let r = f.run(&args);
            assert_eq!(r.status.code(), Some(2), "{args:?}");
            assert!(r.stdout.is_empty());
            assert!(!String::from_utf8_lossy(&r.stderr).contains("could not read"));
        }
        for args in [
            vec!["inspect", command, "--help"],
            vec!["guide", "inspect", command],
        ] {
            let r = f.run(&args);
            assert!(r.status.success() && r.stderr.is_empty());
            assert!(String::from_utf8(r.stdout)
                .unwrap()
                .contains(&format!("scarp inspect {command}")));
        }
    }
}
#[test]
fn explicit_project_directories_use_the_same_trusted_manifest_and_entry_rules() {
    let f = Fixture::new();
    fs::create_dir(f.0.join("src")).unwrap();
    f.file(
        "Scarp.project",
        "scarp-project: 1\nname: Navigation\nentry: src/main.srp\n",
    );
    f.file("src/main.srp", "print(1/0);");
    for command in ["symbols", "references"] {
        let r = f.run(&["inspect", command, "--json", "."]);
        assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
        assert!(r.stderr.is_empty());
    }
    f.file(
        "Scarp.project",
        "scarp-project: 1\nname: Navigation\nentry: ../outside.srp\n",
    );
    let r = f.run(&[
        "inspect",
        "symbols",
        "--json",
        "--diagnostic-format=json",
        ".",
    ]);
    assert_eq!(r.status.code(), Some(1));
    assert!(r.stdout.is_empty());
    assert!(r.stderr.starts_with(b"{\"schema_version\":1,"));
}
#[test]
fn unicode_spaced_and_dash_paths_preserve_filename_semantics() {
    let f = Fixture::new();
    f.file("-café file.srp", "// 🌍\r\nbind x=1; print(x);");
    let r = f.run(&["inspect", "references", "--json", "--", "-café file.srp"]);
    assert!(r.status.success());
    assert!(r.stderr.is_empty());
    let s = String::from_utf8(r.stdout).unwrap();
    assert!(s.contains("-café file.srp"));
}
