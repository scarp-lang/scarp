//! Deterministic, local project test discovery and compiler-backed execution.
//!
//! Test files are ordinary Scarp programs in a project's immediate `tests/`
//! directory. Exiting without a compiler or runtime error is a pass. This is
//! not a separate assertion framework and does not execute files during discovery.

use std::{
    fs,
    io::{Cursor, Read, Write},
    path::{Path, PathBuf},
};

use scarp::{
    modules::{self, FileSourceLoader, ModuleLimits},
    ExecutionOptions, InputLimits,
};

const MAX_TESTS: usize = 64;
const FAILURE_DETAIL_CHARS: usize = 1600;
const MAX_FILTER_BYTES: usize = 128;
const MAX_STDIN_FIXTURE_BYTES: usize = 1024 * 1024;

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

fn suite(directory: &Path) -> Result<(FileSourceLoader, Vec<(String, PathBuf)>), String> {
    // The manifest/entrypoint must validate before inspecting or executing tests.
    let _ = super::project::entry_file(directory)?;
    let root = fs::canonicalize(directory)
        .map_err(|error| format!("could not open project directory: {error}"))?;
    let test_dir = root.join("tests");
    let metadata = fs::symlink_metadata(&test_dir)
        .map_err(|error| format!("could not inspect project tests directory: {error}"))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err("project tests path must be an ordinary directory, not a symlink".into());
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
    let loader = FileSourceLoader::new(&root)
        .map_err(|error| format!("could not open trusted project loader: {error}"))?;
    Ok((loader, files))
}


/// Read the optional sibling test input without reading process stdin.
///
/// A missing fixture preserves the previous no-input execution contract.
/// Malformed, oversized, or unsafe fixtures fail only their selected test;
/// listing remains read-only and does not inspect fixture bytes.
fn input_fixture(label: &str, test_path: &Path, root: &Path) -> Result<Option<Vec<u8>>, String> {
    let path = test_path.with_extension("stdin");
    let meta = match fs::symlink_metadata(&path) {
        Ok(meta) => meta,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("{label}: cannot inspect .stdin fixture: {error}")),
    };
    if !meta.file_type().is_file() {
        return Err(format!(
            "{label}: .stdin fixture must be an ordinary file, not a symlink or directory"
        ));
    }
    let canonical = fs::canonicalize(&path)
        .map_err(|error| format!("{label}: cannot resolve .stdin fixture: {error}"))?;
    if !canonical.starts_with(root) {
        return Err(format!("{label}: .stdin fixture escapes the trusted project root"));
    }
    let file = fs::File::open(&canonical)
        .map_err(|error| format!("{label}: cannot open .stdin fixture: {error}"))?;
    let opened = file
        .metadata()
        .map_err(|error| format!("{label}: cannot stat .stdin fixture: {error}"))?;
    if !opened.is_file() {
        return Err(format!("{label}: .stdin fixture is not a regular file"));
    }
    if opened.len() > MAX_STDIN_FIXTURE_BYTES as u64 {
        return Err(format!(
            "{label}: .stdin fixture exceeds the 1 MiB input limit"
        ));
    }
    let mut bytes = Vec::new();
    file.take(MAX_STDIN_FIXTURE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("{label}: cannot read .stdin fixture: {error}"))?;
    if bytes.len() > MAX_STDIN_FIXTURE_BYTES {
        return Err(format!(
            "{label}: .stdin fixture grew beyond the 1 MiB input limit"
        ));
    }
    Ok(Some(bytes))
}

/// One captured test outcome for a stable JSON report. The error text is
/// bounded and is not mixed with the program's captured stdout or stderr.
struct Case {
    path: String,
    error: Option<String>,
}

fn write_report(
    stdout: &mut impl Write,
    discovered: usize,
    passed: usize,
    failed: usize,
    list_only: bool,
    cases: &[Case],
) -> std::io::Result<()> {
    use super::project_info::quote_json;

    let mode = if list_only { "list" } else { "run" };
    write!(
        stdout,
        "{{\"schema_version\":1,\"mode\":\"{mode}\",\"discovered\":{discovered},\"passed\":{passed},\"failed\":{failed},\"tests\":["
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
            write!(stdout, ",\"error\":{}", quote_json(error))?;
        }
        write!(stdout, "}}")?;
    }
    writeln!(stdout, "]}}")
}

/// Discover direct project tests without compiling or executing anything,
/// or execute them through the real compiler with the original default
/// budgets. JSON reports emit one schema-version-1 result object.
pub fn run(
    directory: &Path,
    stdout: &mut impl Write,
    stderr: &mut impl Write,
    json: bool,
    list_only: bool,
    require_tests: bool,
    filter: Option<&str>,
) -> u8 {
    let (loader, files) = match suite(directory) {
        Ok(suite) => suite,
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
        if json {
            let cases: Vec<_> = files
                .into_iter()
                .map(|(path, _)| Case { path, error: None })
                .collect();
            return match write_report(stdout, discovered, 0, 0, true, &cases) {
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

    if discovered == 0 && !json {
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
    for (label, test_path) in files {
        let mut program_stdout = Vec::new();
        let mut program_stderr = Vec::new();
        let result = modules::compile_with_loader(&label, &loader, ModuleLimits::default())
            .map_err(|error| error.to_string())
            .and_then(|program| {
                let fixture = input_fixture(&label, &test_path, loader.root())?;
                match fixture {
                    Some(bytes) => {
                        let mut reader = Cursor::new(bytes);
                        program
                            .execute_with_input(
                                &mut reader,
                                &mut program_stdout,
                                &mut program_stderr,
                                ExecutionOptions::default(),
                                InputLimits::default(),
                            )
                            .map_err(|error| error.to_string())
                    }
                    None => program
                        .execute(
                            &mut program_stdout,
                            &mut program_stderr,
                            ExecutionOptions::default(),
                        )
                        .map_err(|error| error.to_string()),
                }
            });
        match result {
            Ok(()) => {
                passed += 1;
                if json {
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
                let detail: String = error.chars().take(FAILURE_DETAIL_CHARS).collect();
                if json {
                    cases.push(Case {
                        path: label,
                        error: Some(detail),
                    });
                } else {
                    if let Err(write_error) = writeln!(stdout, "FAIL {label}") {
                        return report(write_error, stderr);
                    }
                    // Ordinary program output is not an authenticated status
                    // line; include bounded compiler/runtime diagnostics only.
                    if let Err(write_error) = writeln!(stderr, "{label}: {detail}") {
                        return report(write_error, stderr);
                    }
                }
            }
        }
    }
    if json {
        if let Err(error) = write_report(stdout, discovered, passed, failed, false, &cases) {
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
        write_report(&mut empty, 0, 0, 0, false, &[]).unwrap();
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
                error: Some("bad\nmessage".into()),
            },
        ];
        let mut out = Vec::new();
        write_report(&mut out, 2, 1, 1, false, &cases).unwrap();
        let output = String::from_utf8(out).unwrap();
        assert!(output.contains("\"path\":\"tests/a\\\".srp\""));
        assert!(output.contains("\"status\":\"failed\",\"error\":\"bad\\nmessage\""));
        let mut listed = Vec::new();
        write_report(&mut listed, 2, 0, 0, true, &cases).unwrap();
        let listed = String::from_utf8(listed).unwrap();
        assert!(listed.contains("\"mode\":\"list\""));
        assert!(listed.contains("\"status\":\"discovered\""));
        assert!(!listed.contains("\"error\""));
    }
}
