//! Explicit file-to-native-artifact frontend.
use super::{read_frontend_source, report_diagnostic, write_output};
use scarp::diagnostics::Diagnostic;
use scarp::native_object::{
    emit_elf64_bounded_integer_entry_with_layout, emit_elf64_linux_executable,
    emit_freestanding_function, emit_pe32plus_uefi_application, emit_pe32plus_uefi_kernel,
    write_elf64_linux_executable_new, write_elf64_object_new, write_pe32plus_uefi_application_new,
    CheckedElf64Kernel, FreestandingCallLayout, FreestandingFunctionMode, FreestandingTarget,
    UefiKernelError,
};
use std::{
    ffi::{OsStr, OsString},
    io::{self, Write},
    path::Path,
};

pub(super) const HELP: &str = "Usage:
  scarp native-object [--json] x86_64-none checked <source.srp> -o <new-object>
  scarp native-object [--json] x86_64-none bounded <max-work> <source.srp> -o <new-object>
  scarp native-object [--json] x86_64-none bounded-entry <function> <max-work> <max-stack-bytes> <source.srp> -o <new-object>

Compile pure integer functions to a relocatable x86-64 ELF64 object.
Checked mode accepts forward scalar control flow. Bounded modes also accept
while and counted repeat loops. Choose bounded-entry for nonrecursive direct
calls within a source file that contains only function declarations.
Budgets are decimal unsigned 32-bit integers. The stack budget is measured
below the aligned caller stack pointer immediately before CALL.
The -o destination must be new and its parent directory must exist.
--json prints ABI, stack and failure-span metadata after writing the object.
";

pub(super) const EXECUTABLE_HELP: &str = "Usage:
  scarp native-exe linux-x86_64 <max-work> <max-stack-bytes> <source.srp> -o <new-executable>
  scarp native-exe uefi-x86_64 <entry> <max-work> <max-stack-bytes> <source.srp> -o <new-app.efi>
  scarp native-exe uefi-kernel-x86_64 <loader-entry> <kernel-entry> <max-work> <max-stack-bytes> <loader.srp> <kernel.srp> -o <new-kernel.efi>

The Linux ELF64 target uses a parameterless pure funcra main() -> integer.
Results 0 through 125 become process exit statuses, other results yield 126,
and checked runtime failures yield 128 plus their native status.

The UEFI PE32+ target takes a named parameterless funcra returning integer.
Zero maps to EFI_SUCCESS. Nonzero and checked failures map to EFI_ABORTED.
UEFI supports literal print lines and x86_pause() in impure functions.
The UEFI kernel target compiles separate loader and kernel source files.
A zero loader result enters the kernel after ExitBootServices. Each stage
gets its own work allowance. All targets support bounded control flow and
nonrecursive direct calls.
";

pub(super) fn run_executable(
    args: &[OsString],
    output: &mut impl Write,
    errors: &mut impl Write,
) -> u8 {
    if matches!(args, [flag] if flag == "--help" || flag == "-h") {
        return write_output(output.write_all(EXECUTABLE_HELP.as_bytes()), errors);
    }
    if let [target, rest @ ..] = args {
        if target == "uefi-kernel-x86_64" {
            return run_kernel(rest, errors);
        }
    }
    let (entry, work, stack, source, destination) = match args {
        [target, work, stack, source, flag, destination]
            if target == "linux-x86_64" && flag == "-o" =>
        {
            (None, work, stack, source, destination)
        }
        [target, entry, work, stack, source, flag, destination]
            if target == "uefi-x86_64" && flag == "-o" =>
        {
            let Some(entry) = entry.to_str().filter(|entry| !entry.is_empty()) else {
                let _ = writeln!(errors, "error: entry function name must be nonempty UTF-8");
                return 2;
            };
            (Some(entry), work, stack, source, destination)
        }
        _ => {
            let _ = write!(errors, "{EXECUTABLE_HELP}");
            return 2;
        }
    };
    let Some(work) = parse_budget(work, "max-work", errors) else {
        return 2;
    };
    let Some(stack) = parse_budget(stack, "max-stack-bytes", errors) else {
        return 2;
    };
    let source = Path::new(source);
    let Some(text) = executable_source(source, errors) else {
        return 1;
    };
    let image = match entry {
        Some(entry) => {
            emit_pe32plus_uefi_application(&text, entry, work, stack).map(|app| app.image)
        }
        None => emit_elf64_linux_executable(&text, work, stack).map(|exe| exe.image),
    };
    let image = match image {
        Ok(image) => image,
        Err(error) => return report_diagnostic(&error, source, &text, errors, false),
    };
    let destination = Path::new(destination);
    let written = if entry.is_some() {
        write_pe32plus_uefi_application_new(destination, &image)
    } else {
        write_elf64_linux_executable_new(destination, &image)
    };
    executable_written(written, destination, errors)
}

