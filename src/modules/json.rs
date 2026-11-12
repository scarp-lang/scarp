//! Streaming versioned metadata. Local byte spans avoid rescanning source text.
use super::*;

use crate::json::string;

fn ids(out: &mut impl io::Write, values: &[SourceId]) -> io::Result<()> {
    out.write_all(b"[")?;
    for (index, value) in values.iter().enumerate() {
        if index != 0 {
            out.write_all(b",")?;
        }
        write!(out, "{value}")?;
    }
    out.write_all(b"]")
}
fn span(out: &mut impl io::Write, file: &SourceFile, value: Span) -> io::Result<()> {
    write!(
        out,
        "[{},{}]",
        value.start - file.start,
        value.end - file.start
    )
}

impl ModuleProgram {
    /// Inventory version: primitives use 1, tuple types use 2, and any array type uses 3.
    pub fn inspection_schema_version(&self) -> u32 {
        let types = self
            .modules
            .iter()
            .flat_map(|m| &m.symbols)
            .flat_map(|s| match &s.kind {
                SymbolKind::Constant(kind) => std::iter::once(kind).chain([].iter()),
                SymbolKind::Function {
                    parameters, result, ..
                } => std::iter::once(result).chain(parameters.iter()),
            });
        let mut version = 1;
        for kind in types {
            if kind.contains_array() {
                return 3;
            }
            if kind.as_primitive().is_none() {
                version = 2;
            }
        }
        version
    }
    /// Write versioned JSON with deterministic order and file-local byte spans.
    /// No source contents, constant values or default expressions are emitted.
    /// Writer errors preserve their kind and may leave a partial byte stream.
    ///
    /// ```
    /// use scarp::modules::{self, MemorySourceLoader, ModuleLimits};
    /// let mut loader = MemorySourceLoader::new();
    /// loader.insert("main.srp", "pub bind answer: integer=6*7; print(1/0);")?;
    /// let program = modules::compile_with_loader("main.srp", &loader, ModuleLimits::default())?;
    /// let mut json = Vec::new();
    /// program.write_inspection_json(&mut json)?;
    /// assert!(String::from_utf8(json)?.starts_with("{\"schema_version\":1,"));
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn write_inspection_json(&self, out: &mut impl io::Write) -> io::Result<()> {
        write!(
            out,
            "{{\"schema_version\":{},\"entry\":0,\"modules\":[",
            self.inspection_schema_version()
        )?;
        for (index, module) in self.modules.iter().enumerate() {
            if index != 0 {
                out.write_all(b",")?;
            }
            let id = module.source;
            let file = &self.sources.files[id];
            write!(out, "{{\"id\":{id},\"path\":")?;
            string(out, &file.name)?;
            out.write_all(b",\"imports\":[")?;
            for (index, import) in self.imports_in(id).unwrap().iter().enumerate() {
                if index != 0 {
                    out.write_all(b",")?;
                }
                out.write_all(b"{\"path\":")?;
                string(out, &import.path)?;
                out.write_all(b",\"alias\":")?;
                string(out, &import.alias)?;
                write!(out, ",\"target\":{},\"span\":", import.target)?;
                span(out, file, import.span)?;
                out.write_all(b",\"path_span\":")?;
                span(out, file, import.path_span)?;
                out.write_all(b",\"alias_span\":")?;
                span(out, file, import.alias_span)?;
                out.write_all(b"}")?;
            }
            out.write_all(b"],\"dependencies\":")?;
            ids(out, self.dependencies(id).unwrap())?;
            out.write_all(b",\"dependents\":")?;
            ids(out, self.dependents(id).unwrap())?;
            out.write_all(b",\"symbols\":[")?;
            let mut functions = self.functions_in(id).unwrap();
            for (index, symbol) in module.symbols.iter().enumerate() {
                if index != 0 {
                    out.write_all(b",")?;
                }
                out.write_all(b"{\"name\":")?;
                string(out, &symbol.name)?;
                write!(out, ",\"public\":{},\"span\":", symbol.public)?;
                span(out, file, symbol.span)?;
                match &symbol.kind {
                    SymbolKind::Constant(kind) => {
                        write!(out, ",\"kind\":\"constant\",\"type\":\"{kind}\"")?
                    }
                    SymbolKind::Function { pure, result, .. } => {
                        let (_, function) = functions.next().unwrap();
                        write!(out,",\"kind\":\"function\",\"pure\":{pure},\"result\":\"{result}\",\"parameters\":[")?;
                        for (index, parameter) in function.parameters.iter().enumerate() {
                            if index != 0 {
                                out.write_all(b",")?;
                            }
                            out.write_all(b"{\"name\":")?;
                            string(out, &parameter.name)?;
                            write!(
                                out,
                                ",\"type\":\"{}\",\"has_default\":{},\"span\":",
                                parameter.kind,
                                parameter.default.is_some()
                            )?;
                            span(out, file, parameter.span)?;
                            out.write_all(b"}")?;
                        }
                        out.write_all(b"]")?;
                    }
                }
                out.write_all(b"}")?;
            }
            out.write_all(b"]}")?;
        }
        out.write_all(b"],\"constant_dependencies\":[")?;
        for (index, edge) in self.constant_dependencies().iter().enumerate() {
            if index != 0 {
                out.write_all(b",")?;
            }
            let file = &self.sources.files[edge.source];
            let target = &self.sources.files[edge.target];
            write!(out, "{{\"source\":{},\"declaration\":", edge.source)?;
            span(out, file, edge.declaration)?;
            write!(out, ",\"target\":{},\"target_declaration\":", edge.target)?;
            span(out, target, edge.target_declaration)?;
            out.write_all(b",\"span\":")?;
            span(out, file, edge.span)?;
            out.write_all(b"}")?;
        }
        out.write_all(b"]}\n")
    }
}
