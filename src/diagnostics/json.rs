use super::{location, Diagnostic, Location, Span};
use crate::{json::string, modules::ModuleDiagnostic};
use std::io::{self, Write};

struct SourceLocation<'a> {
    path: &'a str,
    span: Span,
    start: Location,
    end: Location,
}
fn position(out: &mut impl Write, value: Location) -> io::Result<()> {
    write!(
        out,
        "{{\"line\":{},\"column\":{}}}",
        value.line, value.column
    )
}
fn source_location(out: &mut impl Write, value: Option<SourceLocation<'_>>) -> io::Result<()> {
    let Some(value) = value else {
        return out.write_all(b"null");
    };
    out.write_all(b"{\"path\":")?;
    string(out, value.path)?;
    write!(
        out,
        ",\"span\":[{},{}],\"start\":",
        value.span.start, value.span.end
    )?;
    position(out, value.start)?;
    out.write_all(b",\"end\":")?;
    position(out, value.end)?;
    out.write_all(b"}")
}
fn header(out: &mut impl Write, code: Option<&str>, message: &str) -> io::Result<()> {
    out.write_all(b"{\"schema_version\":1,\"severity\":\"error\",\"code\":")?;
    if let Some(code) = code {
        string(out, code)?;
    } else {
        out.write_all(b"null")?;
    }
    out.write_all(b",\"message\":")?;
    string(out, message)?;
    out.write_all(b",\"span_encoding\":\"utf-8-bytes\",\"column_encoding\":\"unicode-scalar\",\"position_base\":1,\"primary\":")
}
fn finish(out: &mut impl Write, help: Option<&str>) -> io::Result<()> {
    out.write_all(b",\"help\":")?;
    if let Some(help) = help {
        string(out, help)?;
    } else {
        out.write_all(b"null")?;
    }
    out.write_all(b"}\n")
}
fn diagnostic<'a>(
    value: &Diagnostic,
    out: &mut impl Write,
    locate: impl Fn(Span) -> Option<SourceLocation<'a>>,
) -> io::Result<()> {
    header(out, Some(value.kind.code()), &value.message)?;
    source_location(out, locate(value.span))?;
    out.write_all(b",\"related\":[")?;
    for (index, note) in value.related.iter().enumerate() {
        if index != 0 {
            out.write_all(b",")?;
        }
        out.write_all(b"{\"message\":")?;
        string(out, &note.message)?;
        out.write_all(b",\"location\":")?;
        source_location(out, locate(note.span))?;
        out.write_all(b"}")?;
    }
    out.write_all(b"]")?;
    finish(out, value.help.as_deref())
}
impl Diagnostic {
    // Schema 1: half-open local UTF-8 byte spans, one-based scalar positions.
    // Invalid source spans are null. Source excerpts are not serialized.
    pub fn write_json(&self, filename: &str, source: &str, out: &mut impl Write) -> io::Result<()> {
        diagnostic(self, out, |span| {
            if span.start > span.end
                || !source.is_char_boundary(span.start)
                || !source.is_char_boundary(span.end)
            {
                return None;
            }
            Some(SourceLocation {
                path: filename,
                span,
                start: location(source, span.start),
                end: location(source, span.end),
            })
        })
    }
}
impl ModuleDiagnostic {
    pub fn write_json(&self, out: &mut impl Write) -> io::Result<()> {
        diagnostic(&self.diagnostic, out, |span| {
            let value = self.sources.locate_span(span)?;
            Some(SourceLocation {
                path: &self.sources.files().get(value.source)?.name,
                span: value.span,
                start: value.start,
                end: value.end,
            })
        })
    }
}
// Non-source errors share the envelope, with null code and primary location.
pub fn write_json_message(
    message: &str,
    help: Option<&str>,
    out: &mut impl Write,
) -> io::Result<()> {
    header(out, None, message)?;
    out.write_all(b"null,\"related\":[]")?;
    finish(out, help)
}
