//! Compiler-owned executable basics, independent of public teaching repositories.
use scarp::{
    modules::{self, ModuleLimits, ModuleRunError},
    ExecutionOptions,
};
use std::{fs, io::Cursor, path::Path};

#[test]
fn ordinary_programs_match_exact_fixtures_and_failures_keep_both_streams_empty() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/core-language");
    let mut programs: Vec<_> = fs::read_dir(&root)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "srp"))
        .collect();
    programs.sort();
    assert_eq!(
        programs.len(),
        18,
        "keep the basic feature inventory explicit"
    );
    for path in programs {
        let error_path = path.with_extension("error");
        let expected_error = error_path
            .exists()
            .then(|| fs::read_to_string(error_path).unwrap());
        let input_path = path.with_extension("stdin");
        let bytes = if input_path.exists() {
            fs::read(input_path).unwrap()
        } else {
            Vec::new()
        };
        let mut input = Cursor::new(bytes);
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let mut options = ExecutionOptions::default();
        let steps_path = path.with_extension("steps");
        if steps_path.exists() {
            options.limits.max_steps = fs::read_to_string(steps_path)
                .unwrap()
                .trim()
                .parse()
                .unwrap();
        }
        let result = modules::compile_file(&path, ModuleLimits::default())
            .map_err(ModuleRunError::Diagnostic)
            .and_then(|program| {
                program.execute_with_input(
                    &mut input,
                    &mut out,
                    &mut err,
                    options,
                    Default::default(),
                )
            });
        match (result, expected_error) {
            (Ok(()), None) => {
                let expected = fs::read(path.with_extension("stdout")).unwrap();
                assert_eq!(out, expected, "{}", path.display());
                assert!(err.is_empty(), "unexpected stderr: {}", path.display());
            }
            (Err(ModuleRunError::Diagnostic(error)), Some(expected)) => {
                assert_eq!(
                    error.diagnostic.kind.code(),
                    expected.trim(),
                    "{}: {}",
                    path.display(),
                    error.render()
                );
                assert!(
                    out.is_empty() && err.is_empty(),
                    "non-atomic failure: {}",
                    path.display()
                );
                let file = path.file_name().unwrap().to_str().unwrap();
                assert!(error.render().contains(file), "{}", error.render());
            }
            (result, expected) => panic!(
                "{}: expected error {expected:?}, received {result:?}",
                path.display()
            ),
        }
    }
}
