use scarp::{c_backend, diagnostics::DiagnosticKind, ExecutionLimits, RunError};
use std::{
    ffi::OsString,
    fs,
    path::PathBuf,
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "scarp-c-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn compiler() -> Option<OsString> {
    let cc = std::env::var_os("SCARP_CC").unwrap_or_else(|| {
        if cfg!(windows) {
            "clang".into()
        } else {
            "cc".into()
        }
    });
    if Command::new(&cc)
        .arg("--version")
        .output()
        .is_ok_and(|result| result.status.success())
    {
        Some(cc)
    } else {
        assert!(
            std::env::var_os("SCARP_REQUIRE_C").is_none(),
            "required C compiler unavailable"
        );
        eprintln!("C execution checks skipped: no compiler");
        None
    }
}

fn native(
    cc: &OsString,
    source: &str,
    limits: ExecutionLimits,
    opt: &str,
    fail: bool,
    repeats: usize,
) -> String {
    let directory = Directory::new();
    fs::write(
        directory.0.join("program.c"),
        c_backend::emit_c(source).unwrap(),
    )
    .unwrap();
    let harness = format!(
        r#"
#include "program.c"
#include <stdio.h>
static uint8_t bytes[65536]; static size_t used;
static int capture(void *context,const uint8_t *data,size_t count) {{
 size_t i; (void)context; if({fail} || count>sizeof bytes-used) return 1;
 for(i=0;i<count;++i) {{ bytes[used++]=data[i]; }} return 0;
}}
int main(void) {{
 static struct scarp_workspace workspace; struct scarp_host host; uint32_t code=0; size_t i;
 host.context=0; host.write=capture;
 for(i=0;i<{repeats};++i) {{code=scarp_run(&workspace,&host,UINT64_C({steps}),{prints});if(code) break;}}
 printf("%u:%llu:%llu:",(unsigned)code,(unsigned long long)workspace.error_start,(unsigned long long)workspace.error_end);
 for(i=0;i<used;++i) {{ printf("%02x",(unsigned)bytes[i]); }} return 0;
}}
"#,
        fail = usize::from(fail),
        steps = limits.max_steps,
        prints = limits.max_prints
    );
    fs::write(directory.0.join("host.c"), harness).unwrap();
    let executable = directory
        .0
        .join(if cfg!(windows) { "host.exe" } else { "host" });
    let mut command = Command::new(cc);
    command
        .current_dir(&directory.0)
        .args(["-std=c11", "-Wall", "-Wextra", "-Werror", opt]);
    if std::env::var_os("SCARP_C_SANITIZE").is_some() {
        command.args(["-fsanitize=undefined", "-fno-sanitize-recover=all"]);
    }
    let built = command
        .arg("host.c")
        .arg("-o")
        .arg(&executable)
        .output()
        .unwrap();
    assert!(
        built.status.success(),
        "{}",
        String::from_utf8_lossy(&built.stderr)
    );
    let result = Command::new(executable).output().unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    String::from_utf8(result.stdout).unwrap()
}
fn reference(source: &str, limits: ExecutionLimits, repeats: usize) -> String {
    let mut bytes = Vec::new();
    let mut code = 0;
    let (mut start, mut end) = (0, 0);
    for _ in 0..repeats {
        match scarp::run_with_limits(source, &mut bytes, limits) {
            Ok(()) => {}
            Err(RunError::Diagnostic(error)) => {
                code = error.kind.code()[1..].parse::<u32>().unwrap();
                start = error.span.start;
                end = error.span.end;
                break;
            }
            other => panic!("{other:?}"),
        }
    }
    format!(
        "{code}:{start}:{end}:{}",
        bytes
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    )
}

