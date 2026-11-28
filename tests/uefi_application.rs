use scarp::{
    diagnostics::DiagnosticKind,
    native_object::{
        emit_elf64_linux_executable, emit_pe32plus_uefi_application, emit_pe32plus_uefi_kernel,
        write_pe32plus_uefi_application_new, UefiKernelError,
    },
};
use std::{fs, io, path::PathBuf, process::Command};

const BOOT: &str = include_str!("../experiments/bare-metal/uefi.srp");
const LOADER: &str = include_str!("../experiments/bare-metal/uefi-loader.srp");
const KERNEL: &str = include_str!("../experiments/bare-metal/uefi-kernel.srp");

fn word(image: &[u8], at: usize) -> u16 {
    u16::from_le_bytes(image[at..at + 2].try_into().unwrap())
}

fn dword(image: &[u8], at: usize) -> usize {
    u32::from_le_bytes(image[at..at + 4].try_into().unwrap()) as usize
}

fn section_table(image: &[u8]) -> (usize, usize) {
    let pe = dword(image, 0x3c);
    (
        pe + 24 + word(image, pe + 20) as usize,
        word(image, pe + 6) as usize,
    )
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn flatten_image(image: &[u8]) -> Vec<u8> {
    let (table, count) = section_table(image);
    let optional = dword(image, 0x3c) + 24;
    let mut mapped = vec![0; dword(image, optional + 56)];
    for index in 0..count {
        let at = table + 40 * index;
        let rva = dword(image, at + 12);
        let length = dword(image, at + 8).min(dword(image, at + 16));
        let offset = dword(image, at + 20);
        mapped[rva..rva + length].copy_from_slice(&image[offset..offset + length]);
    }
    mapped
}

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!("scarp-{name}-{}", std::process::id()));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn pe_has_a_relocatable_efi_envelope_and_disjoint_sections() {
    let app = emit_pe32plus_uefi_application(BOOT, "boot", 1000, 4096).unwrap();
    assert_eq!(
        app,
        emit_pe32plus_uefi_application(BOOT, "boot", 1000, app.peak_stack_bytes).unwrap()
    );
    assert_eq!(
        emit_pe32plus_uefi_application(BOOT, "boot", 1000, app.peak_stack_bytes - 1)
            .unwrap_err()
            .kind,
        DiagnosticKind::NativeUnsupported
    );
    assert_eq!(app.max_active_frames, 2);

    let image = &app.image;
    assert_eq!(&image[..2], b"MZ");
    let pe = dword(image, 0x3c);
    assert_eq!(&image[pe..pe + 6], b"PE\0\0\x64\x86");
    assert_eq!(word(image, pe + 6), 3);
    assert_eq!(dword(image, pe + 8), 0); // reproducible timestamp
    let opt = pe + 24;
    assert_eq!(word(image, opt), 0x20b);
    assert_eq!(word(image, opt + 68), 10);
    let file_alignment = dword(image, opt + 36);
    let section_alignment = dword(image, opt + 32);
    let entry = dword(image, opt + 16);
    let mut file_end = dword(image, opt + 60);
    let mut mapped_end = file_end;
    let sections = opt + word(image, pe + 20) as usize;
    for (index, (name, flags)) in [
        (b".text\0\0\0", 0x6000_0020),
        (b".rdata\0\0", 0x4000_0040),
        (b".reloc\0\0", 0x4200_0040),
    ]
    .into_iter()
    .enumerate()
    {
        let section = sections + index * 40;
        assert_eq!(&image[section..section + 8], name);
        assert_eq!(dword(image, section + 36), flags);
        let size = dword(image, section + 8);
        let rva = dword(image, section + 12);
        let raw_size = dword(image, section + 16);
        let offset = dword(image, section + 20);
        assert!(offset >= file_end && rva >= mapped_end);
        assert_eq!(offset % file_alignment, 0);
        assert_eq!(rva % section_alignment, 0);
        assert_eq!(raw_size % file_alignment, 0);
        assert!(size != 0 && size <= raw_size);
        file_end = offset + raw_size;
        mapped_end = rva + size;
        let contents = &image[offset..offset + size];
        if index == 0 {
            assert_eq!(entry, rva);
            assert!(contents.windows(2).any(|bytes| bytes == [0xf3, 0x90]));
        }
        if index == 2 {
            assert_eq!(dword(image, opt + 152), rva);
            assert_eq!(dword(image, opt + 156), size);
            assert_eq!(dword(contents, 0), entry);
            assert_eq!(dword(contents, 4), size);
            assert!(contents[8..].iter().all(|&byte| byte == 0));
        }
    }
    assert_eq!(file_end, image.len());
    assert!(mapped_end < dword(image, opt + 56));
    for index in [1, 9, 12, 13, 14] {
        assert_eq!(
            &image[opt + 112 + index * 8..opt + 120 + index * 8],
            &[0; 8]
        );
    }
}

