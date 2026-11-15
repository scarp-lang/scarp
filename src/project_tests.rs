//! Deterministic, local project test discovery and compiler-backed execution.
//!
//! Test files are ordinary Scarp programs in a project's immediate `tests/`
//! directory. Exiting without a compiler or runtime error is a pass. This is
//! not a separate assertion framework and does not execute files during discovery.

use std::{
    fmt, fs,
    io::{self, Cursor, Read, Write},
    path::{Path, PathBuf},
};

use scarp::{
    diagnostics::write_json_message,
    modules::{self, FileSourceLoader, ModuleDiagnostic, ModuleLimits, ModuleRunError},
    ExecutionOptions, InputLimits,
};

const MAX_TESTS: usize = 64;
const FAILURE_DETAIL_CHARS: usize = 1600;
const MAX_FILTER_BYTES: usize = 128;
const MAX_FIXTURE_BYTES: usize = 1024 * 1024;

#[derive(Clone, Copy)]
pub enum JsonVersion {
    V1,
    V2,
}

/// A filter is a literal substring of a single UTF-8 test filename, not a
/// filesystem path, command option, regular expression, or glob.
pub fn valid_filter(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_FILTER_BYTES
        && !value.starts_with('-')
        && !value
            .chars()
            .any(|ch| ch.is_control() || matches!(ch, '/' | '\\' | ':'))
}

fn report(error: impl std::fmt::Display, stderr: &mut impl Write) -> u8 {
    let _ = writeln!(stderr, "error: {error}");
    1
}

fn discover(root: &Path) -> Result<Vec<(String, PathBuf)>, String> {
    let test_dir = root.join("tests");
    let metadata = match fs::symlink_metadata(&test_dir) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(format!(
                "could not inspect project tests directory: {error}"
            ))
        }
    };
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err("project tests path must be an ordinary directory, not a symlink".into());
    }
    let test_dir = fs::canonicalize(test_dir)
        .map_err(|error| format!("could not resolve project tests directory: {error}"))?;
    if !test_dir.starts_with(root) {
        return Err("project tests directory resolves outside the project root".into());
    }
    let mut files = Vec::new();
    for entry in
        fs::read_dir(&test_dir).map_err(|error| format!("could not read project tests: {error}"))?
    {
        let entry = entry.map_err(|error| format!("could not enumerate tests: {error}"))?;
        let path = entry.path();
        if path
            .extension()
            .map_or(true, |extension| extension != "srp")
        {
            continue;
        }
        let kind = entry
            .file_type()
            .map_err(|error| format!("could not inspect test: {error}"))?;
        if !kind.is_file() {
            return Err(format!(
                "test '{}' must be a regular .srp file, not a symlink or directory",
                path.display()
            ));
        }
        let filename = entry
            .file_name()
            .into_string()
            .map_err(|_| "test filenames must be UTF-8")?;
        if filename
            .chars()
            .any(|ch| ch.is_control() || matches!(ch, ':' | '\\'))
        {
            return Err("test filename contains unsafe characters".into());
        }
        files.push((format!("tests/{filename}"), path));
        if files.len() > MAX_TESTS {
            return Err(format!("too many project tests (limit {MAX_TESTS})"));
        }
    }
    files.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(files)
}

/// Read the optional sibling test input without reading process stdin.
///
/// A missing fixture preserves the previous no-input execution contract.
/// Malformed, oversized, or unsafe fixtures fail only their selected test;
/// listing remains read-only and does not inspect fixture bytes.
fn fixture(
    label: &str,
    test_path: &Path,
    root: &Path,
    extension: &str,
) -> Result<Option<Vec<u8>>, String> {
    let path = test_path.with_extension(extension);
    let meta = match fs::symlink_metadata(&path) {
        Ok(meta) => meta,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(format!(
                "{label}: cannot inspect .{extension} fixture: {error}"
            ))
        }
    };
    if !meta.file_type().is_file() {
        return Err(format!(
            "{label}: .{extension} fixture must be an ordinary file, not a symlink or directory"
        ));
    }
    let canonical = fs::canonicalize(&path)
        .map_err(|error| format!("{label}: cannot resolve .{extension} fixture: {error}"))?;
    if !canonical.starts_with(root) {
        return Err(format!(
            "{label}: .{extension} fixture escapes the trusted project root"
        ));
    }
    let file = fs::File::open(&canonical)
        .map_err(|error| format!("{label}: cannot open .{extension} fixture: {error}"))?;
    let opened = file
        .metadata()
        .map_err(|error| format!("{label}: cannot stat .{extension} fixture: {error}"))?;
    if !opened.is_file() {
        return Err(format!(
            "{label}: .{extension} fixture is not a regular file"
        ));
    }
    let limit_name = if extension == "stdin" {
        "input limit"
    } else {
        "limit"
    };
    if opened.len() > MAX_FIXTURE_BYTES as u64 {
        return Err(format!(
            "{label}: .{extension} fixture exceeds the 1 MiB {limit_name}"
        ));
    }
    let mut bytes = Vec::new();
    file.take(MAX_FIXTURE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("{label}: cannot read .{extension} fixture: {error}"))?;
    if bytes.len() > MAX_FIXTURE_BYTES {
        return Err(format!(
            "{label}: .{extension} fixture grew beyond the 1 MiB {limit_name}"
        ));
    }
    Ok(Some(bytes))
}

