use scarp::bigint::file::{
    calculate, calculate_with_progress, FileError, FileLimits, FileOperation, FilePhase,
    FileResource,
};
use std::{
    fs::{self, OpenOptions},
    io::{self, Seek, SeekFrom, Write},
    ops::ControlFlow,
    path::PathBuf,
    process::{Command, Output},
    sync::Barrier,
};
use FileOperation::{Add, Subtract};

struct Directory(PathBuf);

impl Directory {
    fn new(name: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("scarp-bigint-file-{name}-{}", std::process::id()));
        fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn operands(&self, left: impl AsRef<[u8]>, right: impl AsRef<[u8]>) -> [PathBuf; 3] {
        let paths = ["left", "right", "out"].map(|name| self.0.join(name));
        fs::write(&paths[0], left).unwrap();
        fs::write(&paths[1], right).unwrap();
        paths
    }

    fn assert_staging_clean(&self) {
        assert!(fs::read_dir(&self.0).unwrap().all(|entry| {
            !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".scarp-bigint-")
        }));
    }

    fn cli(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_scarp"))
            .current_dir(&self.0)
            .args(args)
            .output()
            .unwrap()
    }
}

impl Drop for Directory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn signed_values_match_the_interpreter_across_chunk_boundaries() {
    let dir = Directory::new("boundaries");
    let nines = "9".repeat(250);
    let power = format!("1{}", "0".repeat(250));
    let zeros = format!("-{}", "0".repeat(38));
    let equal = format!("-000{nines}");
    let negative = format!("-{nines}");
    let uneven = format!("+00090{}700000000001", "0".repeat(41));
    for (left, right) in [
        (nines.as_str(), "1"),
        (power.as_str(), "1"),
        ("1", power.as_str()),
        (negative.as_str(), "-1"),
        (equal.as_str(), negative.as_str()),
        (nines.as_str(), equal.as_str()),
        (zeros.as_str(), "0"),
        (zeros.as_str(), negative.as_str()),
        (uneven.as_str(), "-41000000001"),
        ("123456789000000000001", "123456789000000000002"),
    ] {
        let [left_path, right_path, output] = dir.operands(left, right);
        for (operation, operator) in [(Add, '+'), (Subtract, '-')] {
            let source = format!("print(bigint(\"{left}\") {operator} bigint(\"{right}\"));");
            let mut expected = Vec::new();
            scarp::run(&source, &mut expected).unwrap();
            assert_eq!(expected.pop(), Some(b'\n'));
            for max_buffer_bytes in [30, 52, 118, 256 * 1024] {
                let usage = calculate(
                    operation,
                    &left_path,
                    &right_path,
                    &output,
                    FileLimits {
                        max_buffer_bytes,
                        ..FileLimits::default()
                    },
                )
                .unwrap();
                assert_eq!(fs::read(&output).unwrap(), expected);
                assert_eq!(usage.output_bytes, expected.len() as u64);
                assert_eq!(
                    usage.result_digits,
                    usage.output_bytes - u64::from(expected[0] == b'-')
                );
                assert_eq!(
                    usage.peak_temp_bytes,
                    usage.scratch_bytes + usage.output_bytes
                );
                assert!(usage.buffer_bytes <= max_buffer_bytes);
                fs::remove_file(&output).unwrap();
            }
        }
    }
    dir.assert_staging_clean();
}

#[test]
fn seeded_signed_arithmetic_matches_i128() {
    let dir = Directory::new("oracle");
    let mut seed = 0x3411_234b_a185_70d0_u64;
    let mut sample = || {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        i128::from(seed as i64) * 1_000_000_001
    };
    for case in 0..80 {
        let left = sample() / (case % 7 + 1);
        let right = sample() / (case % 11 + 1);
        let [left_path, right_path, output] = dir.operands(left.to_string(), right.to_string());
        for (operation, expected) in [(Add, left + right), (Subtract, left - right)] {
            calculate(
                operation,
                &left_path,
                &right_path,
                &output,
                FileLimits {
                    max_buffer_bytes: 30 + 22 * (case as usize % 3),
                    ..FileLimits::default()
                },
            )
            .unwrap();
            assert_eq!(fs::read_to_string(&output).unwrap(), expected.to_string());
            fs::remove_file(&output).unwrap();
        }
    }
}

