//! Independent native ABI regression tests for caller-stack arguments.
use scarp::native_object::emit_elf64_checked_integer_function;

fn function(arity: usize, body: &str) -> String {
    let parameters = (0..arity)
        .map(|i| format!("p{i}: integer"))
        .collect::<Vec<_>>()
        .join(", ");
    format!("pure funcra f({parameters}) -> integer {{ {body} }}")
}

fn weighted_body(arity: usize) -> String {
    let terms = (0..arity)
        .map(|i| format!("p{i} * {}", i + 1))
        .collect::<Vec<_>>()
        .join(" + ");
    format!("return {terms};")
}

#[test]
fn stack_argument_arity_is_bounded_and_objects_are_deterministic() {
    for arity in 7..=16 {
        let source = function(arity, &weighted_body(arity));
        let object = emit_elf64_checked_integer_function(&source).unwrap();
        assert_eq!(
            object,
            emit_elf64_checked_integer_function(&source).unwrap()
        );
        assert_eq!(&object.object[..7], b"\x7fELF\x02\x01\x01");
    }
    let too_many = function(17, "return p16;");
    assert!(emit_elf64_checked_integer_function(&too_many).is_err());
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[test]
fn stack_arguments_match_interpreter_for_every_supported_extra_arity() {
    use scarp::{diagnostics::DiagnosticKind, RunError};
    use std::{fmt::Write, fs, process::Command};

    let cc = std::env::var_os("SCARP_CC").unwrap_or_else(|| "cc".into());
    if Command::new(&cc).arg("--version").output().is_err() {
        assert!(
            std::env::var_os("SCARP_REQUIRE_C").is_none(),
            "C compiler required"
        );
        return;
    }
    let directory =
        std::env::temp_dir().join(format!("scarp-native-stack-args-{}", std::process::id()));
    fs::create_dir(&directory).unwrap();

    for arity in 7..=16 {
        let body = format!(
            "if p0 == 0 {{ return p{} / p6; }} {}",
            arity - 1,
            weighted_body(arity)
        );
        let source = function(arity, &body);
        let artifact = emit_elf64_checked_integer_function(&source).unwrap();
        let object = directory.join("function.o");
        let harness = directory.join("harness.c");
        let binary = directory.join("harness");
        fs::write(&object, &artifact.object).unwrap();
        let params = vec!["int64_t"; arity].join(", ");
        let mut c = format!(
            r#"#include <stdint.h>
#include <inttypes.h>
#include <stdio.h>
struct result {{ int64_t value; uint32_t status; uint32_t span_id; }};
extern struct result scarp_checked_integer_function({params});
int main(void) {{
"#
        );
        let mut cases = vec![
            (0..arity).map(|i| i as i64 + 1).collect::<Vec<_>>(),
            (0..arity).map(|i| -(i as i64) - 1).collect::<Vec<_>>(),
            vec![0; arity],
        ];
        let mut divided = vec![0; arity];
        divided[6] = 2;
        divided[arity - 1] = 8;
        cases.push(divided);
        let mut overflow = vec![1; arity];
        overflow[arity - 1] = i64::MAX;
        cases.push(overflow);

        let mut expected = String::new();
        for args in cases {
            let args = args
                .iter()
                .map(i64::to_string)
                .collect::<Vec<_>>()
                .join(", ");
            writeln!(
                c,
                r#"{{ struct result r = scarp_checked_integer_function({args});
                printf("%" PRId64 " %" PRIu32 " %" PRIu32 "\n", r.value, r.status, r.span_id); }}"#
            )
            .unwrap();
            let mut output = Vec::new();
            match scarp::run(&format!("{source}\nprint(f({args}));"), &mut output) {
                Ok(()) => writeln!(
                    expected,
                    "{} 0 0",
                    std::str::from_utf8(&output).unwrap().trim()
                )
                .unwrap(),
                Err(RunError::Diagnostic(error)) => {
                    let status = match error.kind {
                        DiagnosticKind::ArithmeticOverflow => 1,
                        DiagnosticKind::DivisionByZero => 2,
                        _ => panic!("{error:?}"),
                    };
                    let id = artifact
                        .failure_spans
                        .iter()
                        .position(|span| *span == error.span)
                        .unwrap();
                    writeln!(expected, "0 {status} {id}").unwrap();
                }
                Err(error) => panic!("{error:?}"),
            }
        }
        c.push_str("return 0; }\n");
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
        for _ in 0..2 {
            let output = Command::new(&binary).output().unwrap();
            assert!(output.status.success());
            assert_eq!(
                String::from_utf8(output.stdout).unwrap(),
                expected,
                "arity {arity}"
            );
        }
    }
    fs::remove_dir_all(directory).unwrap();
}