// Compare while replaying the interpreter's existing bounded output snapshots.
// Only the fixture and mismatch metadata are retained, never another copy of
// the program's output. With no fixture this writer discards the stream.
#[derive(Default)]
struct ExpectedOutput {
    bytes: Option<Vec<u8>>,
    seen: usize,
    difference: Option<usize>,
}

impl ExpectedOutput {
    fn from_fixture(bytes: Option<Vec<u8>>, stream: &str) -> Result<Self, Failure> {
        if bytes
            .as_deref()
            .is_some_and(|bytes| std::str::from_utf8(bytes).is_err())
        {
            return Err(Failure::Fixture(format!(
                ".{stream} fixture must contain valid UTF-8"
            )));
        }
        Ok(Self {
            bytes,
            ..Self::default()
        })
    }

    fn verify(&self, stream: &str) -> Result<(), Failure> {
        let Some(expected) = &self.bytes else {
            return Ok(());
        };
        if let Some(offset) = self
            .difference
            .or_else(|| (self.seen < expected.len()).then_some(self.seen))
        {
            return Err(Failure::OutputMismatch(format!(
                "{stream} differs from .{stream} at byte {offset} (expected {} bytes, received {})",
                expected.len(),
                self.seen,
            )));
        }
        Ok(())
    }
}

impl Write for ExpectedOutput {
    fn write(&mut self, input: &[u8]) -> io::Result<usize> {
        if let Some(expected) = &self.bytes {
            if self.difference.is_none() {
                let available = &expected[self.seen..];
                self.difference = available
                    .iter()
                    .zip(input)
                    .position(|(a, b)| a != b)
                    .or_else(|| (input.len() > available.len()).then_some(available.len()))
                    .map(|offset| self.seen + offset);
            }
            self.seen = self
                .seen
                .checked_add(input.len())
                .ok_or_else(|| io::Error::other("output length exceeds host range"))?;
        }
        Ok(input.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

enum Failure {
    Compile(ModuleDiagnostic),
    Fixture(String),
    Runtime(ModuleRunError),
    OutputMismatch(String),
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Compile(error) => error.fmt(f),
            Self::Fixture(error) | Self::OutputMismatch(error) => error.fmt(f),
            Self::Runtime(error) => error.fmt(f),
        }
    }
}

impl Failure {
    fn text(&self) -> String {
        self.to_string()
            .chars()
            .take(FAILURE_DETAIL_CHARS)
            .collect()
    }

