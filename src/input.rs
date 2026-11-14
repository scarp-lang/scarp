//! Explicit, bounded program input. No library API accesses process stdin.

use std::io::{self, BufRead, Write};

use crate::{
    ast::OutputTarget,
    diagnostics::{Diagnostic, DiagnosticKind as Kind, Span},
    limits::Budget,
    numeric::{self, Builtin},
    types::SourceType,
    value::{Value, ValueType, MAX_TEXT_BYTES},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum IoBuiltin {
    ReadLine,
    ReadLineOr,
    Ask,
    AskOr,
    TryReadLine,
    Integer,
    Float,
    BigInt,
    FlushStdout,
    FlushStderr,
}

impl IoBuiltin {
    pub(crate) fn from_name(name: &str) -> Option<Self> {
        match name {
            "read_line" => Some(Self::ReadLine),
            "read_line_or" => Some(Self::ReadLineOr),
            "ask" | "input" => Some(Self::Ask),
            "ask_or" => Some(Self::AskOr),
            "try_read_line" => Some(Self::TryReadLine),
            "input_integer" => Some(Self::Integer),
            "input_float" => Some(Self::Float),
            "input_bigint" => Some(Self::BigInt),
            "flush_stdout" => Some(Self::FlushStdout),
            "flush_stderr" => Some(Self::FlushStderr),
            _ => None,
        }
    }

    pub(crate) fn prompts(self) -> bool {
        matches!(
            self,
            Self::Ask | Self::AskOr | Self::Integer | Self::Float | Self::BigInt
        )
    }

    pub(crate) fn has_fallback(self) -> bool {
        matches!(self, Self::ReadLineOr | Self::AskOr)
    }

    pub(crate) fn arity(self) -> usize {
        usize::from(self.prompts()) + usize::from(self.has_fallback())
    }

    pub(crate) fn result_type(self) -> SourceType {
        match self {
            Self::TryReadLine => {
                SourceType::tuple(vec![ValueType::Boolean.into(), ValueType::String.into()])
                    .expect("valid input tuple type")
            }
            Self::Integer => ValueType::Integer.into(),
            Self::Float => ValueType::Float.into(),
            Self::BigInt => ValueType::BigInt.into(),
            Self::FlushStdout | Self::FlushStderr => ValueType::Unit.into(),
            _ => ValueType::String.into(),
        }
    }

    pub(crate) fn output_target(self) -> Option<OutputTarget> {
        match self {
            Self::FlushStdout => Some(OutputTarget::Stdout),
            Self::FlushStderr => Some(OutputTarget::Stderr),
            _ => None,
        }
    }

    pub(crate) fn convert(
        self,
        value: Value,
        budget: &mut Budget,
        span: Span,
    ) -> Result<Value, Diagnostic> {
        let (conversion, action) = match self {
            Self::Integer => (Builtin::TextToInteger, "input_integer"),
            Self::Float => (Builtin::TextToFloat, "input_float"),
            Self::BigInt => (Builtin::BigInt, "input_bigint"),
            _ => return Ok(value),
        };
        numeric::evaluate_with_budget(conversion, &[value], budget, span).map_err(|mut error| {
            error.message = format!("{action}: {}", error.message);
            error
        })
    }
}

/// Independent input quotas; existing `ExecutionOptions` struct literals remain valid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InputLimits {
    /// Raw bytes in one line, including any CR/LF terminator. The existing
    /// 1 MiB string ceiling also applies, even if this limit is raised.
    pub max_line_bytes: usize,
    /// Cumulative raw bytes consumed from the supplied buffered reader.
    pub max_total_bytes: usize,
    /// Selected input calls, including a call that encounters EOF.
    pub max_reads: usize,
}

impl Default for InputLimits {
    fn default() -> Self {
        Self {
            max_line_bytes: 64 * 1024,
            max_total_bytes: 1024 * 1024,
            max_reads: 4096,
        }
    }
}

pub(crate) fn unavailable(span: Span) -> Diagnostic {
    Diagnostic::new(
        Kind::InputUnavailable,
        span,
        "read_line requires an explicit input reader; this execution mode has none",
    )
    .with_help("use run_with_input / execute_with_input, redirect stdin to scarp exec, or explicitly enable --interactive for terminal input; check never reads stdin")
}

pub(crate) fn prompt_unavailable(span: Span) -> Diagnostic {
    Diagnostic::new(
        Kind::InputUnavailable,
        span,
        "interactive input prompts require an explicit reader and prompt writer",
    )
    .with_help(
        "enable --interactive on scarp exec/run, or use an explicit interactive Rust host API",
    )
}

fn limit(span: Span, message: &str) -> Diagnostic {
    Diagnostic::new(Kind::InputLimit, span, message).with_help(
        "reduce input or raise the relevant InputLimits / --max-input-* quota; line limits include CR/LF and the string ceiling still applies",
    )
}

