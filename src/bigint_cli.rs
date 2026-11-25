use crate::write_output;
use scarp::bigint::file::{calculate, FileLimits, FileOperation, FileUsage};
use std::{
    ffi::OsString,
    io::{self, Write},
};

pub(super) const HELP: &str =
    "Usage: scarp bigint <add|sub> [options] [--] <left> <right> <new-output>

Experimental exact addition and subtraction on seekable decimal files.
Each input contains one optionally signed ASCII integer with no newline.
Output is canonical decimal with no newline and must be at a new path.

  --max-input-bytes N   Combined operand file sizes (default 67108864)
  --max-output-bytes N  Canonical decimal file size (default 33554432)
  --max-temp-bytes N    Scratch and staged output together (default 67108864)
  --max-buffer-bytes N  Processing buffer payload (default 262144)
  --stats              Print JSON sizes and phase timings after publication

Options precede paths. Use -- before a path beginning with a dash.
Staging takes place beside the output, on a filesystem that supports hard links.
";

pub(super) fn run(args: &[OsString], output: &mut impl Write, errors: &mut impl Write) -> u8 {
    if matches!(args, [flag] if flag == "--help" || flag == "-h") {
        return write_output(output.write_all(HELP.as_bytes()), errors);
    }
    let (operation, limits, stats, paths) = match parse(args) {
        Ok(request) => request,
        Err(message) => {
            let _ = writeln!(errors, "error: {message}\n\n{HELP}");
            return 2;
        }
    };
    let [left, right, destination] = paths;
    match calculate(operation, left, right, destination, limits) {
        Ok(usage) if stats => write_output(write_stats(output, usage), errors),
        Ok(_) => 0,
        Err(error) => {
            let _ = writeln!(errors, "error: {error}");
            1
        }
    }
}

fn parse(args: &[OsString]) -> Result<(FileOperation, FileLimits, bool, &[OsString; 3]), String> {
    let operation = match args.first().and_then(|arg| arg.to_str()) {
        Some("add") => FileOperation::Add,
        Some("sub") => FileOperation::Subtract,
        _ => return Err("expected bigint add or sub".into()),
    };
    let mut limits = FileLimits::default();
    let mut stats = false;
    let mut rest = &args[1..];
    while let Some((flag, tail)) = rest.split_first() {
        if flag == "--" {
            rest = tail;
            break;
        }
        if !flag.as_encoded_bytes().starts_with(b"-") {
            break;
        }
        if flag == "--stats" {
            stats = true;
            rest = tail;
            continue;
        }
        let flag = flag.to_str().ok_or("expected a UTF-8 option name")?;
        let destination = match flag {
            "--max-input-bytes" => Some(&mut limits.max_input_bytes),
            "--max-output-bytes" => Some(&mut limits.max_output_bytes),
            "--max-temp-bytes" => Some(&mut limits.max_temp_bytes),
            "--max-buffer-bytes" => None,
            _ => return Err(format!("unknown bigint option {flag}")),
        };
        let value = tail
            .first()
            .and_then(|arg| arg.to_str())
            .filter(|arg| arg.bytes().all(|digit| digit.is_ascii_digit()))
            .and_then(|arg| arg.parse::<u64>().ok())
            .ok_or_else(|| format!("expected an unsigned byte count after {flag}"))?;
        if let Some(destination) = destination {
            *destination = value;
        } else {
            limits.max_buffer_bytes = usize::try_from(value)
                .map_err(|_| format!("{flag} exceeds this platform's address range"))?;
        }
        rest = &tail[1..];
    }
    let paths = rest
        .try_into()
        .map_err(|_| "expected left, right and output paths")?;
    Ok((operation, limits, stats, paths))
}

fn write_stats(output: &mut impl Write, usage: FileUsage) -> io::Result<()> {
    writeln!(
        output,
        concat!(
            "{{\"input_bytes\":{},\"result_digits\":{},\"output_bytes\":{},",
            "\"scratch_bytes\":{},\"peak_temp_bytes\":{},\"buffer_bytes\":{},",
            "\"input_ns\":{},\"arithmetic_ns\":{},\"output_ns\":{},\"peak_rss_bytes\":{}}}"
        ),
        usage.input_bytes,
        usage.result_digits,
        usage.output_bytes,
        usage.scratch_bytes,
        usage.peak_temp_bytes,
        usage.buffer_bytes,
        usage.input_time.as_nanos(),
        usage.arithmetic_time.as_nanos(),
        usage.output_time.as_nanos(),
        peak_rss_bytes().map_or_else(|| "null".to_owned(), |bytes| bytes.to_string())
    )
}

fn peak_rss_bytes() -> Option<u64> {
    if !cfg!(target_os = "linux") {
        return None;
    }
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    let value = status
        .lines()
        .find_map(|line| line.strip_prefix("VmHWM:"))?;
    let mut fields = value.split_whitespace();
    let kib = fields.next()?.parse::<u64>().ok()?;
    (fields.next()? == "kB").then_some(kib.checked_mul(1024)?)
}
