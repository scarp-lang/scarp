//! Type checking and dispatch planning for source variants.
use super::*;
use crate::ast::{MatchArm, MatchStatement, VariantConstructor};
use crate::types::{NominalKind, MAX_VARIANT_CASES};

pub(super) fn case_error(span: Span, message: impl Into<String>) -> Diagnostic {
    Diagnostic::new(DiagnosticKind::VariantCase, span, message)
}

fn coverage_error(span: Span, message: impl Into<String>) -> Diagnostic {
    Diagnostic::new(DiagnosticKind::MatchCoverage, span, message)
}

pub(super) fn constructor(
    layout: &nominal::Layouts,
    constructor: &VariantConstructor,
    types: &[SourceType],
    syntax: &Expression,
) -> Result<(SourceType, CompositeOperation), Diagnostic> {
    let kind = nominal::lookup(
        layout,
        NominalKind::Variant,
        &constructor.name,
        constructor.name_span,
    )?;
    let variant = kind.as_variant().expect("variant lookup");
    let (tag, (_, expected)) = variant
        .cases()
        .iter()
        .enumerate()
        .find(|(_, (case, _))| case == &constructor.tag)
        .ok_or_else(|| {
            case_error(
                constructor.tag_span,
                format!("{} has no case {:?}", variant.name(), constructor.tag),
            )
        })?;
    match (expected, constructor.payload) {
        (Some(expected), Some(payload)) => {
            require_type(&types[payload], expected, syntax.nodes()[payload].span)?
        }
        (None, None) => {}
        (Some(_), None) => {
            return Err(case_error(
                constructor.tag_span,
                "this case requires a payload",
            ))
        }
        (None, Some(_)) => {
            return Err(case_error(constructor.tag_span, "this case has no payload"))
        }
    }
    Ok((
        kind.clone(),
        CompositeOperation::Variant {
            kind,
            tag: tag as u8,
        },
    ))
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct MatchTarget {
    pub instruction: usize,
    pub payload_slot: Option<usize>,
}

impl Resolver<'_> {
    #[inline(never)]
    fn plan_match(
        &mut self,
        subject: Expression,
        arms: &[MatchArm],
        span: Span,
    ) -> Result<(usize, SourceType, Vec<Option<usize>>), Diagnostic> {
        let subject = self.expression(subject)?;
        let subject_span = subject.syntax.nodes()[subject.syntax.root()].span;
        let layout = subject.kind.as_variant().ok_or_else(|| {
            case_error(
                subject_span,
                format!("match needs a variant value, found {}", subject.kind),
            )
        })?;
        let cases = layout.cases();
        let mut covered = vec![None; cases.len()];
        let mut tags = Vec::new();
        if arms.is_empty() || arms.len() > MAX_VARIANT_CASES + 1 {
            return Err(coverage_error(span, "match requires 1 to 33 case arms"));
        }
        let mut wildcard = None;
        for arm in arms {
            if let Some(span) = wildcard {
                return Err(
                    coverage_error(arm.tag_span, "case is unreachable after the wildcard")
                        .with_related(span, "the wildcard is here"),
                );
            }
            if arm.tag == "_" {
                if arm.payload.is_some() {
                    return Err(case_error(
                        arm.tag_span,
                        "the wildcard cannot bind a payload",
                    ));
                }
                if covered.iter().all(Option::is_some) {
                    return Err(coverage_error(
                        arm.tag_span,
                        "wildcard is unreachable after all named cases",
                    ));
                }
                wildcard = Some(arm.tag_span);
                tags.push(None);
                continue;
            }
            let (tag, (_, payload)) = cases
                .iter()
                .enumerate()
                .find(|(_, (name, _))| name == &arm.tag)
                .ok_or_else(|| {
                    case_error(
                        arm.tag_span,
                        format!("{} has no case {:?}", layout.name(), arm.tag),
                    )
                })?;
            if let Some(first) = covered[tag].replace(arm.tag_span) {
                return Err(
                    coverage_error(arm.tag_span, format!("duplicate case {:?}", arm.tag))
                        .with_related(first, "the first arm is here"),
                );
            }
            if payload.is_some() != arm.payload.is_some() {
                return Err(case_error(
                    arm.tag_span,
                    if payload.is_some() {
                        "use a payload binding or explicit discard, for example Case(_)"
                    } else {
                        "this case has no payload"
                    },
                ));
            }
            tags.push(Some(tag));
        }
        if wildcard.is_none() {
            let missing = cases
                .iter()
                .zip(&covered)
                .filter(|(_, span)| span.is_none())
                .map(|((tag, _), _)| tag.as_str())
                .collect::<Vec<_>>();
            if !missing.is_empty() {
                return Err(coverage_error(
                    span,
                    format!("non-exhaustive match, missing {}", missing.join(", ")),
                )
                .with_related(layout.declaration_span(), "variant declared here"));
            }
        }

        let kind = subject.kind.clone();
        let b = self.emit(
            ResolvedStatement::Match {
                value: subject,
                dispatch: Box::new([]),
            },
            span,
        );
        Ok((b, kind, tags))
    }

    #[inline(never)]
    fn begin_match_arm(
        &mut self,
        arm: &MatchArm,
        tag: Option<usize>,
        layout: &crate::types::VariantType,
    ) -> Result<MatchTarget, Diagnostic> {
        let instruction = self.statements.len();
        self.enter_scope(arm.span)?;
        let mut payload_slot = None;
        if let Some((name, span)) = arm.payload.as_ref().filter(|(name, _)| name != "_") {
            self.binding_name(name, *span)?;
            let kind = layout.cases()[tag.unwrap()].1.as_ref().unwrap().clone();
            let slot = self.slot_types.len();
            self.slot_types.push(kind.clone());
            self.scopes.last_mut().unwrap().insert(
                name.clone(),
                Binding {
                    slot,
                    declaration: *span,
                    mutable: false,
                    kind,
                    repeat_index: false,
                    parameter: false,
                },
            );
            payload_slot = Some(slot);
        }
        Ok(MatchTarget {
            instruction,
            payload_slot,
        })
    }

    #[inline(never)]
    pub(super) fn resolve_match(&mut self, statement: MatchStatement) -> Result<(), Diagnostic> {
        let (instruction, kind, tags) =
            self.plan_match(statement.subject, &statement.arms, statement.span)?;
        let layout = kind.as_variant().unwrap();
        let mut dispatch = vec![None; layout.cases().len()];
        let mut exits = Vec::new();
        for (arm, tag) in statement.arms.into_iter().zip(tags) {
            let target = self.begin_match_arm(&arm, tag, layout)?;
            if let Some(tag) = tag {
                dispatch[tag] = Some(target);
            } else {
                for entry in &mut dispatch {
                    entry.get_or_insert(target);
                }
            }
            self.statement_list(arm.statements)?;
            self.retire_scope(arm.span);
            self.scopes.pop();
            exits.push(self.emit(ResolvedStatement::Jump { target: 0 }, arm.span));
        }
        let end = self.statements.len();
        for exit in exits {
            self.statements[exit].kind = ResolvedStatement::Jump { target: end };
        }
        if let ResolvedStatement::Match { dispatch: plan, .. } =
            &mut self.statements[instruction].kind
        {
            *plan = dispatch.into_iter().map(Option::unwrap).collect();
        }
        Ok(())
    }
}
