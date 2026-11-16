//! Type-check and lower shared binding patterns.
use super::*;
use crate::ast::PatternField;

impl Resolver<'_> {
    // Keep pattern compilation out of recursively entered statement-list frames.
    #[inline(never)]
    pub(super) fn resolve_destructure(
        &mut self,
        pattern: BindingPattern,
        mutable: bool,
        initializer: Expression,
        span: Span,
    ) -> Result<(), Diagnostic> {
        // Resolve all initializer references before publishing any pattern names.
        let value = self.expression(initializer)?;
        require_value(&value.kind, span)?;
        let steps = self.compile_pattern(&pattern, &value.kind, mutable)?;
        self.emit(ResolvedStatement::Destructure { steps, value }, span);
        Ok(())
    }

    pub(super) fn compile_pattern(
        &mut self,
        pattern: &BindingPattern,
        source_type: &SourceType,
        mutable: bool,
    ) -> Result<Vec<DestructureStep>, Diagnostic> {
        let mut types = vec![source_type];
        let mut seen = HashMap::new();
        let mut bindings = Vec::new();
        let mut steps = Vec::with_capacity(pattern.nodes().len());
        for node in pattern.nodes() {
            let kind = types.pop().expect("validated pattern arity");
            let step = match &node.kind {
                PatternKind::Tuple(arity) => {
                    let elements = kind.tuple_elements().ok_or_else(|| {
                        Diagnostic::new(
                            DiagnosticKind::TypeMismatch,
                            node.span,
                            format!("a tuple destructuring pattern needs a tuple, found {kind}"),
                        )
                    })?;
                    check_pattern_arity("tuple", *arity, elements.len(), node.span)?;
                    types.extend(elements.iter().rev());
                    DestructureStep::Tuple(*arity)
                }
                PatternKind::Array(arity) => {
                    let array = kind.as_array().ok_or_else(|| {
                        Diagnostic::new(
                            DiagnosticKind::TypeMismatch,
                            node.span,
                            format!(
                                "an array destructuring pattern needs a fixed array, found {kind}"
                            ),
                        )
                    })?;
                    check_pattern_arity("array", *arity, array.len(), node.span)?;
                    types.resize(types.len() + *arity, array.element());
                    DestructureStep::Array(*arity)
                }
                PatternKind::Record(fields) => {
                    let record = kind.as_record().ok_or_else(|| {
                        Diagnostic::new(
                            DiagnosticKind::TypeMismatch,
                            node.span,
                            format!("a record destructuring pattern needs a record, found {kind}"),
                        )
                    })?;
                    let offsets = field_offsets(record, fields)?;
                    types.extend(
                        offsets
                            .iter()
                            .rev()
                            .map(|&offset| &record.fields()[offset].1),
                    );
                    DestructureStep::Record(offsets)
                }
                PatternKind::Name(name) => {
                    self.binding_name(name, node.span)?;
                    if let Some(previous) = seen.insert(name, node.span) {
                        return Err(Diagnostic::new(
                            DiagnosticKind::DuplicateBinding,
                            node.span,
                            format!("binding {name:?} occurs more than once in a pattern"),
                        )
                        .with_related(previous, "the first occurrence is here"));
                    }
                    if let Some(previous) = self.scopes.last().unwrap().get(name) {
                        return Err(Diagnostic::new(
                            DiagnosticKind::DuplicateBinding,
                            node.span,
                            format!("binding {name:?} is already declared in this scope"),
                        )
                        .with_related(previous.declaration, "the previous declaration is here"));
                    }
                    let slot = self.slot_types.len() + bindings.len();
                    bindings.push((name, node.span, kind));
                    DestructureStep::Bind(slot)
                }
                PatternKind::Discard => DestructureStep::Discard,
            };
            steps.push(step);
        }
        debug_assert!(types.is_empty());
        for (name, declaration, kind) in bindings {
            let slot = self.slot_types.len();
            self.slot_types.push(kind.clone());
            self.scopes.last_mut().unwrap().insert(
                name.clone(),
                Binding {
                    slot,
                    declaration,
                    mutable,
                    kind: kind.clone(),
                    repeat_index: false,
                    parameter: false,
                },
            );
        }
        Ok(steps)
    }
}

fn field_offsets(
    record: &crate::types::RecordType,
    fields: &[PatternField],
) -> Result<Box<[usize]>, Diagnostic> {
    let mut seen = [None; crate::types::MAX_RECORD_FIELDS];
    let mut offsets = Vec::with_capacity(fields.len());
    for field in fields {
        let offset = record
            .fields()
            .iter()
            .position(|(name, _)| name == &field.name)
            .ok_or_else(|| {
                records::field_error(
                    field.span,
                    format!("{} has no field {:?}", record.name(), field.name),
                )
            })?;
        if let Some(first) = seen[offset].replace(field.span) {
            return Err(records::field_error(
                field.span,
                format!("field {:?} occurs more than once in a pattern", field.name),
            )
            .with_related(first, "the first selection is here"));
        }
        offsets.push(offset);
    }
    Ok(offsets.into_boxed_slice())
}
