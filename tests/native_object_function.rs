use scarp::{diagnostics::DiagnosticKind, native_object::emit_elf64_checked_integer_function};

const EXAMPLE: &str = include_str!("../examples/checked_elf64_function.srp");

#[test]
fn generated_example_matches_its_oracle_and_object_is_deterministic() {
    let mut output = Vec::new();
    scarp::run(EXAMPLE, &mut output).unwrap();
    assert_eq!(
        output,
        include_bytes!("../examples/checked_elf64_function.stdout")
    );
    let source = EXAMPLE.split_once("\nprint(").unwrap().0;
    let object = emit_elf64_checked_integer_function(source).unwrap();
    assert_eq!(object, emit_elf64_checked_integer_function(source).unwrap());
    assert_eq!(&object.object[..7], b"\x7fELF\x02\x01\x01");
    let symbol = b"\0scarp_checked_integer_function\0";
    assert!(object
        .object
        .windows(symbol.len())
        .any(|bytes| bytes == symbol));
    let failure_tokens = object
        .failure_spans
        .iter()
        .map(|span| &source[span.start..span.end])
        .collect::<Vec<_>>();
    // Negating the literal 1 succeeds at compile time. Only the
    // multiplication of runtime arguments still needs an overflow guard.
    assert_eq!(failure_tokens, ["*"]);
}

#[test]
fn zero_argument_functions_emit_deterministic_objects_and_preserve_failure_spans() {
    let source = "pure funcra answer() -> integer { bind a = 6; return a * 7; }";
    let artifact = emit_elf64_checked_integer_function(source).unwrap();
    assert_eq!(
        artifact,
        emit_elf64_checked_integer_function(source).unwrap()
    );
    assert_eq!(&artifact.object[..7], b"\x7fELF\x02\x01\x01");
    assert_eq!(artifact.failure_spans.len(), 1);
    let span = artifact.failure_spans[0];
    assert_eq!(&source[span.start..span.end], "*");
    let mut output = Vec::new();
    scarp::run(&format!("{source} print(answer());"), &mut output).unwrap();
    assert_eq!(output, b"42\n");

    let source = "pure funcra fail() -> integer { bind zero = 0; return 7 / zero; }";
    let artifact = emit_elf64_checked_integer_function(source).unwrap();
    let span = artifact.failure_spans[0];
    assert_eq!(&source[span.start..span.end], "/");
    let mut output = Vec::new();
    let error = scarp::run(&format!("{source} print(fail());"), &mut output).unwrap_err();
    let scarp::RunError::Diagnostic(error) = error else {
        panic!("expected arithmetic diagnostic");
    };
    assert_eq!(error.kind, DiagnosticKind::DivisionByZero);
    assert_eq!(error.span, span);
    assert!(output.is_empty());
}

