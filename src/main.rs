use std::{
    env,
    ffi::{OsStr, OsString},
    fs,
    io::{self, BufWriter, Write},
    path::Path,
    process::ExitCode,
};

use scarp::{
    diagnostics::{Diagnostic, LocationTracker},
    lexer,
    modules::{self, ModuleLimits, ModuleRunError},
    ExecutionOptions,
};

const HELP: &str = "Scarp — Build Beyond Boundaries.

Usage:
  scarp run <file.srp>       Execute a Scarp program
  scarp check <file.srp>     Validate syntax, names, and arithmetic without printing
  scarp tokens <file.srp>    Inspect lexer tokens and source locations
  scarp modules [--json] <file.srp> Inspect checked module metadata
  scarp explain E####       Explain a diagnostic code without loading source
  scarp --help               Show this help
  scarp --version            Show the prototype version

Use -- before a filename that begins with a dash.
File commands accept --diagnostic-format=json before the path for schema-1 errors.
run/check options before the path: --max-steps, --max-prints, --max-call-depth,
--max-calls, --max-live-slots, --max-capture-bytes, --max-vector-bytes, --max-text-bytes (counts).
Defaults: 1000000 execution steps, 100000 selected output statements.
Iteration: while condition { ... }, repeat count as index { ... }.
Use scarp run --help or scarp check --help for budget examples.
";

const RUN_HELP: &str =
    "Usage: scarp run [--diagnostic-format=json|text] [budget options] [--] <file.srp>

Execute checked source, preparing every selected output statement before writing either stream.
while guards are boolean; repeat count as index saves one nonnegative integer count.
break; exits the nearest loop; continue; rechecks while or advances repeat.
Defaults: --max-steps 1000000 and --max-prints 100000. Options precede the path.
Function defaults: --max-call-depth 256, --max-calls 100000,
--max-live-slots 1000000, --max-capture-bytes 16777216,
--max-vector-bytes 8388608 (cumulative numerical element payload).
Vector and tile buffers share this cumulative quota; retirement does not refund it.
--max-text-bytes 8388608 limits cumulative new UTF-8 payload (1048576 bytes per string).
String sharing and direct literal output do not allocate text payload; retirement gives no refund.
Snapshot bytes are a logical storage cap, not an exact process-memory limit.
Counts are unsigned decimal digits, zero is valid, and each option may occur once.
Example: scarp run --max-steps 2000000 --max-prints 10 examples/repeat.srp
Use scarp check with the same options to evaluate without printing.
--diagnostic-format=json writes schema-1 errors to stderr, with local UTF-8 byte spans
and one-based Unicode scalar columns. Program output and error status stay unchanged.
Imports use an explicit alias and resolve relative to each file within the entry directory.
";

const CHECK_HELP: &str =
    "Usage: scarp check [--diagnostic-format=json|text] [budget options] [--] <file.srp>

Check syntax, names, and types, then evaluate the selected path without printing.
Loops and arithmetic execute with fresh storage and the same limits as run.
This evaluates code; it is not a static-only check. All bodies are type-checked.
Defaults: --max-steps 1000000 and --max-prints 100000. Options precede the path.
Function defaults: --max-call-depth 256, --max-calls 100000,
--max-live-slots 1000000, --max-capture-bytes 16777216,
--max-vector-bytes 8388608 (cumulative numerical element payload).
Vector and tile buffers share this cumulative quota; retirement does not refund it.
--max-text-bytes 8388608 limits cumulative new UTF-8 payload (1048576 bytes per string).
String sharing and direct literal output do not allocate text payload; retirement gives no refund.
All eight options take unsigned decimal counts, allow zero, and may occur once.
Snapshot bytes are a logical storage cap, including when check discards output.
Example: scarp check --max-steps 2000000 examples/repeat.srp
--diagnostic-format=json writes schema-1 errors to stderr; the default is text.
Use tokens for lexing without executing a program. No execution-limit flags on tokens.
All imported modules are loaded and checked before evaluation; see docs/modules.md.
";

