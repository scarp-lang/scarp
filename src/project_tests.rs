//! Deterministic, local project test discovery and compiler-backed execution.
//!
//! Test files are ordinary Scarp programs in a project's immediate `tests/`
//! directory. Exiting without a compiler or runtime error is a pass. This is
//! not an assertion framework and does not execute files during discovery.

use std::{
    fs,
    io::{self, Write},
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
    for entry in fs::read_dir(&test_dir)
        .map_err(|error| format!("could not read project tests: {error}"))?
    {
        let entry = entry.map_err(|error| format!("could not enumerate tests: {error}"))?;
        let path = entry.path();
        if path.extension().map_or(true, |extension| extension != "srp") {
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
        if filename.chars().any(|ch| ch.is_control() || matches!(ch, ':' | '\\')) {
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

/// Execute each discovered test using the real module compiler and bounded
/// interpreter, without spawning subprocesses or forwarding interactive stdin.
pub fn run(directory: &Path, stdout: &mut impl Write, stderr: &mut impl Write) -> u8 {
    let (loader, files) = match suite(directory) {
        Ok(suite) => suite,
        Err(error) => return report(error, stderr),
    };
    if files.is_empty() {
        return match writeln!(stdout, "No .srp test files found in tests/ (0 passed, 0 failed)") {
            Ok(()) => 0,
            Err(error) => report(error, stderr),
        };
    }

    let mut passed = 0usize;
    let mut failed = 0usize;
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
                if let Err(error) = writeln!(stdout, "PASS {label}") {
                    return report(error, stderr);
                }
            }
            Err(error) => {
                failed += 1;
                if let Err(write_error) = writeln!(stdout, "FAIL {label}") {
                    return report(write_error, stderr);
                }
                // Compiler-controlled error details are bounded; test output is
                // captured but never printed as an untrusted status line.
                let detail: String = error.chars().take(FAILURE_DETAIL_CHARS).collect();
                if let Err(write_error) = writeln!(stderr, "{label}: {detail}") {
                    return report(write_error, stderr);
                }
            }
        }
    }
    if let Err(error) = writeln!(stdout, "{passed} passed, {failed} failed") {
        return report(error, stderr);
    }
    if failed == 0 { 0 } else { 1 }
}
