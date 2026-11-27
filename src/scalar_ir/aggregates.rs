//! Stack-only aggregate shapes lowered to the existing verified scalar lanes.

use super::{invalid, Diagnostic, Operation, ScalarExpressionIr, Span};
use crate::{
    ast::{ExpressionKind, ExpressionNode},
    native_object::{plan_x86_64_value_layout, NativeLayoutShape},
    resolver::{NodeMetadata, ResolvedExpression},
    types::{TypeId, TypeKind, TypeRegistry},
    value::ValueType,
};
use std::ops::Range;

pub(super) const MAX_AGGREGATE_LANES: usize = 32;

pub(super) fn scalar(id: TypeId) -> Option<ValueType> {
    match id.primitive() {
        Some(kind @ (ValueType::Integer | ValueType::Boolean)) => Some(kind),
        _ => None,
    }
}

pub(super) fn lanes(
    registry: Option<&TypeRegistry>,
    id: TypeId,
    span: Span,
) -> Result<Vec<ValueType>, Diagnostic> {
    if let Some(kind) = scalar(id) {
        return Ok(vec![kind]);
    }
    let registry = registry.ok_or_else(|| invalid(span))?;
    let kinds = match registry.get(id) {
        Some(TypeKind::Tuple(fields) | TypeKind::Record { fields, .. })
            if (1..=MAX_AGGREGATE_LANES).contains(&fields.len()) =>
        {
            fields
                .iter()
                .copied()
                .map(scalar)
                .collect::<Option<Vec<_>>>()
        }
        Some(TypeKind::Sequence {
            element,
            length: Some(length),
        }) if (1..=MAX_AGGREGATE_LANES as u64).contains(length) => {
            scalar(*element).map(|kind| vec![kind; *length as usize])
        }
        _ => None,
    }
    .ok_or_else(|| {
        invalid(span)
            .with_help("native aggregate locals require 1–32 integer or Boolean components")
    })?;
    // Validate the canonical target shape. Within an object we use separate
    // eight-byte frame lanes, so no external aggregate ABI is involved.
    let layout = plan_x86_64_value_layout(registry, id).map_err(|_| invalid(span))?;
    match layout.shape {
        NativeLayoutShape::Fields { offsets } if offsets.len() == kinds.len() => Ok(kinds),
        NativeLayoutShape::Array { length, .. } if length == kinds.len() as u64 => Ok(kinds),
        _ => Err(invalid(span)),
    }
}

fn literal_index(nodes: &[ExpressionNode], mut index: usize) -> Option<usize> {
    loop {
        match nodes.get(index)?.kind {
            ExpressionKind::Integer(value) => return usize::try_from(value).ok(),
            ExpressionKind::Group { expression } => index = expression,
            _ => return None,
        }
    }
}

impl ScalarExpressionIr {
    pub(super) fn copy_lanes(
        &mut self,
        dst: Range<usize>,
        src: Range<usize>,
        span: Span,
    ) -> Result<(), Diagnostic> {
        if dst.len() != src.len() {
            return Err(invalid(span));
        }
        for (dst, src) in dst.zip(src) {
            self.emit(span, Operation::Move { dst, src })?;
        }
        Ok(())
    }

    pub(super) fn finish_aggregate(
        &mut self,
        expression: &ResolvedExpression,
        ranges: &[Range<usize>],
        index: usize,
        registry: &TypeRegistry,
    ) -> Result<(), Diagnostic> {
        let nodes = expression.syntax.nodes();
        let node = &nodes[index];
        let span = node.span;
        let dst = ranges[index].clone();
        match &node.kind {
            ExpressionKind::Tuple { elements } | ExpressionKind::Array { elements, .. } => {
                // Charge the interpreter's finish task and each component.
                // Arrays also account for their scalar type node and header.
                let array = matches!(node.kind, ExpressionKind::Array { .. });
                for _ in 0..1 + elements.len() + 2 * usize::from(array) {
                    self.emit(span, Operation::Work)?;
                }
                self.copy_components(dst, elements.iter().copied(), ranges, span)?;
            }
            ExpressionKind::Record { fields, .. } => {
                let Some(NodeMetadata::Record { order, .. }) = expression.node_metadata.get(&index)
                else {
                    return Err(invalid(span));
                };
                for _ in 0..1 + order.len() {
                    self.emit(span, Operation::Work)?;
                }
                self.copy_components(
                    dst,
                    order.iter().map(|&position| fields[position].expression),
                    ranges,
                    span,
                )?;
            }
            ExpressionKind::Field { receiver, .. } => {
                self.emit(span, Operation::Work)?;
                let Some(NodeMetadata::Field(offset)) = expression.node_metadata.get(&index) else {
                    return Err(invalid(span));
                };
                let src = component(&ranges[*receiver], *offset, span)?;
                self.copy_lanes(dst, src..src + 1, span)?;
            }
            ExpressionKind::Index {
                vector,
                index: subscript,
                operator_span,
                ..
            } => {
                self.emit(*operator_span, Operation::Work)?;
                let source = &ranges[*vector];
                match registry.get(expression.types[*vector]) {
                    Some(TypeKind::Tuple(_)) => {
                        let offset =
                            literal_index(nodes, *subscript).ok_or_else(|| invalid(span))?;
                        let src = component(source, offset, span)?;
                        self.copy_lanes(dst, src..src + 1, span)?;
                    }
                    Some(TypeKind::Sequence {
                        length: Some(_), ..
                    }) if dst.len() == 1 => {
                        self.emit(
                            nodes[*subscript].span,
                            Operation::Index {
                                dst: dst.start,
                                index: ranges[*subscript].start,
                                elements: source.clone().collect(),
                            },
                        )?;
                    }
                    _ => return Err(invalid(span)),
                }
            }
            _ => return Err(invalid(span)),
        }
        Ok(())
    }

    fn copy_components(
        &mut self,
        dst: Range<usize>,
        elements: impl ExactSizeIterator<Item = usize>,
        ranges: &[Range<usize>],
        span: Span,
    ) -> Result<(), Diagnostic> {
        if dst.len() != elements.len() {
            return Err(invalid(span));
        }
        for (dst, source) in dst.zip(elements) {
            let src = &ranges[source];
            self.copy_lanes(dst..dst + 1, src.clone(), span)?;
        }
        Ok(())
    }
}

fn component(range: &Range<usize>, offset: usize, span: Span) -> Result<usize, Diagnostic> {
    if offset >= range.len() {
        return Err(invalid(span));
    }
    Ok(range.start + offset)
}
