//! Immutable source navigation over one successfully compiled module graph.
mod json;
use super::*;
use crate::{ast::FunctionDeclaration, diagnostics::LocationTracker};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SymbolId(u32);
impl SymbolId {
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SymbolKind {
    Module,
    ImportAlias,
    Function,
    Constant,
    Binding,
    Parameter,
    RepeatIndex,
    ContractResult,
    Builtin,
}
impl SymbolKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Module => "module",
            Self::ImportAlias => "import_alias",
            Self::Function => "function",
            Self::Constant => "constant",
            Self::Binding => "binding",
            Self::Parameter => "parameter",
            Self::RepeatIndex => "repeat_index",
            Self::ContractResult => "contract_result",
            Self::Builtin => "builtin",
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReferenceKind {
    Read,
    Write,
    ReadWrite,
    Call,
    Namespace,
    Import,
    ArgumentLabel,
}
impl ReferenceKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Write => "write",
            Self::ReadWrite => "read_write",
            Self::Call => "call",
            Self::Namespace => "namespace",
            Self::Import => "import",
            Self::ArgumentLabel => "argument_label",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SymbolDefinition {
    pub id: SymbolId,
    pub name: Arc<str>,
    pub kind: SymbolKind,
    pub owner: Option<SymbolId>,
    pub public: bool,
    pub mutable: bool,
    pub target_module: Option<SourceId>,
    /// None for a builtin or implicit contract result; never an invented token.
    pub location: Option<SourceLocation>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SymbolReference {
    pub target: SymbolId,
    pub kind: ReferenceKind,
    pub location: SourceLocation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IndexLimits {
    pub max_symbols: usize,
    pub max_references: usize,
    pub max_name_bytes: usize,
    /// Visited syntax nodes/statements and emitted occurrences, independent of runtime steps.
    pub max_work: usize,
}
impl Default for IndexLimits {
    fn default() -> Self {
        Self {
            max_symbols: 65_536,
            max_references: 1_048_576,
            max_name_bytes: 8 * 1024 * 1024,
            max_work: 4_194_304,
        }
    }
}

#[derive(Debug, Clone)]
pub struct IndexedProgram {
    program: ModuleProgram,
    index: SymbolIndex,
}
impl IndexedProgram {
    pub fn program(&self) -> &ModuleProgram {
        &self.program
    }
    pub fn index(&self) -> &SymbolIndex {
        &self.index
    }
    pub fn into_parts(self) -> (ModuleProgram, SymbolIndex) {
        (self.program, self.index)
    }
    pub(super) fn new(program: ModuleProgram, index: SymbolIndex) -> Self {
        Self { program, index }
    }
}

/// Logical index construction counts, not process-memory or runtime statistics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IndexUsage {
    pub symbols: usize,
    pub references: usize,
    pub name_bytes: usize,
    pub work: usize,
}

#[derive(Debug)]
struct Data {
    usage: IndexUsage,
    sources: Arc<SourceMap>,
    symbols: Vec<SymbolDefinition>,
    references: Vec<SymbolReference>,
    by_target: Vec<usize>,
    sites: Vec<(SourceId, Span, SymbolId)>,
}
/// IDs are dense and local to this snapshot, not persistent identities across edits.
#[derive(Debug, Clone)]
pub struct SymbolIndex {
    data: Arc<Data>,
}
impl SymbolIndex {
    pub fn usage(&self) -> IndexUsage {
        self.data.usage
    }
    pub fn sources(&self) -> &Arc<SourceMap> {
        &self.data.sources
    }
    pub fn symbols(&self) -> &[SymbolDefinition] {
        &self.data.symbols
    }
    pub fn references(&self) -> &[SymbolReference] {
        &self.data.references
    }
    pub fn symbol(&self, id: SymbolId) -> Option<&SymbolDefinition> {
        self.symbols().get(id.index())
    }
    /// Half-open UTF-8 byte selection. Whitespace, punctuation, EOF, invalid source
    /// IDs, and non-boundary offsets return None. Implicit symbols have no token.
    pub fn goto_definition(&self, source: SourceId, byte: usize) -> Option<&SymbolDefinition> {
        if !self.sources().source(source)?.is_char_boundary(byte) {
            return None;
        }
        let sites = &self.data.sites;
        let i = sites
            .partition_point(|(s, span, _)| (*s, span.start) <= (source, byte))
            .checked_sub(1)?;
        let (s, span, id) = sites[i];
        (s == source && span.start <= byte && byte < span.end)
            .then(|| self.symbol(id))
            .flatten()
    }
    /// Declaration sites are excluded; references include writes and named labels.
    pub fn find_references(&self, id: SymbolId) -> impl Iterator<Item = &SymbolReference> {
        let order = &self.data.by_target;
        let first = order.partition_point(|&i| self.references()[i].target < id);
        let last = order.partition_point(|&i| self.references()[i].target <= id);
        order[first..last].iter().map(|&i| &self.references()[i])
    }
}

fn failure(span: Span, what: &str) -> Diagnostic {
    Diagnostic::new(Kind::SymbolIndexLimit,span,format!("symbol index {what} limit exceeded"))
        .with_help("reduce navigation metadata or adjust IndexLimits in Rust embedding; module and runtime limits are independent")
}
#[derive(Debug)]
struct PendingDefinition {
    name: Arc<str>,
    kind: SymbolKind,
    owner: Option<SymbolId>,
    public: bool,
    mutable: bool,
    target_module: Option<SourceId>,
    source: Option<SourceId>,
    span: Option<Span>,
}
#[derive(Debug)]
struct PendingReference {
    source: SourceId,
    span: Span,
    target: SymbolId,
    kind: ReferenceKind,
}
// Captures source identities before constant substitution/name rewriting. The
// linker supplies targets from its own lookup and lexical scope policy.
pub(super) struct Builder {
    limits: IndexLimits,
    work: usize,
    name_bytes: usize,
    definitions: Vec<PendingDefinition>,
    references: Vec<PendingReference>,
    by_span: HashMap<(usize, usize), SymbolId>,
    modules: Vec<SymbolId>,
    aliases: Vec<HashMap<Arc<str>, SymbolId>>,
    parameters: HashMap<SymbolId, HashMap<Arc<str>, SymbolId>>,
    builtins: HashMap<Arc<str>, SymbolId>,
}
impl Builder {
    pub(super) fn new(graph: &ModuleGraph, limits: IndexLimits) -> Result<Self, Diagnostic> {
        if limits.max_symbols > u32::MAX as usize {
            return Err(failure(Span::default(), "identity range"));
        }
        let mut b = Self {
            limits,
            work: 0,
            name_bytes: 0,
            definitions: Vec::new(),
            references: Vec::new(),
            by_span: HashMap::new(),
            modules: Vec::new(),
            aliases: vec![HashMap::new(); graph.modules.len()],
            parameters: HashMap::new(),
            builtins: HashMap::new(),
        };
        for (id, file) in graph.sources.files.iter().enumerate() {
            let symbol = b.define(
                Some(id),
                Some(Span::new(file.start, file.start)),
                &file.name,
                SymbolKind::Module,
                None,
                false,
                false,
                Some(id),
            )?;
            b.modules.push(symbol);
        }
        for (id, module) in graph.modules.iter().enumerate() {
            for (import, target) in &module.dependencies {
                let owner = Some(b.modules[id]);
                let alias = b.define(
                    Some(id),
                    Some(import.alias_span),
                    &import.alias,
                    SymbolKind::ImportAlias,
                    owner,
                    false,
                    false,
                    Some(*target),
                )?;
                b.aliases[id].insert(b.definitions[alias.index()].name.clone(), alias);
                b.reference(id, import.span, b.modules[*target], ReferenceKind::Import)?;
            }
            for wrapped in &module.program.statements {
                b.step(wrapped.span())?;
                let (statement, public) = match wrapped {
                    Statement::Export { declaration, .. } => (declaration.as_ref(), true),
                    s => (s, false),
                };
                match statement {
                    Statement::Function(f) => b.function(id, f, public)?,
                    Statement::Declaration {
                        name,
                        name_span,
                        mutable,
                        ..
                    } if id != 0 || public => {
                        b.define(
                            Some(id),
                            Some(*name_span),
                            name,
                            SymbolKind::Constant,
                            Some(b.modules[id]),
                            public,
                            *mutable,
                            None,
                        )?;
                    }
                    _ => {}
                }
            }
        }
        Ok(b)
    }
    pub(super) fn step(&mut self, span: Span) -> Result<(), Diagnostic> {
        if self.work >= self.limits.max_work {
            return Err(failure(span, "work"));
        }
        self.work += 1;
        Ok(())
    }
    #[allow(clippy::too_many_arguments)]
    fn define(
        &mut self,
        source: Option<SourceId>,
        span: Option<Span>,
        name: &str,
        kind: SymbolKind,
        owner: Option<SymbolId>,
        public: bool,
        mutable: bool,
        target_module: Option<SourceId>,
    ) -> Result<SymbolId, Diagnostic> {
        let at = span.unwrap_or_default();
        self.step(at)?;
        if self.definitions.len() >= self.limits.max_symbols {
            return Err(failure(at, "symbol count"));
        }
        let bytes = self
            .name_bytes
            .checked_add(name.len())
            .filter(|&n| n <= self.limits.max_name_bytes)
            .ok_or_else(|| failure(at, "name bytes"))?;
        self.definitions
            .try_reserve(1)
            .map_err(|_| failure(at, "storage reservation"))?;
        let id = SymbolId(self.definitions.len() as u32);
        self.definitions.push(PendingDefinition {
            name: Arc::from(name),
            kind,
            owner,
            public,
            mutable,
            target_module,
            source,
            span,
        });
        self.name_bytes = bytes;
        if let Some(span) = span.filter(|s| s.start < s.end) {
            self.by_span.insert((span.start, span.end), id);
        }
        Ok(id)
    }
    fn function(
        &mut self,
        source: SourceId,
        f: &FunctionDeclaration,
        public: bool,
    ) -> Result<(), Diagnostic> {
        let id = self.define(
            Some(source),
            Some(f.name_span),
            &f.name,
            SymbolKind::Function,
            Some(self.modules[source]),
            public,
            false,
            None,
        )?;
        let mut parameters = HashMap::new();
        for p in &f.parameters {
            let parameter = self.define(
                Some(source),
                Some(p.span),
                &p.name,
                SymbolKind::Parameter,
                Some(id),
                false,
                false,
                None,
            )?;
            parameters.insert(self.definitions[parameter.index()].name.clone(), parameter);
        }
        self.parameters.insert(id, parameters);
        if f.result != crate::value::ValueType::Unit && f.contracts.iter().any(|c| c.postcondition)
        {
            let result = self.define(
                Some(source),
                None,
                "result",
                SymbolKind::ContractResult,
                Some(id),
                false,
                false,
                None,
            )?;
            for c in f.contracts.iter().filter(|c| c.postcondition) {
                self.by_span.insert((c.span.start, c.span.end), result);
            }
        }
        Ok(())
    }
    pub(super) fn binding(
        &mut self,
        source: SourceId,
        owner: Option<Span>,
        name: &str,
        span: Span,
        kind: SymbolKind,
        mutable: bool,
    ) -> Result<(), Diagnostic> {
        let owner = owner
            .and_then(|s| self.at(s))
            .unwrap_or(self.modules[source]);
        self.define(
            Some(source),
            Some(span),
            name,
            kind,
            Some(owner),
            false,
            mutable,
            None,
        )?;
        Ok(())
    }
    fn at(&self, span: Span) -> Option<SymbolId> {
        self.by_span.get(&(span.start, span.end)).copied()
    }
    fn reference(
        &mut self,
        source: SourceId,
        span: Span,
        target: SymbolId,
        kind: ReferenceKind,
    ) -> Result<(), Diagnostic> {
        self.step(span)?;
        if self.references.len() >= self.limits.max_references {
            return Err(failure(span, "reference count"));
        }
        self.references
            .try_reserve(1)
            .map_err(|_| failure(span, "storage reservation"))?;
        self.references.push(PendingReference {
            source,
            span,
            target,
            kind,
        });
        Ok(())
    }
    pub(super) fn use_symbol(
        &mut self,
        source: SourceId,
        name: &str,
        span: Span,
        declaration: Span,
        kind: ReferenceKind,
    ) -> Result<(), Diagnostic> {
        if let Some(target) = self.at(declaration) {
            let member = if let Some((alias, member)) = name.split_once('.') {
                if let Some(&id) = self.aliases[source].get(alias) {
                    self.reference(
                        source,
                        Span::new(span.start, span.start + alias.len()),
                        id,
                        ReferenceKind::Namespace,
                    )?;
                }
                Span::new(span.end - member.len(), span.end)
            } else {
                span
            };
            self.reference(source, member, target, kind)?;
        }
        Ok(())
    }
    pub(super) fn label(
        &mut self,
        source: SourceId,
        function: Span,
        name: &str,
        span: Span,
    ) -> Result<(), Diagnostic> {
        if let Some(id) = self
            .at(function)
            .and_then(|id| self.parameters.get(&id))
            .and_then(|p| p.get(name))
            .copied()
        {
            self.reference(source, span, id, ReferenceKind::ArgumentLabel)?;
        }
        Ok(())
    }
    pub(super) fn builtin(
        &mut self,
        source: SourceId,
        name: &str,
        span: Span,
    ) -> Result<(), Diagnostic> {
        let id = if let Some(&id) = self.builtins.get(name) {
            id
        } else {
            let id = self.define(
                None,
                None,
                name,
                SymbolKind::Builtin,
                None,
                false,
                false,
                None,
            )?;
            self.builtins
                .insert(self.definitions[id.index()].name.clone(), id);
            id
        };
        self.reference(source, span, id, ReferenceKind::Call)
    }
    pub(super) fn finish(mut self, sources: Arc<SourceMap>) -> SymbolIndex {
        let mut order: Vec<_> = (0..self.definitions.len()).collect();
        order.sort_unstable_by_key(|&i| {
            let d = &self.definitions[i];
            (
                d.source.unwrap_or(usize::MAX),
                d.span.map_or(usize::MAX, |s| s.start),
                d.kind,
                d.name.as_ref(),
                d.owner,
                i,
            )
        });
        let mut remap = vec![SymbolId(0); order.len()];
        for (new, &old) in order.iter().enumerate() {
            remap[old] = SymbolId(new as u32);
        }
        let mut symbols = Vec::with_capacity(order.len());
        let mut trackers: Vec<_> = (0..sources.files.len())
            .map(|id| LocationTracker::new(sources.source(id).unwrap()))
            .collect();
        for old in order {
            let d = &self.definitions[old];
            let location = d
                .source
                .zip(d.span)
                .map(|(source, span)| locate(&sources, &mut trackers, source, span));
            symbols.push(SymbolDefinition {
                id: remap[old],
                name: d.name.clone(),
                kind: d.kind,
                owner: d.owner.map(|id| remap[id.index()]),
                public: d.public,
                mutable: d.mutable,
                target_module: d.target_module,
                location,
            });
        }
        self.references
            .sort_unstable_by_key(|r| (r.source, r.span.start, r.span.end, r.target));
        let mut trackers: Vec<_> = (0..sources.files.len())
            .map(|id| LocationTracker::new(sources.source(id).unwrap()))
            .collect();
        let references: Vec<_> = self
            .references
            .into_iter()
            .map(|r| SymbolReference {
                target: remap[r.target.index()],
                kind: r.kind,
                location: locate(&sources, &mut trackers, r.source, r.span),
            })
            .collect();
        let mut by_target: Vec<_> = (0..references.len()).collect();
        by_target.sort_unstable_by_key(|&i| (references[i].target, i));
        let mut sites: Vec<_> = symbols
            .iter()
            .filter_map(|d| {
                d.location
                    .filter(|l| l.span.start < l.span.end)
                    .map(|l| (l.source, l.span, d.id))
            })
            .chain(
                references
                    .iter()
                    .map(|r| (r.location.source, r.location.span, r.target)),
            )
            .collect();
        sites.sort_unstable_by_key(|(source, span, _)| (*source, span.start, span.end));
        let usage = IndexUsage {
            symbols: symbols.len(),
            references: references.len(),
            name_bytes: self.name_bytes,
            work: self.work,
        };
        SymbolIndex {
            data: Arc::new(Data {
                usage,
                sources,
                symbols,
                references,
                by_target,
                sites,
            }),
        }
    }
}
fn locate(
    sources: &SourceMap,
    trackers: &mut [LocationTracker<'_>],
    source: SourceId,
    span: Span,
) -> SourceLocation {
    let base = sources.files[source].start;
    let span = Span::new(span.start - base, span.end - base);
    SourceLocation {
        source,
        span,
        start: trackers[source].at(span.start),
        end: trackers[source].at(span.end),
    }
}
