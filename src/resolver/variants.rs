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

#[derive(Debug, Clone)]
pub(crate) struct MatchTarget {
    pub instruction: usize,
    pub payload: Option<MatchPayload>,
}

#[derive(Debug, Clone)]
pub(crate) struct MatchPayload {
    pub steps: Box<[DestructureStep]>,
    pub span: Span,
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
        if subject.kind == ValueType::Boolean {
            return self.plan_boolean_match(subject, arms, span);
        }
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

    // A small refutable-pattern slice: booleans are a finite two-element
    // domain, so exhaustiveness and redundant-arm checks are exact without
    // expanding an unbounded decision tree. Variant matches retain their
    // existing nominal, typed payload and wildcard rules.
    #[inline(never)]
    fn plan_boolean_match(
        &mut self,
        subject: ResolvedExpression,
        arms: &[MatchArm],
        span: Span,
    ) -> Result<(usize, SourceType, Vec<Option<usize>>), Diagnostic> {
        if arms.is_empty() || arms.len() > 3 {
            return Err(coverage_error(span, "boolean match requires 1 to 3 arms"));
        }
        let names = ["false", "true"];
        let mut covered: [Option<Span>; 2] = [None, None];
        let mut tags = Vec::with_capacity(arms.len());
        let mut wildcard = None;
        for arm in arms {
            if let Some(previous) = wildcard {
                return Err(
                    coverage_error(arm.tag_span, "case is unreachable after the wildcard")
                        .with_related(previous, "the wildcard is here"),
                );
            }
            if arm.payload.is_some() {
                return Err(case_error(
                    arm.tag_span,
                    "boolean literal patterns cannot bind a payload",
                ));
            }
            let tag = match arm.tag.as_str() {
                "false" => Some(0),
                "true" => Some(1),
                "_" => None,
                _ => {
                    return Err(case_error(
                        arm.tag_span,
                        format!("boolean match has no literal case {:?}", arm.tag),
                    ))
                }
            };
            match tag {
                Some(index) => {
                    if let Some(first) = covered[index].replace(arm.tag_span) {
                        return Err(
                            coverage_error(arm.tag_span, format!("duplicate case {:?}", arm.tag))
                                .with_related(first, "the first arm is here"),
                        );
                    }
                }
                None => {
                    if covered.iter().all(Option::is_some) {
                        return Err(coverage_error(
                            arm.tag_span,
                            "wildcard is unreachable after all boolean cases",
                        ));
                    }
                    wildcard = Some(arm.tag_span);
                }
            }
            tags.push(tag);
        }
        if wildcard.is_none() {
            let missing = names
                .iter()
                .zip(&covered)
                .filter_map(|(name, covered)| covered.is_none().then_some(*name))
                .collect::<Vec<_>>();
            if !missing.is_empty() {
                return Err(coverage_error(
                    span,
                    format!("non-exhaustive boolean match, missing {}", missing.join(", ")),
                ));
            }
        }
        let instruction = self.emit(
            ResolvedStatement::Match {
                value: subject,
                dispatch: Box::new([]),
            },
            span,
        );
        Ok((instruction, ValueType::Boolean.into(), tags))
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
        let payload = arm
            .payload
            .as_ref()
            .map(|pattern| {
                let kind = layout.cases()[tag.expect("named payload arm")]
                    .1
                    .as_ref()
                    .expect("checked payload case");
                self.compile_pattern(pattern, kind, false)
                    .map(|steps| MatchPayload {
                        steps: steps.into_boxed_slice(),
                        span: pattern.nodes()[0].span,
                    })
            })
            .transpose()?;
        Ok(MatchTarget {
            instruction,
            payload,
        })
    }

    #[inline(never)]
    pub(super) fn resolve_match(&mut self, statement: MatchStatement) -> Result<(), Diagnostic> {
        let (instruction, kind, tags) =
            self.plan_match(statement.subject, &statement.arms, statement.span)?;
        let layout = kind.as_variant();
        let mut dispatch = vec![None; layout.map_or(2, |variant| variant.cases().len())];
        let mut exits = Vec::new();
        for (arm, tag) in statement.arms.into_iter().zip(tags) {
            let target = if let Some(layout) = layout {
                self.begin_match_arm(&arm, tag, layout)?
            } else {
                // The boolean planner established an exhaustive literal match
                // with no payload binding on any arm.
                self.enter_scope(arm.span)?;
                MatchTarget {
                    instruction: self.statements.len(),
                    payload: None,
                }
            };
            if let Some(tag) = tag {
                dispatch[tag] = Some(target);
            } else {
                for entry in &mut dispatch {
                    entry.get_or_insert_with(|| target.clone());
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
