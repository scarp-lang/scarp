//! Deterministic, local project test discovery and compiler-backed execution.
//!
//! Test files are ordinary Scarp programs in a project's immediate `tests/`
//! directory. Exiting without a compiler or runtime error is a pass. This is
//! not an assertion framework and does not execute files during discovery.

use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

use scarp::{
    modules::{self, FileSourceLoader, ModuleLimits},
    ExecutionOptions,
};

const MAX_TESTS: usize = 64;
const FAILURE_DETAIL_CHARS: usize = 1600;

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
        if let Some(error) = &case.error {
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
) -> u8 {
    let (loader, files) = match suite(directory) {
        Ok(suite) => suite,
        Err(error) => return report(error, stderr),
    };
    let discovered = files.len();

    if list_only {
        if json {
            let cases: Vec<_> = files
                .into_iter()
                .map(|(path, _)| Case { path, error: None })
                .collect();
            return match write_report(stdout, discovered, 0, 0, true, &cases) {
                Ok(()) => 0,
                Err(error) => report(error, stderr),
            };
        }
        if discovered == 0 {
            return match writeln!(stdout, "No .srp test files found in tests/ (0 discovered)") {
                Ok(()) => 0,
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
            Ok(()) => 0,
            Err(error) => report(error, stderr),
        };
    }

    let mut passed = 0usize;
    let mut failed = 0usize;
    let mut cases = Vec::new();
    for (label, _) in files {
        let mut program_stdout = Vec::new();
        let mut program_stderr = Vec::new();
        let result = modules::compile_with_loader(&label, &loader, ModuleLimits::default())
            .map_err(|error| error.to_string())
            .and_then(|program| {
                program
                    .execute(
                        &mut program_stdout,
                        &mut program_stderr,
                        ExecutionOptions::default(),
                    )
                    .map_err(|error| error.to_string())
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
        0
    } else {
        1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_report_handles_empty_and_mixed_cases_without_inventing_diagnostics() {
        let mut empty = Vec::new();
        write_report(&mut empty, 0, 0, 0, false, &[]).unwrap();
        assert_eq!(
            std::str::from_utf8(&empty).unwrap(),
            "{\"schema_version\":1,\"mode\":\"run\",\"discovered\":0,\"passed\":0,\"failed\":0,\"tests\":[]}\n"
        );
        let cases = vec![
            Case { path: "tests/a\".srp".into(), error: None },
            Case { path: "tests/b.srp".into(), error: Some("bad\nmessage".into()) },
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
    }
}
