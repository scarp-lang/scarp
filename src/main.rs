mod project;

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
  scarp <command> [options]

Commands:
  exec <file.srp>                  Execute a Scarp program
  verify <file.srp|directory>      Perform static source or project validation
  inspect tokens <file.srp>        Inspect lexer tokens and source locations
  create <directory>               Create a new runnable Scarp project
  launch <directory>               Execute a Scarp project entrypoint
  inspect ast <file.srp>           Show the parsed syntax tree (development)
  inspect modules [--json] <file.srp|directory>  Inspect statically checked modules
  explain E####                   Explain a real diagnostic code
  guide [command]                 Show contextual CLI guidance
  diagnose [--json]               Report actual compiler/host capabilities
  version                         Show the prototype version

Options:
  -h, --help                      Show this help
  -V, --version                   Show the prototype version

Examples:
  scarp exec examples/hello.srp
  scarp create demo
  scarp launch demo
  scarp verify examples/hello.srp
  scarp inspect tokens examples/hello.srp
  scarp inspect ast examples/hello.srp
  scarp inspect modules --json examples/module_inspection.srp
  scarp guide exec
  scarp diagnose --json

The original run, check, tokens and modules commands remain available during
migration. The old check command evaluates selected paths; it is not static-only
verification. Native executable generation and package management are not supported yet.
";

const VERIFY_HELP: &str =
    "Usage: scarp verify [--diagnostic-format=json|text] [--] <file.srp|project-directory>

Statically validate the selected source file or an explicit Scarp project
directory. Project mode validates Scarp.project and resolves its contained
entrypoint using the same safety checks as launch.
The real compiler loads imports, resolves names/types and checks constants
without executing runtime statements or writing artifacts.
Runtime arithmetic, contracts, dynamic paths and budgets are not evaluated.
--diagnostic-format=json is supported for errors; --strict is not.

";

const INSPECT_HELP: &str = "Usage:
  scarp inspect tokens [--] <file.srp>
  scarp inspect ast [--] <file.srp>
  scarp inspect modules [--json] [--] <file.srp|project-directory>

Read-only frontend or module inspection. Tokens only lexes the entry file; AST
parses it without loading imports, resolving names or executing code. Modules
performs static loading and linking, including scalar constants.
--json is available only for modules.
";

const INSPECT_TOKENS_HELP: &str = "Usage: scarp inspect tokens [--] <file.srp>

List lexer tokens and UTF-8 source locations without parsing or executing code.
Runtime execution-limit options and --json are not accepted.
--diagnostic-format=json is available for lexer errors.
";

const INSPECT_AST_HELP: &str =
    "Usage: scarp inspect ast [--diagnostic-format=json|text] [--] <file.srp>

Parse one source file and print its actual syntax tree in development debug
format. This does not resolve imported modules, type-check or execute code.
The debug layout is not a stable machine-readable interface. --json is
unsupported for successful AST output; --diagnostic-format=json serializes
only source/parser errors with the existing diagnostic schema.
";

const INSPECT_MODULES_HELP: &str =
    "Usage: scarp inspect modules [--json] [--] <file.srp|project-directory>

Statically check the source module graph and list real imports, dependencies,
declarations and typed signatures without executing runtime statements.
An explicit project directory resolves Scarp.project to its contained entrypoint.
--json emits existing schema-version-1 module metadata; --diagnostic-format=json
selects JSON stderr only for actual errors.
";

const EXEC_HELP: &str = "Usage: scarp exec [budget options] [--] <file.srp>

Execute a checked Scarp source file with the existing interpreter.
--diagnostic-format=json is available for real compiler errors.
Execution-limit options: --max-steps, --max-prints, --max-call-depth, --max-calls,
--max-live-slots, --max-capture-bytes, --max-vector-bytes, --max-text-bytes. Options precede the
source path. -- before a filename permits filenames beginning with a dash.
Program-argument forwarding and project entrypoint execution are not supported.
";

