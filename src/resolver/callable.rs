//! Checked, host-only identities for named functions in one resolved program.
//! These handles do not implement indirect execution or closure capture.

use std::{error::Error, fmt, io, sync::Arc};

mod slot;
pub use slot::{CallableSlot, CallableSlotError, CallableSlotInvokeError};

use super::{FunctionInfo, ResolvedProgram};
use crate::{
    diagnostics::{Diagnostic, DiagnosticKind},
    input::InputLimits,
    limits::{ExecutionOptions, ExecutionUsage},
    types::{PrimitiveType, SourceType, TypeId, TypeRegistryError},
    value::{Value, MAX_TEXT_BYTES},
    RunError,
};

/// An immutable, caller-owned compiled-program snapshot.
#[derive(Debug, Clone)]
pub struct CallableCatalog {
    program: Arc<ResolvedProgram>,
}

/// Host-provided positional and explicitly named arguments for a single
/// checked invocation. The host already evaluated these Rust values; Scarp
/// evaluates only omitted defaults, then needs/body/ensures in declaration
/// order. Names match a function's original parameters, not a semantic TypeId.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HostArguments {
    pub positional: Vec<Value>,
    pub named: Vec<(String, Value)>,
}

impl HostArguments {
    pub fn new(positional: Vec<Value>, named: Vec<(String, Value)>) -> Self {
        Self { positional, named }
    }

    pub fn positional(values: Vec<Value>) -> Self {
        Self::new(values, Vec::new())
    }
}

/// A named declaration in one retained compiled program. This is inspection
/// metadata, not a source-language callable Value or an indirect-call API.
#[derive(Debug, Clone)]
pub struct NamedCallableRef {
    program: Arc<ResolvedProgram>,
    declaration: usize,
    signature: TypeId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallableBindError {
    UnknownFunction(usize),
    Registry(TypeRegistryError),
    IncompatibleSignature { expected: TypeId, actual: TypeId },
}

impl fmt::Display for CallableBindError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownFunction(index) => {
                write!(f, "no named function at declaration index {index}")
            }
            Self::Registry(error) => error.fmt(f),
            Self::IncompatibleSignature { expected, actual } => {
                write!(
                    f,
                    "callable signature {actual} is incompatible with required {expected}"
                )
            }
        }
    }
}
impl Error for CallableBindError {}
impl From<TypeRegistryError> for CallableBindError {
    fn from(error: TypeRegistryError) -> Self {
        Self::Registry(error)
    }
}

impl CallableCatalog {
    /// Share an existing immutable resolved program without copying it.
    pub fn new(program: Arc<ResolvedProgram>) -> Self {
        Self { program }
    }

    /// Move a resolved program into an immutable, reference-counted snapshot.
    pub fn from_program(program: ResolvedProgram) -> Self {
        Self::new(Arc::new(program))
    }

    pub fn len(&self) -> usize {
        self.program.functions().len()
    }

    pub fn is_empty(&self) -> bool {
        self.program.functions().is_empty()
    }

    /// Resolve an index into this snapshot's already-validated named
    /// declarations; this does not allocate or intern a new semantic type.
    pub fn bind_exact(&self, declaration: usize) -> Result<NamedCallableRef, CallableBindError> {
        let signature = self
            .program
            .function_type_id(declaration)
            .ok_or(CallableBindError::UnknownFunction(declaration))?;
        Ok(NamedCallableRef {
            program: Arc::clone(&self.program),
            declaration,
            signature,
        })
    }

    /// Check a required type in the same registry before binding a target.
    /// Parameters and return type are invariant; pure can fill impure.
    /// Unsupported syntax, runtime dispatch and captures are not enabled.
    pub fn bind(
        &self,
        declaration: usize,
        required: TypeId,
    ) -> Result<NamedCallableRef, CallableBindError> {
        let candidate = self
            .program
            .function_type_id(declaration)
            .ok_or(CallableBindError::UnknownFunction(declaration))?;
        if !self
            .program
            .type_registry()
            .accepts_callable(required, candidate)?
        {
            return Err(CallableBindError::IncompatibleSignature {
                expected: required,
                actual: candidate,
            });
        }
        self.bind_exact(declaration)
    }
}

