use scarp::{
    diagnostics::DiagnosticKind,
    native_object::{emit_elf64_bounded_integer_function, emit_elf64_checked_integer_function},
};

const EXAMPLE: &str = include_str!("../examples/native_conditional_function.srp");

fn function_source() -> &'static str {
    EXAMPLE.split_once("\nprint(").unwrap().0
}

#[test]
fn scalar_conditional_functions_emit_deterministically() {
    let source = function_source();
    assert_eq!(
        emit_elf64_checked_integer_function(source).unwrap(),
        emit_elf64_checked_integer_function(source).unwrap()
    );
    assert_eq!(
        emit_elf64_bounded_integer_function(source, 100).unwrap(),
        emit_elf64_bounded_integer_function(source, 100).unwrap()
    );

    let mut output = Vec::new();
    scarp::run(EXAMPLE, &mut output).unwrap();
    assert_eq!(
        output,
        include_bytes!("../examples/native_conditional_function.stdout")
    );
}

#[test]
fn conditional_aggregate_projections_require_the_bounded_function_api() {
    let source = "pure funcra f() -> integer {
        return if true { 1 } else { (2, 3)[0] };
    }";
    scarp::compile(source).unwrap();
    assert_eq!(
        emit_elf64_checked_integer_function(source)
            .unwrap_err()
            .kind,
        DiagnosticKind::NativeUnsupported
    );
    assert!(emit_elf64_bounded_integer_function(source, 100).is_ok());

    let kernel = "bind number = if true { 1 } else { 2 };";
    for error in [
        scarp::c_backend::emit_c(kernel).unwrap_err(),
        scarp::native_object::emit_elf64_integer_kernel(kernel).unwrap_err(),
        scarp::native_object::emit_elf64_checked_integer_kernel(kernel).unwrap_err(),
    ] {
        assert_eq!(error.kind, DiagnosticKind::NativeUnsupported);
    }
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[test]
fn native_conditional_paths_preserve_results_spans_and_work() {
    use scarp::{
        diagnostics::Span,
        native_object::{
            CheckedElf64Kernel, CHECKED_KERNEL_DIVISION_BY_ZERO, CHECKED_KERNEL_INTEGER_OVERFLOW,
            CHECKED_KERNEL_WORK_LIMIT,
        },
        RunError,
    };
    use std::{fs, path::PathBuf, process::Command};

    let cc = std::env::var_os("SCARP_CC").unwrap_or_else(|| "cc".into());
    if Command::new(&cc).arg("--version").output().is_err() {
        assert!(
            std::env::var_os("SCARP_REQUIRE_C").is_none(),
            "C compiler required"
        );
        return;
    }
    struct TempDirectory(PathBuf);
    impl Drop for TempDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    let directory = TempDirectory(
        std::env::temp_dir().join(format!("scarp-conditional-native-{}", std::process::id())),
    );
    fs::create_dir(&directory.0).unwrap();
    let object = directory.0.join("function.o");
    let harness = directory.0.join("harness.c");
    let binary = directory.0.join("harness");
    fs::write(
        &harness,
        r#"#include <stdint.h>
#include <inttypes.h>
#include <stdio.h>
#include <stdlib.h>
#include <unistd.h>
struct result { int64_t value; uint32_t status; uint32_t span_id; };
extern struct result ENTRY(int64_t);
int main(int argc, char **argv) {
    alarm(5);
    for (int i = 1; i < argc; ++i) {
        struct result r = ENTRY(strtoll(argv[i], NULL, 10));
        printf("%" PRId64 " %" PRIu32 " %" PRIu32 "\n", r.value, r.status, r.span_id);
    }
    return 0;
}
"#,
    )
    .unwrap();
    let execute = |artifact: &CheckedElf64Kernel, bounded, arguments: &[i64]| {
        let entry = if bounded {
            "-DENTRY=scarp_bounded_integer_function"
        } else {
            "-DENTRY=scarp_checked_integer_function"
        };
        fs::write(&object, &artifact.object).unwrap();
        let link = Command::new(&cc)
            .args(["-std=c11", "-O2", "-Wall", "-Wextra", "-Werror", entry])
            .arg(&harness)
            .arg(&object)
            .arg("-o")
            .arg(&binary)
            .output()
            .unwrap();
        assert!(
            link.status.success(),
            "{}",
            String::from_utf8_lossy(&link.stderr)
        );
        let output = Command::new(&binary)
            .args(arguments.iter().map(i64::to_string))
            .output()
            .unwrap();
        assert!(output.status.success(), "{:?}", output.status);
        let stdout = std::str::from_utf8(&output.stdout).unwrap();
        let rows: Vec<_> = stdout
            .lines()
            .map(|line| {
                let fields = line.split_whitespace().collect::<Vec<_>>();
                assert_eq!(fields.len(), 3);
                (
                    fields[0].parse::<i64>().unwrap(),
                    fields[1].parse::<u32>().unwrap(),
                    fields[2].parse::<usize>().unwrap(),
                )
            })
            .collect();
        assert_eq!(rows.len(), arguments.len());
        rows
    };

    let source = function_source();
    let mut cases = vec![i64::MIN, -2, -1, 0, 1, 3, 4, 5, 63, i64::MAX];
    let mut seed = 0x48d0_ae55_u64;
    for _ in 0..32 {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        cases.push(((seed >> 32) % 41) as i64 - 20);
    }
    for (bounded, artifact) in [
        (false, emit_elf64_checked_integer_function(source).unwrap()),
        (
            true,
            emit_elf64_bounded_integer_function(source, 100).unwrap(),
        ),
    ] {
        for ((value, status, id), input) in
            execute(&artifact, bounded, &cases).into_iter().zip(&cases)
        {
            let mut expected = Vec::new();
            match scarp::run(&format!("{source} print(f({input}));"), &mut expected) {
                Ok(()) => {
                    assert_eq!(format!("{value}\n").as_bytes(), expected);
                    assert_eq!((status, id), (0, 0));
                }
                Err(RunError::Diagnostic(error)) => {
                    let code = match error.kind {
                        DiagnosticKind::ArithmeticOverflow => CHECKED_KERNEL_INTEGER_OVERFLOW,
                        DiagnosticKind::DivisionByZero => CHECKED_KERNEL_DIVISION_BY_ZERO,
                        _ => panic!("{error:?}"),
                    };
                    assert_eq!(
                        (value, status, artifact.failure_spans[id]),
                        (0, code, error.span)
                    );
                    assert!(expected.is_empty());
                }
                Err(error) => panic!("{error:?}"),
            }
        }
    }

    let source = "// चयन\r\npure funcra f(x: integer) -> integer {
        return if x == 0 { 6 * 7 } else { 1 / 0 };
    }";
    let at = |text: &str| {
        let start = source.find(text).unwrap();
        Span::new(start, start + text.len())
    };
    let slash = at(" / ");
    let slash = Span::new(slash.start + 1, slash.end - 1);
    for (allowance, input, value, status, span) in [
        (6, 0, 0, CHECKED_KERNEL_WORK_LIMIT, Some(at("x == 0"))),
        (7, 0, 0, CHECKED_KERNEL_WORK_LIMIT, Some(at("6 * 7"))),
        (8, 0, 0, CHECKED_KERNEL_WORK_LIMIT, Some(at("6"))),
        (9, 0, 0, CHECKED_KERNEL_WORK_LIMIT, Some(at("7"))),
        (10, 0, 0, CHECKED_KERNEL_WORK_LIMIT, Some(at("*"))),
        (11, 0, 42, 0, None),
        (7, 1, 0, CHECKED_KERNEL_WORK_LIMIT, Some(at("1 / 0"))),
        (10, 1, 0, CHECKED_KERNEL_WORK_LIMIT, Some(slash)),
        (11, 1, 0, CHECKED_KERNEL_DIVISION_BY_ZERO, Some(slash)),
    ] {
        let artifact = emit_elf64_bounded_integer_function(source, allowance).unwrap();
        let actual = execute(&artifact, true, &[input])[0];
        assert_eq!(
            (actual.0, actual.1),
            (value, status),
            "allowance {allowance}"
        );
        match span {
            Some(span) => assert_eq!(artifact.failure_spans[actual.2], span),
            None => assert_eq!(actual.2, 0),
        }
    }
}