const CREATE_HELP: &str = "Usage: scarp create [--] <new-directory>

Create one new Scarp project with Scarp.project and src/main.srp.
Reject existing directories (even empty ones); never overwrite existing files.
No network access, package installation, or native build occurs.
";

const LAUNCH_HELP: &str =
    "Usage: scarp launch [--diagnostic-format=json|text] [budget options] [--] <project-directory>

Load Scarp.project from an explicit project directory, validate its entrypoint
and execute the .srp file through the existing interpreter. The entrypoint
must stay inside the project. Runtime budgets and diagnostic formats are
identical to scarp exec. No implicit ancestor search or artifact generation.
";

const GUIDE_HELP: &str = "Usage: scarp guide [command [subcommand]]

Show help for a supported Scarp command. For example: scarp guide exec,
scarp guide inspect tokens, scarp guide inspect modules.
";

const DIAGNOSE_HELP: &str = "Usage: scarp diagnose [--json]

Report the version of this running Scarp compiler, the actual host operating
system and architecture, and known available compiler/CLI capabilities.
--json emits one schema-version-1 JSON object. This command performs no
network requests and does not test external package registries or targets.
It does not claim unsupported native compilation or package tools are installed.
";

fn diagnose(output: &mut impl Write, errors: &mut impl Write, json: bool) -> u8 {
    let version = env!("CARGO_PKG_VERSION");
    let os = env::consts::OS;
    let arch = env::consts::ARCH;
    if json {
        write_output(
            writeln!(
                output,
                r#"{{"schema_version":1,"version":"{}","host_os":"{}","host_arch":"{}","execution":"interpreter","static_verify":true,"module_inspection":true,"project_workflow":true,"native_executable":false,"package_manager":false}}"#,
                version, os, arch
            ),
            errors,
        )
    } else {
        write_output(
            writeln!(
                output,
                "Scarp compiler: {version}\nHost: {os}/{arch}\nExecution: interpreter\nStatic verification: available\nModule inspection: available\nProject create/launch: available\nNative executable emission: unavailable\nPackage management: unavailable"
            ),
            errors,
        )
    }
}

const VERSION_HELP: &str = "Usage: scarp version

Print the actual compiler package version.
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

