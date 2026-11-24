//! Verified direct-call graph for one bounded native object.

use super::{ScalarProgramIr, Span};
use crate::{
    diagnostics::{Diagnostic, DiagnosticKind},
    resolver::ResolvedProgram,
    scalar_ir::Operation,
};

const MAX_FUNCTIONS: usize = 32;
const MAX_IR_STEPS: usize = 32_768;
const MAX_CALL_DEPTH: usize = 16;
const MAX_STACK_BYTES: usize = 64 * 1024;
const RETURN_ADDRESS_BYTES: usize = 8;

pub(in crate::scalar_ir) struct ScalarCallGraph {
    bodies: Vec<ScalarProgramIr>,
    entry: usize,
    edges: Vec<Vec<(usize, Span)>>,
    postorder: Vec<usize>,
    spans: Vec<Span>,
}

fn unsupported(span: Span, message: &str) -> Diagnostic {
    Diagnostic::new(DiagnosticKind::NativeUnsupported, span, message)
}

impl ScalarCallGraph {
    pub(in crate::scalar_ir) fn lower(
        resolved: &ResolvedProgram,
        entry: &str,
        max_parameters: usize,
    ) -> Result<Self, Diagnostic> {
        let signatures = resolved.functions();
        if let Some(step) = resolved.statements.first() {
            return Err(unsupported(
                step.span,
                "a native entry object requires function declarations only",
            ));
        }
        if signatures.len() > MAX_FUNCTIONS || signatures.len() != resolved.functions.len() {
            return Err(unsupported(
                Span::default(),
                "a native entry object supports up to 32 functions",
            ));
        }
        let entry = signatures
            .iter()
            .position(|signature| signature.name == entry)
            .ok_or_else(|| {
                unsupported(
                    Span::default(),
                    "the selected native entry function was not declared",
                )
            })?;
        let mut arities = Vec::with_capacity(signatures.len());
        let mut spans = Vec::with_capacity(signatures.len());
        for signature in signatures {
            if signature.parameters.len() > max_parameters {
                return Err(unsupported(
                    signature.span,
                    "this native call convention supports at most six integer arguments",
                ));
            }
            arities.push(signature.parameters.len());
            spans.push(signature.span);
        }

        let mut bodies = Vec::with_capacity(signatures.len());
        let mut edges = Vec::with_capacity(signatures.len());
        let mut remaining = MAX_IR_STEPS;
        for (id, &span) in spans.iter().enumerate() {
            let body = ScalarProgramIr::lower_selected_function(
                resolved,
                id,
                max_parameters,
                true,
                Some(&arities),
            )?;
            remaining = remaining
                .checked_sub(body.operations().len())
                .ok_or_else(|| {
                    unsupported(
                        span,
                        "native call graph exceeds its total IR instruction limit",
                    )
                })?;
            let mut calls = Vec::new();
            for expression in body
                .operations()
                .iter()
                .filter_map(|step| step.kind.expression())
            {
                remaining = remaining
                    .checked_sub(expression.steps.len())
                    .ok_or_else(|| {
                        unsupported(
                            span,
                            "native call graph exceeds its total IR instruction limit",
                        )
                    })?;
                for instruction in &expression.steps {
                    if let Operation::Call { callee, .. } = instruction.kind {
                        calls.push((callee, instruction.span));
                    }
                }
            }
            bodies.push(body);
            edges.push(calls);
        }
        let mut postorder = Vec::with_capacity(bodies.len());
        let mut state = vec![Visit::New; bodies.len()];
        for id in 0..bodies.len() {
            postorder_visit(id, &edges, &mut state, &mut postorder)?;
        }

        let mut depth = vec![0usize; bodies.len()];
        for &id in &postorder {
            let deepest_callee = edges[id].iter().map(|&(callee, _)| depth[callee]).max();
            depth[id] = 1 + deepest_callee.unwrap_or(0);
            if depth[id] > MAX_CALL_DEPTH {
                return Err(unsupported(
                    spans[id],
                    "native call chain exceeds the 16-frame limit",
                ));
            }
        }
        Ok(Self {
            bodies,
            entry,
            edges,
            postorder,
            spans,
        })
    }