impl NamedCallableRef {
    /// Index within the retained snapshot, not a stable cross-build symbol.
    pub fn declaration_index(&self) -> usize {
        self.declaration
    }

    pub fn signature(&self) -> TypeId {
        self.signature
    }

    /// Immutable metadata from the same snapshot as the declaration.
    pub fn metadata(&self) -> &FunctionInfo {
        &self.program.functions()[self.declaration]
    }

    /// Identity means same compiled snapshot and declaration, not type shape.
    pub fn same_target(&self, other: &Self) -> bool {
        self.declaration == other.declaration && Arc::ptr_eq(&self.program, &other.program)
    }

    pub fn belongs_to(&self, catalog: &CallableCatalog) -> bool {
        Arc::ptr_eq(&self.program, &catalog.program)
    }

    /// Invoke with the existing positional-only API. This remains source- and
    /// signature-compatible with alpha.19's original Rust embedding method.
    pub fn invoke_with_io_and_options(
        &self,
        arguments: Vec<Value>,
        stdout: &mut impl io::Write,
        stderr: &mut impl io::Write,
        options: ExecutionOptions,
    ) -> Result<Value, RunError> {
        self.invoke_bound_with_io_and_options(
            HostArguments::positional(arguments),
            stdout,
            stderr,
            options,
        )
    }

    /// Invoke with optional positional arguments followed by named arguments.
    /// Named labels are checked before any Scarp code, read, or output.
    ///
    /// A named binding may target an earlier required parameter or a later
    /// optional parameter. A supplied value suppresses its default. Unknown
    /// names, duplicates and omissions of required parameters are errors.
    pub fn invoke_bound_with_io_and_options(
        &self,
        arguments: HostArguments,
        stdout: &mut impl io::Write,
        stderr: &mut impl io::Write,
        options: ExecutionOptions,
    ) -> Result<Value, RunError> {
        let bound = self.bind_arguments(arguments)?;
        crate::interpreter::invoke_named_with_io_and_options(
            &self.program,
            self.declaration,
            bound,
            stdout,
            stderr,
            options,
        )
    }

    /// Positional-only reader-enabled invocation. Input consumption remains
    /// irreversible; evaluated output is buffered until successful completion.
    pub fn invoke_with_input_and_options(
        &self,
        arguments: Vec<Value>,
        input: &mut impl io::BufRead,
        stdout: &mut impl io::Write,
        stderr: &mut impl io::Write,
        options: ExecutionOptions,
        input_limits: InputLimits,
    ) -> Result<Value, RunError> {
        self.invoke_bound_with_input_and_options(
            HostArguments::positional(arguments),
            input,
            stdout,
            stderr,
            options,
            input_limits,
        )
    }

    /// Reader-enabled invocation accepting positional and named host values.
    pub fn invoke_bound_with_input_and_options(
        &self,
        arguments: HostArguments,
        input: &mut impl io::BufRead,
        stdout: &mut impl io::Write,
        stderr: &mut impl io::Write,
        options: ExecutionOptions,
        input_limits: InputLimits,
    ) -> Result<Value, RunError> {
        let bound = self.bind_arguments(arguments)?;
        crate::interpreter::invoke_named_with_input_and_options(
            &self.program,
            self.declaration,
            bound,
            input,
            (stdout, stderr),
            options,
            input_limits,
        )
    }

    /// Return exact work, allocation and input/output charges from this
    /// invocation, even on runtime failure. Preflight argument errors reset
    /// 'usage' to zero before any program or I/O operation begins.
    pub fn invoke_bound_with_usage_and_options(
        &self,
        arguments: HostArguments,
        stdout: &mut impl io::Write,
        stderr: &mut impl io::Write,
        options: ExecutionOptions,
        usage: &mut ExecutionUsage,
    ) -> Result<Value, RunError> {
        *usage = ExecutionUsage::default();
        let bound = self.bind_arguments(arguments)?;
        crate::interpreter::invoke_named_with_usage_and_options(
            &self.program,
            self.declaration,
            bound,
            stdout,
            stderr,
            options,
            usage,
        )
    }