#[test]
fn unsupported_functions_return_diagnostics_without_an_object() {
    for source in [
        "funcra f(x: integer) -> integer { return x; }",
        "pure funcra f(x: boolean) -> integer { return 0; }",
        "pure funcra f(x: integer = 2) -> integer { return x; }",
        "pure funcra f(x: integer) -> boolean { return x == 0; }",
        "pure funcra f(x: integer) -> integer { return f(x - 1); }",
        "pure funcra f(x: integer) -> integer { while x > 0 { return x; } return 0; }",
        "pure funcra f(x: integer) -> integer { bind y = 0.1d; return x; }",
        "pure funcra f(x: integer) -> integer { return x; } print(f(1));",
        "pure funcra f(x: integer) -> integer { return x; } pure funcra g(x: integer) -> integer { return x; }",
    ] {
        let error = emit_elf64_checked_integer_function(source).unwrap_err();
        assert_eq!(error.kind, DiagnosticKind::NativeUnsupported, "{source}");
    }
    let missing_return = emit_elf64_checked_integer_function(
        "pure funcra f(x: integer) -> integer { if x < 0 { return x; } }",
    )
    .unwrap_err();
    assert_eq!(missing_return.kind, DiagnosticKind::MissingReturn);
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[test]
fn sysv_function_matches_interpreter_for_registers_branches_and_failure_paths() {
    use scarp::{
        native_object::{
            CHECKED_KERNEL_DIVISION_BY_ZERO, CHECKED_KERNEL_INTEGER_OVERFLOW,
            CHECKED_KERNEL_INVALID_SHIFT,
        },
        RunError,
    };
    use std::{fmt::Write, fs, process::Command};

    const SOURCE: &str = "
    // यूटीएफ-8 makes failure offsets sensitive to source byte indexing.
    pure funcra score(a: integer, b: integer, c: integer,
        d: integer, e: integer, f: integer) -> integer {
        var total = a;
        total += b * 2;
        bind ready = c != d and (d == 0 or c / d >= 0);
        if ready and not (e < f) {
            bind total = total + c * 4 + d * 8 + e * 16 + f * 32;
            return total;
        } elif c <= d {
            return total / f;
        } else {
            return total << e;
        }
    }";
    let cc = std::env::var_os("SCARP_CC").unwrap_or_else(|| "cc".into());
    if Command::new(&cc).arg("--version").output().is_err() {
        assert!(
            std::env::var_os("SCARP_REQUIRE_C").is_none(),
            "C compiler required"
        );
        return;
    }
    let kernel = emit_elf64_checked_integer_function(SOURCE).unwrap();
    let directory =
        std::env::temp_dir().join(format!("scarp-elf64-function-{}", std::process::id()));
    fs::create_dir(&directory).unwrap();
    let object = directory.join("function.o");
    let harness = directory.join("harness.c");
    let binary = directory.join("harness");
    fs::write(&object, &kernel.object).unwrap();
    let mut c = String::from(
        r#"#include <stddef.h>
#include <stdint.h>
#include <inttypes.h>
#include <stdio.h>
struct result { int64_t value; uint32_t status; uint32_t span_id; };
_Static_assert(sizeof(struct result) == 16, "result size");
_Static_assert(offsetof(struct result, status) == 8, "status offset");
_Static_assert(offsetof(struct result, span_id) == 12, "span offset");
extern struct result scarp_checked_integer_function(int64_t, int64_t, int64_t,
    int64_t, int64_t, int64_t);
int main(void) {
"#,
    );
    let mut cases = vec![
        [1, 2, 3, 0, 5, 4],
        [10, -2, -4, -2, 3, 1],
        [19, -2, 0, 0, 5, -3],
        [1, 2, 0, 0, 1, 0],
        [1, 2, 1, 0, 70, 71],
        [i64::MAX, 1, 0, 0, 0, 0],
        [0, i64::MIN, 0, 0, 0, 0],
        [0, 0, i64::MIN, -1, 0, 0],
        [i64::MIN, 0, 0, 0, 0, -1],
    ];
    for a in -3..=3 {
        for c in -3..=3 {
            for d in -2..=2 {
                cases.push([a, a - 2, c, d, c - d, a + d]);
            }
        }
    }
    writeln!(c, "const int64_t cases[][6] = {{").unwrap();
    let mut expected = String::new();
    for args in &cases {
        let c_args = args
            .iter()
            .map(|v| {
                if *v == i64::MIN {
                    "INT64_MIN".into()
                } else {
                    format!("{v}LL")
                }
            })
            .collect::<Vec<_>>()
            .join(", ");
        writeln!(c, "    {{{c_args}}},").unwrap();
        let args = args.map(|v| v.to_string()).join(", ");
        let mut output = Vec::new();
        match scarp::run(&format!("{SOURCE}\nprint(score({args}));"), &mut output) {
            Ok(()) => writeln!(
                expected,
                "{} 0 0",
                std::str::from_utf8(&output).unwrap().trim()
            )
            .unwrap(),
            Err(RunError::Diagnostic(error)) => {
                let status = match error.kind {
                    DiagnosticKind::ArithmeticOverflow => CHECKED_KERNEL_INTEGER_OVERFLOW,
                    DiagnosticKind::DivisionByZero => CHECKED_KERNEL_DIVISION_BY_ZERO,
                    DiagnosticKind::InvalidShift => CHECKED_KERNEL_INVALID_SHIFT,
                    _ => panic!("{error:?}"),
                };
                let id = kernel
                    .failure_spans
                    .iter()
                    .position(|span| *span == error.span)
                    .unwrap();
                writeln!(expected, "0 {status} {id}").unwrap();
            }
            Err(error) => panic!("{error:?}"),
        }
    }
    c.push_str(
        r#"};
    for (int pass = 0; pass < 2; ++pass) {
        for (size_t i = 0; i < sizeof(cases) / sizeof(cases[0]); ++i) {
            const int64_t *args = cases[i];
            struct result r = scarp_checked_integer_function(
                args[0], args[1], args[2], args[3], args[4], args[5]);
            printf("%" PRId64 " %" PRIu32 " %" PRIu32 "\n", r.value, r.status, r.span_id);
        }
    }
    return 0;
}
"#,
    );
    fs::write(&harness, c).unwrap();
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
    let result = Command::new(&binary).output().unwrap();
    fs::remove_dir_all(directory).unwrap();
    assert!(result.status.success(), "{:?}", result.status);
    assert_eq!(
        String::from_utf8(result.stdout).unwrap(),
        expected.repeat(2)
    );
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[test]
fn sysv_zero_argument_function_matches_interpreter_success_and_failure() {
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
        std::env::temp_dir().join(format!("scarp-elf64-zero-function-{}", std::process::id()));
    fs::create_dir(&directory).unwrap();
    let harness = directory.join("harness.c");
    fs::write(
        &harness,
        r#"#include <stdint.h>
#include <inttypes.h>
#include <stdio.h>
struct result { int64_t value; uint32_t status; uint32_t span_id; };
extern struct result scarp_checked_integer_function(void);
int main(void) {
    for (int i = 0; i < 2; ++i) {
        struct result r = scarp_checked_integer_function();
        printf("%" PRId64 " %" PRIu32 " %" PRIu32 "\n", r.value, r.status, r.span_id);
    }
    return 0;
}
"#,
    )
    .unwrap();
    for (index, (source, expected)) in [
        ("pure funcra f() -> integer { return 42; }", "42 0 0\n"),
        (
            "pure funcra f() -> integer { bind zero = 0; return 7 / zero; }",
            "0 2 0\n",
        ),
        (
            "pure funcra f() -> integer { bind a = 4; if a > 0 { return a; } return 9; }",
            "4 0 0\n",
        ),
        (
            "pure funcra f() -> integer { bind a = 9223372036854775807; return a + 1; }",
            "0 1 0\n",
        ),
    ]
    .iter()
    .enumerate()
    {
        let artifact = emit_elf64_checked_integer_function(source).unwrap();
        let object = directory.join(format!("function-{index}.o"));
        let binary = directory.join(format!("function-{index}"));
        fs::write(&object, artifact.object).unwrap();
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
        let result = Command::new(&binary).output().unwrap();
        assert!(result.status.success());
        assert_eq!(
            String::from_utf8(result.stdout).unwrap(),
            expected.repeat(2)
        );
    }
    fs::remove_dir_all(directory).unwrap();
}