fn native_batch(
    cc: &OsString,
    cases: &[(&str, ExecutionLimits)],
    optimization: &str,
) -> Vec<String> {
    let directory = Directory::new();
    let mut source = String::from("#include <stdio.h>\n");
    for (index, (program, _)) in cases.iter().enumerate() {
        source.push_str(
            &c_backend::emit_c(program)
                .unwrap()
                .replace("scarp_", &format!("scarp_case{index}_")),
        );
        source.push_str(
            "\n#undef SCARP_SLOT_COUNT\n#undef SCARP_TEMP_COUNT\n#undef SCARP_OUTPUT_ERROR\n",
        );
    }
    source.push_str("static uint8_t bytes[65536]; static size_t used;\nstatic int capture(void *context,const uint8_t *data,size_t count) {size_t i; (void)context; if(count>sizeof bytes-used) {return 1;} for(i=0;i<count;++i) {bytes[used++]=data[i];} return 0;}\nint main(void) {\n");
    for (index, (_, limits)) in cases.iter().enumerate() {
        source.push_str(&format!("{{static struct scarp_case{index}_workspace w; struct scarp_case{index}_host host; uint32_t code,checked; uint64_t start,end; size_t i; host.context=0;host.write=capture;used=0;checked=scarp_case{index}_validate(&w,UINT64_C({}),{});start=w.error_start;end=w.error_end;code=scarp_case{index}_run(&w,&host,UINT64_C({}),{});if(code!=checked||start!=w.error_start||end!=w.error_end) return 2;printf(\"%u:%llu:%llu:\",(unsigned)code,(unsigned long long)w.error_start,(unsigned long long)w.error_end);for(i=0;i<used;++i)printf(\"%02x\",(unsigned)bytes[i]);printf(\"\\n\");}}\n",limits.max_steps,limits.max_prints,limits.max_steps,limits.max_prints));
    }
    source.push_str("return 0;}\n");
    fs::write(directory.0.join("host.c"), source).unwrap();
    let executable = directory
        .0
        .join(if cfg!(windows) { "host.exe" } else { "host" });
    let mut command = Command::new(cc);
    command.current_dir(&directory.0).args([
        "-std=c11",
        "-Wall",
        "-Wextra",
        "-Werror",
        optimization,
    ]);
    if std::env::var_os("SCARP_C_SANITIZE").is_some() {
        command.args(["-fsanitize=undefined", "-fno-sanitize-recover=all"]);
    }
    let built = command
        .arg("host.c")
        .arg("-o")
        .arg(&executable)
        .output()
        .unwrap();
    assert!(
        built.status.success(),
        "{}",
        String::from_utf8_lossy(&built.stderr)
    );
    let run = Command::new(executable).output().unwrap();
    assert!(
        run.status.success(),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8(run.stdout)
        .unwrap()
        .lines()
        .map(str::to_owned)
        .collect()
}

#[test]
fn scalar_preflight_keeps_exact_c_workspace_and_typed_plan_parity() {
    let source = "var count=7; print(count+1); print(\"é\");";
    let typed = scarp::compile(source).unwrap();
    let emitted = c_backend::emit_c(source).unwrap();
    assert_eq!(emitted, c_backend::emit_program(&typed).unwrap());
    assert!(emitted.contains("#define SCARP_SLOT_COUNT 1\\n#define SCARP_TEMP_COUNT 3\\n"));
    assert!(emitted.contains("static const uint8_t text[]={195,169,0};"));

    let no_bindings = c_backend::emit_c("print(true);").unwrap();
    assert!(no_bindings.contains("#define SCARP_SLOT_COUNT 1\\n#define SCARP_TEMP_COUNT 1\\n"));

    // Structural acceptance is unchanged for the experimental scalar subset.
    for source in [
        "print(1.5);",
        "print();",
        "eprint(1);",
        "write(1);",
        "print(1, 2);",
    ] {
        assert_eq!(
            c_backend::emit_c(source).unwrap_err().kind,
            DiagnosticKind::NativeUnsupported,
            "{source}"
        );
    }
}

#[test]
fn unsupported_modern_features_are_rejected_before_generating_c() {
    use DiagnosticKind::NativeUnsupported;
    for source in [
        "bind x = 1.0;",
        "bind x = \"text\";",
        "print(1.0);",
        "print(1, 2);",
        "write(1);",
        "eprint(1);",
        "print(to_float(1));",
        "funcra answer() -> integer { return 42; } print(1);",
        "print([1.0]);",
    ] {
        assert_eq!(
            c_backend::emit_c(source).unwrap_err().kind,
            NativeUnsupported,
            "{source}"
        );
    }
    for source in [
        format!("print(0{});", "+1".repeat(1024)),
        "print(1);".repeat(4097),
        (0..257)
            .map(|index| format!("bind x{index}=1;"))
            .collect::<String>(),
        format!("print(\"{}\");", "x".repeat(65537)),
    ] {
        assert_eq!(
            c_backend::emit_c(&source).unwrap_err().kind,
            NativeUnsupported
        );
    }

    assert_eq!(
        c_backend::emit_c("print(missing);").unwrap_err().kind,
        DiagnosticKind::UnknownName
    );
    assert!(c_backend::emit_c("print(1 / 0);").is_ok());
    assert!(c_backend::emit_c(&format!("print(0{});", "+1".repeat(500))).is_ok());
}

#[test]
fn emitted_scalar_c_matches_interpreter_outputs_errors_spans_and_work_budgets() {
    let Some(cc) = compiler() else { return };
    let defaults = ExecutionLimits::default();
    let cases = [
        ("", defaults),
        ("print(\"Hello 🌍\\nA\"); print(true); print(false);", defaults),
        ("print(-9223372036854775808); print(9223372036854775807); print(-9223372036854775808 * 1);", defaults),
        ("print(-7 / 2); print(-7 % 2); print(-1 >>> 1); print(-8 >> 1); print(1 << 63); print(~0);", defaults),
        ("var x = 3; { bind y = x + 1; print(y); } x *= 7; x -= 2; x /= 3; x %= 5; x |= 8; x &= 15; x ^= 3; x <<= 2; x >>= 1; x >>>= 1; print(x);", defaults),
        ("var x = 0; while x < 4 { x += 1; if x == 2 { continue; } elif x == 4 { break; } else { print(x); } }", defaults),
        ("print(false and (1 / 0 == 0)); print(true or (1 << 64 == 0)); print(not (1 < 2));", defaults),
        ("print(1); print(9223372036854775807 + 1);", defaults),
        ("print(-9223372036854775808 - 1);", defaults),
        ("print(-9223372036854775808 * -1);", defaults),
        ("print(-(-9223372036854775808));", defaults),
        ("print(1); print(2 / 0);", defaults),
        ("print(-9223372036854775808 % -1);", defaults),
        ("var x = 1; x >>>= 64;", defaults),
        ("print(1 << -1);", defaults),
        ("print(1); while true { continue; }", ExecutionLimits { max_steps: 40, max_prints: 10 }),
        ("print(\"first\"); print(2);", ExecutionLimits { max_steps: 100, max_prints: 1 }),
        ("print(1+2*3);", ExecutionLimits { max_steps: 5, max_prints: 1 }),
        ("print(\"x\");", ExecutionLimits { max_steps: 1, max_prints: 1 }),
        ("print(\"\"); print(\"नमस्ते 🌍\");", defaults),
        ("print(1 == 1); print(2 != 3); print(true == false);", defaults),
        ("var x = 1; { var x = 2; print(x); } print(x);", defaults),
        ("var x = 0; while x < 5 { x += 1; if x % 2 == 0 { print(x); } }", defaults),
        ("var x = 2; while x > 0 { x -= 1; continue; } print(x);", defaults),
        ("if false { print(1 / 0); } else { print(42); }", defaults),
        ("print(false and (9223372036854775807 + 1 > 0));", defaults),
        ("print(true or (1 / 0 == 1));", defaults),
        ("print(1 << 64);", defaults),
        ("print(1 >> -1);", defaults),
        ("print(1); print(2);", ExecutionLimits { max_steps: 100, max_prints: 0 }),
        ("var x = 0; while x < 3 { x += 1; } print(x);", ExecutionLimits { max_steps: 3, max_prints: 1 }),
        ("repeat 0 as i { print(1/0); } print(42);", defaults),
        ("repeat 1 as i { print(i); } repeat 4 as j { print(j); }", defaults),
        ("var total=0; repeat 5 as i { total+=i; } print(total);", defaults),
        ("var n=4; repeat n+1 as index { n=0; print(index); } print(n);", defaults),
        ("bind n=3; repeat n as n { print(n); } print(n);", defaults),
        ("repeat 4 as i { repeat i as j { print(i*10+j); } }", defaults),
        ("repeat 4 as i { if i==1 { continue; } if i==3 { break; } print(i); }", defaults),
        ("repeat 3 as i { repeat 5 as j { if j==1 { continue; } if j==3 { break; } print(i*10+j); } }", defaults),
        ("var x=0; while x<3 { x+=1; repeat 4 as i { if i==1 { continue; } if i==3 { break; } print(x*10+i); } }", defaults),
        ("repeat 3 as i { var j=0; while true { j+=1; if j==1 {continue;} if j==3 {break;} print(i*10+j); } }", defaults),
        ("print(1); repeat -(1+2) as i {}", defaults),
        ("repeat -9223372036854775808 as i {}", defaults),
        ("repeat 9223372036854775807 as i {}", ExecutionLimits { max_steps: 100, max_prints: 10 }),
        ("repeat 9223372036854775807 as i { print(i); break; }", defaults),
        ("repeat 999999999 as i { repeat 999999999 as j {} }", ExecutionLimits { max_steps: 100, max_prints: 10 }),
        ("print(3); repeat 3 as i { print(i); }", ExecutionLimits { max_steps: 10, max_prints: 10 }),
        ("repeat 2 as i { print(i); }", ExecutionLimits { max_steps: 100, max_prints: 1 }),
        (include_str!("../examples/native_scalar_repeat.srp"), defaults),
        ("", ExecutionLimits { max_steps: 0, max_prints: 0 }),
    ];
    for optimization in ["-O0", "-O2"] {
        let results = native_batch(&cc, &cases, optimization);
        assert_eq!(results.len(), cases.len());
        for ((source, limits), actual) in cases.iter().zip(results) {
            assert_eq!(
                actual,
                reference(source, *limits, 1),
                "{optimization}: {source}"
            );
        }
    }
}

#[test]
fn native_validation_precedes_io_and_scalar_retirement_does_not_consume_work() {
    let Some(cc) = compiler() else { return };
    let limits = ExecutionLimits::default();
    let source = "var x = 0; while x < 3 { { bind y = x; print(y); } x += 1; }";
    assert_eq!(
        native(&cc, source, limits, "-O2", false, 2),
        reference(source, limits, 2)
    );
    assert!(native(&cc, "print(1);", limits, "-O2", true, 1).starts_with("65535:"));
    let invalid = "print(1); print(1 / 0);";
    assert_eq!(
        native(&cc, invalid, limits, "-O2", true, 1),
        reference(invalid, limits, 1)
    );
}

#[test]
fn empty_string_output_never_calls_host_with_zero_bytes() {
    let Some(cc) = compiler() else { return };
    let directory = Directory::new();
    fs::write(
        directory.0.join("program.c"),
        c_backend::emit_c(r#"print(""); print("x");"#).unwrap(),
    )
    .unwrap();
    let host = r#"
#include "program.c"
#include <stdio.h>
#include <inttypes.h>
static int calls = 0;
static size_t total_bytes = 0;
static int write_bytes(void *ctx, const uint8_t *ptr, size_t count) {
    (void)ctx; (void)ptr;
    if (count == 0) return 1;
    ++calls;
    total_bytes += count;
    return 0;
}
int main(void) {
    struct scarp_workspace workspace;
    struct scarp_host host = {0, write_bytes};
    unsigned result = scarp_run(&workspace, &host, UINT64_C(10000), 20);
    printf("%u:%d:%" PRIuMAX "\n", result, calls, (uintmax_t)total_bytes);
    return 0;
}
"#;
    fs::write(directory.0.join("host.c"), host).unwrap();
    let executable = directory
        .0
        .join(if cfg!(windows) { "host.exe" } else { "host" });
    let built = Command::new(cc)
        .current_dir(&directory.0)
        .args([
            "-std=c11", "-Wall", "-Wextra", "-Werror", "-O2", "host.c", "-o",
        ])
        .arg(&executable)
        .output()
        .unwrap();
    assert!(
        built.status.success(),
        "{}",
        String::from_utf8_lossy(&built.stderr)
    );
    let run = Command::new(executable).output().unwrap();
    assert!(run.status.success(), "{run:?}");
    // LF from print(""); followed by "x" and LF: three nonempty writes.
    assert_eq!(String::from_utf8(run.stdout).unwrap().trim_end(), "0:3:3");
}

#[test]
fn freestanding_scalar_object_has_no_undefined_symbols_when_requested() {
    if std::env::var_os("SCARP_FREESTANDING").is_none() {
        return;
    }
    let cc = compiler().expect("freestanding compiler required");
    let directory = Directory::new();
    fs::write(
        directory.0.join("program.c"),
        c_backend::emit_c("print(0xFF & 7);").unwrap(),
    )
    .unwrap();
    for target in ["x86_64-unknown-none-elf", "aarch64-unknown-none-elf"] {
        let object = directory.0.join(format!("{target}.o"));
        let built = Command::new(&cc)
            .args([
                "-target",
                target,
                "-std=c11",
                "-ffreestanding",
                "-fno-builtin",
                "-fno-stack-protector",
                "-O2",
                "-c",
            ])
            .arg(directory.0.join("program.c"))
            .arg("-o")
            .arg(&object)
            .output()
            .unwrap();
        assert!(
            built.status.success(),
            "{}",
            String::from_utf8_lossy(&built.stderr)
        );
        let symbols = Command::new("readelf")
            .args(["--wide", "--symbols"])
            .arg(&object)
            .output()
            .unwrap();
        assert!(symbols.status.success());
        let inventory = String::from_utf8(symbols.stdout).unwrap();
        // ELF symtab entry zero is the mandatory undefined *null* symbol.
        // Any other UND entry would require a link-time external dependency.
        let unresolved: Vec<_> = inventory
            .lines()
            .filter(|line| {
                let fields: Vec<_> = line.split_whitespace().collect();
                fields.len() >= 7 && fields[0] != "0:" && fields[6] == "UND"
            })
            .collect();
        assert!(unresolved.is_empty(), "{target}: {unresolved:?}");
        let bytes = fs::read(object).unwrap();
        assert_eq!(&bytes[..4], b"\x7fELF");
    }
}