#[test]
fn console_abi_reserves_stack_only_when_output_is_emitted() {
    let quiet =
        emit_pe32plus_uefi_application("funcra boot() -> integer { return 0; }", "boot", 10, 4096)
            .unwrap();
    let printed = emit_pe32plus_uefi_application(
        "funcra boot() -> integer { print(\"A\\nB\\r\\nC\"); return 0; }",
        "boot",
        10,
        4096,
    )
    .unwrap();
    assert_eq!(printed.peak_stack_bytes, quiet.peak_stack_bytes + 56);
    let pe = dword(&quiet.image, 0x3c);
    assert_eq!(word(&quiet.image, pe + 6), 2);
    let expected: Vec<_> = "A\r\nB\r\nC\r\n\0"
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect();
    assert!(printed
        .image
        .windows(expected.len())
        .any(|bytes| bytes == expected));
}

#[test]
fn native_uefi_packs_locals_from_disjoint_scopes() {
    let mut source = "pure funcra main() -> integer { var result = 0;".to_owned();
    for _ in 0..10 {
        source.push_str("{ bind values = [array: 1, 2, 3, 4, 5, 6, 7, 8]; result += values[4]; }");
    }
    source.push_str("return result - 50; }");
    let linux = emit_elf64_linux_executable(&source, 1000, 4096).unwrap();
    let uefi = emit_pe32plus_uefi_application(&source, "main", 1000, 192).unwrap();
    assert!(uefi.peak_stack_bytes + 512 < linux.peak_stack_bytes);
}

#[test]
fn target_gate_rejects_host_runtime_and_effect_mismatches() {
    for source in [
        "funcra boot(i: integer) -> integer { return i; }",
        "funcra boot() -> integer { var n = bigint_from_integer(4); return 0; }",
        "funcra boot() -> integer { print(123); return 0; }",
        "funcra boot() -> integer { print(\"\\u{0}\"); return 0; }",
        "funcra boot() -> integer { print(\"😀\"); return 0; }",
        "funcra boot() -> integer { return boot(); }",
        "funcra boot() -> integer { return 0; } print(boot());",
    ] {
        let error = emit_pe32plus_uefi_application(source, "boot", 100, 4096).unwrap_err();
        assert_eq!(error.kind, DiagnosticKind::NativeUnsupported, "{source}");
    }
    let too_long = format!(
        "funcra boot() -> integer {{ print(\"{}\"); return 0; }}",
        "x".repeat(1025)
    );
    assert_eq!(
        emit_pe32plus_uefi_application(&too_long, "boot", 100, 4096)
            .unwrap_err()
            .kind,
        DiagnosticKind::NativeUnsupported
    );
    for (source, kind) in [
        (
            "pure funcra boot() -> integer { x86_pause(); return 0; }",
            DiagnosticKind::ImpureFunction,
        ),
        (
            "funcra boot() -> integer { x86_pause(0); return 0; }",
            DiagnosticKind::CallArguments,
        ),
    ] {
        assert_eq!(
            emit_pe32plus_uefi_application(source, "boot", 100, 4096)
                .unwrap_err()
                .kind,
            kind
        );
    }
    assert_eq!(
        scarp::compile("funcra boot() -> integer { x86_pause(); return 0; }")
            .unwrap_err()
            .kind,
        DiagnosticKind::NativeUnsupported
    );
    assert!(scarp::compile(
        "pure funcra x86_pause() -> integer { return 5; } bind result = x86_pause();"
    )
    .is_ok());
}

