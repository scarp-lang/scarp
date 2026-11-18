use scarp::{
    diagnostics::{DiagnosticKind, Span},
    native_object::{emit_elf64_checked_integer_kernel, emit_elf64_integer_kernel},
    RunError,
};

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
use scarp::native_object::CHECKED_KERNEL_INTEGER_OVERFLOW;

// The expected statuses and byte spans below come from interpreter execution.
const CASES: &[&str] = &[
    "bind result = 0;",
    "bind left = 12; bind right = 2; bind result = ((left - right) * 4) + right;",
    "bind left = -9223372036854775808; bind result = left + 0;",
    "bind left = 9223372036854775807; bind result = left - 0;",
    "bind left = -9223372036854775808; bind result = left * 1;",
    "bind left = -9223372036854775807; bind result = -left;",
    "bind left = -7; bind result = (~left & 15) + (left * 2);",
    "// नमस्ते 🌍\r\nbind left = 9223372036854775807; bind result = (left | 0) + 1;",
    "bind left = -9223372036854775808; bind result = left - 1;",
    "bind left = 9223372036854775807; bind result = left * 2;",
    "bind left = -9223372036854775808; bind result = -left;",
    "bind left = -9223372036854775808; bind result = left * -1;",
    "bind max = 9223372036854775807; bind result = (1 + 1) + (max + 1) + (max * 2);",
];

fn interpreter_result(source: &str) -> Result<i64, Span> {
    let mut output = Vec::new();
    match scarp::run(&format!("{source} print(result);"), &mut output) {
        Ok(()) => Ok(std::str::from_utf8(&output)
            .unwrap()
            .trim()
            .parse()
            .unwrap()),
        Err(RunError::Diagnostic(error)) => {
            assert_eq!(error.kind, DiagnosticKind::ArithmeticOverflow, "{source}");
            assert!(output.is_empty(), "{source}");
            Err(error.span)
        }
        Err(error) => panic!("interpreter failed to write captured output: {error}"),
    }
}

#[test]
fn object_and_operator_span_table_are_deterministic() {
    for source in CASES {
        let kernel = emit_elf64_checked_integer_kernel(source).unwrap();
        assert_eq!(kernel, emit_elf64_checked_integer_kernel(source).unwrap());
        assert!(kernel.object.starts_with(b"\x7fELF"));
        assert!(kernel.object.len() <= 262_144);
        let symbol = b"\0scarp_checked_scalar_kernel\0";
        assert!(kernel
            .object
            .windows(symbol.len())
            .any(|part| part == symbol));
        for span in &kernel.failure_spans {
            assert!(matches!(
                source.get(span.start..span.end),
                Some("+" | "-" | "*")
            ));
        }
        if let Err(span) = interpreter_result(source) {
            assert!(kernel.failure_spans.contains(&span), "{source}");
        }
    }

    let bitwise = emit_elf64_checked_integer_kernel("bind result = ~42;").unwrap();
    assert!(bitwise.failure_spans.is_empty());
}

#[test]
fn checked_subset_rejects_other_operations_and_keeps_the_legacy_abi() {
    for source in [
        "",
        "bind result = 4 / 2;",
        "bind result = 4 % 2;",
        "bind result = 4 << 2;",
        "bind result = 4 < 2;",
        "var result = 1; result += 2;",
        "bind result = 1; print(result);",
        "bind result = [array: 1, 2];",
    ] {
        let error = emit_elf64_checked_integer_kernel(source).unwrap_err();
        assert_eq!(error.kind, DiagnosticKind::NativeUnsupported, "{source}");
    }
    let source = "bind result = 4 + 2;";
    assert!(emit_elf64_checked_integer_kernel(source).is_ok());
    assert_eq!(
        emit_elf64_integer_kernel(source).unwrap_err().kind,
        DiagnosticKind::NativeUnsupported
    );
}

#[test]
fn checked_kernel_applies_the_native_slot_limit() {
    let mut source = String::from("bind v0 = 0;");
    for slot in 1..256 {
        source.push_str(&format!("bind v{slot} = v{} + 1;", slot - 1));
    }
    let kernel = emit_elf64_checked_integer_kernel(&source).unwrap();
    assert_eq!(kernel.failure_spans.len(), 255);

    source.push_str("bind excess = v255 + 1;");
    assert_eq!(
        emit_elf64_checked_integer_kernel(&source).unwrap_err().kind,
        DiagnosticKind::NativeUnsupported
    );
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[test]
fn system_v_return_value_status_and_span_match_the_interpreter() {
    use std::{fs, process::Command};

    let cc = std::env::var("SCARP_CC").unwrap_or_else(|_| "cc".to_owned());
    if Command::new(&cc).arg("--version").output().is_err() {
        assert!(
            std::env::var_os("SCARP_REQUIRE_C").is_none(),
            "a C compiler is required"
        );
        return;
    }
    let directory =
        std::env::temp_dir().join(format!("scarp-checked-elf64-{}", std::process::id()));
    fs::create_dir(&directory).unwrap();
    let object = directory.join("kernel.o");
    let harness = directory.join("harness.c");
    let executable = directory.join("harness");
    fs::write(
        &harness,
        r#"#include <inttypes.h>
#include <stddef.h>
#include <stdio.h>
struct result { int64_t value; uint32_t status; uint32_t span_id; };
_Static_assert(sizeof(struct result) == 16, "kernel result ABI size");
_Static_assert(offsetof(struct result, status) == 8, "status offset");
_Static_assert(offsetof(struct result, span_id) == 12, "span offset");
extern struct result scarp_checked_scalar_kernel(void);
int main(void) {
    for (int call = 0; call < 2; ++call) {
        struct result result = scarp_checked_scalar_kernel();
        printf("%" PRId64 " %" PRIu32 " %" PRIu32 "\n", result.value, result.status, result.span_id);
    }
    return 0;
}
"#,
    )
    .unwrap();

    for source in CASES {
        let kernel = emit_elf64_checked_integer_kernel(source).unwrap();
        fs::write(&object, &kernel.object).unwrap();
        let link = Command::new(&cc)
            .args(["-std=c11", "-O2", "-Wall", "-Wextra", "-Werror"])
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
        let output = Command::new(&executable).output().unwrap();
        assert!(output.status.success(), "{source}");
        let expected = match interpreter_result(source) {
            Ok(value) => format!("{value} 0 0\n"),
            Err(span) => {
                let span_id = kernel
                    .failure_spans
                    .iter()
                    .position(|&site| site == span)
                    .unwrap();
                format!("0 {CHECKED_KERNEL_INTEGER_OVERFLOW} {span_id}\n")
            }
        };
        assert_eq!(
            String::from_utf8(output.stdout).unwrap(),
            expected.repeat(2),
            "{source}"
        );
    }
    fs::remove_dir_all(directory).unwrap();
}