#[test]
fn resource_limits_accept_equality_and_reject_before_publication() {
    let dir = Directory::new("limits");
    let [left, right, output] = dir.operands("9999999999", "1");
    let mut limits = FileLimits {
        max_buffer_bytes: 30,
        ..FileLimits::default()
    };
    let usage = calculate(Add, &left, &right, &output, limits).unwrap();
    assert_eq!(usage.result_digits, 11);
    fs::remove_file(&output).unwrap();
    limits.max_input_bytes = usage.input_bytes;
    limits.max_temp_bytes = usage.peak_temp_bytes;
    limits.max_output_bytes = usage.output_bytes;
    for (changed, kind) in [
        (
            FileLimits {
                max_input_bytes: limits.max_input_bytes - 1,
                ..limits
            },
            FileResource::InputBytes,
        ),
        (
            FileLimits {
                max_output_bytes: limits.max_output_bytes - 1,
                ..limits
            },
            FileResource::OutputBytes,
        ),
        (
            FileLimits {
                max_temp_bytes: limits.max_temp_bytes - 1,
                ..limits
            },
            FileResource::TemporaryBytes,
        ),
        (
            FileLimits {
                max_buffer_bytes: limits.max_buffer_bytes - 1,
                ..limits
            },
            FileResource::BufferBytes,
        ),
    ] {
        let error = calculate(Add, &left, &right, &output, changed).unwrap_err();
        assert!(
            matches!(error, FileError::Limit { resource, required, limit } if resource == kind && required == limit + 1),
            "{error}"
        );
        assert!(!output.exists());
        dir.assert_staging_clean();
    }
    assert_eq!(
        calculate(Add, &left, &right, &output, limits)
            .unwrap()
            .peak_temp_bytes,
        usage.peak_temp_bytes
    );
    assert_eq!(fs::read_to_string(&output).unwrap(), "10000000000");
}

#[test]
fn malformed_input_reports_the_byte_and_produces_no_output() {
    let dir = Directory::new("malformed");
    for (text, offset) in [
        ("", 0),
        ("-", 1),
        ("+", 1),
        ("1\r\n", 1),
        ("1_2", 1),
        ("12é", 2),
        ("012345678901+4", 12),
    ] {
        let [left, right, output] = dir.operands("0010", text);
        let limits = FileLimits {
            max_buffer_bytes: 30,
            ..FileLimits::default()
        };
        assert!(matches!(
            calculate(Add, &left, &right, &output, limits),
            Err(FileError::InvalidDecimal { operand: "right operand", offset: actual }) if actual == offset
        ));
        assert!(!output.exists());
        dir.assert_staging_clean();
    }
    let [left, _, output] = dir.operands("1", "1");
    assert!(matches!(
        calculate(Add, left, &dir.0, output, FileLimits::default()),
        Err(FileError::NotRegularFile { .. })
    ));
}

#[test]
fn cancellation_at_every_phase_leaves_inputs_and_destination_intact() {
    let dir = Directory::new("cancel");
    let [left, right, output] = dir.operands("80000000000000000001", "79999999999999999999");
    for phase in [
        FilePhase::ReadInputs,
        FilePhase::CompareMagnitudes,
        FilePhase::Arithmetic,
        FilePhase::WriteDecimal,
        FilePhase::Publish,
    ] {
        assert!(matches!(
            calculate_with_progress(
                Subtract,
                &left,
                &right,
                &output,
                FileLimits::default(),
                |progress| {
                    assert!(progress.completed <= progress.total);
                    assert!(!output.exists());
                    if progress.phase == phase
                        && (phase == FilePhase::Publish || progress.completed != 0)
                    {
                        ControlFlow::Break(())
                    } else {
                        ControlFlow::Continue(())
                    }
                }
            ),
            Err(FileError::Cancelled)
        ));
        assert!(!output.exists());
        dir.assert_staging_clean();
    }
    assert_eq!(fs::read_to_string(&left).unwrap(), "80000000000000000001");
    assert_eq!(fs::read_to_string(&right).unwrap(), "79999999999999999999");
}