fn run_kernel(args: &[OsString], errors: &mut impl Write) -> u8 {
    let [loader_entry, kernel_entry, work, stack, loader, kernel, flag, output] = args else {
        let _ = write!(errors, "{EXECUTABLE_HELP}");
        return 2;
    };
    if flag != "-o" {
        let _ = write!(errors, "{EXECUTABLE_HELP}");
        return 2;
    }
    let (Some(loader_entry), Some(kernel_entry)) = (
        loader_entry.to_str().filter(|entry| !entry.is_empty()),
        kernel_entry.to_str().filter(|entry| !entry.is_empty()),
    ) else {
        let _ = writeln!(errors, "error: entry function names must be nonempty UTF-8");
        return 2;
    };
    let Some(work) = parse_budget(work, "max-work", errors) else {
        return 2;
    };
    let Some(stack) = parse_budget(stack, "max-stack-bytes", errors) else {
        return 2;
    };
    let (loader, kernel) = (Path::new(loader), Path::new(kernel));
    let Some(loader_text) = executable_source(loader, errors) else {
        return 1;
    };
    let Some(kernel_text) = executable_source(kernel, errors) else {
        return 1;
    };
    let artifact = match emit_pe32plus_uefi_kernel(
        &loader_text,
        loader_entry,
        &kernel_text,
        kernel_entry,
        work,
        stack,
    ) {
        Ok(artifact) => artifact,
        Err(UefiKernelError::Loader(error) | UefiKernelError::Image(error)) => {
            return report_diagnostic(&error, loader, &loader_text, errors, false);
        }
        Err(UefiKernelError::Kernel(error)) => {
            return report_diagnostic(&error, kernel, &kernel_text, errors, false);
        }
    };
    let destination = Path::new(output);
    executable_written(
        write_pe32plus_uefi_application_new(destination, &artifact.image),
        destination,
        errors,
    )
}

fn executable_source(path: &Path, errors: &mut impl Write) -> Option<String> {
    match read_frontend_source(path) {
        Ok(text) => Some(text),
        Err(error) => {
            let _ = writeln!(errors, "error: could not read {}: {error}", path.display());
            None
        }
    }
}

fn executable_written(written: io::Result<()>, destination: &Path, errors: &mut impl Write) -> u8 {
    match written {
        Ok(()) => 0,
        Err(error) => {
            let _ = writeln!(
                errors,
                "error: could not write {}: {error}",
                destination.display()
            );
            1
        }
    }
}

enum Request<'a> {
    Single(FreestandingFunctionMode),
    Entry {
        name: &'a str,
        max_work: u32,
        max_stack_bytes: u32,
    },
}

struct ObjectOutput {
    kernel: CheckedElf64Kernel,
    layout: FreestandingCallLayout,
    mode: &'static str,
    symbol: &'static str,
    max_work: Option<u32>,
    entry_stack: Option<EntryStack>,
}

struct EntryStack {
    max_stack_bytes: u32,
    peak_stack_bytes: u32,
    max_active_frames: usize,
}

pub(super) fn run(args: &[OsString], output: &mut impl Write, errors: &mut impl Write) -> u8 {
    if matches!(args, [flag] if flag == "--help" || flag == "-h") {
        return write_output(output.write_all(HELP.as_bytes()), errors);
    }
    let (json, args) = match args {
        [flag, rest @ ..] if flag == "--json" => (true, rest),
        _ => (false, args),
    };
    let (target, request, source, destination) = match args {
        [target, mode, source, flag, destination] if mode == "checked" && flag == "-o" => (
            target,
            Request::Single(FreestandingFunctionMode::Checked),
            source,
            destination,
        ),
        [target, mode, work, source, flag, destination] if mode == "bounded" && flag == "-o" => {
            let Some(max_work) = parse_budget(work, "max-work", errors) else {
                return 2;
            };
            (
                target,
                Request::Single(FreestandingFunctionMode::Bounded { max_work }),
                source,
                destination,
            )
        }
        [target, mode, entry, work, stack, source, flag, destination]
            if mode == "bounded-entry" && flag == "-o" =>
        {
            let Some(name) = entry.to_str().filter(|name| !name.is_empty()) else {
                let _ = writeln!(errors, "error: entry function name must be nonempty UTF-8");
                return 2;
            };
            let Some(max_work) = parse_budget(work, "max-work", errors) else {
                return 2;
            };
            let Some(max_stack_bytes) = parse_budget(stack, "max-stack-bytes", errors) else {
                return 2;
            };
            (
                target,
                Request::Entry {
                    name,
                    max_work,
                    max_stack_bytes,
                },
                source,
                destination,
            )
        }
        _ => {
            let _ = write!(errors, "{HELP}");
            return 2;
        }
    };
    let Some(target) = target
        .to_str()
        .and_then(|s| s.parse::<FreestandingTarget>().ok())
    else {
        let _ = writeln!(
            errors,
            "error: unsupported native object target; expected x86_64-none"
        );
        return 2;
    };
    let input = Path::new(source);
    let text = match read_frontend_source(input) {
        Ok(text) => text,
        Err(error) => {
            let _ = writeln!(errors, "error: could not read {}: {error}", input.display());
            return 1;
        }
    };
    let artifact = match compile_object(&text, target, request) {
        Ok(artifact) => artifact,
        Err(error) => return report_diagnostic(&error, input, &text, errors, false),
    };
    let path = Path::new(destination);
    match write_elf64_object_new(path, &artifact.kernel.object) {
        Ok(()) if json => write_output(write_metadata(target, &artifact, output), errors),
        Ok(()) => 0,
        Err(error) => {
            let _ = writeln!(errors, "error: could not write {}: {error}", path.display());
            1
        }
    }
}

