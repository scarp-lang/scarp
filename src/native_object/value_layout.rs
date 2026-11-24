//! Checked x86-64 value layout planning from canonical semantic type IDs.

use crate::types::{PrimitiveType, TypeId, TypeKind, TypeRegistry};
use std::{collections::HashMap, fmt};

const MAX_DEPTH: usize = 64;
const MAX_COMPONENTS: usize = 65_536;

/// In-memory size, required alignment and shape under the experimental
/// Linux x86-64 native value layout rules.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeValueLayout {
    /// Size in bytes, including trailing padding.
    pub size: u64,
    /// Required alignment in bytes.
    pub alignment: u64,
    pub shape: NativeLayoutShape,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NativeLayoutShape {
    Scalar,
    /// Byte offsets in tuple position or record declaration order.
    Fields {
        offsets: Vec<u64>,
    },
    /// Element count and byte distance between adjacent elements.
    Array {
        length: u64,
        stride: u64,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeLayoutError {
    UnknownType(TypeId),
    UnsupportedType(TypeId),
    SizeOverflow,
    Complexity,
    Allocation,
}

impl fmt::Display for NativeLayoutError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownType(id) => write!(f, "{id} does not belong to this type registry"),
            Self::UnsupportedType(id) => write!(f, "{id} has no supported native value layout"),
            Self::SizeOverflow => f.write_str("the native value exceeds the target size range"),
            Self::Complexity => f.write_str("the native value layout exceeds its complexity limit"),
            Self::Allocation => f.write_str("unable to allocate native value layout workspace"),
        }
    }
}

impl std::error::Error for NativeLayoutError {}

/// Plan one experimental Linux x86-64 value layout from a semantic registry.
///
/// Integers and binary64 floats occupy eight bytes, and Booleans occupy one.
/// Tuples and records preserve declaration order with natural alignment.
/// Fixed arrays use the padded size of their element as their stride, even
/// when the array is empty. All sizes and offsets are checked `u64` values.
/// Heap-backed, unit, variant and callable types have no layout in this API.
///
/// This query does not change interpreter storage or enable aggregate native
/// code generation. It describes target values, not frame slots or a stable
/// public calling convention.
pub fn plan_x86_64_value_layout(
    registry: &TypeRegistry,
    root: TypeId,
) -> Result<NativeValueLayout, NativeLayoutError> {
    match registry.depth(root) {
        None => return Err(NativeLayoutError::UnknownType(root)),
        Some(depth) if depth > MAX_DEPTH => return Err(NativeLayoutError::Complexity),
        _ => {}
    }
    let mut planner = Planner {
        registry,
        known: HashMap::new(),
        remaining: MAX_COMPONENTS,
    };
    let kind = registry
        .get(root)
        .ok_or(NativeLayoutError::UnknownType(root))?;
    let (extent, shape) = match kind {
        TypeKind::Tuple(fields) | TypeKind::Record { fields, .. } => {
            if fields.len() > MAX_COMPONENTS {
                return Err(NativeLayoutError::Complexity);
            }
            let mut offsets = Vec::new();
            offsets
                .try_reserve_exact(fields.len())
                .map_err(|_| NativeLayoutError::Allocation)?;
            let extent = planner.fields(fields, |offset| offsets.push(offset))?;
            (extent, NativeLayoutShape::Fields { offsets })
        }
        TypeKind::Sequence {
            element,
            length: Some(length),
        } => {
            let (extent, stride) = planner.array(*element, *length)?;
            (
                extent,
                NativeLayoutShape::Array {
                    length: *length,
                    stride,
                },
            )
        }
        _ => (planner.extent(root)?, NativeLayoutShape::Scalar),
    };
    Ok(NativeValueLayout {
        size: extent.size,
        alignment: extent.alignment,
        shape,
    })
}

#[derive(Clone, Copy)]
struct Extent {
    size: u64,
    alignment: u64,
}

struct Planner<'a> {
    registry: &'a TypeRegistry,
    known: HashMap<TypeId, Extent>,
    remaining: usize,
}

impl Planner<'_> {
    fn extent(&mut self, id: TypeId) -> Result<Extent, NativeLayoutError> {
        // Charge visits even for repeated DAG edges. Arrays visit their
        // element descriptor once, regardless of the declared length.
        self.remaining = self
            .remaining
            .checked_sub(1)
            .ok_or(NativeLayoutError::Complexity)?;
        if let Some(extent) = self.known.get(&id) {
            return Ok(*extent);
        }
        let kind = self
            .registry
            .get(id)
            .ok_or(NativeLayoutError::UnknownType(id))?;
        let extent = match kind {
            TypeKind::Primitive(PrimitiveType::Integer | PrimitiveType::Float) => Extent {
                size: 8,
                alignment: 8,
            },
            TypeKind::Primitive(PrimitiveType::Boolean) => Extent {
                size: 1,
                alignment: 1,
            },
            TypeKind::Tuple(fields) | TypeKind::Record { fields, .. } => {
                self.fields(fields, |_| {})?
            }
            TypeKind::Sequence {
                element,
                length: Some(length),
            } => self.array(*element, *length)?.0,
            _ => return Err(NativeLayoutError::UnsupportedType(id)),
        };
        self.known
            .try_reserve(1)
            .map_err(|_| NativeLayoutError::Allocation)?;
        self.known.insert(id, extent);
        Ok(extent)
    }

    fn fields(
        &mut self,
        fields: &[TypeId],
        mut record_offset: impl FnMut(u64),
    ) -> Result<Extent, NativeLayoutError> {
        let mut alignment = 1;
        let mut end = 0u64;
        for field in fields {
            let field = self.extent(*field)?;
            alignment = alignment.max(field.alignment);
            let offset = align_up(end, field.alignment)?;
            record_offset(offset);
            end = offset
                .checked_add(field.size)
                .ok_or(NativeLayoutError::SizeOverflow)?;
        }
        Ok(Extent {
            size: align_up(end, alignment)?,
            alignment,
        })
    }

    fn array(&mut self, element: TypeId, length: u64) -> Result<(Extent, u64), NativeLayoutError> {
        let element = self.extent(element)?;
        let stride = align_up(element.size, element.alignment)?;
        let size = stride
            .checked_mul(length)
            .ok_or(NativeLayoutError::SizeOverflow)?;
        Ok((
            Extent {
                size,
                alignment: element.alignment,
            },
            stride,
        ))
    }
}

fn align_up(size: u64, alignment: u64) -> Result<u64, NativeLayoutError> {
    let padding = (alignment - size % alignment) % alignment;
    size.checked_add(padding)
        .ok_or(NativeLayoutError::SizeOverflow)
}