#[test]
fn corruption_and_changed_inputs_are_detected_before_commit() {
    let dir = Directory::new("integrity");
    let [left, right, output] = dir.operands("1234567890123", "456789123456");
    for (offset, message) in [(0, "header"), (24, "limb")] {
        assert!(
            matches!(
                calculate_with_progress(
                    Add,
                    &left,
                    &right,
                    &output,
                    FileLimits::default(),
                    |progress| {
                        if progress.phase == FilePhase::WriteDecimal && progress.completed == 0 {
                            let scratch = fs::read_dir(&dir.0)
                                .unwrap()
                                .map(|entry| entry.unwrap().path().join("scratch"))
                                .find(|path| path.is_file())
                                .unwrap();
                            let mut file = OpenOptions::new().write(true).open(scratch).unwrap();
                            file.seek(SeekFrom::Start(offset)).unwrap();
                            file.write_all(&[0xff]).unwrap();
                        }
                        ControlFlow::Continue(())
                    }
                ),
                Err(FileError::ScratchIntegrity)
            ),
            "modified {message}"
        );
        assert!(!output.exists());
        dir.assert_staging_clean();
    }
    assert!(matches!(
        calculate_with_progress(
            Add,
            &left,
            &right,
            &output,
            FileLimits::default(),
            |progress| {
                if progress.phase == FilePhase::Publish {
                    OpenOptions::new()
                        .append(true)
                        .open(&left)
                        .unwrap()
                        .write_all(b"1")
                        .unwrap();
                }
                ControlFlow::Continue(())
            }
        ),
        Err(FileError::InputChanged {
            operand: "left operand"
        })
    ));
    assert!(!output.exists());
    dir.assert_staging_clean();
}

#[test]
fn concurrent_publication_commits_exactly_one_complete_result() {
    let dir = Directory::new("race");
    let [left, right, output] = dir.operands("3", "1");
    let barrier = Barrier::new(2);
    let [sum, difference] = std::thread::scope(|scope| {
        let run = |operation| {
            calculate_with_progress(
                operation,
                &left,
                &right,
                &output,
                FileLimits::default(),
                |progress| {
                    if progress.phase == FilePhase::Publish {
                        barrier.wait();
                    }
                    ControlFlow::Continue(())
                },
            )
        };
        let add = scope.spawn(move || run(Add));
        let sub = scope.spawn(move || run(Subtract));
        [add.join().unwrap(), sub.join().unwrap()]
    });
    assert_ne!(sum.is_ok(), difference.is_ok());
    let expected = if sum.is_ok() { "4" } else { "2" };
    let error = sum.err().or_else(|| difference.err()).unwrap();
    assert!(
        matches!(error, FileError::Io { source, .. } if source.kind() == io::ErrorKind::AlreadyExists)
    );
    assert_eq!(fs::read_to_string(output).unwrap(), expected);
    dir.assert_staging_clean();
}

#[test]
fn command_accepts_non_ascii_paths_and_keeps_existing_files() {
    let dir = Directory::new("cli");
    fs::write(dir.0.join("-é.txt"), "999999999999999999").unwrap();
    fs::write(dir.0.join("right file.txt"), "1").unwrap();
    let args = [
        "bigint",
        "add",
        "--stats",
        "--max-buffer-bytes",
        "30",
        "--",
        "-é.txt",
        "right file.txt",
        "out",
    ];
    let output = dir.cli(&args);
    assert!(output.status.success(), "{:?}", output.stderr);
    let stats = String::from_utf8(output.stdout).unwrap();
    assert!(stats.contains("\"result_digits\":19") && stats.contains("\"buffer_bytes\":30"));
    assert_eq!(
        fs::read_to_string(dir.0.join("out")).unwrap(),
        "1000000000000000000"
    );
    let conflict = dir.cli(&args);
    assert_eq!(conflict.status.code(), Some(1));
    assert!(conflict.stdout.is_empty());
    assert_eq!(
        fs::read_to_string(dir.0.join("out")).unwrap(),
        "1000000000000000000"
    );
    assert_eq!(
        dir.cli(&["bigint", "sub", "--max-temp-bytes", "-1", "a", "b", "out"])
            .status
            .code(),
        Some(2)
    );
    assert_eq!(
        dir.cli(&["guide", "bigint"]).stdout,
        dir.cli(&["bigint", "--help"]).stdout
    );
    dir.assert_staging_clean();
}
