//! Stack-only aggregate shapes lowered to the existing verified scalar lanes.

use super::{invalid, Diagnostic, Operation, ScalarExpressionIr, Span};
use crate::{
    ast::{ExpressionKind, ExpressionNode},
    native_object::{plan_x86_64_value_layout, NativeLayoutShape},
    resolver::{NodeMetadata, ResolvedExpression},
    types::{TypeId, TypeKind, TypeRegistry},
    value::ValueType,
};
use std::{collections::HashMap, ops::Range};

pub(super) const MAX_AGGREGATE_LANES: usize = 32;

pub(super) fn scalar(id: TypeId) -> Option<ValueType> {
    id.primitive()
        .filter(|&kind| super::integer_width::scalar(kind))
}

/// Each logical component occupies a contiguous range of scalar lanes.
/// These ranges describe private stack storage, independent of ABI padding.
struct AggregateLayout {
    lanes: Vec<ValueType>,
    components: Vec<Range<usize>>,
    type_nodes: usize,
    array_element_nodes: usize,
}

pub(super) struct AggregateLayouts<'a> {
    registry: &'a TypeRegistry,
    layouts: HashMap<TypeId, AggregateLayout>,
}

impl<'a> AggregateLayouts<'a> {
    pub(super) fn new(registry: &'a TypeRegistry) -> Self {
        Self {
            registry,
            layouts: HashMap::new(),
        }
    }

    pub(super) fn lanes(&mut self, id: TypeId, span: Span) -> Result<&[ValueType], Diagnostic> {
        if !self.layouts.contains_key(&id) {
            // The target planner bounds shape depth and verifies the type ID
            // before the private layout descends through the registry.
            let target = plan_x86_64_value_layout(self.registry, id).map_err(|_| invalid(span))?;
            let layout = self.plan(id, span)?;
            let components = layout.components.len();
            match target.shape {
                NativeLayoutShape::Fields { offsets } if offsets.len() == components => {}
                NativeLayoutShape::Array { length, .. } if length == components as u64 => {}
                NativeLayoutShape::Scalar if components == 0 => {}
                _ => return Err(invalid(span)),
            }
            self.layouts.insert(id, layout);
        }
        Ok(&self.get(id, span)?.lanes)
    }

    fn get(&self, id: TypeId, span: Span) -> Result<&AggregateLayout, Diagnostic> {
        self.layouts.get(&id).ok_or_else(|| invalid(span))
    }

    fn plan(&mut self, id: TypeId, span: Span) -> Result<AggregateLayout, Diagnostic> {
        let unsupported = || {
            invalid(span)
                .with_help("native aggregate locals require 1–32 integer or Boolean leaves")
        };
        let registry = self.registry;
        let mut layout = AggregateLayout {
            lanes: Vec::new(),
            components: Vec::new(),
            type_nodes: 1,
            array_element_nodes: 0,
        };
        if let Some(kind) = scalar(id) {
            layout.lanes.push(kind);
        } else {
            match registry.get(id) {
                Some(TypeKind::Tuple(fields) | TypeKind::Record { fields, .. })
                    if (1..=MAX_AGGREGATE_LANES).contains(&fields.len()) =>
                {
                    for &field in fields.iter() {
                        let child = self.child(field, span)?;
                        layout.push_component(child, span)?;
                        layout.type_nodes += child.type_nodes;
                    }
                }
                Some(TypeKind::Sequence {
                    element,
                    length: Some(length),
                }) if (1..=MAX_AGGREGATE_LANES as u64).contains(length) => {
                    let child = self.child(*element, span)?;
                    for _ in 0..*length {
                        layout.push_component(child, span)?;
                    }
                    layout.array_element_nodes = child.type_nodes;
                    layout.type_nodes += child.type_nodes;
                }
                _ => return Err(unsupported()),
            }
        }
        Ok(layout)
    }