    pub(in crate::scalar_ir) fn entry(&self) -> usize {
        self.entry
    }

    pub(in crate::scalar_ir) fn bodies(&self) -> &[ScalarProgramIr] {
        &self.bodies
    }

    /// The longest active path includes each frame's actual allocation and
    /// its saved return address. Sequential calls do not accumulate stack.
    pub(in crate::scalar_ir) fn verify_stack(&self, frames: &[u32]) -> Result<(), Diagnostic> {
        if frames.len() != self.bodies.len() {
            return Err(super::invalid(Span::default()));
        }
        let mut peak = vec![0usize; frames.len()];
        for &id in &self.postorder {
            let child = self.edges[id].iter().map(|&(callee, _)| peak[callee]).max();
            peak[id] = (frames[id] as usize)
                .checked_add(RETURN_ADDRESS_BYTES)
                .and_then(|frame| frame.checked_add(child.unwrap_or(0)))
                .filter(|&bytes| bytes <= MAX_STACK_BYTES)
                .ok_or_else(|| {
                    unsupported(
                        self.spans[id],
                        "native call chain exceeds the 64 KiB stack limit",
                    )
                })?;
        }
        Ok(())
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Visit {
    New,
    Active,
    Finished,
}

// Recursion here is bounded by the compiler's 32-function input limit.
fn postorder_visit(
    id: usize,
    edges: &[Vec<(usize, Span)>],
    state: &mut [Visit],
    postorder: &mut Vec<usize>,
) -> Result<(), Diagnostic> {
    if state[id] == Visit::Finished {
        return Ok(());
    }
    state[id] = Visit::Active;
    for &(callee, span) in &edges[id] {
        if state[callee] == Visit::Active {
            return Err(unsupported(
                span,
                "the native call graph must be nonrecursive",
            ));
        }
        postorder_visit(callee, edges, state, postorder)?;
    }
    state[id] = Visit::Finished;
    postorder.push(id);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scalar_ir::program::ProgramOp;

    const CHAIN: &str = "pure funcra top(n: integer) -> integer { return leaf(n); }
                         pure funcra leaf(n: integer) -> integer { return n; }";

    #[test]
    fn stack_limit_includes_every_active_return_address() {
        let resolved = crate::compile(CHAIN).unwrap();
        let graph = ScalarCallGraph::lower(&resolved, "top", 6).unwrap();
        assert_eq!(graph.postorder, [1, 0]);
        assert!(graph.verify_stack(&[32_760, 32_760]).is_ok());
        assert!(graph.verify_stack(&[32_776, 32_760]).is_err());
        assert!(graph.verify_stack(&[32_760]).is_err());
    }

    #[test]
    fn verifier_rejects_forged_callee_and_missing_argument() {
        let resolved = crate::compile(CHAIN).unwrap();
        let graph = ScalarCallGraph::lower(&resolved, "top", 6).unwrap();
        for missing_argument in [false, true] {
            let mut body = graph.bodies[0].clone();
            let call = body
                .steps
                .iter_mut()
                .find_map(|step| {
                    let ProgramOp::Return { value } = &mut step.kind else {
                        return None;
                    };
                    value
                        .steps
                        .iter_mut()
                        .find(|step| matches!(step.kind, Operation::Call { .. }))
                })
                .unwrap();
            let Operation::Call {
                callee, arguments, ..
            } = &mut call.kind
            else {
                unreachable!()
            };
            if missing_argument {
                arguments.clear();
            } else {
                *callee = usize::MAX;
            }
            let span = call.span;
            assert_eq!(
                body.verify_with_calls(Some(&[1, 1])).unwrap_err().span,
                span
            );
        }
    }
}
