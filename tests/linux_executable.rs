use scarp::diagnostics::DiagnosticKind;
use scarp::native_object::emit_elf64_linux_executable;

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
        "pure funcra main() -> integer { bind x = to_u128(1); return to_integer(x); }",
        "pure funcra main() -> integer { return to_integer(to_u8(bigint(\"1\"))); }",
        "pure funcra main() -> integer { return 0; } print(main());",
    ] {
        let error = emit_elf64_linux_executable(source, 1000, 4096).unwrap_err();
        assert_eq!(error.kind, DiagnosticKind::NativeUnsupported, "{source}");
    }
}

#[test]
fn bounded_native_functions_compile_width_qualified_aggregate_lanes() {
    let source = "
        pure funcra compute(input: integer, by: integer) -> integer {
            var result = to_u8(input);
            result >>= by;
            var values = [array: result, to_u8(19)];
            bind snapshot = values;
            values = [array: to_u8(1), values[1]];
            return to_integer(snapshot[0]) + to_integer(values[0]);
        }
        pure funcra main() -> integer { return compute(130, 1); }
    ";
    let image = emit_elf64_linux_executable(source, 1000, 4096).unwrap();
    assert_eq!(image.max_active_frames, 2);
    assert_eq!(
        image,
        emit_elf64_linux_executable(source, 1000, 4096).unwrap()
    );
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[test]
fn native_fixed_width_signedness_and_failures_match_the_interpreter() {
    use scarp::native_object::write_elf64_linux_executable_new;
    use std::{fs, path::PathBuf, process::Command};

    struct Cleanup(PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    let directory =
        Cleanup(std::env::temp_dir().join(format!("scarp-fixed-native-{}", std::process::id())));
    fs::create_dir(&directory.0).unwrap();
    let cases = [
        ("to_integer((to_i8(a) >> b) + to_i8(4))", -8, 2, None),
        ("to_integer(to_i8(a) >>> b)", -128, 1, None),
        ("to_integer(to_i8(a) << b) + 100", -33, 1, None),
        ("to_integer(~to_u8(a) & to_u8(b))", 0, 63, None),
        (
            "to_integer((~to_u64(a) / to_u64(b)) % to_u64(100))",
            0,
            5,
            None,
        ),
        ("if ~to_u64(a) > to_u64(b) { 3 } else { 9 }", 0, 1, None),
        ("if to_i64(a) < to_i64(b) { 5 } else { 8 }", -2, 1, None),
        ("to_integer(to_i16(a) * to_i16(b))", 8, 9, None),
        ("to_integer(to_isize(a + b))", 12, 30, None),
        ("to_integer(to_usize(a) / to_usize(b))", 48, 4, None),
        ("to_integer(-to_u16(a))", 0, 1, None),
        (
            "to_integer(to_i8(a) + to_i8(b))",
            127,
            1,
            Some((DiagnosticKind::FixedIntegerOverflow, 129)),
        ),
        (
            "to_integer(~to_u64(a) + to_u64(b))",
            0,
            1,
            Some((DiagnosticKind::FixedIntegerOverflow, 129)),
        ),
        (
            "to_integer(~to_u64(a) * to_u64(b))",
            0,
            2,
            Some((DiagnosticKind::FixedIntegerOverflow, 129)),
        ),
        (
            "to_integer(to_u32(a) - to_u32(b))",
            0,
            1,
            Some((DiagnosticKind::FixedIntegerOverflow, 129)),
        ),
        (
            "to_integer(to_i8(a) % to_i8(b))",
            -128,
            -1,
            Some((DiagnosticKind::FixedIntegerOverflow, 129)),
        ),
        (
            "to_integer(-to_i16(a))",
            -32768,
            0,
            Some((DiagnosticKind::FixedIntegerOverflow, 129)),
        ),
        (
            "to_integer(-to_u16(a))",
            1,
            0,
            Some((DiagnosticKind::FixedIntegerOverflow, 129)),
        ),
        (
            "to_integer(to_u32(a) / to_u32(b))",
            1,
            0,
            Some((DiagnosticKind::DivisionByZero, 130)),
        ),
        (
            "to_integer(to_u8(a) << b)",
            3,
            8,
            Some((DiagnosticKind::InvalidShift, 131)),
        ),
        (
            "to_integer(to_i16(a) >> b)",
            3,
            -1,
            Some((DiagnosticKind::InvalidShift, 131)),
        ),
        (
            "to_integer(to_u8(a))",
            256,
            0,
            Some((DiagnosticKind::NumericConversion, 142)),
        ),
        (
            "to_integer(to_i8(a))",
            -129,
            0,
            Some((DiagnosticKind::NumericConversion, 142)),
        ),
        (
            "to_integer(to_u64(a))",
            -1,
            0,
            Some((DiagnosticKind::NumericConversion, 142)),
        ),
        (
            "to_integer(~to_u64(a))",
            0,
            0,
            Some((DiagnosticKind::NumericConversion, 142)),
        ),
    ];
    for (index, (expression, a, b, failure)) in cases.into_iter().enumerate() {
        let source = format!(
            "pure funcra vm(a: integer, b: integer) -> integer {{ return {expression}; }}
             pure funcra main() -> integer {{ return vm({a}, {b}); }}"
        );
        let interpreted = format!("{source}\nprint(main());");
        let expected = if let Some((kind, status)) = failure {
            assert_eq!(
                scarp::check(&interpreted).unwrap_err().kind,
                kind,
                "{expression}"
            );
            status
        } else {
            let mut output = Vec::new();
            scarp::run(&interpreted, &mut output).unwrap();
            String::from_utf8(output).unwrap().trim().parse().unwrap()
        };
        let image = emit_elf64_linux_executable(&source, 1000, 4096).unwrap();
        let path = directory.0.join(format!("case-{index}"));
        write_elf64_linux_executable_new(&path, &image.image).unwrap();
        let ran = Command::new(&path).output().unwrap();
        assert_eq!(ran.status.code(), Some(expected), "{expression}");
        assert!(ran.stdout.is_empty() && ran.stderr.is_empty());
    }
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[test]
fn linux_process_results_match_interpreter_and_checked_failures() {
    use scarp::native_object::write_elf64_linux_executable_new;
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
