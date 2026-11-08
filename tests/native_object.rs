use scarp::{diagnostics::DiagnosticKind, native_object::emit_elf64_integer_kernel};

fn le16(bytes: &[u8], at: usize) -> u16 {
    u16::from_le_bytes(bytes[at..at + 2].try_into().unwrap())
}
fn le32(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
}
fn le64(bytes: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(bytes[at..at + 8].try_into().unwrap())
}

#[test]
fn deterministic_elf64_headers_sections_and_symbols() {
    let source = "bind value = (42 & 15) ^ (~0 & 8);";
    let a = emit_elf64_integer_kernel(source).unwrap();
    let b = emit_elf64_integer_kernel(source).unwrap();
    assert_eq!(a, b);
    assert!(a.len() < 262_144);
    assert_eq!(&a[0..7], b"\x7fELF\x02\x01\x01");
    assert_eq!(le16(&a, 16), 1); // relocatable
    assert_eq!(le16(&a, 18), 62); // x86-64
    assert_eq!(le32(&a, 20), 1);
    assert_eq!(le64(&a, 24), 0); // no entry point
    assert_eq!(le64(&a, 32), 0); // no program headers
    assert_eq!(le16(&a, 52), 64);
    assert_eq!(le16(&a, 58), 64);
    assert_eq!(le16(&a, 60), 6);
    assert_eq!(le16(&a, 62), 4);
    let shoff = le64(&a, 40) as usize;
    assert_eq!(shoff % 8, 0);
    assert_eq!(shoff + 6 * 64, a.len());
    let text = shoff + 64;
    let text_at = le64(&a, text + 24) as usize;
    let text_len = le64(&a, text + 32) as usize;
    assert_eq!(le32(&a, text + 4), 1);
    assert_eq!(le64(&a, text + 8), 6);
    assert_eq!(text_at % 16, 0);
    assert_eq!(&a[text_at..text_at + 3], &[0x48, 0x81, 0xec]);
    assert_eq!(a[text_at + text_len - 1], 0xc3);
    let sym = shoff + 2 * 64;
    assert_eq!(le32(&a, sym + 4), 2);
    assert_eq!(le32(&a, sym + 40), 3);
    assert_eq!(le32(&a, sym + 44), 1);
    let sym_at = le64(&a, sym + 24) as usize;
    assert_eq!(a[sym_at + 24 + 4], 0x12);
    assert_eq!(le16(&a, sym_at + 24 + 6), 1);
    assert_eq!(le64(&a, sym_at + 24 + 16), text_len as u64);
    let str_hdr = shoff + 3 * 64;
    let str_off = le64(&a, str_hdr + 24) as usize;
    assert!(a[str_off..].starts_with(b"\0scarp_scalar_kernel\0"));
    let note_hdr = shoff + 5 * 64;
    assert_eq!(le64(&a, note_hdr + 8), 0); // nonexecutable stack
}

#[test]
fn supported_bitwise_forms_have_checked_lowering() {
    for expr in ["0", "1", "9223372036854775807", "~0", "((1 | 2) ^ 3) & 7"] {
        let source = format!("bind result = {expr};");
        let object = emit_elf64_integer_kernel(&source).unwrap();
        assert!(object.len() >= 6 * 64 + 64, "{expr}");
    }
}

#[test]
fn unsupported_programs_fail_without_an_artifact() {
    for source in [
        "", "print(1);", "bind value = true;",
        "bind value = 1 + 2;", "bind value = 1 / 0;",
        "bind value = 1 << 3;", "bind value = 2.0;",
        "bind x = 1; bind y = 2;",
        "bind x = 1; print(x);",
    ] {
        let result = emit_elf64_integer_kernel(source).unwrap_err();
        assert_eq!(result.kind, DiagnosticKind::NativeUnsupported, "{source}");
    }
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[test]
fn independent_host_link_and_execution_matches_integer_oracle() {
    use std::{fs, process::Command};
    let cc = std::env::var("SCARP_CC").unwrap_or_else(|_| "cc".to_owned());
    if Command::new(&cc).arg("--version").output().is_err() {
        assert!(std::env::var_os("SCARP_REQUIRE_C").is_none(),
            "CI requires a C compiler for independent ELF host validation");
        return;
    }
    let directory = std::env::temp_dir().join(format!(
        "scarp-native-kernel-{}", std::process::id()
    ));
    fs::create_dir(&directory).unwrap();
    let object_path = directory.join("kernel.o");
    let source_path = directory.join("harness.c");
    let executable = directory.join("harness");
    fs::write(&object_path,
        emit_elf64_integer_kernel("bind result = (42 & 15) ^ (~0 & 8);").unwrap()
    ).unwrap();
    fs::write(&source_path,
        "extern long long scarp_scalar_kernel(void);\nint main(void) { return scarp_scalar_kernel() == 2LL ? 0 : 1; }\n"
    ).unwrap();
    let status = Command::new(&cc)
        .arg("-std=c11").arg("-Wall").arg("-Wextra").arg("-Werror")
        .arg(&source_path).arg(&object_path).arg("-o").arg(&executable)
        .status().unwrap();
    assert!(status.success(), "independent C host failed to link ELF object");
    assert!(Command::new(&executable).status().unwrap().success());
    if let Ok(view) = Command::new("readelf").arg("-h").arg("-S").arg("-s")
        .arg(&object_path).output() {
        assert!(view.status.success());
        let out = String::from_utf8(view.stdout).unwrap();
        assert!(out.contains("REL (Relocatable file)"));
        assert!(out.contains("scarp_scalar_kernel"));
    }
    fs::remove_dir_all(directory).unwrap();
}
