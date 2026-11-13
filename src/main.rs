mod project;
mod project_info;
mod project_manifest;
mod project_tests;

use std::{
    env,
    ffi::{OsStr, OsString},
    fs,
    io::{self, BufWriter, IsTerminal, Write},
    path::Path,
    process::ExitCode,
};

use scarp::{
    diagnostics::{Diagnostic, LocationTracker},
    lexer,
    modules::{self, ModuleLimits, ModuleRunError},
    ExecutionOptions, InputLimits,
};

const HELP: &str = "Scarp — Build Beyond Boundaries.

Usage:
  scarp <command> [options]

Commands:
  inspect symbols|references [--json] <file.srp|project-directory>  Source navigation
  exec <file.srp>                  Execute a Scarp program
  verify [file.srp|directory]      Perform static source or project validation
  inspect tokens <file.srp>        Inspect lexer tokens and source locations
  create <directory>               Create a legacy Scarp.project app
  init <directory>                 Initialize a new scarp.project app
  project info [--json] [directory]  Display validated project metadata
  launch <directory>               Execute a Scarp project entrypoint
  run [directory|file.srp]          Run a project or legacy source file
  test [--json|--json=v2] [--list] [--require-tests] [--filter SUBSTRING] [directory]  Project tests
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
  scarp init modern-demo
  scarp project info --json modern-demo
  scarp run modern-demo
  scarp test modern-demo
  scarp test --json modern-demo
  scarp test --list modern-demo
  scarp test --require-tests modern-demo
  scarp test --filter math modern-demo
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
    "Usage: scarp verify [--diagnostic-format=json|text] [--] [file.srp|project-directory]

Statically validate the selected source file or a Scarp project. With no
arguments, validate the current directory if it contains a project manifest.
Project mode validates scarp.project or legacy Scarp.project and resolves its
contained entrypoint using the same safety checks as launch.
The real compiler loads imports, resolves names/types and checks constants
without executing runtime statements or writing artifacts.
Runtime arithmetic, contracts, dynamic paths and budgets are not evaluated.
--diagnostic-format=json is supported for errors; --strict is not.

";

const INSPECT_HELP: &str = "Usage:
  scarp inspect symbols [--json] <file.srp|project-directory>
  scarp inspect references [--json] <file.srp|project-directory>
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

const SYMBOLS_HELP: &str = "Usage: scarp inspect symbols [--json] [--diagnostic-format=json] [--] <file.srp|project-directory>

Compile a complete source graph and inspect source-ordered definitions without
executing statements, defaults or contracts. Compile-time constants are checked.
--json emits schema-v1 scarp.symbols metadata; compiler errors use the existing
diagnostic format. Limits are bounded independently from runtime budgets.
Use inspect references for use sites, including calls, writes and named labels.
";
const REFERENCES_HELP: &str = "Usage: scarp inspect references [--json] [--diagnostic-format=json] [--] <file.srp|project-directory>

Build the same static navigation snapshot as inspect symbols and include every
resolved reference with its target symbol ID and file-local UTF-8 byte span.
--json emits schema-v1 scarp.references metadata, including the symbol table.
IDs belong to one snapshot. Invalid graphs fail before any inventory is written.
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
An explicit project directory resolves scarp.project or legacy Scarp.project.
--json emits existing schema-version-1 module metadata; --diagnostic-format=json
selects JSON stderr only for actual errors.
";

const EXEC_HELP: &str = "Usage: scarp exec [budget options] [--] <file.srp>

Execute a checked Scarp source file with the existing interpreter.
--diagnostic-format=json is available for real compiler errors.
Execution-limit options: --max-steps, --max-prints, --max-call-depth, --max-calls,
--max-live-slots, --max-capture-bytes, --max-vector-bytes, --max-text-bytes,
--max-tuple-bytes, --max-bigint-digits, --max-bigint-bytes,
--max-input-line-bytes, --max-input-bytes, --max-input-reads.
Options precede the
source path. -- before a filename permits filenames beginning with a dash.
Program-argument forwarding and project entrypoint execution are not supported.
read_line reads redirected UTF-8 stdin; terminal input is rejected at the selected call.
Input defaults: 65536 raw bytes per line, 1048576 total bytes, 4096 reads.
CR/LF count toward input/text quotas. Input cannot be rolled back; output stays buffered.
No interactive prompt is displayed or flushed before a read.
";

const CREATE_HELP: &str = "Usage: scarp create [--] <new-directory>

Create one new Scarp project with Scarp.project and src/main.srp.
Reject existing directories (even empty ones); never overwrite existing files.
No network access, package installation, or native build occurs.
";

