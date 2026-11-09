//! Deterministic cross-checks for the experimental ELF64 integer-only backend.
//! Generated programs use only documented pure bitwise dependency forms.

use scarp::native_object::emit_elf64_integer_kernel;

fn generated_cases() -> Vec<(String, i64)> {
    let mut state = 0x9e37_79b9_7f4a_7c15_u64;
    let mut cases = Vec::new();
    for _ in 0..24 {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
        let mut expected = (state >> 1) as i64;
        let mut source = format!("bind v0={expected};");
        for step in 1..=16 {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
            let rhs = (state >> 1) as i64;
            let previous = format!("v{}", step - 1);
            let (expression, next) = match step % 4 {
                0 => (format!("~{previous}"), !expected),
                1 => (format!("{previous} & {rhs}"), expected & rhs),
                2 => (format!("{previous} | {rhs}"), expected | rhs),
                _ => (format!("{previous} ^ {rhs}"), expected ^ rhs),
            };
            source.push_str(&format!(" bind v{step}=({expression});"));
            expected = next;
        }
        cases.push((source, expected));
    }
    cases
}

#[test]
fn generated_dependency_chains_match_independent_rust_oracle_and_interpreter() {
    for (source, expected) in generated_cases() {
        let object = emit_elf64_integer_kernel(&source).unwrap();
        assert_eq!(object, emit_elf64_integer_kernel(&source).unwrap());
        assert_eq!(&object[0..4], b"\x7fELF");

        let mut output = Vec::new();
        scarp::run(&format!("{source} print(v16);"), &mut output).unwrap();
        assert_eq!(
            String::from_utf8(output).unwrap(),
            format!("{expected}\n"),
            "interpreter differs for {source}"
        );
    }
}

// The ELF64 object is linked by a separately compiled C host, not by Scarp.
// Compare return bits as unsigned to avoid C's signed minimum literal edge.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[test]
fn generated_elf64_kernels_match_independent_c_host() {
    use std::{fs, process::Command};

    let cc = std::env::var("SCARP_CC").unwrap_or_else(|_| "cc".to_owned());
    if Command::new(&cc).arg("--version").output().is_err() {
        assert!(
            std::env::var_os("SCARP_REQUIRE_C").is_none(),
            "CI requires a C compiler for the ELF64 differential test"
        );
        return;
    }

    let directory =
        std::env::temp_dir().join(format!("scarp-generated-elf64-{}", std::process::id()));
    fs::create_dir_all(&directory).unwrap();
    let object_path = directory.join("kernel.o");
    let harness_path = directory.join("harness.c");
    let executable_path = directory.join("harness");

    for (case, (source, expected)) in generated_cases().into_iter().take(12).enumerate() {
        fs::write(&object_path, emit_elf64_integer_kernel(&source).unwrap()).unwrap();
        let harness = format!(
            "#include <stdint.h>\nextern int64_t scarp_scalar_kernel(void);\nint main(void) {{ return (uint64_t)scarp_scalar_kernel() == UINT64_C({}) ? 0 : 1; }}\n",
            expected as u64
        );
        fs::write(&harness_path, harness).unwrap();
        let status = Command::new(&cc)
            .arg("-std=c11")
            .arg("-Wall")
            .arg("-Wextra")
            .arg("-Werror")
            .arg(&harness_path)
            .arg(&object_path)
            .arg("-o")
            .arg(&executable_path)
            .status()
            .unwrap();
        assert!(status.success(), "native linker failed for case {case}");
        assert!(
            Command::new(&executable_path).status().unwrap().success(),
            "native result differs from Rust oracle for case {case}"
        );
    }

    fs::remove_dir_all(directory).unwrap();
}
