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
    // 2 status clear + 8 epilogue. Compact encodings save 13 bytes.
    assert_eq!(machine.len(), 35);
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