const TOKENS_HELP: &str = "Usage: scarp tokens [--diagnostic-format=json|text] [--] <file.srp>

List tokens and UTF-8 byte/source locations without parsing or executing loops.
No execution-limit options are accepted. Use scarp check to evaluate source.
Import declarations are tokenized without loading their target files.
--diagnostic-format=json writes schema-1 errors to stderr; successful tokens stay unchanged.
";

const MODULES_HELP: &str =
    "Usage: scarp modules [--json] [--diagnostic-format=json|text] [--] <file.srp>

Load and statically check the module graph, then list files, imports, dependencies
and public/private declarations with typed function signatures and source locations.
No runtime statements, defaults or contracts execute. Scalar module constants are
evaluated during compilation with separate work limits. No runtime-budget options.
Uses the same trusted source root and default module-loading limits as run/check.
Filenames are escaped; dependency labels use the loader's canonical paths.
--diagnostic-format=json writes schema-1 errors to stderr; --json controls successful metadata.
--json emits schema-version-1 JSON with file-local UTF-8 byte spans.
Example: scarp modules examples/module_inspection.srp
";

const EXPLAIN_HELP: &str = "Usage: scarp explain E####

Explain an exact diagnostic code and its repairs without opening source files.
Example: scarp explain E0044
";

fn main() -> ExitCode {
    let args: Vec<_> = env::args_os().skip(1).collect();
    let json_diagnostics = diagnostic_format_hint(&args);
    let stdout = io::stdout();
    let stderr = io::stderr();
    let mut output = BufWriter::new(stdout.lock());
    let mut errors = stderr.lock();
    let status = dispatch(args, &mut output, &mut errors);
    if let Err(error) = output.flush() {
        return output_error(error, &mut errors, json_diagnostics);
    }
    if let Err(error) = errors.flush() {
        return output_error(error, &mut errors, json_diagnostics);
    }
    ExitCode::from(status)
}

