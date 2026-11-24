//! Native function code-density and execution regressions.
use scarp::native_object::emit_elf64_checked_integer_function;

fn text_bytes(object: &[u8]) -> &[u8] {
    let read = |at: usize| u64::from_le_bytes(object[at..at + 8].try_into().unwrap()) as usize;
    let header = read(40) + 64;
    let start = read(header + 24);
    let len = read(header + 32);
    &object[start..start + len]
}

#[test]
fn compact_literal_function_reduces_machine_code_without_changing_abi() {
    let source = "pure funcra f() -> integer { return 42; }";
    let artifact = emit_elf64_checked_integer_function(source).unwrap();
    let machine = text_bytes(&artifact.object);
    // The previous encoder used 48 bytes for this exact function:
    // 7 prologue + 10 immediate + 8 store + 8 load + 5 jump +
    // 2 status clear + 8 epilogue. The actual compact encoding saves 11 bytes.
    assert_eq!(machine.len(), 37);
    assert_eq!(machine.last(), Some(&0xc3));
    assert!(artifact.failure_spans.is_empty());
    assert_eq!(
        artifact,
        emit_elf64_checked_integer_function(source).unwrap()
    );
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[test]
fn compact_native_literals_match_interpreter_across_immediate_boundaries() {
    use std::{fs, process::Command};

    let cc = std::env::var_os("SCARP_CC").unwrap_or_else(|| "cc".into());
    if Command::new(&cc).arg("--version").output().is_err() {
        assert!(
            std::env::var_os("SCARP_REQUIRE_C").is_none(),
            "C compiler required"
        );
        return;
    }
    let directory =
        std::env::temp_dir().join(format!("scarp-compact-native-{}", std::process::id()));
    fs::create_dir(&directory).unwrap();
    let harness = directory.join("harness.c");
    let object = directory.join("function.o");
    let binary = directory.join("harness");
    fs::write(
        &harness,
        r#"#include <stdint.h>
#include <inttypes.h>
#include <stdio.h>
struct result { int64_t value; uint32_t status; uint32_t span_id; };
extern struct result scarp_checked_integer_function(void);
int main(void) {
    struct result r = scarp_checked_integer_function();
    printf("%" PRId64 "\n", r.value);
    return r.status != 0 || r.span_id != 0;
}
"#,
    )
    .unwrap();
    for literal in [
        "0",
        "2147483647",
        "2147483648",
        "4294967295",
        "4294967296",
        "-1",
        "-2147483648",
        "-2147483649",
        "-9223372036854775808",
        "9223372036854775807",
    ] {
        let source = format!("pure funcra f() -> integer {{ return {literal}; }}");
        let artifact = emit_elf64_checked_integer_function(&source).unwrap();
        fs::write(&object, &artifact.object).unwrap();
        let link = Command::new(&cc)
            .args(["-std=c11", "-O2", "-Wall", "-Wextra", "-Werror"])
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
        let output = Command::new(&binary).output().unwrap();
        assert!(output.status.success(), "{literal}");
        let mut expected = Vec::new();
        scarp::run(&format!("{source} print(f());"), &mut expected).unwrap();
        assert_eq!(output.stdout, expected, "{literal}");
    }
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn long_native_expression_uses_a_small_bounded_stack_frame() {
    let source = format!(
        "pure funcra f(x: integer) -> integer {{ return x{}; }}",
        " + 1".repeat(200)
    );
    let artifact = emit_elf64_checked_integer_function(&source).unwrap();
    let machine = text_bytes(&artifact.object);
    assert_eq!(&machine[..3], &[0x48, 0x81, 0xec]);
    let frame = u32::from_le_bytes(machine[3..7].try_into().unwrap());
    // One parameter and three simultaneous expression temporaries, plus
    // SysV stack alignment. The original layout required over 3 KiB.
    assert_eq!(frame, 40);
    assert_eq!(
        artifact,
        emit_elf64_checked_integer_function(&source).unwrap()
    );
}

fn branch_chain() -> String {
    format!(
        "pure funcra f(x: integer) -> integer {{
            if x == 0{} {{ return 17; }}
            return 29;
        }}",
        " or x == 1".repeat(200)
    )
}

