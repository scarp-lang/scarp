use crate::write_output;
use scarp::bigint::file::{
    calculate, divide, divide_wide, FileOperation, FileUsage, WideDivisionLimits, WideRemainder,
};
use std::{
    ffi::OsString,
    io::{self, Write},
};

pub(super) const HELP: &str =
    "Usage: scarp bigint <add|sub|mul|div|div-wide> [options] [--] <left> <right> <new-output>

Experimental exact arithmetic on seekable decimal files.
Multiplication requires either operand to have magnitude at most 999999999.
The div command requires a nonzero right operand with magnitude at most 999999999.
The div-wide command supports a nonzero divisor that fits the limb workspace.
The file receives the truncated quotient and stdout receives the signed remainder.
Each input contains one optionally signed ASCII integer with no newline.
Output is canonical decimal with no newline and must be at a new path.

  --max-input-bytes N   Combined operand file sizes (default 67108864)
  --max-output-bytes N  Canonical decimal file size (default 33554432)
  --max-temp-bytes N    Scratch and staged output together (default 67108864)
  --max-buffer-bytes N  Processing buffer payload (default 262144)
  --max-limb-steps N    Arithmetic work bound for div-wide (default 100000000)
  --stats              Print JSON sizes, phase timings and any remainder

Options precede paths. Use -- before a path beginning with a dash.
Staging takes place beside the output, on a filesystem that supports hard links.
";

enum Operation {
    Arithmetic(FileOperation),
    Divide,
    DivideWide,
}

enum Remainder {
    Small(i32),
    Wide(WideRemainder, u64),
}

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
    let result = match operation {
        Operation::Arithmetic(operation) => {
            calculate(operation, left, right, destination, limits.file).map(|usage| (usage, None))
        }
        Operation::Divide => divide(left, right, destination, limits.file)
            .map(|usage| (usage.quotient, Some(Remainder::Small(usage.remainder)))),
        Operation::DivideWide => divide_wide(left, right, destination, limits).map(|usage| {
            (
                usage.quotient,
                Some(Remainder::Wide(usage.remainder, usage.limb_steps)),
            )
        }),
    };
    match result {
        Ok((usage, remainder)) if stats => {
            write_output(write_stats(output, usage, remainder.as_ref()), errors)
        }
        Ok((_, Some(Remainder::Small(remainder)))) => {
            write_output(writeln!(output, "{remainder}"), errors)
        }
        Ok((_, Some(Remainder::Wide(remainder, _)))) => {
            write_output(writeln!(output, "{remainder}"), errors)
        }
        Ok(_) => 0,
        Err(error) => {
            let _ = writeln!(errors, "error: {error}");
            1
        }
    }
}

fn parse(
    args: &[OsString],
) -> Result<(Operation, WideDivisionLimits, bool, &[OsString; 3]), String> {
    let operation = match args.first().and_then(|arg| arg.to_str()) {
        Some("add") => Operation::Arithmetic(FileOperation::Add),
        Some("sub") => Operation::Arithmetic(FileOperation::Subtract),
        Some("mul") => Operation::Arithmetic(FileOperation::Multiply),
        Some("div") => Operation::Divide,
        Some("div-wide") => Operation::DivideWide,
        _ => return Err("expected bigint add, sub, mul, div or div-wide".into()),
    };
    let mut limits = WideDivisionLimits::default();
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
            "--max-input-bytes" => Some(&mut limits.file.max_input_bytes),
            "--max-output-bytes" => Some(&mut limits.file.max_output_bytes),
            "--max-temp-bytes" => Some(&mut limits.file.max_temp_bytes),
            "--max-limb-steps" if matches!(operation, Operation::DivideWide) => {
                Some(&mut limits.max_limb_steps)
            }
            "--max-buffer-bytes" => None,
            _ => return Err(format!("unknown bigint option {flag}")),
        };
        let value = tail
            .first()
            .and_then(|arg| arg.to_str())
            .filter(|arg| arg.bytes().all(|digit| digit.is_ascii_digit()))
            .and_then(|arg| arg.parse::<u64>().ok())
            .ok_or_else(|| format!("expected an unsigned count after {flag}"))?;
        if let Some(destination) = destination {
            *destination = value;
        } else {
            limits.file.max_buffer_bytes = usize::try_from(value)
                .map_err(|_| format!("{flag} exceeds this platform's address range"))?;
        }
        rest = &tail[1..];
    }
    let paths = rest
        .try_into()
        .map_err(|_| "expected left, right and output paths")?;
    Ok((operation, limits, stats, paths))
}

fn write_stats(
    output: &mut impl Write,
    usage: FileUsage,
    remainder: Option<&Remainder>,
) -> io::Result<()> {
    write!(
        output,
        concat!(
            "{{\"input_bytes\":{},\"result_digits\":{},\"output_bytes\":{},",
            "\"scratch_bytes\":{},\"peak_temp_bytes\":{},\"buffer_bytes\":{},",
            "\"input_ns\":{},\"arithmetic_ns\":{},\"output_ns\":{},\"peak_rss_bytes\":{}"
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
        peak_rss_bytes().map_or_else(|| "null".to_owned(), |bytes| bytes.to_string()),
    )?;
    match remainder {
        Some(Remainder::Small(value)) => write!(output, ",\"remainder\":{value}")?,
        Some(Remainder::Wide(value, steps)) => {
            write!(output, ",\"remainder\":\"{value}\",\"limb_steps\":{steps}")?;
        }
        None => {}
    }
    writeln!(output, "}}")
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
