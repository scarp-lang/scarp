use scarp::native_object::{
    emit_freestanding_function, FreestandingFunctionMode, FreestandingTarget,
};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

struct Directory(PathBuf);
impl Directory {
    fn new(name: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("scarp-object-cli-{name}-{}", std::process::id()));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_scarp"))
            .current_dir(&self.0)
            .arg("native-object")
            .args(args)
            .output()
            .unwrap()
    }
    fn path(&self, path: &str) -> PathBuf {
        self.0.join(Path::new(path))
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn cli_emits_exact_api_bytes_and_preserves_existing_destinations() {
    let d = Directory::new("bytes");
    for (args, mode, src) in [
        (
            vec!["x86_64-none", "checked"],
            FreestandingFunctionMode::Checked,
            "pure funcra f(a:integer)->integer{return a+1;}",
        ),
        (
            vec!["x86_64-none", "bounded", "0"],
            FreestandingFunctionMode::Bounded { max_work: 0 },
            "pure funcra f(a:integer)->integer{var n=a; while n>0 {n-=1;} return n;}",
        ),
        (
            vec!["x86_64-none", "bounded", "4294967295"],
            FreestandingFunctionMode::Bounded { max_work: u32::MAX },
            "pure funcra f()->integer{return 42;}",
        ),
    ] {
        fs::write(d.path("-é.srp"), src).unwrap();
        let mut args = args;
        args.extend(["-é.srp", "out file.o"]);
        let result = d.run(&args);
        assert!(result.status.success(), "{:?}", result.stderr);
        assert!(result.stdout.is_empty() && result.stderr.is_empty());
        let expected =
            emit_freestanding_function(src, FreestandingTarget::X86_64None, mode).unwrap();
        assert_eq!(
            fs::read(d.path("out file.o")).unwrap(),
            expected.kernel.object
        );
        fs::write(d.path("out file.o"), b"keep").unwrap();
        let conflict = d.run(&args);
        assert_eq!(conflict.status.code(), Some(1));
        assert!(conflict.stdout.is_empty());
        assert_eq!(fs::read(d.path("out file.o")).unwrap(), b"keep");
        fs::remove_file(d.path("out file.o")).unwrap();
    }
}

#[test]
fn bad_target_mode_and_budget_fail_before_loading_source() {
    let d = Directory::new("usage");
    for args in [
        vec!["x86_64", "checked", "missing.srp", "out.o"],
        vec!["x86_64-none", "automatic", "missing.srp", "out.o"],
        vec![
            "x86_64-none",
            "bounded",
            "4294967296",
            "missing.srp",
            "out.o",
        ],
        vec!["x86_64-none", "bounded", "-1", "missing.srp", "out.o"],
        vec!["x86_64-none", "bounded", "+1", "missing.srp", "out.o"],
        vec!["x86_64-none", "bounded", "missing.srp", "out.o"],
    ] {
        let r = d.run(&args);
        assert_eq!(r.status.code(), Some(2));
        assert!(!r.stderr.is_empty() && r.stdout.is_empty());
        assert_eq!(fs::read_dir(&d.0).unwrap().count(), 0);
    }
    let help = d.run(&["--help"]);
    let guide = Command::new(env!("CARGO_BIN_EXE_scarp"))
        .args(["guide", "native-object"])
        .output()
        .unwrap();
    assert!(guide.status.success() && help.status.success());
    assert_eq!(guide.stdout, help.stdout);
}

#[test]
fn source_errors_publish_nothing_and_do_not_execute_top_level_effects() {
    use scarp::{diagnostics::DiagnosticKind, modules::ModuleLimits};
    let d = Directory::new("source");
    let args = ["x86_64-none", "checked", "source.srp", "out.o"];
    for bytes in [
        b"// UTF-8\r\nprint(42);".to_vec(),
        vec![0xff],
        vec![b' '; ModuleLimits::default().max_file_bytes + 1],
    ] {
        fs::write(d.path("source.srp"), &bytes).unwrap();
        let result = d.run(&args);
        assert_eq!(result.status.code(), Some(1));
        assert!(result.stdout.is_empty() && !result.stderr.is_empty());
        if bytes.starts_with(b"//") {
            assert!(String::from_utf8(result.stderr)
                .unwrap()
                .contains(DiagnosticKind::NativeUnsupported.code()));
        }
        assert!(!d.path("out.o").exists());
    }
    fs::write(d.path("source.srp"), "pure funcra f()->integer{return 42;}").unwrap();
    let result = d.run(&["x86_64-none", "checked", "source.srp", "missing/out.o"]);
    assert_eq!(result.status.code(), Some(1));
    assert!(!d.path("missing").exists());
}
