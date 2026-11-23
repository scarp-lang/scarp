//! Export-constrained dynamic selection of linked Scarp functions by Rust hosts.
//! Untrusted names never reach the raw resolver declaration-index API.

use std::{error::Error, fmt, io, sync::Arc};

use super::{
    ModuleCallableBindError, ModuleCallableCatalog, ModuleDiagnostic, ModuleRunError, SourceId,
};
use crate::{
    resolver::{CallableSlot, CallableSlotError, CallableSlotInvokeError, HostArguments},
    types::TypeId,
    value::Value,
    ExecutionOptions, InputLimits, RunError,
};

/// Rebindable host slot whose only selectable targets are exported module
/// function symbols in one immutable linked ModuleProgram snapshot.
#[derive(Debug, Clone)]
pub struct ExportedCallableSlot {
    catalog: ModuleCallableCatalog,
    slot: CallableSlot,
    selected: Option<(SourceId, String)>,
}

/// A rejected selection never changes the current target or its source/name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportedSlotBindError {
    Export(ModuleCallableBindError),
    Slot(CallableSlotError),
}
impl From<ModuleCallableBindError> for ExportedSlotBindError {
    fn from(error: ModuleCallableBindError) -> Self {
        Self::Export(error)
    }
}
impl From<CallableSlotError> for ExportedSlotBindError {
    fn from(error: CallableSlotError) -> Self {
        Self::Slot(error)
    }
}
impl fmt::Display for ExportedSlotBindError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Export(error) => error.fmt(f),
            Self::Slot(error) => error.fmt(f),
        }
    }
}
impl Error for ExportedSlotBindError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Export(error) => Some(error),
            Self::Slot(error) => Some(error),
        }
    }
}

/// Distinguishes unbound host selection from a failed Scarp execution.
#[derive(Debug)]
pub enum ExportedSlotInvokeError {
    Unbound,
    Runtime(ModuleRunError),
}
impl fmt::Display for ExportedSlotInvokeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unbound => f.write_str("no public Scarp function selected"),
            Self::Runtime(error) => error.fmt(f),
        }
    }
}
impl Error for ExportedSlotInvokeError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Unbound => None,
            Self::Runtime(error) => Some(error),
        }
    }
}

impl ModuleCallableCatalog {
    /// Create a bound slot for a publicly exported function. The source ID,
    /// symbol kind, visibility and signature are verified before selection.
    /// A private target cannot be seeded into this API through an index.
    pub fn slot_exported(
        &self,
        source: SourceId,
        name: &str,
        required: TypeId,
    ) -> Result<ExportedCallableSlot, ExportedSlotBindError> {
        let next = self.bind_exported_as(source, name, required)?;
        let mut slot = self.callables.slot(required)?;
        slot.set(&next.target)?;
        Ok(ExportedCallableSlot {
            catalog: self.clone(),
            slot,
            selected: Some((source, next.name().to_owned())),
        })
    }
}

impl ExportedCallableSlot {
    /// Required function signature, fixed for this slot's lifetime.
    pub fn required_signature(&self) -> TypeId {
        self.slot.required_signature()
    }

    pub fn is_bound(&self) -> bool {
        self.slot.is_bound()
    }

    /// The selected public module and original unmangled declaration name.
    pub fn selected(&self) -> Option<(SourceId, &str)> {
        self.selected
            .as_ref()
            .map(|(source, name)| (*source, name.as_str()))
    }

    /// Rebind only after export visibility and function signature checks.
    /// Both the actual target and selected name stay unchanged on failure.
    pub fn select_exported(
        &mut self,
        source: SourceId,
        name: &str,
    ) -> Result<(), ExportedSlotBindError> {
        let next = self
            .catalog
            .bind_exported_as(source, name, self.slot.required_signature())?;
        self.slot.set(&next.target)?;
        self.selected = Some((source, next.name().to_owned()));
        Ok(())
    }

    /// Clearing does not revoke unrelated cloned slots or release this
    /// catalog's compiled program. An unbound invocation never reads input.
    pub fn clear(&mut self) {
        self.slot.clear();
        self.selected = None;
    }

    pub fn invoke_with_io_and_options(
        &self,
        arguments: HostArguments,
        stdout: &mut impl io::Write,
        stderr: &mut impl io::Write,
        options: ExecutionOptions,
    ) -> Result<Value, ExportedSlotInvokeError> {
        self.slot
            .invoke_with_io_and_options(arguments, stdout, stderr, options)
            .map_err(|error| self.translate(error))
    }

    pub fn invoke_with_input_and_options(
        &self,
        arguments: HostArguments,
        input: &mut impl io::BufRead,
        stdout: &mut impl io::Write,
        stderr: &mut impl io::Write,
        options: ExecutionOptions,
        input_limits: InputLimits,
    ) -> Result<Value, ExportedSlotInvokeError> {
        self.slot
            .invoke_with_input_and_options(
                arguments,
                input,
                stdout,
                stderr,
                options,
                input_limits,
            )
            .map_err(|error| self.translate(error))
    }

    fn translate(&self, error: CallableSlotInvokeError) -> ExportedSlotInvokeError {
        match error {
            CallableSlotInvokeError::Unbound => ExportedSlotInvokeError::Unbound,
            CallableSlotInvokeError::Runtime(runtime) => {
                let module = match runtime {
                    RunError::Diagnostic(diagnostic) => {
                        ModuleRunError::Diagnostic(ModuleDiagnostic {
                            diagnostic,
                            sources: Arc::clone(self.catalog.linked.sources()),
                        })
                    }
                    RunError::Output(error) => ModuleRunError::Output(error),
                };
                ExportedSlotInvokeError::Runtime(module)
            }
        }
    }
}
