//! Rebindable, same-program named-call slots for Rust embedding.
//!
//! Unlike a source-language first-class function, a slot is host-owned and
//! never stored in a Scarp Value, source binding, closure or native ABI.

use std::{error::Error, fmt, io, sync::Arc};

use super::{CallableBindError, CallableCatalog, HostArguments, NamedCallableRef};
use crate::{
    input::InputLimits,
    limits::{ExecutionOptions, ExecutionUsage},
    resolver::ResolvedProgram,
    types::{TypeId, TypeKind, TypeRegistryError},
    value::Value,
    RunError,
};

/// One required canonical function type and a switchable, checked target.
/// The resolved program is retained even after all catalogs are dropped.
#[derive(Debug, Clone)]
pub struct CallableSlot {
    program: Arc<ResolvedProgram>,
    required: TypeId,
    current: Option<NamedCallableRef>,
}

/// Reassignment is atomic: a rejected target never replaces the old one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallableSlotError {
    Registry(TypeRegistryError),
    NotCallable(TypeId),
    UnknownFunction(usize),
    GenericFunction(usize),
    ForeignProgram,
    IncompatibleSignature { required: TypeId, actual: TypeId },
}

impl fmt::Display for CallableSlotError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Registry(error) => error.fmt(f),
            Self::NotCallable(_) => f.write_str("required callable slot type is not a function"),
            Self::UnknownFunction(index) => {
                write!(f, "no named function at declaration index {index}")
            }
            Self::GenericFunction(index) => write!(f, "generic function at declaration index {index} requires a concrete Scarp wrapper for host invocation"),
            Self::ForeignProgram => {
                f.write_str("callable target belongs to a different compiled snapshot")
            }
            Self::IncompatibleSignature { .. } => {
                f.write_str("callable target is incompatible with the slot signature")
            }
        }
    }
}

impl Error for CallableSlotError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Registry(error) => Some(error),
            _ => None,
        }
    }
}

impl From<TypeRegistryError> for CallableSlotError {
    fn from(error: TypeRegistryError) -> Self {
        Self::Registry(error)
    }
}

/// A missing host selection is not a Scarp runtime diagnostic: no program was
/// evaluated. Once bound, actual runtime errors retain their usual kind and
/// IO semantics.
#[derive(Debug)]
pub enum CallableSlotInvokeError {
    Unbound,
    Runtime(RunError),
}
impl fmt::Display for CallableSlotInvokeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unbound => f.write_str("no named function is selected in the callable slot"),
            Self::Runtime(error) => error.fmt(f),
        }
    }
}
impl Error for CallableSlotInvokeError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Unbound => None,
            Self::Runtime(error) => Some(error),
        }
    }
}
impl From<RunError> for CallableSlotInvokeError {
    fn from(error: RunError) -> Self {
        Self::Runtime(error)
    }
}

impl CallableCatalog {
    /// Create an empty slot for a real function signature from this program's
    /// immutable semantic type registry. Non-functions and foreign type IDs
    /// cannot acquire a slot; no type is interned or allocated.
    pub fn slot(&self, required: TypeId) -> Result<CallableSlot, CallableSlotError> {
        match self.program.type_registry().get(required) {
            None => Err(CallableSlotError::Registry(TypeRegistryError::UnknownType(
                required,
            ))),
            Some(TypeKind::Function { .. }) => Ok(CallableSlot {
                program: Arc::clone(&self.program),
                required,
                current: None,
            }),
            Some(_) => Err(CallableSlotError::NotCallable(required)),
        }
    }

    /// Bind the initial target using the same checked operation as every
    /// later rebind. The slot remains tied to this compiled snapshot.
    pub fn slot_bound(
        &self,
        required: TypeId,
        declaration: usize,
    ) -> Result<CallableSlot, CallableSlotError> {
        let mut slot = self.slot(required)?;
        let target = self.bind_exact(declaration).map_err(|error| match error {
            CallableBindError::UnknownFunction(index) => CallableSlotError::UnknownFunction(index),
            CallableBindError::GenericFunction(index) => CallableSlotError::GenericFunction(index),
            CallableBindError::Registry(error) => CallableSlotError::Registry(error),
            CallableBindError::IncompatibleSignature { expected, actual } => {
                CallableSlotError::IncompatibleSignature {
                    required: expected,
                    actual,
                }
            }
        })?;
        slot.set(&target)?;
        Ok(slot)
    }
}