    /// Reader-capable dispatch with measured input attempts and raw bytes.
    pub fn invoke_bound_with_input_and_usage_and_options(
        &self,
        arguments: HostArguments,
        input: &mut impl io::BufRead,
        writers: (&mut impl io::Write, &mut impl io::Write),
        options: ExecutionOptions,
        input_limits: InputLimits,
        usage: &mut ExecutionUsage,
    ) -> Result<Value, RunError> {
        *usage = ExecutionUsage::default();
        let bound = self.bind_arguments(arguments)?;
        crate::interpreter::invoke_named_with_input_and_usage_and_options(
            &self.program,
            self.declaration,
            bound,
            input,
            writers,
            options,
            (input_limits, usage),
        )
    }

    /// Resolve host labels into sparse, declaration-indexed argument slots.
    /// The interpreter itself resolves holes using its existing default stage.
    /// Preflight is side-effect-free and bounded by the function signature.
    fn bind_arguments(&self, arguments: HostArguments) -> Result<Vec<Option<Value>>, RunError> {
        let info = self.metadata();
        let HostArguments { positional, named } = arguments;
        let len = info.parameters.len();
        if positional.len() > len || named.len() > len.saturating_sub(positional.len()) {
            return Err(RunError::Diagnostic(Diagnostic::new(
                DiagnosticKind::CallArguments,
                info.span,
                "too many supplied host arguments for the named function",
            )));
        }
        let mut slots = vec![None; len];
        for (index, value) in positional.into_iter().enumerate() {
            let parameter = &info.parameters[index];
            Self::check_value(parameter, &value)?;
            slots[index] = Some(value);
        }
        for (name, value) in named {
            let index = info
                .parameters
                .iter()
                .position(|parameter| parameter.name == name)
                .ok_or_else(|| {
                    RunError::Diagnostic(Diagnostic::new(
                        DiagnosticKind::CallArguments,
                        info.span,
                        "unknown named parameter in host callable invocation",
                    ))
                })?;
            let parameter = &info.parameters[index];
            if slots[index].is_some() {
                return Err(RunError::Diagnostic(Diagnostic::new(
                    DiagnosticKind::CallArguments,
                    parameter.span,
                    "duplicate host argument for a named parameter",
                )));
            }
            Self::check_value(parameter, &value)?;
            slots[index] = Some(value);
        }
        for (parameter, value) in info.parameters.iter().zip(&slots) {
            if value.is_none() && parameter.default.is_none() {
                return Err(RunError::Diagnostic(Diagnostic::new(
                    DiagnosticKind::CallArguments,
                    parameter.span,
                    "missing required host function argument",
                )));
            }
        }
        Ok(slots)
    }

    fn check_value(parameter: &super::ParameterInfo, value: &Value) -> Result<(), RunError> {
        if !matches!(
            &parameter.kind,
            SourceType::Primitive(
                PrimitiveType::Integer
                    | PrimitiveType::Boolean
                    | PrimitiveType::Float
                    | PrimitiveType::String
            )
        ) {
            return Err(RunError::Diagnostic(Diagnostic::new(
                DiagnosticKind::FunctionUnsupported,
                parameter.span,
                "host callable invocation currently supports only scalar integer, boolean, float and string parameters",
            )));
        }
        if value.kind() != parameter.kind {
            return Err(RunError::Diagnostic(Diagnostic::new(
                DiagnosticKind::TypeMismatch,
                parameter.span,
                format!(
                    "host argument {} expects {}, found {}",
                    parameter.name,
                    parameter.kind,
                    value.kind()
                ),
            )));
        }
        if let Value::String(text) = value {
            if text.len() > MAX_TEXT_BYTES {
                return Err(RunError::Diagnostic(Diagnostic::new(
                    DiagnosticKind::TextConstruction,
                    parameter.span,
                    format!("host string parameter exceeds {MAX_TEXT_BYTES} UTF-8 bytes"),
                )));
            }
        }
        Ok(())
    }
}