pub(crate) struct InputState<'a> {
    reader: Option<&'a mut dyn BufRead>,
    prompt_writer: Option<&'a mut dyn Write>,
    error_writer: Option<&'a mut dyn Write>,
    limits: InputLimits,
    consumed: usize,
    reads: usize,
}

impl<'a> InputState<'a> {
    pub(crate) fn new(reader: Option<&'a mut dyn BufRead>, limits: InputLimits) -> Self {
        Self {
            reader,
            prompt_writer: None,
            error_writer: None,
            limits,
            consumed: 0,
            reads: 0,
        }
    }

    /// The caller explicitly supplies both capabilities; no ambient stdin or
    /// stdout is acquired through the library.
    pub(crate) fn interactive(
        reader: &'a mut dyn BufRead,
        prompt_writer: &'a mut dyn Write,
        limits: InputLimits,
    ) -> Self {
        Self {
            reader: Some(reader),
            prompt_writer: Some(prompt_writer),
            error_writer: None,
            limits,
            consumed: 0,
            reads: 0,
        }
    }

    pub(crate) fn with_error_writer(mut self, writer: &'a mut dyn Write) -> Self {
        self.error_writer = Some(writer);
        self
    }

    pub(crate) fn output_writer(&mut self, target: OutputTarget) -> Option<&mut (dyn Write + 'a)> {
        match target {
            OutputTarget::Stdout => self.prompt_writer.as_deref_mut(),
            OutputTarget::Stderr => self.error_writer.as_deref_mut(),
        }
    }

    fn check_read_limits(&self, span: Span) -> Result<(), Diagnostic> {
        if self.reads >= self.limits.max_reads {
            return Err(limit(span, "input line-read quota exceeded"));
        }
        if self.consumed >= self.limits.max_total_bytes || self.limits.max_line_bytes == 0 {
            return Err(limit(span, "input byte quota exhausted before line read"));
        }
        Ok(())
    }

    /// Visible prompts are intentionally not transactional. Preflight local
    /// quotas before showing the prompt, and charge work for prompt length.
    pub(crate) fn ask(
        &mut self,
        prompt: &str,
        budget: &mut Budget,
        span: Span,
    ) -> Result<Value, Diagnostic> {
        if self.reader.is_none() || self.prompt_writer.is_none() {
            return Err(prompt_unavailable(span));
        }
        self.check_read_limits(span)?;
        budget.charge(prompt.len() as u64, span)?;
        // A selected read must be able to pay for its first buffer probe.
        // Preflight it before the prompt becomes irrevocably visible, without
        // changing the established work charge when the read actually starts.
        budget.preflight_steps(1, span)?;
        let writer = self
            .prompt_writer
            .as_deref_mut()
            .expect("checked prompt writer");
        let mut remaining = prompt.as_bytes();
        while !remaining.is_empty() {
            match writer.write(remaining) {
                Ok(written) if (1..=remaining.len()).contains(&written) => {
                    remaining = &remaining[written..];
                }
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {
                    budget.charge(1, span)?;
                    budget.preflight_steps(1, span)?;
                }
                _ => return Err(prompt_io(span)),
            }
        }
        writer.flush().map_err(|_| prompt_io(span))?;
        self.read_line(budget, span)
    }

    pub(crate) fn read_line(
        &mut self,
        budget: &mut Budget,
        span: Span,
    ) -> Result<Value, Diagnostic> {
        if self.reader.is_none() {
            return Err(unavailable(span));
        }
        self.check_read_limits(span)?;
        let reader = self.reader.as_mut().expect("checked input reader");
        self.reads += 1;
        let line_limit = self.limits.max_line_bytes.min(MAX_TEXT_BYTES);
        let mut bytes = Vec::new();
        loop {
            // Reader calls (including Interrupted retries and EOF probes) cost
            // work. An adversarial reader cannot cause an unlimited retry loop.
            budget.charge(1, span)?;
            let available = match reader.fill_buf() {
                Ok(bytes) => bytes,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(_) => {
                    // Do not include arbitrary, potentially unbounded host error text.
                    return Err(Diagnostic::new(
                        Kind::InputIo,
                        span,
                        "program input reader failed",
                    ));
                }
            };
            if available.is_empty() {
                if bytes.is_empty() {
                    return Err(Diagnostic::new(
                        Kind::InputEof,
                        span,
                        "end of input before a new line",
                    )
                    .with_help(
                        "supply another line; an empty line needs an LF or CRLF terminator",
                    ));
                }
                break;
            }
            let allowance = (line_limit - bytes.len())
                .min(self.limits.max_total_bytes - self.consumed)
                .min(4096);
            if allowance == 0 {
                return Err(limit(span, "input line or cumulative byte quota exceeded"));
            }
            let scan = &available[..available.len().min(allowance)];
            // Prepay each inspected byte without charging future lines that
            // happen to be present in the host buffer.
            let mut newline = None;
            for (index, &byte) in scan.iter().enumerate() {
                budget.charge(3, span)?;
                if byte == b'\n' {
                    newline = Some(index);
                    break;
                }
            }
            let count = newline.map_or(scan.len(), |index| index + 1);
            // Charge raw payload before reserving, including terminators. This
            // bounds temporary input storage, not just the final shared string.
            budget.charge_text_allocation(count, span)?;
            let needed = bytes.len() + count;
            if needed > bytes.capacity() {
                // Bounded geometric growth avoids quadratic reallocation with
                // readers that expose only one byte at a time.
                let capacity = needed
                    .max(bytes.capacity().saturating_mul(2))
                    .max(64)
                    .min(line_limit);
                budget.charge(bytes.len() as u64, span)?;
                bytes
                    .try_reserve_exact(capacity - bytes.len())
                    .map_err(|_| limit(span, "could not reserve bounded program input storage"))?;
            }
            bytes.extend_from_slice(&scan[..count]);
            reader.consume(count);
            self.consumed += count;
            if newline.is_some() {
                bytes.pop();
                if bytes.last() == Some(&b'\r') {
                    bytes.pop();
                }
                break;
            }
        }
        let text = String::from_utf8(bytes).map_err(|_| {
            Diagnostic::new(
                Kind::InputEncoding,
                span,
                "program input line is not valid UTF-8",
            )
            .with_help("encode program input as UTF-8; invalid bytes are never replaced silently")
        })?;
        Ok(Value::from(text))
    }
}