#[test]
fn cli_publishes_at_a_new_path_only() {
    let work = TestDirectory::new("uefi-cli");
    let source = work.0.join("boot.srp");
    let output = work.0.join("test app.efi");
    fs::write(&source, BOOT).unwrap();
    let invoke = |entry: &str, stack: &str| {
        Command::new(env!("CARGO_BIN_EXE_scarp"))
            .args(["native-exe", "uefi-x86_64", entry, "1000", stack])
            .arg(&source)
            .arg("-o")
            .arg(&output)
            .output()
            .unwrap()
    };
    assert!(!invoke("missing", "4096").status.success());
    assert!(!invoke("boot", "0").status.success());
    assert!(!output.exists());
    let result = invoke("boot", "4096");
    assert!(
        result.status.success(),
        "{:?}",
        String::from_utf8_lossy(&result.stderr)
    );
    let original = fs::read(&output).unwrap();
    assert_eq!(
        original,
        emit_pe32plus_uefi_application(BOOT, "boot", 1000, 4096)
            .unwrap()
            .image
    );
    assert_eq!(
        write_pe32plus_uefi_application_new(&output, &original)
            .unwrap_err()
            .kind(),
        io::ErrorKind::AlreadyExists
    );
    assert_eq!(original, fs::read(&output).unwrap());
    assert_eq!(
        write_pe32plus_uefi_application_new(work.0.join("bad.efi"), b"MZ")
            .unwrap_err()
            .kind(),
        io::ErrorKind::InvalidInput
    );
    assert_eq!(fs::read_dir(&work.0).unwrap().count(), 2);
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[test]
fn relocated_entry_and_firmware_callback_preserve_their_abi() {
    let cc = std::env::var_os("SCARP_CC").unwrap_or_else(|| "cc".into());
    if Command::new(&cc).arg("--version").output().is_err() {
        assert!(
            std::env::var_os("SCARP_REQUIRE_C").is_none(),
            "C compiler required"
        );
        return;
    }
    let work = TestDirectory::new("uefi-abi");
    let source = "
        pure funcra sum(a: integer, b: integer) -> integer { return a + b; }
        funcra boot() -> integer {
            print(\"OK\");
            x86_pause();
            bind value = sum(12, 30);
            print(\"OK\");
            return value - 42;
        }
    ";
    let image = emit_pe32plus_uefi_application(source, "boot", 100, 4096)
        .unwrap()
        .image;
    let pe = dword(&image, 0x3c);
    let opt = pe + 24;
    let (section_table, _) = section_table(&image);
    let flattened = work.0.join("image.bin");
    fs::write(&flattened, flatten_image(&image)).unwrap();
    let probe = work.0.join("probe");
    let built = Command::new(cc)
        .args(["-std=gnu11", "-O2", "-Wall", "-Wextra", "-Werror"])
        .arg(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/uefi_abi_probe.c"))
        .arg("-o")
        .arg(&probe)
        .output()
        .unwrap();
    assert!(
        built.status.success(),
        "{:?}",
        String::from_utf8_lossy(&built.stderr)
    );
    for mode in ["0", "1", "2", "3"] {
        let result = Command::new(&probe)
            .arg(&flattened)
            .arg(dword(&image, opt + 16).to_string())
            .arg(dword(&image, section_table + 8).to_string())
            .arg(mode)
            .output()
            .unwrap();
        assert!(result.status.success(), "mode {mode}: {result:?}");
    }
}

#[test]
fn kernel_layout_has_its_own_uninitialized_nonexecutable_storage() {
    let build = |stack| emit_pe32plus_uefi_kernel(LOADER, "boot", KERNEL, "kernel", 10000, stack);
    let kernel = build(1024).unwrap();
    let exact = kernel.loader_stack_bytes.max(kernel.kernel_stack_bytes);
    assert!((112..1024).contains(&exact));
    assert_eq!(kernel, build(exact).unwrap());
    assert!(build(exact - 1).is_err());
    assert!(kernel
        .kernel_failure_spans
        .iter()
        .any(|span| { KERNEL.get(span.start..span.end) == Some("kernel_exchange64(0, 41)") }));
    let (table, count) = section_table(&kernel.image);
    assert_eq!(count, 4);
    let bss = table + 80;
    assert_eq!(&kernel.image[bss..bss + 8], b".bss\0\0\0\0");
    assert_eq!(dword(&kernel.image, bss + 8), 0x33000);
    assert_eq!(dword(&kernel.image, bss + 12) % 4096, 0);
    assert_eq!(dword(&kernel.image, bss + 16), 0);
    assert_eq!(dword(&kernel.image, bss + 20), 0);
    assert_eq!(dword(&kernel.image, bss + 36), 0xc000_0080);
}

#[test]
fn kernel_semantics_separate_firmware_and_privileged_code() {
    for (body, kind) in [
        (
            "funcra kernel() -> integer { print(\"firmware\"); return 0; }",
            DiagnosticKind::NativeUnsupported,
        ),
        (
            "pure funcra kernel() -> integer { return kernel_load64(0); }",
            DiagnosticKind::ImpureFunction,
        ),
        (
            "funcra kernel() -> integer { return kernel_map_count(0); }",
            DiagnosticKind::CallArguments,
        ),
        (
            "funcra kernel() -> integer { return kernel_load64(true); }",
            DiagnosticKind::TypeMismatch,
        ),
        (
            "funcra kernel() -> integer { return boot(); }",
            DiagnosticKind::UnknownFunction,
        ),
    ] {
        let result = emit_pe32plus_uefi_kernel(LOADER, "boot", body, "kernel", 100, 4096);
        assert!(
            matches!(result, Err(UefiKernelError::Kernel(error)) if error.kind == kind),
            "{body}"
        );
    }
    let loader = "funcra boot() -> integer { return kernel_load64(0); }";
    assert!(matches!(
        emit_pe32plus_uefi_kernel(loader, "boot", KERNEL, "kernel", 100, 4096),
        Err(UefiKernelError::Loader(error)) if error.kind == DiagnosticKind::NativeUnsupported
    ));
    assert_eq!(
        scarp::compile("funcra boot() -> integer { return x86_read_cr3(); }")
            .unwrap_err()
            .kind,
        DiagnosticKind::NativeUnsupported
    );
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[test]
fn firmware_retry_stops_at_the_correct_privilege_boundary() {
    let cc = std::env::var_os("SCARP_CC").unwrap_or_else(|| "cc".into());
    if Command::new(&cc).arg("--version").output().is_err() {
        assert!(
            std::env::var_os("SCARP_REQUIRE_C").is_none(),
            "C compiler required"
        );
        return;
    }
    let work = TestDirectory::new("uefi-kernel-abi");
    let loader = "pure funcra boot() -> integer { return 0; }";
    let kernel = "pure funcra kernel() -> integer { return 0; }";
    let image = emit_pe32plus_uefi_kernel(loader, "boot", kernel, "kernel", 100, 4096)
        .unwrap()
        .image;
    let (table, count) = section_table(&image);
    assert_eq!(count, 3);
    let bss = table + 40;
    let flattened = work.0.join("kernel.bin");
    fs::write(&flattened, flatten_image(&image)).unwrap();
    let probe = work.0.join("probe");
    let built = Command::new(cc)
        .args(["-std=gnu11", "-O2", "-Wall", "-Wextra", "-Werror"])
        .arg(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/uefi_kernel_abi_probe.c"),
        )
        .arg("-o")
        .arg(&probe)
        .output()
        .unwrap();
    assert!(
        built.status.success(),
        "{:?}",
        String::from_utf8_lossy(&built.stderr)
    );
    for case in 0..7 {
        let result = Command::new(&probe)
            .arg(&flattened)
            .arg(dword(&image, table + 12).to_string())
            .arg(dword(&image, table + 8).to_string())
            .arg(dword(&image, bss + 12).to_string())
            .arg(dword(&image, bss + 8).to_string())
            .arg(case.to_string())
            .output()
            .unwrap();
        assert!(result.status.success(), "mode {case}: {result:?}");
    }
}
