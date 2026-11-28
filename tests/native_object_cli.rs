use scarp::native_object::{
    emit_elf64_bounded_integer_entry_with_layout, emit_freestanding_function,
    emit_x86_64_flat_entry, FreestandingFunctionMode, FreestandingTarget,
};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

struct Directory(PathBuf);
impl Directory {
    fn new(name: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("scarp-object-cli-{name}-{}", std::process::id()));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_scarp"))
            .current_dir(&self.0)
            .arg("native-object")
            .args(args)
            .output()
            .unwrap()
    }
    fn path(&self, path: &str) -> PathBuf {
        self.0.join(Path::new(path))
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn flat_entry_emits_the_elf_text_and_reports_a_callable_offset_zero() {
    let family = Directory::new("flat");
    let source = "pure funcra helper(x: integer) -> integer { return x + 2; }
        pure funcra entry(x: integer) -> integer { return helper(x) * 2; }";
    fs::write(family.path("test.srp"), source).unwrap();
    let flat = emit_x86_64_flat_entry(source, "entry", 1000, 1024).unwrap();
    let elf = emit_elf64_bounded_integer_entry_with_layout(source, "entry", 1000, 1024).unwrap();
    assert_eq!(flat.entry_layout, elf.entry_layout);
    assert_eq!(flat.failure_spans, elf.kernel.failure_spans);
    assert_eq!(flat.peak_stack_bytes, elf.peak_stack_bytes);

    let image = &elf.kernel.object;
    let field = |at| u64::from_le_bytes(image[at..at + 8].try_into().unwrap()) as usize;
    let text = field(40) + 64;
    let offset = field(text + 24);
    let length = field(text + 32);
    assert_eq!(flat.bytes, image[offset..offset + length]);

    let args = [
        "--json",
        "x86_64-none",
        "flat-entry",
        "entry",
        "1000",
        "1024",
        "test.srp",
        "-o",
        "entry.bin",
    ];
    let run = family.run(&args);
    assert!(run.status.success(), "{:?}", run.stderr);
    let metadata = String::from_utf8(run.stdout).unwrap();
    assert!(metadata.contains("\"mode\":\"flat-entry\"") && metadata.contains("\"symbol\":null"));
    assert_eq!(fs::read(family.path("entry.bin")).unwrap(), flat.bytes);
    assert!(!family.run(&args).status.success());
    assert_eq!(fs::read(family.path("entry.bin")).unwrap(), flat.bytes);
}

#[test]
fn cli_emits_exact_api_bytes_and_preserves_existing_destinations() {
    let d = Directory::new("bytes");
    for (args, mode, src) in [
        (
            vec!["x86_64-none", "checked"],
            FreestandingFunctionMode::Checked,
            "pure funcra f(a:integer)->integer{return a+1;}",
        ),
        (
            vec!["x86_64-none", "bounded", "0"],
            FreestandingFunctionMode::Bounded { max_work: 0 },
            "pure funcra f(a:integer)->integer{var n=a; while n>0 {n-=1;} return n;}",
        ),
        (
            vec!["x86_64-none", "bounded", "4294967295"],
            FreestandingFunctionMode::Bounded { max_work: u32::MAX },
            "pure funcra f()->integer{return 42;}",
        ),
    ] {
        fs::write(d.path("-é.srp"), src).unwrap();
        let mut args = args;
        args.extend(["-é.srp", "-o", "out file.o"]);
        let result = d.run(&args);
        assert!(result.status.success(), "{:?}", result.stderr);
        assert!(result.stdout.is_empty() && result.stderr.is_empty());
        let expected =
            emit_freestanding_function(src, FreestandingTarget::X86_64None, mode).unwrap();
        assert_eq!(
            fs::read(d.path("out file.o")).unwrap(),
            expected.kernel.object
        );
        fs::write(d.path("out file.o"), b"keep").unwrap();
        let conflict = d.run(&args);
        assert_eq!(conflict.status.code(), Some(1));
        assert!(conflict.stdout.is_empty());
        assert_eq!(fs::read(d.path("out file.o")).unwrap(), b"keep");
        fs::remove_file(d.path("out file.o")).unwrap();
    }
}

#[test]
fn bad_target_mode_and_budget_fail_before_loading_source() {
    let d = Directory::new("usage");
    for args in [
        vec!["x86_64", "checked", "missing.srp", "-o", "out.o"],
        vec!["x86_64-none", "automatic", "missing.srp", "-o", "out.o"],
        vec!["x86_64-none", "checked", "missing.srp", "out.o"],
        vec![
            "x86_64-none",
            "bounded",
            "4294967296",
            "missing.srp",
            "-o",
            "out.o",
        ],
        vec!["x86_64-none", "bounded", "-1", "missing.srp", "-o", "out.o"],
        vec!["x86_64-none", "bounded", "+1", "missing.srp", "-o", "out.o"],
        vec!["x86_64-none", "bounded", "missing.srp", "-o", "out.o"],
    ] {
        let r = d.run(&args);
        assert_eq!(r.status.code(), Some(2));
        assert!(!r.stderr.is_empty() && r.stdout.is_empty());
        assert_eq!(fs::read_dir(&d.0).unwrap().count(), 0);
    }
    let help = d.run(&["--help"]);
    let guide = Command::new(env!("CARGO_BIN_EXE_scarp"))
        .args(["guide", "native-object"])
        .output()
        .unwrap();
    assert!(guide.status.success() && help.status.success());
    assert_eq!(guide.stdout, help.stdout);
}

#[test]
fn source_errors_publish_nothing_and_do_not_execute_top_level_effects() {
    use scarp::{diagnostics::DiagnosticKind, modules::ModuleLimits};
    let d = Directory::new("source");
    let args = [
        "--json",
        "x86_64-none",
        "checked",
        "source.srp",
        "-o",
        "out.o",
    ];
    for bytes in [
        b"// UTF-8\r\nprint(42);".to_vec(),
        vec![0xff],
        vec![b' '; ModuleLimits::default().max_file_bytes + 1],
    ] {
        fs::write(d.path("source.srp"), &bytes).unwrap();
        let result = d.run(&args);
        assert_eq!(result.status.code(), Some(1));
        assert!(result.stdout.is_empty() && !result.stderr.is_empty());
        if bytes.starts_with(b"//") {
            assert!(String::from_utf8(result.stderr)
                .unwrap()
                .contains(DiagnosticKind::NativeUnsupported.code()));
        }
        assert!(!d.path("out.o").exists());
    }
    fs::write(d.path("source.srp"), "pure funcra f()->integer{return 42;}").unwrap();
    let result = d.run(&[
        "x86_64-none",
        "checked",
        "source.srp",
        "-o",
        "missing/out.o",
    ]);
    assert_eq!(result.status.code(), Some(1));
    assert!(!d.path("missing").exists());
}

#[test]
fn json_reports_the_emitted_abi_and_original_byte_spans() {
    let d = Directory::new("metadata");
    let source = "// जहाँ 😀\r\npure funcra f(a:integer,b:integer,c:integer,d:integer,e:integer,unused:integer,g:integer)->integer{return a/g;}";
    fs::write(d.path("é source.srp"), source).unwrap();
    for (mut args, mode, mode_name, budget) in [
        (
            vec!["checked"],
            FreestandingFunctionMode::Checked,
            "checked",
            "null",
        ),
        (
            vec!["bounded", "0"],
            FreestandingFunctionMode::Bounded { max_work: 0 },
            "bounded",
            "0",
        ),
    ] {
        let mut command = vec!["--json", "x86_64-none"];
        command.append(&mut args);
        command.extend(["é source.srp", "-o", "report.o"]);
        let result = d.run(&command);
        assert!(result.status.success(), "{:?}", result.stderr);
        assert!(result.stderr.is_empty());
        let artifact =
            emit_freestanding_function(source, FreestandingTarget::X86_64None, mode).unwrap();
        assert_eq!(
            fs::read(d.path("report.o")).unwrap(),
            artifact.kernel.object
        );
        let spans = &artifact.kernel.failure_spans;
        assert!(spans
            .iter()
            .any(|span| &source[span.start..span.end] == "/"));
        let ranges = spans
            .iter()
            .map(|span| format!("[{},{}]", span.start, span.end))
            .collect::<Vec<_>>()
            .join(",");
        let layout = artifact.layout;
        let expected = format!(concat!(
            "{{\"schema_version\":1,\"target\":\"x86_64-none\",\"mode\":\"{}\",\"symbol\":\"{}\",\"max_work\":{},",
            "\"integer_arguments\":7,\"call_stack_alignment\":16,\"callee_frame_bytes\":{},\"callee_stack_bytes\":{},",
            "\"stack_argument_bytes\":8,\"span_encoding\":\"utf-8-bytes\",\"failure_spans\":[{}]}}\n"
        ), mode_name, mode.symbol(), budget, layout.frame_bytes, layout.callee_stack_bytes(), ranges);
        assert_eq!(result.stdout, expected.as_bytes());

        let duplicate = d.run(&command);
        assert_eq!(duplicate.status.code(), Some(1));
        assert!(duplicate.stdout.is_empty());
        assert_eq!(
            fs::read(d.path("report.o")).unwrap(),
            artifact.kernel.object
        );
        fs::remove_file(d.path("report.o")).unwrap();
    }
}

const ENTRY_SOURCE: &str = include_str!("../examples/native_bounded_entry.srp");

#[test]
fn bounded_entry_emits_the_verified_object_at_the_exact_stack_budget() {
    let d = Directory::new("entry");
    let source = format!("// जहाँ 😀\r\n{ENTRY_SOURCE}");
    fs::write(d.path("entry.srp"), &source).unwrap();
    let artifact =
        emit_elf64_bounded_integer_entry_with_layout(&source, "evaluate", 1_000, u32::MAX).unwrap();
    let peak = artifact.peak_stack_bytes;
    let stack = peak.to_string();
    let command = [
        "--json",
        "x86_64-none",
        "bounded-entry",
        "evaluate",
        "1000",
        &stack,
        "entry.srp",
        "-o",
        "out.o",
    ];
    let result = d.run(&command);
    assert!(result.status.success(), "{:?}", result.stderr);
    assert!(result.stderr.is_empty());
    let json = String::from_utf8(result.stdout).unwrap();
    assert!(json.contains("\"mode\":\"bounded-entry\",\"symbol\":\"scarp_bounded_integer_entry\","));
    for (field, value) in [
        ("max_work", 1_000),
        ("max_stack_bytes", peak),
        ("peak_stack_bytes", peak),
        ("max_active_frames", 3),
        ("integer_arguments", 2),
        ("call_stack_alignment", 16),
        ("callee_frame_bytes", artifact.entry_layout.frame_bytes),
        ("stack_argument_bytes", 0),
    ] {
        assert!(json.contains(&format!("\"{field}\":{value},")), "{json}");
    }
    let divisor = source.find("/ b").unwrap();
    assert!(json.contains(&format!("[{divisor},{}]", divisor + 1)));
    assert_eq!(fs::read(d.path("out.o")).unwrap(), artifact.kernel.object);
    let duplicate = d.run(&command);
    assert_eq!(duplicate.status.code(), Some(1));
    assert!(duplicate.stdout.is_empty());
    assert_eq!(fs::read(d.path("out.o")).unwrap(), artifact.kernel.object);
}

#[test]
fn bounded_entry_errors_leave_the_destination_untouched() {
    let d = Directory::new("entry-failure");
    fs::write(d.path("entry.srp"), ENTRY_SOURCE).unwrap();
    let artifact =
        emit_elf64_bounded_integer_entry_with_layout(ENTRY_SOURCE, "evaluate", 1_000, 512).unwrap();
    let too_small = (artifact.peak_stack_bytes - 1).to_string();
    for (entry, work, stack, code, diagnostic) in [
        (
            "evaluate",
            "1000",
            too_small.as_str(),
            1,
            "exceeding the caller budget",
        ),
        (
            "missing",
            "1000",
            "512",
            1,
            "entry function was not declared",
        ),
        ("evaluate", "1000", "4294967296", 2, "max-stack-bytes"),
        ("evaluate", "+1", "512", 2, "max-work"),
        ("", "1000", "512", 2, "entry function name"),
    ] {
        let result = d.run(&[
            "--json",
            "x86_64-none",
            "bounded-entry",
            entry,
            work,
            stack,
            "entry.srp",
            "-o",
            "out.o",
        ]);
        assert_eq!(result.status.code(), Some(code));
        assert!(result.stdout.is_empty());
        let stderr = String::from_utf8(result.stderr).unwrap();
        assert!(stderr.contains(diagnostic), "{stderr}");
        assert_eq!(fs::read_dir(&d.0).unwrap().count(), 1);
    }
}
