//! Explicit, bounded program input. No library API accesses process stdin.

use std::io::{self, BufRead, Write};

use crate::{
    diagnostics::{Diagnostic, DiagnosticKind as Kind, Span},
    limits::Budget,
    value::{Value, MAX_TEXT_BYTES},
};

/// Independent input quotas; existing `ExecutionOptions` struct literals remain valid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InputLimits {
    /// Raw bytes in one line, including any CR/LF terminator. The existing
    /// 1 MiB string ceiling also applies, even if this limit is raised.
    pub max_line_bytes: usize,
    /// Cumulative raw bytes consumed from the supplied buffered reader.
    pub max_total_bytes: usize,
    /// Selected `read_line()` calls, including a call that encounters EOF.
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
    .with_help("use run_with_input / execute_with_input, or redirect stdin to scarp exec; check never reads stdin and terminal prompts are not yet supported")
}

pub(crate) fn prompt_unavailable(span: Span) -> Diagnostic {
    Diagnostic::new(
        Kind::InputUnavailable,
        span,
        "ask requires interactive input and a prompt writer",
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
    limits: InputLimits,
    consumed: usize,
    reads: usize,
}

impl<'a> InputState<'a> {
    pub(crate) fn new(reader: Option<&'a mut dyn BufRead>, limits: InputLimits) -> Self {
        Self {
            reader,
            prompt_writer: None,
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
            limits,
            consumed: 0,
            reads: 0,
        }
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
        if self.reads >= self.limits.max_reads {
            return Err(limit(span, "input line-read quota exceeded"));
        }
        if self.consumed >= self.limits.max_total_bytes || self.limits.max_line_bytes == 0 {
            return Err(limit(span, "input byte quota exhausted before line read"));
        }
        budget.charge(prompt.len() as u64, span)?;
        let writer = self
            .prompt_writer
            .as_deref_mut()
            .expect("checked prompt writer");
        writer
            .write_all(prompt.as_bytes())
            .and_then(|_| writer.flush())
            .map_err(|_| {
                Diagnostic::new(
                    Kind::PromptIo,
                    span,
                    "interactive prompt could not be written and flushed",
                )
                .with_help(
                    "check the host prompt writer; visible prompt bytes cannot be rolled back",
                )
            })?;
        self.read_line(budget, span)
    }

    pub(crate) fn read_line(
        &mut self,
        budget: &mut Budget,
        span: Span,
    ) -> Result<Value, Diagnostic> {
        let reader = self.reader.as_mut().ok_or_else(|| unavailable(span))?;
        if self.reads >= self.limits.max_reads {
            return Err(limit(span, "input line-read quota exceeded"));
        }
        self.reads += 1;
        if self.consumed >= self.limits.max_total_bytes || self.limits.max_line_bytes == 0 {
            return Err(limit(span, "input byte quota exhausted before line read"));
        }
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
