//! Checked, host-only identities for named functions in one resolved program.
//! These handles do not implement indirect execution or closure capture.

use std::{error::Error, fmt, sync::Arc};

use super::{FunctionInfo, ResolvedProgram};
use crate::types::{TypeId, TypeRegistryError};

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
}