fn dispatch(args: Vec<OsString>, output: &mut impl Write, errors: &mut impl Write) -> u8 {
    let format_hint = diagnostic_format_hint(&args);
    if args.first().is_some_and(|command| command == "explain") {
        return match args.as_slice() {
            [_, flag] if flag == "--help" || flag == "-h" => {
                write_output(output.write_all(EXPLAIN_HELP.as_bytes()), errors)
            }
            [_, code] => match code
                .to_str()
                .and_then(scarp::diagnostics::DiagnosticKind::from_code)
            {
                Some(kind) => write_output(
                    writeln!(output, "{}: {}", kind.code(), kind.explanation()),
                    errors,
                ),
                None => {
                    let _ = writeln!(
                        errors,
                        "error: unknown diagnostic code {:?}\n\n{EXPLAIN_HELP}",
                        code.to_string_lossy()
                    );
                    2
                }
            },
            _ => {
                let _ = writeln!(
                    errors,
                    "error: expected one diagnostic code\n\n{EXPLAIN_HELP}"
                );
                2
            }
        };
    }
    match args.as_slice() {
        [flag] if flag == "--help" || flag == "-h" => {
            return write_output(output.write_all(HELP.as_bytes()), errors);
        }
        [command, flag] if file_command(command) && (flag == "--help" || flag == "-h") => {
            let help = match command.to_str() {
                Some("run") => RUN_HELP,
                Some("check") => CHECK_HELP,
                Some("modules") => MODULES_HELP,
                _ => TOKENS_HELP,
            };
            return write_output(output.write_all(help.as_bytes()), errors);
        }
        [flag] if flag == "--version" || flag == "-V" => {
            return write_output(
                writeln!(output, "scarp {}", env!("CARGO_PKG_VERSION")),
                errors,
            );
        }
        _ => {}
    }
    let Invocation {
        command,
        filename,
        options,
        json,
        json_diagnostics,
    } = match invocation(&args) {
        Ok(invocation) => invocation,
        Err(message) => {
            if format_hint {
                let _ = scarp::diagnostics::write_json_message(message, Some(HELP), errors);
            } else {
                let _ = writeln!(errors, "error: {message}\n\n{HELP}");
            }
            return 2;
        }
    };
    if !file_command(command) {
        let _ = writeln!(
            errors,
            "error: unknown command {:?}\n\n{HELP}",
            command.to_string_lossy()
        );
        return 2;
    }
    let path = Path::new(filename);
    if command != "tokens" {
        let program = match modules::compile_file(path, ModuleLimits::default()) {
            Ok(program) => program,
            Err(error) => {
                return report_module_diagnostic(&error, errors, json_diagnostics);
            }
        };
        return if command == "modules" {
            write_formatted_output(
                if json {
                    program.write_inspection_json(output)
                } else {
                    program.write_inspection(output)
                },
                errors,
                json_diagnostics,
            )
        } else if command == "check" {
            match program.check(options) {
                Ok(()) => 0,
                Err(error) => report_module_diagnostic(&error, errors, json_diagnostics),
            }
        } else {
            match program.execute(output, errors, options) {
                Ok(()) => 0,
                Err(ModuleRunError::Diagnostic(error)) => {
                    report_module_diagnostic(&error, errors, json_diagnostics)
                }
                Err(ModuleRunError::Output(error)) => {
                    write_formatted_output(Err(error), errors, json_diagnostics)
                }
            }
        };
    }
    let source = match fs::read_to_string(path) {
        Ok(source) => source,
        Err(error) => {
            let message = format!("could not read '{}': {error}", path.display());
            if json_diagnostics {
                let _ = scarp::diagnostics::write_json_message(&message, None, errors);
            } else {
                let _ = writeln!(errors, "error: {message}");
            }
            return 1;
        }
    };
    if command == "tokens" {
        match lexer::lex(&source) {
            Ok(tokens) => {
                let mut locations = LocationTracker::new(&source);
                for token in tokens {
                    let start = locations.at(token.span.start);
                    let end = locations.at(token.span.end);
                    if let Err(error) = writeln!(
                        output,
                        "{}:{}..{}:{}  {}  [{}..{}]",
                        start.line,
                        start.column,
                        end.line,
                        end.column,
                        token.kind,
                        token.span.start,
                        token.span.end,
                    ) {
                        return write_formatted_output(Err(error), errors, json_diagnostics);
                    }
                }
                0
            }
            Err(error) => report_diagnostic(&error, path, &source, errors, json_diagnostics),
        }
    } else {
        unreachable!("run/check handled by the module compiler")
    }
}

fn file_command(command: &OsStr) -> bool {
    matches!(
        command.to_str(),
        Some("run" | "tokens" | "check" | "modules")
    )
}

fn execution_limit_flag(flag: &str) -> bool {
    matches!(
        flag,
        "--max-steps"
            | "--max-prints"
            | "--max-call-depth"
            | "--max-calls"
            | "--max-live-slots"
            | "--max-capture-bytes"
            | "--max-vector-bytes"
            | "--max-text-bytes"
    )
}

// Usage errors can occur before a complete invocation exists. Respect a JSON
// request only in the option prefix; filenames and arguments after -- are opaque.
fn diagnostic_format_hint(args: &[OsString]) -> bool {
    let mut args = args.iter().skip(1);
    let mut json = false;
    while let Some(argument) = args.next() {
        let Some(argument) = argument.to_str() else {
            break;
        };
        if let Some(value) = argument.strip_prefix("--diagnostic-format=") {
            json |= value == "json";
        } else if execution_limit_flag(argument) {
            args.next();
        } else if argument != "--json" {
            break;
        }
    }
    json
}

struct Invocation<'a> {
    command: &'a OsStr,
    filename: &'a OsStr,
    options: ExecutionOptions,
    json: bool,
    json_diagnostics: bool,
}

