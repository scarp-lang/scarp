//! Checked Rust-host access to publicly exported functions of linked modules.

use super::{ModuleDiagnostic, ModuleProgram, ModuleRunError, SourceId, SourceMap, SymbolKind};
use crate::{
    resolver::{CallableBindError, CallableCatalog, HostArguments, NamedCallableRef},
    types::TypeId,
    value::Value,
    ExecutionOptions, InputLimits, RunError,
};
use std::{error::Error, fmt, io, sync::Arc};

/// A single linked program and its reusable resolved callable snapshot.
#[derive(Debug, Clone)]
pub struct ModuleCallableCatalog {
    linked: Arc<ModuleProgram>,
    callables: CallableCatalog,
}

/// A public, source-identified named target with source-aware diagnostics.
#[derive(Debug, Clone)]
pub struct ExportedNamedCallable {
    target: NamedCallableRef,
    sources: Arc<SourceMap>,
    source: SourceId,
    name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModuleCallableBindError {
    UnknownSource(SourceId),
    UnknownSymbol,
    PrivateSymbol,
    NotFunction,
    Callable(CallableBindError),
}
impl fmt::Display for ModuleCallableBindError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownSource(source) => write!(f, "unknown linked source ID {source}"),
            Self::UnknownSymbol => f.write_str("no matching exported function symbol"),
            Self::PrivateSymbol => f.write_str("module symbol is private"),
            Self::NotFunction => f.write_str("exported module symbol is not a function"),
            Self::Callable(error) => error.fmt(f),
        }
    }
}
impl Error for ModuleCallableBindError {}
impl From<CallableBindError> for ModuleCallableBindError {
    fn from(error: CallableBindError) -> Self {
        Self::Callable(error)
    }
}

impl ModuleCallableCatalog {
    /// Clone the compiled resolver plan once; share it across all handles.
    /// This does not mutate the original linked ModuleProgram.
    pub fn new(linked: Arc<ModuleProgram>) -> Self {
        let callables = CallableCatalog::from_program(linked.program().clone());
        Self { linked, callables }
    }

    pub fn from_program(linked: ModuleProgram) -> Self {
        Self::new(Arc::new(linked))
    }

    pub fn modules(&self) -> &[super::ModuleInfo] {
        self.linked.modules()
    }

    /// Only pub funcra declarations may be bound by name.
    pub fn bind_exported(
        &self,
        source: SourceId,
        name: &str,
    ) -> Result<ExportedNamedCallable, ModuleCallableBindError> {
        self.lookup(source, name, None)
    }

    /// Require a compatible signature from the same compiled registry.
    pub fn bind_exported_as(
        &self,
        source: SourceId,
        name: &str,
        required: TypeId,
    ) -> Result<ExportedNamedCallable, ModuleCallableBindError> {
        self.lookup(source, name, Some(required))
    }

    fn lookup(
        &self,
        source: SourceId,
        name: &str,
        required: Option<TypeId>,
    ) -> Result<ExportedNamedCallable, ModuleCallableBindError> {
        let module = self
            .linked
            .modules()
            .get(source)
            .ok_or(ModuleCallableBindError::UnknownSource(source))?;
        let range = self
            .linked
            .inspection
            .function_range(source)
            .ok_or(ModuleCallableBindError::UnknownSource(source))?;
        let mut ordinal = 0;
        for symbol in &module.symbols {
            let function = matches!(&symbol.kind, SymbolKind::Function { .. });
            if symbol.name == name {
                if !symbol.public {
                    return Err(ModuleCallableBindError::PrivateSymbol);
                }
                if !function {
                    return Err(ModuleCallableBindError::NotFunction);
                }
                let index = range.start + ordinal;
                let target = match required {
                    Some(signature) => self.callables.bind(index, signature)?,
                    None => self.callables.bind_exact(index)?,
                };
                return Ok(ExportedNamedCallable {
                    target,
                    sources: Arc::clone(self.linked.sources()),
                    source,
                    name: symbol.name.clone(),
                });
            }
            if function {
                ordinal += 1;
            }
        }
        Err(ModuleCallableBindError::UnknownSymbol)
    }
}

impl ExportedNamedCallable {
    pub fn source(&self) -> SourceId {
        self.source
    }

    /// Original declaration name, not the linker's mangled target identity.
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn signature(&self) -> TypeId {
        self.target.signature()
    }

    pub fn metadata(&self) -> &crate::resolver::FunctionInfo {
        self.target.metadata()
    }

    /// Invoke only the selected function using the normal interpreter machine.
    pub fn invoke_with_io_and_options(
        &self,
        arguments: HostArguments,
        stdout: &mut impl io::Write,
        stderr: &mut impl io::Write,
        options: ExecutionOptions,
    ) -> Result<Value, ModuleRunError> {
        self.target
            .invoke_bound_with_io_and_options(arguments, stdout, stderr, options)
            .map_err(|error| self.wrap(error))
    }

    /// Reader-capable invocation; input is irreversible, output transactional.
    pub fn invoke_with_input_and_options(
        &self,
        arguments: HostArguments,
        input: &mut impl io::BufRead,
        stdout: &mut impl io::Write,
        stderr: &mut impl io::Write,
        options: ExecutionOptions,
        input_limits: InputLimits,
    ) -> Result<Value, ModuleRunError> {
        self.target
            .invoke_bound_with_input_and_options(
                arguments, input, stdout, stderr, options, input_limits,
            )
            .map_err(|error| self.wrap(error))
    }

    fn wrap(&self, error: RunError) -> ModuleRunError {
        match error {
            RunError::Diagnostic(diagnostic) => ModuleRunError::Diagnostic(ModuleDiagnostic {
                diagnostic,
                sources: Arc::clone(&self.sources),
            }),
            RunError::Output(error) => ModuleRunError::Output(error),
        }
    }
}