const INIT_HELP: &str = "Usage: scarp init [--] <new-directory>

Initialize a new standalone application using the declarative scarp.project
manifest, src/main.srp, README.md, .gitignore, tests/ and examples/.
Reject existing paths and do not install packages or run any source.
Use scarp verify DIRECTORY and scarp run DIRECTORY afterwards.
";

const TEST_HELP: &str =
    "Usage: scarp test [--json|--json=v2] [--list] [--require-tests] [--filter SUBSTRING] [--] [project-directory]

Run the immediate tests/*.srp files, or list them without source execution.
Without a directory argument, use the current project directory.
--json emits one schema-version-1 test report on stdout; failed test diagnostics
are bounded and embedded in the JSON rather than printed separately.
--json=v2 opts into structured per-test diagnostics with codes, source spans,
and failure phases. Choose at most one JSON flag.
--list reports discovered tests without compiling or running them; combine
--json --list to obtain a machine-readable discovery report.
--require-tests returns exit status 1 when no selected test files are discovered,
even for --list/--json; the zero-test report remains visible for CI tooling.
--filter SUBSTRING selects filenames containing a case-sensitive UTF-8 substring.
It does not support regex/globs; use 1..128 bytes, no leading dash, separators,
colon or control characters. An empty selection reports zero tests.
Files are sorted lexicographically, limited to 64, and checked for symlinks.
An absent tests/ directory counts as an empty suite.
An optional sibling NAME.stdin fixture supplies bounded per-test input.
Tests run only trusted Scarp code with existing deterministic default budgets.
Interactive stdin and ordinary program stdout are not forwarded.
A suite with no tests succeeds by default and reports an explicit zero-test result.
Project/CLI errors go to stderr and do not output successful-report JSON.
No dependency installation, per-test timeout, or custom budgets are supported.
";

const PROJECT_HELP: &str = "Usage:
  scarp project info [--json] [--] [project-directory]

Display validated local project metadata without executing Scarp source.
The default directory is the current working directory. The parser checks
manifest syntax and the entrypoint's canonical containment, existence and
file type, but does not compile the source graph.
--json prints schema-version-1 JSON with name, version, type, entry,
manifest and format. Legacy Scarp.project reports a null version.
No dependencies, lockfiles or network operations are involved.
Errors go to stderr and do not emit partial successful JSON output.
";

const LAUNCH_HELP: &str =
    "Usage: scarp launch [--diagnostic-format=json|text] [budget options] [--] <project-directory>

Load scarp.project or legacy Scarp.project from an explicit project directory, validate its entrypoint
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
--max-tuple-bytes 8388608 limits cumulative immutable tuple member/type payload.
--max-bigint-digits 10000 (ceiling 1000000), --max-bigint-bytes 8388608 (limbs and scratch).
String sharing and direct literal output do not allocate text payload; retirement gives no refund.
Snapshot bytes are a logical storage cap, not an exact process-memory limit.
Counts are unsigned decimal digits, zero is valid, and each option may occur once.
Example: scarp run --max-steps 2000000 --max-prints 10 examples/repeat.srp
Use scarp check with the same options to evaluate without printing.
read_line requires redirected UTF-8 stdin; terminal prompts are not supported.
--max-input-line-bytes 65536, --max-input-bytes 1048576, --max-input-reads 4096.
Raw input counts CR/LF and uses the text/work budgets. Output stays buffered.
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
--max-tuple-bytes 8388608 limits cumulative immutable tuple member/type payload.
--max-bigint-digits 10000 (ceiling 1000000), --max-bigint-bytes 8388608 (limbs and scratch).
String sharing and direct literal output do not allocate text payload; retirement gives no refund.
All resource options take unsigned decimal counts, allow zero, and may occur once.
Input-limit options are accepted, but check never reads stdin: a selected read_line
reports E0054. Use verify for static validation of input programs.
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

// Activate project execution only when the directory actually contains a
// manifest. Otherwise preserve legacy "run" usage errors and the original
// "could not read DIRECTORY" single-file diagnostic.
fn has_project_manifest(directory: &Path) -> bool {
    directory.is_dir()
        && (fs::symlink_metadata(directory.join(project::MANIFEST_V2)).is_ok()
            || fs::symlink_metadata(directory.join(project::MANIFEST)).is_ok())
}

fn dispatch(mut args: Vec<OsString>, output: &mut impl Write, errors: &mut impl Write) -> u8 {
    if args.is_empty() {
        return write_output(output.write_all(HELP.as_bytes()), errors);
    }
    // Canonical CLI routes reuse the existing Scarp library/lexer backends.
    // Existing entry points stay functional during an explicit migration window.
    match args.as_slice() {
        [command, flag] if command == "init" && (flag == "--help" || flag == "-h") => {
            return write_output(output.write_all(INIT_HELP.as_bytes()), errors);
        }
        [command, flag] if command == "create" && (flag == "--help" || flag == "-h") => {
            return write_output(output.write_all(CREATE_HELP.as_bytes()), errors);
        }
        [command, flag] if command == "launch" && (flag == "--help" || flag == "-h") => {
            return write_output(output.write_all(LAUNCH_HELP.as_bytes()), errors);
        }
        [command, flag] if command == "test" && (flag == "--help" || flag == "-h") => {
            return write_output(output.write_all(TEST_HELP.as_bytes()), errors);
        }
        [command, flag] if command == "project" && (flag == "--help" || flag == "-h") => {
            return write_output(output.write_all(PROJECT_HELP.as_bytes()), errors);
        }
        [command, topic, flag]
            if command == "project" && topic == "info" && (flag == "--help" || flag == "-h") =>
        {
            return write_output(output.write_all(PROJECT_HELP.as_bytes()), errors);
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
                Some("init") => Some(INIT_HELP),
                Some("test") => Some(TEST_HELP),
                Some("project") => Some(PROJECT_HELP),
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
                Some("symbols") => Some(SYMBOLS_HELP),
                Some("references") => Some(REFERENCES_HELP),
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
                Some("symbols") => Some(SYMBOLS_HELP),
                Some("references") => Some(REFERENCES_HELP),
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
    if args.first().is_some_and(|command| command == "project") {
        if args.get(1).map_or(true, |topic| topic != "info") {
            let _ = writeln!(errors, "error: expected 'project info'\n\n{PROJECT_HELP}");
            return 2;
        }
        let mut json = false;
        let mut directory = None;
        let mut separator = false;
        let mut invalid = false;
        for arg in args.iter().skip(2) {
            if !separator && arg == "--" {
                separator = true;
            } else if !separator && arg == "--json" {
                if json {
                    invalid = true;
                }
                json = true;
            } else if (!separator && arg.to_string_lossy().starts_with('-')) || directory.is_some()
            {
                invalid = true;
            } else {
                directory = Some(Path::new(arg));
            }
        }
        if invalid {
            let _ = writeln!(errors, "error: expected at most one project directory and a single --json flag\n\n{PROJECT_HELP}");
            return 2;
        }
        let directory = directory.unwrap_or(Path::new("."));
        return match project::info(directory) {
            Ok(info) => write_output(project_info::write(&info, output, json), errors),
            Err(message) => {
                let _ = writeln!(errors, "error: {message}");
                1
            }
        };
    }
    if args.first().is_some_and(|command| command == "test") {
        use project_tests::JsonVersion;

        let mut json = None;
        let mut list_only = false;
        let mut require_tests = false;
        let mut filter: Option<String> = None;
        let mut separator = false;
        let mut directory = None;
        let mut invalid = false;
        let mut arguments = args.iter().skip(1);
        while let Some(arg) = arguments.next() {
            if !separator && arg == "--" {
                separator = true;
            } else if !separator && (arg == "--json" || arg == "--json=v2") {
                if json.is_some() {
                    invalid = true;
                }
                json = Some(if arg == "--json" {
                    JsonVersion::V1
                } else {
                    JsonVersion::V2
                });
            } else if !separator && arg == "--list" {
                if list_only {
                    invalid = true;
                }
                list_only = true;
            } else if !separator && arg == "--require-tests" {
                if require_tests {
                    invalid = true;
                }
                require_tests = true;
            } else if !separator && arg == "--filter" {
                if filter.is_some() {
                    invalid = true;
                }
                match arguments.next().and_then(|value| value.to_str()) {
                    Some(value) if project_tests::valid_filter(value) => {
                        filter = Some(value.to_owned());
                    }
                    _ => invalid = true,
                }
            } else if (!separator && arg.to_string_lossy().starts_with('-')) || directory.is_some()
            {
                invalid = true;
            } else {
                directory = Some(Path::new(arg));
            }
        }
        if invalid {
            let _ = writeln!(
                errors,
                "error: expected at most one project directory and optional test flags\n\n{TEST_HELP}"
            );
            return 2;
        }
        return project_tests::run(
            directory.unwrap_or(Path::new(".")),
            output,
            errors,
            json,
            list_only,
            require_tests,
            filter.as_deref(),
        );
    }
    if args
        .first()
        .is_some_and(|command| command == "create" || command == "init")
    {
        let modern = args[0] == "init";
        let command_help = if modern { INIT_HELP } else { CREATE_HELP };
        let destination = match args.as_slice() {
            [_, path] if !path.to_string_lossy().starts_with('-') => Some(path),
            [_, separator, path] if separator == "--" => Some(path),
            _ => None,
        };
        let Some(destination) = destination else {
            let _ = writeln!(
                errors,
                "error: expected a new project directory\n\n{command_help}"
            );
            return 2;
        };
        let destination = Path::new(destination);
        let created = if modern {
            project::init(destination)
        } else {
            project::create(destination)
        };
        return match created {
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
    if matches!(args.as_slice(), [command] if command == "run" || command == "verify")
        && has_project_manifest(Path::new("."))
    {
        args.push(OsString::from("."));
    }
    // A source path keeps the historical single-file run behavior. A project
    // directory uses the project-aware launch path with identical budgets.
    if args.first().is_some_and(|command| command == "run")
        && args.len() > 1
        && args
            .last()
            .is_some_and(|path| has_project_manifest(Path::new(path)))
    {
        let mut forwarded = args.clone();
        forwarded[0] = OsString::from("launch");
        return dispatch(forwarded, output, errors);
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
    let navigation = if args.first().is_some_and(|command| command == "inspect") {
        match args.get(1).and_then(|s| s.to_str()) {
            Some("symbols") => Some(false),
            Some("references") => Some(true),
            _ => None,
        }
    } else {
        None
    };
    let inspect_ast = args.first().is_some_and(|command| command == "inspect")
        && args.get(1).is_some_and(|topic| topic == "ast");
    if args.first().is_some_and(|command| command == "inspect") {
        match args.get(1).and_then(|topic| topic.to_str()) {
            Some("tokens" | "ast" | "modules" | "symbols" | "references") => {
                args.remove(0);
                if navigation.is_some() {
                    args[0] = OsString::from("modules");
                }
                if inspect_ast {
                    // Route through the existing file option parser without
                    // adding an unintended top-level 'ast' alias.
                    args[0] = OsString::from("tokens");
                }
            }
            _ => {
                let _ = writeln!(
                    errors,
                    "error: expected inspect tokens, inspect ast, inspect modules, inspect symbols or inspect references\n\n{INSPECT_HELP}"
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
        input_limits,
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
    if let Some(references) = navigation {
        let indexed = match modules::compile_indexed_file(
            path,
            ModuleLimits::default(),
            modules::symbols::IndexLimits::default(),
        ) {
            Ok(p) => p,
            Err(error) => return report_module_diagnostic(&error, errors, json_diagnostics),
        };
        return write_formatted_output(
            match (references, json) {
                (false, false) => indexed.index().write_symbols(output),
                (true, false) => indexed.index().write_references(output),
                (false, true) => indexed.index().write_symbols_json(output),
                (true, true) => indexed.index().write_references_json(output),
            },
            errors,
            json_diagnostics,
        );
    }
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
            let stdin = io::stdin();
            let result = if stdin.is_terminal() {
                // A selected read fails immediately; buffered output cannot
                // serve as a visible interactive prompt in this mode.
                program.execute(output, errors, options)
            } else {
                program.execute_with_input(&mut stdin.lock(), output, errors, options, input_limits)
            };
            match result {
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
            | "--max-tuple-bytes"
            | "--max-bigint-digits"
            | "--max-bigint-bytes"
            | "--max-input-line-bytes"
            | "--max-input-bytes"
            | "--max-input-reads"
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
    input_limits: InputLimits,
    json: bool,
    json_diagnostics: bool,
}

fn invocation(args: &[OsString]) -> Result<Invocation<'_>, &'static str> {
    let command = args.first().ok_or("expected a command and a file path")?;
    let mut options = ExecutionOptions::default();
    let mut input_limits = InputLimits::default();
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
                    "--max-tuple-bytes" => options.max_tuple_bytes = value,
                    "--max-bigint-digits" => {
                        if value > scarp::bigint::MAX_BIGINT_DIGITS {
                            return Err("bigint digit limit cannot exceed 1000000");
                        }
                        options.max_bigint_digits = value;
                    }
                    "--max-bigint-bytes" => options.max_bigint_bytes = value,
                    "--max-input-line-bytes" => input_limits.max_line_bytes = value,
                    "--max-input-bytes" => input_limits.max_total_bytes = value,
                    "--max-input-reads" => input_limits.max_reads = value,
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
        input_limits,
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