fn invocation(args: &[OsString]) -> Result<Invocation<'_>, &'static str> {
    let command = args.first().ok_or("expected a command and a file path")?;
    let mut options = ExecutionOptions::default();
    let mut index = 1;
    let mut seen = Vec::new();
    let mut separator = false;
    let mut json = false;
    let mut json_diagnostics = false;
    let mut seen_format = false;
    while let Some(argument) = args.get(index) {
        if argument == "--" {
            separator = true;
            index += 1;
            break;
        }
        if let Some(value) = argument
            .to_str()
            .and_then(|arg| arg.strip_prefix("--diagnostic-format="))
        {
            if !file_command(command) {
                return Err(
                    "diagnostic format is available only for run, check, tokens, and modules",
                );
            }
            if seen_format {
                return Err("diagnostic format cannot be repeated");
            }
            json_diagnostics = match value {
                "json" => true,
                "text" => false,
                _ => return Err("diagnostic format must be json or text"),
            };
            seen_format = true;
            index += 1;
            continue;
        }
        if command == "modules" && argument == "--json" {
            if json {
                return Err("--json cannot be repeated");
            }
            json = true;
            index += 1;
            continue;
        }
        let flag = match argument.to_str() {
            Some(flag) if execution_limit_flag(flag) => flag,
            _ => break,
        };
        if command != "run" && command != "check" {
            return Err("execution limits are available only for run and check");
        }
        if seen.contains(&flag) {
            return Err("execution limit options cannot be repeated");
        }
        let digits = args
            .get(index + 1)
            .and_then(|value| value.to_str())
            .ok_or("expected a decimal execution limit")?;
        if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err("execution limits must contain decimal digits only");
        }
        let value = digits
            .parse::<u64>()
            .map_err(|_| "execution limit is too large")?;
        match flag {
            "--max-steps" => options.limits.max_steps = value,
            "--max-calls" => options.max_calls = value,
            _ => {
                let value = usize::try_from(value).map_err(|_| "resource limit is too large")?;
                match flag {
                    "--max-prints" => options.limits.max_prints = value,
                    "--max-call-depth" => options.max_call_depth = value,
                    "--max-live-slots" => options.max_live_slots = value,
                    "--max-capture-bytes" => options.max_capture_bytes = value,
                    "--max-vector-bytes" => options.max_vector_bytes = value,
                    "--max-text-bytes" => options.max_text_bytes = value,
                    _ => unreachable!(),
                }
            }
        }
        seen.push(flag);
        index += 2;
    }
    let filename = args
        .get(index)
        .ok_or("expected a command and a file path")?;
    if index + 1 != args.len() || (!separator && filename.to_string_lossy().starts_with('-')) {
        return Err("expected a command and a file path");
    }
    Ok(Invocation {
        command: command.as_os_str(),
        filename: filename.as_os_str(),
        options,
        json,
        json_diagnostics,
    })
}

fn report_diagnostic(
    error: &Diagnostic,
    path: &Path,
    source: &str,
    errors: &mut impl Write,
    json: bool,
) -> u8 {
    if json {
        let _ = error.write_json(&path.to_string_lossy(), source, errors);
    } else {
        let _ = writeln!(errors, "{}", error.render(&path.to_string_lossy(), source));
    }
    1
}

fn report_module_diagnostic(
    error: &modules::ModuleDiagnostic,
    errors: &mut impl Write,
    json: bool,
) -> u8 {
    if json {
        let _ = error.write_json(errors);
    } else {
        let _ = writeln!(errors, "{}", error.render());
    }
    1
}

fn write_formatted_output(result: io::Result<()>, errors: &mut impl Write, json: bool) -> u8 {
    match result {
        Ok(()) => 0,
        Err(error) if error.kind() == io::ErrorKind::BrokenPipe => 0,
        Err(error) => {
            if json {
                let _ = scarp::diagnostics::write_json_message(
                    &format!("could not write output: {error}"),
                    None,
                    errors,
                );
            } else {
                let _ = writeln!(errors, "error: could not write output: {error}");
            }
            1
        }
    }
}

fn write_output(result: io::Result<()>, errors: &mut impl Write) -> u8 {
    write_formatted_output(result, errors, false)
}

fn output_error(error: io::Error, errors: &mut impl Write, json: bool) -> ExitCode {
    ExitCode::from(write_formatted_output(Err(error), errors, json))
}