    fn child(&mut self, id: TypeId, span: Span) -> Result<&AggregateLayout, Diagnostic> {
        if !self.layouts.contains_key(&id) {
            let layout = self.plan(id, span)?;
            self.layouts.insert(id, layout);
        }
        self.get(id, span)
    }
}

impl AggregateLayout {
    fn push_component(&mut self, child: &Self, span: Span) -> Result<(), Diagnostic> {
        let start = self.lanes.len();
        let end = start + child.lanes.len();
        if end > MAX_AGGREGATE_LANES {
            return Err(
                invalid(span).with_help("native aggregate locals support at most 32 leaves")
            );
        }
        self.lanes.extend_from_slice(&child.lanes);
        self.components.push(start..end);
        Ok(())
    }

    fn component(
        &self,
        storage: &Range<usize>,
        member: usize,
        span: Span,
    ) -> Result<Range<usize>, Diagnostic> {
        let member = self.components.get(member).ok_or_else(|| invalid(span))?;
        if storage.len() != self.lanes.len() {
            return Err(invalid(span));
        }
        Ok(storage.start + member.start..storage.start + member.end)
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
        layouts: &AggregateLayouts<'_>,
    ) -> Result<(), Diagnostic> {
        let nodes = expression.syntax.nodes();
        let node = &nodes[index];
        let span = node.span;
        let dst = ranges[index].clone();
        match &node.kind {
            ExpressionKind::Tuple { elements } | ExpressionKind::Array { elements, .. } => {
                let layout = layouts.get(expression.types[index], span)?;
                // Array construction charges the element type tree and the
                // array header as well as the finish task and element count.
                let array_shape = if matches!(node.kind, ExpressionKind::Array { .. }) {
                    1 + layout.array_element_nodes
                } else {
                    0
                };
                for _ in 0..1 + elements.len() + array_shape {
                    self.emit(span, Operation::Work)?;
                }
                self.copy_components(layout, dst, elements.iter().copied(), ranges, span)?;
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
                    layouts.get(expression.types[index], span)?,
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
                let layout = layouts.get(expression.types[*receiver], span)?;
                let src = layout.component(&ranges[*receiver], *offset, span)?;
                self.copy_lanes(dst, src, span)?;
            }
            ExpressionKind::Index {
                vector,
                index: subscript,
                operator_span,
                ..
            } => {
                self.emit(*operator_span, Operation::Work)?;
                let source = &ranges[*vector];
                let layout = layouts.get(expression.types[*vector], span)?;
                match layouts.registry.get(expression.types[*vector]) {
                    Some(TypeKind::Tuple(_)) => {
                        let offset =
                            literal_index(nodes, *subscript).ok_or_else(|| invalid(span))?;
                        let src = layout.component(source, offset, span)?;
                        self.copy_lanes(dst, src, span)?;
                    }
                    Some(TypeKind::Sequence {
                        length: Some(_), ..
                    }) => {
                        // The subscript is evaluated once. Each lane selects
                        // from the same checked index and uses the same span.
                        for (lane, dst) in dst.enumerate() {
                            let elements = layout
                                .components
                                .iter()
                                .map(|component| source.start + component.start + lane)
                                .collect();
                            self.emit(
                                nodes[*subscript].span,
                                Operation::Index {
                                    dst,
                                    index: ranges[*subscript].start,
                                    elements,
                                },
                            )?;
                        }
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
        layout: &AggregateLayout,
        dst: Range<usize>,
        elements: impl ExactSizeIterator<Item = usize>,
        ranges: &[Range<usize>],
        span: Span,
    ) -> Result<(), Diagnostic> {
        if layout.components.len() != elements.len() {
            return Err(invalid(span));
        }
        for (member, source) in elements.enumerate() {
            let src = &ranges[source];
            self.copy_lanes(layout.component(&dst, member, span)?, src.clone(), span)?;
        }
        Ok(())
    }
}
