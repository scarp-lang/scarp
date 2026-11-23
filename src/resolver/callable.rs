//! Checked, host-only identities for named functions in one resolved program.
//! These handles do not implement indirect execution or closure capture.

use std::{error::Error, fmt, io, sync::Arc};

use super::{FunctionInfo, ResolvedProgram};
use crate::{
    diagnostics::{Diagnostic, DiagnosticKind},
    input::InputLimits,
    limits::ExecutionOptions,
    types::{PrimitiveType, SourceType, TypeId, TypeRegistryError},
    value::{Value, MAX_TEXT_BYTES},
    RunError,
};

/// An immutable, caller-owned compiled-program snapshot.
#[derive(Debug, Clone)]
pub struct CallableCatalog {
    program: Arc<ResolvedProgram>,
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

    /// Execute a checked named target with supplied positional scalar values,
    /// returning the function result. Omitted trailing defaults are evaluated
    /// by the existing call machine. Does not run entry-script statements.
    ///
    /// Accepted host parameters are integer, boolean, finite float and bounded
    /// string. Composite values, including nominal types from other program
    /// snapshots, are deliberately unsupported in this first executable slice.
    pub fn invoke_with_io_and_options(
        &self,
        arguments: Vec<Value>,
        stdout: &mut impl io::Write,
        stderr: &mut impl io::Write,
        options: ExecutionOptions,
    ) -> Result<Value, RunError> {
        self.check_arguments(&arguments)?;
        crate::interpreter::invoke_named_with_io_and_options(
            &self.program,
            self.declaration,
            arguments,
            stdout,
            stderr,
            options,
        )
    }

    /// Invoke with an explicit buffered input reader and independent input
    /// limits. Evaluation failures publish no ordinary print events; input
    /// bytes already consumed from the host reader cannot be rolled back.
    pub fn invoke_with_input_and_options(
        &self,
        arguments: Vec<Value>,
        input: &mut impl io::BufRead,
        stdout: &mut impl io::Write,
        stderr: &mut impl io::Write,
        options: ExecutionOptions,
        input_limits: InputLimits,
    ) -> Result<Value, RunError> {
        self.check_arguments(&arguments)?;
        crate::interpreter::invoke_named_with_input_and_options(
            &self.program,
            self.declaration,
            arguments,
            input,
            stdout,
            stderr,
            options,
            input_limits,
        )
    }

    fn check_arguments(&self, arguments: &[Value]) -> Result<(), RunError> {
        let info = self.metadata();
        if arguments.len() > info.parameters.len()
            || info.parameters[arguments.len()..]
                .iter()
                .any(|parameter| parameter.default.is_none())
        {
            return Err(RunError::Diagnostic(Diagnostic::new(
                DiagnosticKind::CallArguments,
                info.span,
                format!(
                    "funcra {} expects at most {} positional arguments and all required arguments",
                    info.name,
                    info.parameters.len()
                ),
            )));
        }
        for (parameter, value) in info.parameters.iter().zip(arguments) {
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
        }
        Ok(())
    }
}
