use scarp::{
    diagnostics::DiagnosticKind,
    native_object::{
        emit_elf64_bounded_integer_entry_with_layout, emit_elf64_bounded_integer_function,
        emit_elf64_checked_integer_function, BOUNDED_INTEGER_ENTRY_MAX_STACK_BYTES,
    },
};

const SOURCE: &str = include_str!("../examples/native_aggregate_locals.srp");

#[test]
fn local_aggregates_use_the_existing_bounded_entry_and_stack_gate() {
    let emit = |stack| emit_elf64_bounded_integer_entry_with_layout(SOURCE, "entry", 200, stack);
    let artifact = emit(BOUNDED_INTEGER_ENTRY_MAX_STACK_BYTES).unwrap();
    assert_eq!(artifact.kernel.object.get(..4), Some(b"\x7fELF".as_slice()));
    assert_eq!(artifact.entry_layout.integer_arguments, 2);
    assert_eq!(artifact, emit(artifact.peak_stack_bytes).unwrap());
    assert_eq!(
        emit(artifact.peak_stack_bytes - 1).unwrap_err().kind,
        DiagnosticKind::NativeUnsupported
    );
    assert!(emit_elf64_bounded_integer_function(SOURCE, 200).is_ok());
    assert_eq!(
        emit_elf64_checked_integer_function(SOURCE)
            .unwrap_err()
            .kind,
        DiagnosticKind::NativeUnsupported
    );

    for initializer in ["[array: (1, 2)]", "(1, \"text\")", "[array: <integer>]"] {
        let source =
            format!("pure funcra entry() -> integer {{ bind local = {initializer}; return 7; }}");
        assert_eq!(
            emit_elf64_bounded_integer_function(&source, 200)
                .unwrap_err()
                .kind,
            DiagnosticKind::NativeUnsupported,
            "{initializer}"
        );
    }
    let elements = vec!["0"; 32].join(", ");
    let stores = (0..8)
        .map(|i| format!("bind lane{i} = [array: {elements}];"))
        .collect::<Vec<_>>()
        .join("\n");
    let source = format!("pure funcra entry() -> integer {{ {stores} return 0; }}");
    assert_eq!(
        emit_elf64_bounded_integer_function(&source, 200)
            .unwrap_err()
            .kind,
        DiagnosticKind::NativeUnsupported
    );
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[test]
fn linked_aggregates_agree_with_interpreter_and_keep_failure_order() {
    use scarp::{
        limits::{ExecutionLimits, ExecutionOptions},
        native_object::{
            emit_elf64_bounded_integer_entry, CHECKED_KERNEL_ARRAY_INDEX,
            CHECKED_KERNEL_INTEGER_OVERFLOW, CHECKED_KERNEL_WORK_LIMIT,
        },
        RunError,
    };
    use std::{fs, process::Command};

    let cc = std::env::var("SCARP_CC").unwrap_or_else(|_| "cc".into());
    if Command::new(&cc).arg("--version").output().is_err() {
        assert!(
            std::env::var_os("SCARP_REQUIRE_C").is_none(),
            "a C compiler is required"
        );
        return;
    }

    let directory =
        std::env::temp_dir().join(format!("scarp-native-aggregates-{}", std::process::id()));
    fs::create_dir_all(&directory).unwrap();
    let object = directory.join("aggregate.o");
    let harness = directory.join("harness.c");
    let executable = directory.join("run");
    fs::write(
        &harness,
        r#"
#include <inttypes.h>
#include <stdio.h>
#include <stdlib.h>
struct result { int64_t value; uint32_t status; uint32_t span; };
extern struct result scarp_bounded_integer_entry(int64_t, int64_t);
int main(int argc, char **argv) {
    if (argc != 3) return 1;
    struct result result = scarp_bounded_integer_entry(strtoll(argv[1], NULL, 10),
                                                        strtoll(argv[2], NULL, 10));
    printf("%" PRId64 " %" PRIu32 " %" PRIu32 "\n", result.value, result.status, result.span);
}
"#,
    )
    .unwrap();
    for max_work in [1, 16, 19, 30, 35, 45, 55, 65, 75, 200] {
        let artifact = emit_elf64_bounded_integer_entry(SOURCE, "entry", max_work).unwrap();
        fs::write(&object, &artifact.object).unwrap();
        let link = Command::new(&cc)
            .args(["-std=c11", "-Wall", "-Wextra", "-Werror"])
            .arg(&harness)
            .arg(&object)
            .arg("-o")
            .arg(&executable)
            .output()
            .unwrap();
        assert!(
            link.status.success(),
            "{}",
            String::from_utf8_lossy(&link.stderr)
        );

        for (index, input) in [
            (0, 7),
            (1, 7),
            (2, 7),
            (3, 7),
            (-1, 7),
            (i64::MAX, 7),
            (i64::MIN, 7),
            (-1, i64::MAX),
        ] {
            let out = Command::new(&executable)
                .args([index.to_string(), input.to_string()])
                .output()
                .unwrap();
            assert!(out.status.success());
            let line = String::from_utf8(out.stdout).unwrap();
            let mut fields = line.split_ascii_whitespace();
            let value = fields.next().unwrap().parse::<i64>().unwrap();
            let status = fields.next().unwrap().parse::<u32>().unwrap();
            let span_id = fields.next().unwrap().parse::<usize>().unwrap();
            let script = format!("{SOURCE}\nprint(entry({index}, {input}));");
            let options = |max_steps| ExecutionOptions {
                limits: ExecutionLimits {
                    max_steps,
                    ..ExecutionLimits::default()
                },
                ..ExecutionOptions::default()
            };
            // Remove interpreter work for the print wrapper, its argument
            // expressions, and frame entry. These are outside the native ABI.
            let wrapper_work = (1..32)
                .find(|&work| {
                    matches!(
                        scarp::run_with_options(&script, &mut Vec::new(), options(work)),
                        Err(RunError::Diagnostic(diag))
                            if diag.kind == DiagnosticKind::ExecutionLimit
                                && diag.span.end <= SOURCE.len()
                    )
                })
                .expect("interpreter reaches the first metered operation");
            let mut interpreted = Vec::new();
            match scarp::run_with_options(
                &script,
                &mut interpreted,
                options(wrapper_work + u64::from(max_work)),
            ) {
                Ok(()) => {
                    assert_eq!(status, 0);
                    assert_eq!(span_id, 0);
                    assert_eq!(format!("{value}\n").as_bytes(), interpreted);
                }
                Err(RunError::Diagnostic(diag)) => {
                    assert_eq!(value, 0);
                    let expected = match diag.kind {
                        DiagnosticKind::ExecutionLimit => CHECKED_KERNEL_WORK_LIMIT,
                        DiagnosticKind::ArrayIndex => CHECKED_KERNEL_ARRAY_INDEX,
                        DiagnosticKind::ArithmeticOverflow => CHECKED_KERNEL_INTEGER_OVERFLOW,
                        other => panic!("unexpected interpreter error {other:?}: {diag:?}"),
                    };
                    assert_eq!(status, expected, "{diag:?}");
                    assert_eq!(artifact.failure_spans[span_id], diag.span);
                }
                other => panic!("unexpected interpreter outcome {other:?}"),
            }
        }
    }
    fs::remove_dir_all(directory).unwrap();
}