fn prompt_io(span: Span) -> Diagnostic {
    Diagnostic::new(
        Kind::PromptIo,
        span,
        "interactive prompt could not be written and flushed",
    )
    .with_help("check the host prompt writer; visible prompt bytes cannot be rolled back")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn prompt_is_not_exposed_without_work_for_first_read() {
        let mut reader = Cursor::new(b"Ada\n");
        let mut stdout = Vec::new();
        let mut budget = Budget::new(6);
        let span = Span::default();
        {
            let mut input =
                InputState::interactive(&mut reader, &mut stdout, InputLimits::default());
            let error = input.ask("Name: ", &mut budget, span).unwrap_err();
            assert_eq!(error.kind, Kind::ExecutionLimit);
        }
        assert!(stdout.is_empty());
        assert_eq!(reader.position(), 0);
    }

    #[test]
    fn first_read_probe_is_not_charged_twice() {
        let mut reader = Cursor::new(b"");
        let mut stdout = Vec::new();
        let mut budget = Budget::new(2);
        let span = Span::default();
        {
            let mut input =
                InputState::interactive(&mut reader, &mut stdout, InputLimits::default());
            let error = input.ask(">", &mut budget, span).unwrap_err();
            assert_eq!(error.kind, Kind::InputEof);
        }
        assert_eq!(stdout, b">");
        assert_eq!(reader.position(), 0);
    }

    #[test]
    fn an_interrupted_prompt_writer_cannot_retry_past_the_work_limit() {
        #[derive(Default)]
        struct InterruptedWriter(usize);

        impl Write for InterruptedWriter {
            fn write(&mut self, _: &[u8]) -> io::Result<usize> {
                self.0 += 1;
                Err(io::ErrorKind::Interrupted.into())
            }

            fn flush(&mut self) -> io::Result<()> {
                panic!("the interrupted prompt should never flush");
            }
        }

        let mut reader = Cursor::new(b"Ada\n");
        let mut writer = InterruptedWriter::default();
        let mut input = InputState::interactive(&mut reader, &mut writer, InputLimits::default());
        let error = input
            .ask(">", &mut Budget::new(3), Span::default())
            .unwrap_err();
        assert_eq!(error.kind, Kind::ExecutionLimit);
        assert_eq!(writer.0, 2);
        assert_eq!(reader.position(), 0);
    }

    #[test]
    fn prompt_retry_cannot_spend_the_first_reader_probe() {
        struct InterruptOnce(Vec<u8>, bool);

        impl Write for InterruptOnce {
            fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
                if !std::mem::replace(&mut self.1, true) {
                    return Err(io::ErrorKind::Interrupted.into());
                }
                self.0.write(bytes)
            }

            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }

        let mut writer = InterruptOnce(Vec::new(), false);
        let mut reader = Cursor::new(b"");
        let mut input = InputState::interactive(&mut reader, &mut writer, InputLimits::default());
        let error = input
            .ask(">", &mut Budget::new(2), Span::default())
            .unwrap_err();
        assert_eq!(error.kind, Kind::ExecutionLimit);
        assert!(writer.0.is_empty());

        let mut writer = InterruptOnce(Vec::new(), false);
        let mut input = InputState::interactive(&mut reader, &mut writer, InputLimits::default());
        let error = input
            .ask(">", &mut Budget::new(3), Span::default())
            .unwrap_err();
        assert_eq!(error.kind, Kind::InputEof);
        assert_eq!(writer.0, b">");
    }
}
