use scarp::native_object::{
    emit_elf64_bounded_integer_function, emit_elf64_checked_integer_function,
};

const SUM: &str = "pure funcra f(n: integer) -> integer {
    var i = n;
    var total = 0;
    while i > 0 {
        total += i;
        i -= 1;
    }
    return total;
}";

#[test]
fn bounded_loop_objects_are_deterministic_and_opt_in() {
    let first = emit_elf64_bounded_integer_function(SUM, 1000).unwrap();
    let second = emit_elf64_bounded_integer_function(SUM, 1000).unwrap();
    assert_eq!(first, second);
    assert_ne!(
        first.object,
        emit_elf64_bounded_integer_function(SUM, 1001)
            .unwrap()
            .object
    );
    assert!(first
        .object
        .windows(b"\0scarp_bounded_integer_function\0".len())
        .any(|part| part == b"\0scarp_bounded_integer_function\0"));
    assert!(emit_elf64_checked_integer_function(SUM).is_err());
    assert!(emit_elf64_bounded_integer_function(SUM, 0).is_ok());
    for span in first.failure_spans {
        assert!(SUM.get(span.start..span.end).is_some());
    }
}

#[test]
fn bounded_native_rejects_unsupported_effects_and_calls() {
    for source in [
        "funcra f() -> integer { return 1; }",
        "pure funcra f(n: integer) -> integer { return f(n); }",
        "pure funcra f() -> integer { bind a = [array: 1, 2]; return 1; }",
        "pure funcra f() -> integer { return 1; } print(f());",
    ] {
        assert!(
            emit_elf64_bounded_integer_function(source, 100).is_err(),
            "{source}"
        );
    }
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[test]
fn native_loops_match_interpreter_and_exhaust_budget_per_invocation() {
    use scarp::{
        native_object::{CHECKED_KERNEL_DIVISION_BY_ZERO, CHECKED_KERNEL_WORK_LIMIT},
        RunError,
    };
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
        std::env::temp_dir().join(format!("scarp-native-bounded-{}", std::process::id()));
    fs::create_dir(&directory).unwrap();
    let object = directory.join("function.o");
    let harness = directory.join("harness.c");
    let binary = directory.join("harness");
    fs::write(
        &harness,
        r#"#include <stdint.h>
#include <inttypes.h>
#include <stdio.h>
#include <stdlib.h>
struct result { int64_t value; uint32_t status; uint32_t span_id; };
extern struct result scarp_bounded_integer_function(int64_t);
int main(int argc, char **argv) {
    if (argc != 2) return 1;
    int64_t input = strtoll(argv[1], NULL, 10);
    for (int i = 0; i < 2; ++i) {
        struct result r = scarp_bounded_integer_function(input);
        printf("%" PRId64 " %" PRIu32 " %" PRIu32 "\n", r.value, r.status, r.span_id);
    }
    return 0;
}
"#,
    )
    .unwrap();
    let control = "pure funcra f(n: integer) -> integer {
        var i = 0; var total = 0;
        while i < n {
            i += 1;
            if i == 2 { continue; }
            if i == 5 { break; }
            bind item = i * 2;
            total += item;
        }
        return total;
    }";
    let nested = "pure funcra f(n: integer) -> integer {
        var i = 0; var total = 0;
        while i < n {
            var j = 0;
            while j < 3 {
                j += 1;
                total += i + j;
            }
            i += 1;
        }
        return total;
    }";
    let short = "pure funcra f(n: integer) -> integer {
        var i = n;
        while i > 0 and (i == 1 or 12 / i >= 0) {
            i -= 1;
        }
        return i;
    }";
    let constant = "pure funcra f(n: integer) -> integer { return 42; }";
    let failure = "pure funcra f(n: integer) -> integer { return n / 0; }";
    let cases = [
        (constant, 0, 0, true),
        (constant, 1, 0, true),
        (constant, 2, 0, false),
        (constant, u32::MAX, 0, false),
        (failure, 0, 8, true),
        (failure, 4, 8, true),
        (failure, 5, 8, false),
        (failure, 100, 8, false),
        (SUM, 1000, 10, false),
        (SUM, 1000, 0, false),
        (SUM, 1000, -3, false),
        (SUM, 0, 0, true),
        (SUM, 5, 10, true),
        (SUM, 100, 1000000, true),
        (control, 1000, 10, false),
        (nested, 1000, 4, false),
        (short, 1000, 0, false),
        (short, 1000, 4, false),
    ];
    for (source, allowance, argument, exhausted) in cases {
        let artifact = emit_elf64_bounded_integer_function(source, allowance).unwrap();
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
        let result = Command::new(&binary)
            .arg(argument.to_string())
            .output()
            .unwrap();
        assert!(result.status.success());
        let stdout = std::str::from_utf8(&result.stdout).unwrap();
        let rows = stdout.lines().collect::<Vec<_>>();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0], rows[1], "meter must reset on each invocation");
        let values = rows[0].split_whitespace().collect::<Vec<_>>();
        assert_eq!(values.len(), 3);
        if exhausted {
            assert_eq!(values[0], "0");
            assert_eq!(values[1].parse::<u32>().unwrap(), CHECKED_KERNEL_WORK_LIMIT);
            let id = values[2].parse::<usize>().unwrap();
            let span = artifact.failure_spans[id];
            let text = &source[span.start..span.end];
            if source == constant {
                assert_eq!(text, if allowance == 0 { "return 42;" } else { "42" });
            } else if source == failure && allowance == 4 {
                assert_eq!(text, "/");
            }
        } else {
            let mut expected = Vec::new();
            match scarp::run(&format!("{source} print(f({argument}));"), &mut expected) {
                Ok(()) => {
                    let expected = std::str::from_utf8(&expected).unwrap().trim();
                    assert_eq!(values, [expected, "0", "0"]);
                }
                Err(RunError::Diagnostic(error)) => {
                    assert_eq!(
                        error.kind,
                        scarp::diagnostics::DiagnosticKind::DivisionByZero
                    );
                    assert_eq!(values[0], "0");
                    assert_eq!(
                        values[1].parse::<u32>().unwrap(),
                        CHECKED_KERNEL_DIVISION_BY_ZERO
                    );
                    let id = values[2].parse::<usize>().unwrap();
                    assert_eq!(artifact.failure_spans[id], error.span);
                    assert!(expected.is_empty());
                }
                Err(error) => panic!("{error:?}"),
            }
        }
    }
    fs::remove_dir_all(directory).unwrap();
}