fn dispatch(mut args: Vec<OsString>, output: &mut impl Write, errors: &mut impl Write) -> u8 {
    if args.is_empty() {
        return write_output(output.write_all(HELP.as_bytes()), errors);
    }
    // Canonical CLI routes reuse the existing Scarp library/lexer backends.
    // Existing entry points stay functional during an explicit migration window.
    match args.as_slice() {
        [command, flag] if command == "create" && (flag == "--help" || flag == "-h") => {
            return write_output(output.write_all(CREATE_HELP.as_bytes()), errors);
        }
        [command, flag] if command == "launch" && (flag == "--help" || flag == "-h") => {
            return write_output(output.write_all(LAUNCH_HELP.as_bytes()), errors);
        }
        [command] if command == "guide" => {
            return write_output(output.write_all(HELP.as_bytes()), errors);
        }
        [command, flag] if command == "guide" && (flag == "--help" || flag == "-h") => {
            return write_output(output.write_all(GUIDE_HELP.as_bytes()), errors);
        }
        [command, target] if command == "guide" => {
            let page = match target.to_str() {
                Some("exec") => Some(EXEC_HELP),
                Some("inspect") => Some(INSPECT_HELP),
                Some("verify") => Some(VERIFY_HELP),
                Some("explain") => Some(EXPLAIN_HELP),
                Some("guide") => Some(GUIDE_HELP),
                Some("version") => Some(VERSION_HELP),
                Some("diagnose") => Some(DIAGNOSE_HELP),
                Some("create") => Some(CREATE_HELP),
                Some("launch") => Some(LAUNCH_HELP),
                _ => None,
            };
            return match page {
                Some(page) => write_output(output.write_all(page.as_bytes()), errors),
                None => {
                    let _ = writeln!(
                        errors,
                        "error: unknown guide topic {:?}\n\n{GUIDE_HELP}",
                        target.to_string_lossy()
                    );
                    2
                }
            };
        }
        [command, group, topic] if command == "guide" && group == "inspect" => {
            let page = match topic.to_str() {
                Some("tokens") => Some(INSPECT_TOKENS_HELP),
                Some("ast") => Some(INSPECT_AST_HELP),
                Some("modules") => Some(INSPECT_MODULES_HELP),
                _ => None,
            };
            return match page {
                Some(page) => write_output(output.write_all(page.as_bytes()), errors),
                None => {
                    let _ = writeln!(
                        errors,
                        "error: unknown inspect topic {:?}\n\n{INSPECT_HELP}",
                        topic.to_string_lossy()
                    );
                    2
                }
            };
        }
        [command, ..] if command == "guide" => {
            let _ = writeln!(errors, "error: invalid guide arguments\n\n{GUIDE_HELP}");
            return 2;
        }
        [command] if command == "diagnose" => {
            return diagnose(output, errors, false);
        }
        [command, flag] if command == "diagnose" && (flag == "--help" || flag == "-h") => {
            return write_output(output.write_all(DIAGNOSE_HELP.as_bytes()), errors);
        }
        [command, flag] if command == "diagnose" && flag == "--json" => {
            return diagnose(output, errors, true);
        }
        [command, ..] if command == "diagnose" => {
            let _ = writeln!(
                errors,
                "error: invalid diagnose arguments\n\n{DIAGNOSE_HELP}"
            );
            return 2;
        }
        [command] if command == "version" => {
            return write_output(
                writeln!(output, "scarp {}", env!("CARGO_PKG_VERSION")),
                errors,
            );
        }
        [command, flag] if command == "version" && (flag == "--help" || flag == "-h") => {
            return write_output(output.write_all(VERSION_HELP.as_bytes()), errors);
        }
        [command, ..] if command == "version" => {
            let _ = writeln!(
                errors,
                "error: unexpected version arguments\n\n{VERSION_HELP}"
            );
            return 2;
        }
        [command] if command == "inspect" => {
            return write_output(output.write_all(INSPECT_HELP.as_bytes()), errors);
        }
        [command, flag] if command == "inspect" && (flag == "--help" || flag == "-h") => {
            return write_output(output.write_all(INSPECT_HELP.as_bytes()), errors);
        }
        [command, topic, flag] if command == "inspect" && (flag == "--help" || flag == "-h") => {
            let page = match topic.to_str() {
                Some("tokens") => Some(INSPECT_TOKENS_HELP),
                Some("ast") => Some(INSPECT_AST_HELP),
                Some("modules") => Some(INSPECT_MODULES_HELP),
                _ => None,
            };
            return match page {
                Some(page) => write_output(output.write_all(page.as_bytes()), errors),
                None => {
                    let _ = writeln!(
                        errors,
                        "error: unknown inspect topic {:?}\n\n{INSPECT_HELP}",
                        topic.to_string_lossy()
                    );
                    2
                }
            };
        }
        [command, flag] if command == "verify" && (flag == "--help" || flag == "-h") => {
            return write_output(output.write_all(VERIFY_HELP.as_bytes()), errors);
        }
        [command, flag] if command == "exec" && (flag == "--help" || flag == "-h") => {
            return write_output(output.write_all(EXEC_HELP.as_bytes()), errors);
        }
        _ => {}
    }
    if args.first().is_some_and(|command| command == "create") {
        let destination = match args.as_slice() {
            [_, path] if !path.to_string_lossy().starts_with('-') => Some(path),
            [_, separator, path] if separator == "--" => Some(path),
            _ => None,
        };
        let Some(destination) = destination else {
            let _ = writeln!(
                errors,
                "error: expected a new project directory\n\n{CREATE_HELP}"
            );
            return 2;
        };
        let destination = Path::new(destination);
        return match project::create(destination) {
            Ok(()) => write_output(
                writeln!(output, "Created Scarp project: {}", destination.display()),
                errors,
            ),
            Err(message) => {
                let _ = writeln!(errors, "error: {message}");
                1
            }
        };
    }
    if args.first().is_some_and(|command| command == "launch") {
        let mut forwarded = args.clone();
        forwarded[0] = OsString::from("run");
        let format_hint = diagnostic_format_hint(&forwarded);
        let root = match invocation(&forwarded) {
            Ok(invocation) => Path::new(invocation.filename).to_path_buf(),
            Err(message) => {
                if format_hint {
                    let _ =
                        scarp::diagnostics::write_json_message(message, Some(LAUNCH_HELP), errors);
                } else {
                    let _ = writeln!(errors, "error: {message}\n\n{LAUNCH_HELP}");
                }
                return 2;
            }
        };
        let entry = match project::entry_file(&root) {
            Ok(entry) => entry,
            Err(message) => {
                if format_hint {
                    let _ = scarp::diagnostics::write_json_message(&message, None, errors);
                } else {
                    let _ = writeln!(errors, "error: {message}");
                }
                return 1;
            }
        };
        // Pass the exact existing option prefix to the real interpreter.
        // This keeps all runtime budgets, diagnostics and output semantics.
        *forwarded
            .last_mut()
            .expect("validated invocation has an entry") = entry.into_os_string();
        return dispatch(forwarded, output, errors);
    }
    let inspect_ast = args.first().is_some_and(|command| command == "inspect")
        && args.get(1).is_some_and(|topic| topic == "ast");
    if args.first().is_some_and(|command| command == "inspect") {
        match args.get(1).and_then(|topic| topic.to_str()) {
            Some("tokens" | "ast" | "modules") => {
                args.remove(0);
                if inspect_ast {
                    // Route through the existing file option parser without
                    // adding an unintended top-level 'ast' alias.
                    args[0] = OsString::from("tokens");
                }
            }
            _ => {
                let _ = writeln!(
                    errors,
                    "error: expected inspect tokens, inspect ast or inspect modules\n\n{INSPECT_HELP}"
                );
                return 2;
            }
        }
    } else if args.first().is_some_and(|command| command == "exec") {
        args[0] = OsString::from("run");
    }
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
                Some("verify") => VERIFY_HELP,
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
    // Static verification and module inspection accept an explicit project
    // directory. Both reuse launch's bounded manifest/contained entrypoint
    // checks and the existing nonexecuting source-graph compiler path.
    let project_entry = if matches!(command.to_str(), Some("verify" | "modules")) && path.is_dir() {
        match project::entry_file(path) {
            Ok(entry) => Some(entry),
            Err(message) => {
                if json_diagnostics {
                    let _ = scarp::diagnostics::write_json_message(&message, None, errors);
                } else {
                    let _ = writeln!(errors, "error: {message}");
                }
                return 1;
            }
        }
    } else {
        None
    };
    let path = project_entry.as_deref().unwrap_or(path);
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
        } else if command == "verify" {
            // The compiler loaded and resolved the entire source graph. Do not
            // evaluate runtime statements (legacy check does evaluate them).
            0
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
    if command == "tokens" && inspect_ast {
        // Parse-only frontend inspection: does not load imports, resolve names,
        // execute code, or create artifacts. Debug output is not a stable API.
        match scarp::parse(&source) {
            Ok(program) => {
                write_formatted_output(writeln!(output, "{program:#?}"), errors, json_diagnostics)
            }
            Err(error) => report_diagnostic(&error, path, &source, errors, json_diagnostics),
        }
    } else if command == "tokens" {
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
        Some("run" | "verify" | "tokens" | "check" | "modules")
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
    let mut args = args.iter();
    let first = args.next();
    // Nested inspection routes use a second command word before option parsing.
    if first.is_some_and(|command| command == "inspect") {
        args.next();
    }
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
                return Err("diagnostic format is available only for source-file commands");
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