impl CallableSlot {
    /// Exact same-arena semantic signature, retained after catalog drop.
    pub fn required_signature(&self) -> TypeId {
        self.required
    }

    pub fn current(&self) -> Option<&NamedCallableRef> {
        self.current.as_ref()
    }

    pub fn is_bound(&self) -> bool {
        self.current.is_some()
    }

    /// Clear the selection. Other clones of the slot remain independently
    /// bound, and this slot still retains its canonical program/type arena.
    pub fn clear(&mut self) {
        self.current = None;
    }

    /// Select an existing verified handle only if it belongs to the exact
    /// same `Arc<ResolvedProgram>`. A structurally equivalent recompilation
    /// remains foreign, and pure can fill impure but not the converse.
    pub fn set(&mut self, target: &NamedCallableRef) -> Result<(), CallableSlotError> {
        if !Arc::ptr_eq(&self.program, &target.program) {
            return Err(CallableSlotError::ForeignProgram);
        }
        if !self
            .program
            .type_registry()
            .accepts_callable(self.required, target.signature)?
        {
            return Err(CallableSlotError::IncompatibleSignature {
                required: self.required,
                actual: target.signature,
            });
        }
        self.current = Some(target.clone());
        Ok(())
    }

    /// Rebind from a declaration index without ever exposing unchecked
    /// function identity. Failed bounds/type checks keep the prior target.
    pub fn select(&mut self, declaration: usize) -> Result<(), CallableBindError> {
        let selected =
            CallableCatalog::new(Arc::clone(&self.program)).bind(declaration, self.required)?;
        self.current = Some(selected);
        Ok(())
    }

    /// Indirect **Rust host** execution through NamedCallableRef, preserving
    /// positional/named arguments, omitted defaults, contracts and quotas.
    /// There is no Scarp-source function Value or closure capture.
    pub fn invoke_with_io_and_options(
        &self,
        arguments: HostArguments,
        stdout: &mut impl io::Write,
        stderr: &mut impl io::Write,
        options: ExecutionOptions,
    ) -> Result<Value, CallableSlotInvokeError> {
        let target = self
            .current
            .as_ref()
            .ok_or(CallableSlotInvokeError::Unbound)?;
        target
            .invoke_bound_with_io_and_options(arguments, stdout, stderr, options)
            .map_err(Into::into)
    }

    /// Same interpreter path with explicitly provided program input.
    pub fn invoke_with_input_and_options(
        &self,
        arguments: HostArguments,
        input: &mut impl io::BufRead,
        stdout: &mut impl io::Write,
        stderr: &mut impl io::Write,
        options: ExecutionOptions,
        input_limits: InputLimits,
    ) -> Result<Value, CallableSlotInvokeError> {
        let target = self
            .current
            .as_ref()
            .ok_or(CallableSlotInvokeError::Unbound)?;
        target
            .invoke_bound_with_input_and_options(
                arguments,
                input,
                stdout,
                stderr,
                options,
                input_limits,
            )
            .map_err(Into::into)
    }
    /// Exact interpreter charges for host-side indirect invocation.
    /// An unbound slot resets the receipt and never performs I/O.
    pub fn invoke_with_usage_and_options(
        &self,
        arguments: HostArguments,
        stdout: &mut impl io::Write,
        stderr: &mut impl io::Write,
        options: ExecutionOptions,
        usage: &mut ExecutionUsage,
    ) -> Result<Value, CallableSlotInvokeError> {
        *usage = ExecutionUsage::default();
        let target = self
            .current
            .as_ref()
            .ok_or(CallableSlotInvokeError::Unbound)?;
        target
            .invoke_bound_with_usage_and_options(arguments, stdout, stderr, options, usage)
            .map_err(Into::into)
    }

    /// Reader-capable indirect invocation with measured input consumption.
    pub fn invoke_with_input_and_usage_and_options(
        &self,
        arguments: HostArguments,
        input: &mut impl io::BufRead,
        writers: (&mut impl io::Write, &mut impl io::Write),
        options: ExecutionOptions,
        input_limits: InputLimits,
        usage: &mut ExecutionUsage,
    ) -> Result<Value, CallableSlotInvokeError> {
        *usage = ExecutionUsage::default();
        let target = self
            .current
            .as_ref()
            .ok_or(CallableSlotInvokeError::Unbound)?;
        target
            .invoke_bound_with_input_and_usage_and_options(
                arguments,
                input,
                writers,
                options,
                input_limits,
                usage,
            )
            .map_err(Into::into)
    }
}
