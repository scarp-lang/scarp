//! Explicit file-to-object frontend for a single freestanding function.
use super::{read_frontend_source, report_diagnostic, write_output};
use scarp::native_object::{
    emit_freestanding_function, write_elf64_object_new, FreestandingFunction,
    FreestandingFunctionMode, FreestandingTarget,
};
use std::{
    ffi::OsString,
    io::{self, Write},
    path::Path,
};

pub(super) const HELP: &str = "Usage:
  scarp native-object [--json] x86_64-none checked <source.srp> -o <new-object>
  scarp native-object [--json] x86_64-none bounded <max-work> <source.srp> -o <new-object>

Compile one pure integer function to a relocatable x86-64 ELF64 object.
Checked mode accepts forward scalar control flow. Bounded mode also accepts
while and counted repeat loops and requires a decimal work allowance (0..4294967295).
The -o destination must be new and its parent directory must exist.
--json prints ABI, stack and failure-span metadata after writing the object.
";

pub(super) fn run(args: &[OsString], output: &mut impl Write, errors: &mut impl Write) -> u8 {
    if matches!(args, [flag] if flag == "--help" || flag == "-h") {
        return write_output(output.write_all(HELP.as_bytes()), errors);
    }
    let (json, args) = match args {
        [flag, rest @ ..] if flag == "--json" => (true, rest),
        _ => (false, args),
    };
    let (target, mode, source, destination) = match args {
        [target, mode, source, flag, destination] if mode == "checked" && flag == "-o" => (
            target,
            FreestandingFunctionMode::Checked,
            source,
            destination,
        ),
        [target, mode, work, source, flag, destination] if mode == "bounded" && flag == "-o" => {
            let work = work
                .to_str()
                .filter(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()));
            let Some(max_work) = work.and_then(|s| s.parse::<u32>().ok()) else {
                let _ = writeln!(
                    errors,
                    "error: max-work must be a decimal integer from 0 through 4294967295"
                );
                return 2;
            };
            (
                target,
                FreestandingFunctionMode::Bounded { max_work },
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
    let artifact = match emit_freestanding_function(&text, target, mode) {
        Ok(artifact) => artifact,
        Err(error) => return report_diagnostic(&error, input, &text, errors, false),
    };
    let path = Path::new(destination);
    match write_elf64_object_new(path, &artifact.kernel.object) {
        Ok(()) if json => write_output(write_metadata(&artifact, output), errors),
        Ok(()) => 0,
        Err(error) => {
            let _ = writeln!(errors, "error: could not write {}: {error}", path.display());
            1
        }
    }
}

fn write_metadata(artifact: &FreestandingFunction, out: &mut impl Write) -> io::Result<()> {
    let mode = artifact.mode;
    let layout = artifact.layout;
    let mode_name = match mode {
        FreestandingFunctionMode::Checked => "checked",
        FreestandingFunctionMode::Bounded { .. } => "bounded",
    };
    // Target, mode and symbol names are fixed ASCII strings supplied by the
    // emitter. The report contains no caller-controlled path or source text.
    write!(
        out,
        "{{\"schema_version\":1,\"target\":\"{}\",\"mode\":\"{mode_name}\",\"symbol\":\"{}\",\"max_work\":",
        artifact.target.name(),
        mode.symbol(),
    )?;
    match mode {
        FreestandingFunctionMode::Checked => out.write_all(b"null")?,
        FreestandingFunctionMode::Bounded { max_work } => write!(out, "{max_work}")?,
    }
    write!(
        out,
        concat!(
            ",\"integer_arguments\":{},\"call_stack_alignment\":{},",
            "\"callee_frame_bytes\":{},\"callee_stack_bytes\":{},\"stack_argument_bytes\":{},",
            "\"span_encoding\":\"utf-8-bytes\",\"failure_spans\":["
        ),
        layout.integer_arguments,
        artifact.target.call_stack_alignment(),
        layout.frame_bytes,
        layout.callee_stack_bytes(),
        layout.stack_argument_bytes(),
    )?;
    for (index, span) in artifact.kernel.failure_spans.iter().enumerate() {
        if index != 0 {
            out.write_all(b",")?;
        }
        write!(out, "[{},{}]", span.start, span.end)?;
    }
    out.write_all(b"]}\n")
}
