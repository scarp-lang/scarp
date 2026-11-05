use super::*;
use crate::json::string;
fn location(out: &mut impl io::Write, value: Option<SourceLocation>) -> io::Result<()> {
    let Some(l) = value else {
        return out.write_all(b"null");
    };
    write!(out,"{{\"source\":{},\"span\":[{},{}],\"start\":{{\"line\":{},\"column\":{}}},\"end\":{{\"line\":{},\"column\":{}}}}}",l.source,l.span.start,l.span.end,l.start.line,l.start.column,l.end.line,l.end.column)
}
fn optional(out: &mut impl io::Write, value: Option<usize>) -> io::Result<()> {
    match value {
        Some(v) => write!(out, "{v}"),
        None => out.write_all(b"null"),
    }
}
impl SymbolIndex {
    /// Stream the complete schema-v1 reference index, including its symbol table.
    pub fn write_references_json(&self, out: &mut impl io::Write) -> io::Result<()> {
        self.write_json(out, true)
    }
    /// Stream schema-v1 definitions; IDs match the corresponding reference index.
    pub fn write_symbols_json(&self, out: &mut impl io::Write) -> io::Result<()> {
        self.write_json(out, false)
    }
    fn write_json(&self, out: &mut impl io::Write, references: bool) -> io::Result<()> {
        out.write_all(b"{\"schema_version\":1,\"kind\":")?;
        string(
            out,
            if references {
                "scarp.references"
            } else {
                "scarp.symbols"
            },
        )?;
        out.write_all(b",\"span_encoding\":\"utf-8-bytes\",\"column_encoding\":\"unicode-scalar\",\"position_base\":1,\"files\":[")?;
        for (id, file) in self.sources().files().iter().enumerate() {
            if id != 0 {
                out.write_all(b",")?;
            }
            write!(out, "{{\"id\":{id},\"path\":")?;
            string(out, &file.name)?;
            out.write_all(b"}")?;
        }
        out.write_all(b"],\"symbols\":[")?;
        for (n, d) in self.symbols().iter().enumerate() {
            if n != 0 {
                out.write_all(b",")?;
            }
            write!(out, "{{\"id\":{},\"name\":", d.id.index())?;
            string(out, &d.name)?;
            out.write_all(b",\"kind\":")?;
            string(out, d.kind.as_str())?;
            out.write_all(b",\"owner\":")?;
            optional(out, d.owner.map(SymbolId::index))?;
            write!(
                out,
                ",\"public\":{},\"mutable\":{},\"target_module\":",
                d.public, d.mutable
            )?;
            optional(out, d.target_module)?;
            out.write_all(b",\"location\":")?;
            location(out, d.location)?;
            out.write_all(b"}")?;
        }
        out.write_all(b"]")?;
        if references {
            out.write_all(b",\"references\":[")?;
            for (n, r) in self.references().iter().enumerate() {
                if n != 0 {
                    out.write_all(b",")?;
                }
                write!(out, "{{\"target\":{},\"kind\":", r.target.index())?;
                string(out, r.kind.as_str())?;
                out.write_all(b",\"location\":")?;
                location(out, Some(r.location))?;
                out.write_all(b"}")?;
            }
            out.write_all(b"]")?;
        }
        out.write_all(b"}\n")
    }
    pub fn write_symbols(&self, out: &mut impl io::Write) -> io::Result<()> {
        self.write_text(out, false)
    }
    pub fn write_references(&self, out: &mut impl io::Write) -> io::Result<()> {
        self.write_text(out, true)
    }
    fn write_text(&self, out: &mut impl io::Write, references: bool) -> io::Result<()> {
        writeln!(
            out,
            "Scarp symbol index 1: {} symbols, {} references",
            self.symbols().len(),
            self.references().len()
        )?;
        for d in self.symbols() {
            write!(out, "{} {} {:?}", d.id.index(), d.kind.as_str(), d.name)?;
            if let Some(l) = d.location {
                writeln!(
                    out,
                    " at {:?}:{}:{} [{}..{}]",
                    self.sources().files()[l.source].name,
                    l.start.line,
                    l.start.column,
                    l.span.start,
                    l.span.end
                )?;
            } else {
                writeln!(out, " (implicit or builtin)")?;
            }
        }
        if references {
            for r in self.references() {
                let l = r.location;
                writeln!(
                    out,
                    "{} -> {} at {:?}:{}:{} [{}..{}]",
                    r.kind.as_str(),
                    r.target.index(),
                    self.sources().files()[l.source].name,
                    l.start.line,
                    l.start.column,
                    l.span.start,
                    l.span.end
                )?;
            }
        }
        Ok(())
    }
}
