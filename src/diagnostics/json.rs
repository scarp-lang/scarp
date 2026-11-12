use super::{location, Diagnostic, Location, Span};
use crate::{json::string, modules::ModuleDiagnostic};
use std::{
    borrow::Cow,
    io::{self, Write},
    path::{Component, Path},
};

struct SourceLocation<'a> {
    path: Cow<'a, str>,
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
    string(out, &value.path)?;
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
    limits: ReportLimits,
) -> io::Result<()> {
    header(
        out,
        Some(value.kind.code()),
        prefix(&value.message, limits.message),
    )?;
    source_location(out, locate(value.span))?;
    out.write_all(b",\"related\":[")?;
    for (index, note) in value.related.iter().take(limits.related).enumerate() {
        if index != 0 {
            out.write_all(b",")?;
        }
        out.write_all(b"{\"message\":")?;
        string(out, prefix(&note.message, limits.detail))?;
        out.write_all(b",\"location\":")?;
        source_location(out, locate(note.span))?;
        out.write_all(b"}")?;
    }
    out.write_all(b"]")?;
    finish(
        out,
        value
            .help
            .as_deref()
            .map(|text| prefix(text, limits.detail)),
    )
}

#[derive(Clone, Copy)]
struct ReportLimits {
    message: usize,
    detail: usize,
    related: usize,
}

const UNRESTRICTED: ReportLimits = ReportLimits {
    message: usize::MAX,
    detail: usize::MAX,
    related: usize::MAX,
};
const PROJECT: ReportLimits = ReportLimits {
    message: 1600,
    detail: 512,
    related: 8,
};
const MAX_PROJECT_PATH_BYTES: usize = 4096;

fn prefix(value: &str, limit: usize) -> &str {
    if value.len() <= limit {
        return value;
    }
    value
        .char_indices()
        .nth(limit)
        .map_or(value, |(end, _)| &value[..end])
}
impl Diagnostic {
    // Schema 1: half-open local UTF-8 byte spans, one-based scalar positions.
    // Invalid source spans are null. Source excerpts are not serialized.
    pub fn write_json(&self, filename: &str, source: &str, out: &mut impl Write) -> io::Result<()> {
        diagnostic(
            self,
            out,
            |span| {
                if span.start > span.end
                    || !source.is_char_boundary(span.start)
                    || !source.is_char_boundary(span.end)
                {
                    return None;
                }
                Some(SourceLocation {
                    path: Cow::Borrowed(filename),
                    span,
                    start: location(source, span.start),
                    end: location(source, span.end),
                })
            },
            UNRESTRICTED,
        )
    }
}
impl ModuleDiagnostic {
    pub fn write_json(&self, out: &mut impl Write) -> io::Result<()> {
        self.write_json_with_paths(out, UNRESTRICTED, |path| Some(Cow::Borrowed(path)))
    }

    /// Write the compiler diagnostic envelope for a bounded project test report.
    /// Locations use project-relative paths with forward slashes. Pass the
    /// canonical root used by the file source loader for this diagnostic.
    pub fn write_project_json(&self, root: &Path, out: &mut impl Write) -> io::Result<()> {
        let root = root.to_string_lossy();
        self.write_json_with_paths(out, PROJECT, |name| {
            project_path(Path::new(root.as_ref()), name)
        })
    }

    fn write_json_with_paths<'a>(
        &'a self,
        out: &mut impl Write,
        limits: ReportLimits,
        path: impl Fn(&'a str) -> Option<Cow<'a, str>>,
    ) -> io::Result<()> {
        diagnostic(
            &self.diagnostic,
            out,
            |span| {
                let value = self.sources.locate_span(span)?;
                Some(SourceLocation {
                    path: path(&self.sources.files().get(value.source)?.name)?,
                    span: value.span,
                    start: value.start,
                    end: value.end,
                })
            },
            limits,
        )
    }
}

fn project_path<'a>(root: &Path, name: &'a str) -> Option<Cow<'a, str>> {
    let path = Path::new(name);
    let relative = if path.is_absolute() {
        path.strip_prefix(root).ok()?
    } else {
        path
    };
    let value = relative.to_str()?;
    if value.is_empty()
        || value.len() > MAX_PROJECT_PATH_BYTES
        || relative
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
        || value.chars().any(|ch| {
            ch.is_control() || ch == ':' || (ch == '\\' && std::path::MAIN_SEPARATOR != '\\')
        })
    {
        return None;
    }
    Some(
        if std::path::MAIN_SEPARATOR == '\\' && value.contains('\\') {
            Cow::Owned(value.replace('\\', "/"))
        } else {
            Cow::Borrowed(value)
        },
    )
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