#[test]
fn long_short_circuit_expression_uses_a_small_bounded_stack_frame() {
    let source = branch_chain();
    let artifact = emit_elf64_checked_integer_function(&source).unwrap();
    let machine = text_bytes(&artifact.object);
    assert_eq!(&machine[..3], &[0x48, 0x81, 0xec]);
    assert_eq!(u32::from_le_bytes(machine[3..7].try_into().unwrap()), 40);
    assert_eq!(
        artifact,
        emit_elf64_checked_integer_function(&source).unwrap()
    );
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[test]
fn reused_temporary_slots_preserve_results_and_first_failure_spans() {
    use scarp::{
        diagnostics::DiagnosticKind,
        native_object::{CHECKED_KERNEL_DIVISION_BY_ZERO, CHECKED_KERNEL_INTEGER_OVERFLOW},
        RunError,
    };
    use std::{fs, process::Command};

    let cc = std::env::var_os("SCARP_CC").unwrap_or_else(|| "cc".into());
    if Command::new(&cc).arg("--version").output().is_err() {
        assert!(std::env::var_os("SCARP_REQUIRE_C").is_none());
        return;
    }
    let directory = std::env::temp_dir().join(format!(
        "scarp-native-temporary-reuse-{}",
        std::process::id()
    ));
    fs::create_dir(&directory).unwrap();
    let object = directory.join("function.o");
    let harness = directory.join("harness.c");
    let binary = directory.join("harness");
    let arithmetic = format!(
        "pure funcra f(x: integer) -> integer {{ return x{}; }}",
        " + 1".repeat(200)
    );
    let mixed = "pure funcra f(x: integer) -> integer {
        // संयुक्त
        if (x > 0) == ((x < 0 and 7 / 0 == 0) or (x == 0 or 30 / x > 5)) {
            return x + 10;
        }
        return 99;
    }";
    fs::write(
        &harness,
        r#"#include <stdint.h>
#include <inttypes.h>
#include <stdio.h>
#include <unistd.h>
struct result { int64_t value; uint32_t status; uint32_t span_id; };
extern struct result scarp_checked_integer_function(int64_t);
int main(void) {
    const int64_t inputs[] = {0, 1, -200, 3, 72, INT64_MIN, INT64_MAX - 200, INT64_MAX};
    alarm(5);
    for (unsigned i = 0; i < sizeof(inputs) / sizeof(inputs[0]); ++i) {
        struct result r = scarp_checked_integer_function(inputs[i]);
        printf("%" PRId64 " %" PRIu32 " %" PRIu32 "\n", r.value, r.status, r.span_id);
    }
    return 0;
}
"#,
    )
    .unwrap();
    for source in [arithmetic.as_str(), &branch_chain(), mixed] {
        let artifact = emit_elf64_checked_integer_function(source).unwrap();
        fs::write(&object, &artifact.object).unwrap();
        let link = Command::new(&cc)
            .args(["-std=c11", "-O2", "-Wall", "-Wextra", "-Werror"])
            .arg(&harness)
            .arg(&object)
            .arg("-o")
            .arg(&binary)
            .output()
            .unwrap();
        assert_eq!(
            link.status.code(),
            Some(0),
            "{}",
            String::from_utf8_lossy(&link.stderr)
        );
        let output = Command::new(&binary).output().unwrap();
        assert!(output.status.success());
        let stdout = std::str::from_utf8(&output.stdout).unwrap();
        let rows: Vec<_> = stdout.lines().collect();
        let inputs = [0i64, 1, -200, 3, 72, i64::MIN, i64::MAX - 200, i64::MAX];
        assert_eq!(rows.len(), inputs.len());
        for (row, input) in rows.iter().zip(inputs) {
            let mut expected = Vec::new();
            match scarp::run(&format!("{source} print(f({input}));"), &mut expected) {
                Ok(()) => assert_eq!(
                    *row,
                    format!("{} 0 0", String::from_utf8(expected).unwrap().trim())
                ),
                Err(RunError::Diagnostic(error)) => {
                    let expected_status = match error.kind {
                        DiagnosticKind::ArithmeticOverflow => CHECKED_KERNEL_INTEGER_OVERFLOW,
                        DiagnosticKind::DivisionByZero => CHECKED_KERNEL_DIVISION_BY_ZERO,
                        _ => panic!("unexpected interpreter diagnostic: {error:?}"),
                    };
                    let fields = row.split_whitespace().collect::<Vec<_>>();
                    assert_eq!(fields.len(), 3);
                    assert_eq!(fields[0], "0");
                    assert_eq!(fields[1].parse::<u32>().unwrap(), expected_status);
                    let span_id = fields[2].parse::<usize>().unwrap();
                    assert_eq!(artifact.failure_spans[span_id], error.span);
                    assert!(expected.is_empty());
                }
                Err(error) => panic!("{error:?}"),
            }
        }
    }
    fs::remove_dir_all(directory).unwrap();
}