fn parse_budget(value: &OsStr, label: &str, errors: &mut impl Write) -> Option<u32> {
    let parsed = value
        .to_str()
        .filter(|value| !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()))
        .and_then(|value| value.parse().ok());
    if parsed.is_none() {
        let _ = writeln!(
            errors,
            "error: {label} must be a decimal integer from 0 through 4294967295"
        );
    }
    parsed
}

fn compile_object(
    source: &str,
    target: FreestandingTarget,
    request: Request<'_>,
) -> Result<ObjectOutput, Diagnostic> {
    match request {
        Request::Single(mode) => {
            let artifact = emit_freestanding_function(source, target, mode)?;
            let (mode_name, max_work) = match mode {
                FreestandingFunctionMode::Checked => ("checked", None),
                FreestandingFunctionMode::Bounded { max_work } => ("bounded", Some(max_work)),
            };
            Ok(ObjectOutput {
                kernel: artifact.kernel,
                layout: artifact.layout,
                mode: mode_name,
                symbol: mode.symbol(),
                max_work,
                entry_stack: None,
            })
        }
        Request::Entry {
            name,
            max_work,
            max_stack_bytes,
        } => match target {
            FreestandingTarget::X86_64None => {
                let artifact = emit_elf64_bounded_integer_entry_with_layout(
                    source,
                    name,
                    max_work,
                    max_stack_bytes,
                )?;
                Ok(ObjectOutput {
                    kernel: artifact.kernel,
                    layout: artifact.entry_layout,
                    mode: "bounded-entry",
                    symbol: "scarp_bounded_integer_entry",
                    max_work: Some(max_work),
                    entry_stack: Some(EntryStack {
                        max_stack_bytes,
                        peak_stack_bytes: artifact.peak_stack_bytes,
                        max_active_frames: artifact.max_active_frames,
                    }),
                })
            }
        },
    }
}

fn write_metadata(
    target: FreestandingTarget,
    artifact: &ObjectOutput,
    out: &mut impl Write,
) -> io::Result<()> {
    let layout = artifact.layout;
    // Target, mode and symbol names are fixed ASCII strings supplied by the
    // emitter. The report contains no caller-controlled path or source text.
    write!(
        out,
        "{{\"schema_version\":1,\"target\":\"{}\",\"mode\":\"{}\",\"symbol\":\"{}\",\"max_work\":",
        target.name(),
        artifact.mode,
        artifact.symbol,
    )?;
    match artifact.max_work {
        None => out.write_all(b"null")?,
        Some(max_work) => write!(out, "{max_work}")?,
    }
    write!(
        out,
        concat!(
            ",\"integer_arguments\":{},\"call_stack_alignment\":{},",
            "\"callee_frame_bytes\":{},\"callee_stack_bytes\":{},\"stack_argument_bytes\":{},"
        ),
        layout.integer_arguments,
        target.call_stack_alignment(),
        layout.frame_bytes,
        layout.callee_stack_bytes(),
        layout.stack_argument_bytes(),
    )?;
    if let Some(stack) = &artifact.entry_stack {
        write!(
            out,
            "\"max_stack_bytes\":{},\"peak_stack_bytes\":{},\"max_active_frames\":{},",
            stack.max_stack_bytes, stack.peak_stack_bytes, stack.max_active_frames,
        )?;
    }
    out.write_all(b"\"span_encoding\":\"utf-8-bytes\",\"failure_spans\":[")?;
    for (index, span) in artifact.kernel.failure_spans.iter().enumerate() {
        if index != 0 {
            out.write_all(b",")?;
        }
        write!(out, "[{},{}]", span.start, span.end)?;
    }
    out.write_all(b"]}\n")
}
