//! Lower named record fields to checked layout offsets.
use super::*;
use crate::ast::CallArgument;

pub(super) fn field_error(span: Span, message: impl Into<String>) -> Diagnostic {
    Diagnostic::new(DiagnosticKind::RecordField, span, message)
}

pub(super) fn constructor(
    kind: &SourceType,
    fields: &[CallArgument],
    types: &[SourceType],
    syntax: &Expression,
    span: Span,
) -> Result<NodeMetadata, Diagnostic> {
    let layout = kind.as_record().expect("record lookup returns a layout");
    let mut order = vec![None; layout.fields().len()];
    for (input, field) in fields.iter().enumerate() {
        let (name, field_span) = field.label.as_ref().expect("constructor fields have names");
        let (index, (_, expected)) = layout
            .fields()
            .iter()
            .enumerate()
            .find(|(_, (s, _))| s == name)
            .ok_or_else(|| {
                field_error(
                    *field_span,
                    format!("{} has no field {name:?}", layout.name()),
                )
            })?;
        if let Some(first) = order[index].replace(input) {
            let (_, first_span) = fields[first].label.as_ref().expect("field name");
            return Err(
                field_error(*field_span, format!("field {name:?} is initialized twice"))
                    .with_related(*first_span, "the first initializer is here"),
            );
        }
        require_type(
            &types[field.expression],
            expected,
            syntax.nodes()[field.expression].span,
        )?;
    }
    let order = order
        .into_iter()
        .zip(layout.fields())
        .map(|(position, (name, _))| {
            position.ok_or_else(|| field_error(span, format!("missing field {name:?}")))
        })
        .collect::<Result<Box<[_]>, _>>()?;
    Ok(NodeMetadata::Record {
        kind: kind.clone(),
        order,
    })
}
