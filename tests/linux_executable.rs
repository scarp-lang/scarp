use scarp::diagnostics::DiagnosticKind;
use scarp::native_object::{emit_elf64_linux_executable, write_elf64_linux_executable_new};

const SOURCE: &str = "
    pure funcra compute(x: integer) -> integer {
        var count = 0;
        repeat x as i { count += i; }
        return count;
    }
    pure funcra main() -> integer { return compute(10); }
";

#[test]
fn standalone_elf_has_a_deterministic_executable_envelope() {
    let first = emit_elf64_linux_executable(SOURCE, 1000, 4096).unwrap();
    let second = emit_elf64_linux_executable(SOURCE, 1000, 4096).unwrap();
    assert_eq!(first, second);
    assert_eq!(&first.image[..7], b"\x7fELF\x02\x01\x01");
    assert_eq!(&first.image[16..20], &[2, 0, 62, 0]);
    assert_eq!(&first.image[64..72], &[1, 0, 0, 0, 5, 0, 0, 0]);
    assert_eq!(
        &first.image[120..128],
        &[0x51, 0xe5, 0x74, 0x64, 6, 0, 0, 0]
    );
    assert_eq!(first.max_active_frames, 2);
    assert!(first.peak_stack_bytes < 4096);
    let exact = emit_elf64_linux_executable(SOURCE, 1000, first.peak_stack_bytes).unwrap();
    assert_eq!(first, exact);
    assert_eq!(
        emit_elf64_linux_executable(SOURCE, 1000, first.peak_stack_bytes - 1)
            .unwrap_err()
            .kind,
        DiagnosticKind::NativeUnsupported,
    );
}

#[test]
fn executable_rejects_unsupported_programs_before_writing() {
    for source in [
        "pure funcra other() -> integer { return 0; }",
        "pure funcra main(x: integer) -> integer { return x; }",
        "pure funcra main() -> float { return 1.0; }",
        "pure funcra main() -> integer { return main(); }",
        "pure funcra main() -> integer { bind x: tensor<float, 1> = [4.0]; return 0; }",
        "pure funcra main() -> integer { return 0; } print(main());",
    ] {
        let error = emit_elf64_linux_executable(source, 1000, 4096).unwrap_err();
        assert_eq!(error.kind, DiagnosticKind::NativeUnsupported, "{source}");
    }
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[test]
fn linux_process_results_match_interpreter_and_checked_failures() {
    use std::{fs, process::Command};
    let directory = std::env::temp_dir().join(format!("scarp-linux-exe-{}", std::process::id()));
    fs::create_dir(&directory).unwrap();
    struct Cleanup(std::path::PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    let _cleanup = Cleanup(directory.clone());
    let program = directory.join("program.srp");
    fs::write(&program, SOURCE).unwrap();
    let binary = directory.join("program");
    let result = Command::new(env!("CARGO_BIN_EXE_scarp"))
        .args(["native-exe", "linux-x86_64", "1000", "4096"])
        .arg(&program)
        .arg("-o")
        .arg(&binary)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{:?}",
        String::from_utf8_lossy(&result.stderr)
    );

    let mut interpreter = Vec::new();
    scarp::run(&format!("{SOURCE}\nprint(main());"), &mut interpreter).unwrap();
    let expected = String::from_utf8(interpreter)
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    let actual = Command::new(&binary).output().unwrap();
    assert_eq!(actual.status.code(), Some(expected));
    assert!(actual.stdout.is_empty() && actual.stderr.is_empty());

    for (i, (expression, work, code)) in [
        ("0", 100, 0),
        ("125", 100, 125),
        ("126", 100, 126),
        ("-1", 100, 126),
        ("9223372036854775807 + 1", 100, 129),
        ("1 / 0", 100, 130),
        ("1 << 64", 100, 131),
        ("1", 0, 132),
    ]
    .into_iter()
    .enumerate()
    {
        let source = format!("pure funcra main() -> integer {{ return {expression}; }}");
        let executable = emit_elf64_linux_executable(&source, work, 4096).unwrap();
        let path = directory.join(format!("case-{i}"));
        write_elf64_linux_executable_new(&path, &executable.image).unwrap();
        assert_eq!(
            Command::new(&path).status().unwrap().code(),
            Some(code),
            "{expression}"
        );
    }
    let bytes = fs::read(&binary).unwrap();
    assert_eq!(
        write_elf64_linux_executable_new(&binary, &bytes)
            .unwrap_err()
            .kind(),
        std::io::ErrorKind::AlreadyExists,
    );
    assert_eq!(fs::read(&binary).unwrap(), bytes);
}
