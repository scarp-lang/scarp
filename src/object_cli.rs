//! Explicit file-to-object frontend for a single freestanding function.
use super::{read_frontend_source, report_diagnostic, write_output};
use scarp::native_object::{
    emit_freestanding_function, write_elf64_object_new, FreestandingFunctionMode,
    FreestandingTarget,
};
use std::{ffi::OsString, io::Write, path::Path};

pub(super) const HELP: &str = "Usage:
  scarp native-object x86_64-none checked <source.srp> <new-object>
  scarp native-object x86_64-none bounded <max-work> <source.srp> <new-object>

Compile one pure integer function to a relocatable x86-64 ELF64 object.
Checked mode accepts forward scalar control flow. Bounded mode also accepts
while loops and requires an unsigned decimal work allowance (0..4294967295).
The destination must be new and its parent directory must exist.
";

pub(super) fn run(args: &[OsString], output: &mut impl Write, errors: &mut impl Write) -> u8 {
    if matches!(args, [flag] if flag == "--help" || flag == "-h") {
        return write_output(output.write_all(HELP.as_bytes()), errors);
    }
    let (target, mode, source, destination) = match args {
        [target, mode, source, destination] if mode == "checked" => (
            target,
            FreestandingFunctionMode::Checked,
            source,
            destination,
        ),
        [target, mode, work, source, destination] if mode == "bounded" => {
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
        Ok(()) => 0,
        Err(error) => {
            let _ = writeln!(errors, "error: could not write {}: {error}", path.display());
            1
        }
    }
}