    fn structured(&self) -> io::Result<ReportFailure> {
        let mut diagnostic = Vec::new();
        let phase = match self {
            Self::Compile(error) => {
                error.write_project_json(&mut diagnostic)?;
                "compile"
            }
            Self::Fixture(_) => {
                write_json_message(&self.text(), None, &mut diagnostic)?;
                "fixture"
            }
            Self::OutputMismatch(_) => {
                write_json_message(
                    &self.text(),
                    Some(
                        "compare the expected fixture with program output using exact UTF-8 bytes",
                    ),
                    &mut diagnostic,
                )?;
                "runtime"
            }
            Self::Runtime(error) => {
                match error {
                    ModuleRunError::Diagnostic(error) => {
                        error.write_project_json(&mut diagnostic)?;
                    }
                    ModuleRunError::Output(_) => {
                        write_json_message(&self.text(), None, &mut diagnostic)?;
                    }
                }
                "runtime"
            }
        };
        // The compiler writer terminates a standalone diagnostic with LF.
        // Strip that terminator when embedding it in the single-line report.
        if diagnostic.last() == Some(&b'\n') {
            diagnostic.pop();
        }
        Ok(ReportFailure::Structured { phase, diagnostic })
    }
}

enum ReportFailure {
    Text(String),
    Structured {
        phase: &'static str,
        diagnostic: Vec<u8>,
    },
}

struct Case {
    path: String,
    error: Option<ReportFailure>,
}

fn write_report(
    stdout: &mut impl Write,
    discovered: usize,
    passed: usize,
    failed: usize,
    list_only: bool,
    version: JsonVersion,
    cases: &[Case],
) -> std::io::Result<()> {
    use super::project_info::quote_json;

    let mode = if list_only { "list" } else { "run" };
    let schema = match version {
        JsonVersion::V1 => 1,
        JsonVersion::V2 => 2,
    };
    write!(
        stdout,
        "{{\"schema_version\":{schema},\"mode\":\"{mode}\",\"discovered\":{discovered},\"passed\":{passed},\"failed\":{failed},\"tests\":["
    )?;
    for (index, case) in cases.iter().enumerate() {
        if index != 0 {
            write!(stdout, ",")?;
        }
        let status = if list_only {
            "discovered"
        } else if case.error.is_some() {
            "failed"
        } else {
            "passed"
        };
        write!(
            stdout,
            "{{\"path\":{},\"status\":\"{status}\"",
            quote_json(&case.path)
        )?;
        if let (false, Some(error)) = (list_only, &case.error) {
            match error {
                ReportFailure::Text(error) => write!(stdout, ",\"error\":{}", quote_json(error))?,
                ReportFailure::Structured { phase, diagnostic } => {
                    write!(stdout, ",\"phase\":\"{phase}\",\"diagnostic\":")?;
                    stdout.write_all(diagnostic)?;
                }
            }
        }
        write!(stdout, "}}")?;
    }
    writeln!(stdout, "]}}")
}

/// Discover direct project tests without compiling or executing anything,
/// or execute them through the real compiler with the original default
/// budgets. JSON reports emit one object in the selected schema.
pub fn run(
    directory: &Path,
    stdout: &mut impl Write,
    stderr: &mut impl Write,
    json: Option<JsonVersion>,
    list_only: bool,
    require_tests: bool,
    filter: Option<&str>,
) -> u8 {
    let project = match super::project::info(directory) {
        Ok(project) => project,
        Err(error) => return report(error, stderr),
    };
    let files = match discover(&project.root) {
        Ok(files) => files,
        Err(error) => return report(error, stderr),
    };
    // Inspect and validate *every* .srp entry before selecting by filename.
    // Filtering cannot bypass the 64-file limit or symlink safety checks.
    let files: Vec<_> = match filter {
        Some(fragment) => files
            .into_iter()
            .filter(|(label, _)| {
                label
                    .strip_prefix("tests/")
                    .is_some_and(|filename| filename.contains(fragment))
            })
            .collect(),
        None => files,
    };
    let discovered = files.len();
    // Optional CI gate: a valid but empty suite must not silently pass.
    // Preserve the original zero-test text or JSON report for inspection.
    let empty_status = u8::from(require_tests && discovered == 0);

    if list_only {
        if let Some(version) = json {
            let cases: Vec<_> = files
                .into_iter()
                .map(|(path, _)| Case { path, error: None })
                .collect();
            return match write_report(stdout, discovered, 0, 0, true, version, &cases) {
                Ok(()) => empty_status,
                Err(error) => report(error, stderr),
            };
        }
        if discovered == 0 {
            return match writeln!(stdout, "No .srp test files found in tests/ (0 discovered)") {
                Ok(()) => empty_status,
                Err(error) => report(error, stderr),
            };
        }
        for (path, _) in files {
            if let Err(error) = writeln!(stdout, "{path}") {
                return report(error, stderr);
            }
        }
        return match writeln!(stdout, "{discovered} tests discovered") {
            Ok(()) => 0,
            Err(error) => report(error, stderr),
        };
    }

    if discovered == 0 && json.is_none() {
        return match writeln!(
            stdout,
            "No .srp test files found in tests/ (0 passed, 0 failed)"
        ) {
            Ok(()) => empty_status,
            Err(error) => report(error, stderr),
        };
    }

    let mut passed = 0usize;
    let mut failed = 0usize;
    let mut cases = Vec::new();
    let loader = match FileSourceLoader::new(&project.root) {
        Ok(loader) if matches!(json, Some(JsonVersion::V2)) => loader.with_relative_diagnostics(),
        Ok(loader) => loader,
        Err(error) => {
            return report(
                format_args!("could not open trusted project loader: {error}"),
                stderr,
            )
        }
    };
    for (label, test_path) in files {
        let result = modules::compile_with_loader(&label, &loader, ModuleLimits::default())
            .map_err(Failure::Compile)
            .and_then(|program| {
                let read = |extension| {
                    fixture(&label, &test_path, loader.root(), extension).map_err(Failure::Fixture)
                };
                let input = read("stdin")?;
                let mut stdout = ExpectedOutput::from_fixture(read("stdout")?, "stdout")?;
                let mut stderr = ExpectedOutput::from_fixture(read("stderr")?, "stderr")?;
                let options = ExecutionOptions::default();
                match input {
                    Some(bytes) => program.execute_with_input(
                        &mut Cursor::new(bytes),
                        &mut stdout,
                        &mut stderr,
                        options,
                        InputLimits::default(),
                    ),
                    None => program.execute(&mut stdout, &mut stderr, options),
                }
                .map_err(Failure::Runtime)?;
                stdout.verify("stdout")?;
                stderr.verify("stderr")
            });
        match result {
            Ok(()) => {
                passed += 1;
                if json.is_some() {
                    cases.push(Case {
                        path: label,
                        error: None,
                    });
                } else if let Err(error) = writeln!(stdout, "PASS {label}") {
                    return report(error, stderr);
                }
            }
            Err(error) => {
                failed += 1;
                if let Some(version) = json {
                    let error = match version {
                        JsonVersion::V1 => ReportFailure::Text(error.text()),
                        JsonVersion::V2 => match error.structured() {
                            Ok(failure) => failure,
                            Err(error) => return report(error, stderr),
                        },
                    };
                    cases.push(Case {
                        path: label,
                        error: Some(error),
                    });
                } else {
                    if let Err(write_error) = writeln!(stdout, "FAIL {label}") {
                        return report(write_error, stderr);
                    }
                    // Ordinary program output is not an authenticated status
                    // line; include bounded compiler/runtime diagnostics only.
                    if let Err(write_error) = writeln!(stderr, "{label}: {}", error.text()) {
                        return report(write_error, stderr);
                    }
                }
            }
        }
    }
    if let Some(version) = json {
        if let Err(error) = write_report(stdout, discovered, passed, failed, false, version, &cases)
        {
            return report(error, stderr);
        }
    } else if let Err(error) = writeln!(stdout, "{passed} passed, {failed} failed") {
        return report(error, stderr);
    }

    if failed == 0 {
        empty_status
    } else {
        1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filename_filter_rejects_paths_options_controls_and_oversized_text() {
        for invalid in ["", "-skip", "a/b", "a\\\\b", "c:d", "bad\\nname"] {
            assert!(!valid_filter(invalid), "{invalid:?}");
        }
        assert!(!valid_filter(&"a".repeat(129)));
        for valid in ["test", "हिंदी", "a b", "a.srp", &"a".repeat(128)] {
            assert!(valid_filter(valid), "{valid:?}");
        }
    }

    #[test]
    fn json_report_handles_empty_and_mixed_cases_without_inventing_diagnostics() {
        let mut empty = Vec::new();
        write_report(&mut empty, 0, 0, 0, false, JsonVersion::V1, &[]).unwrap();
        assert_eq!(
            std::str::from_utf8(&empty).unwrap(),
            "{\"schema_version\":1,\"mode\":\"run\",\"discovered\":0,\"passed\":0,\"failed\":0,\"tests\":[]}\n"
        );
        let cases = vec![
            Case {
                path: "tests/a\".srp".into(),
                error: None,
            },
            Case {
                path: "tests/b.srp".into(),
                error: Some(ReportFailure::Text("bad\nmessage".into())),
            },
        ];
        let mut out = Vec::new();
        write_report(&mut out, 2, 1, 1, false, JsonVersion::V1, &cases).unwrap();
        let output = String::from_utf8(out).unwrap();
        assert!(output.contains("\"path\":\"tests/a\\\".srp\""));
        assert!(output.contains("\"status\":\"failed\",\"error\":\"bad\\nmessage\""));
        let mut listed = Vec::new();
        write_report(&mut listed, 2, 0, 0, true, JsonVersion::V1, &cases).unwrap();
        let listed = String::from_utf8(listed).unwrap();
        assert!(listed.contains("\"mode\":\"list\""));
        assert!(listed.contains("\"status\":\"discovered\""));
        assert!(!listed.contains("\"error\""));
    }
}
