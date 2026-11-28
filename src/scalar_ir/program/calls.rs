//! Verified direct-call graph for one bounded native object.

use super::{ScalarProgramIr, Span};
use crate::{
    diagnostics::{Diagnostic, DiagnosticKind},
    native_object::{BOUNDED_INTEGER_ENTRY_MAX_STACK_BYTES, RECURSIVE_INTEGER_ENTRY_MAX_FRAMES},
    native_platform::NativeProfile,
    resolver::ResolvedProgram,
    scalar_ir::{CallTargets, Operation},
};

const MAX_FUNCTIONS: usize = 32;
const MAX_IR_STEPS: usize = 32_768;
const MAX_CALL_DEPTH: usize = 16;
const RETURN_ADDRESS_BYTES: u32 = 8;

pub(in crate::scalar_ir) struct ScalarCallGraph {
    bodies: Vec<ScalarProgramIr>,
    entry: usize,
    edges: Vec<Vec<(usize, Span)>>,
    mode: GraphMode,
    spans: Vec<Span>,
}

enum GraphMode {
    Acyclic { postorder: Vec<usize>, depth: usize },
    Recursive { max_frames: usize },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::scalar_ir) struct StackBound {
    pub bytes: u32,
    pub frames: usize,
}

fn unsupported(span: Span, message: &str) -> Diagnostic {
    Diagnostic::new(DiagnosticKind::NativeUnsupported, span, message)
}

impl ScalarCallGraph {
    pub(in crate::scalar_ir) fn lower(
        resolved: &ResolvedProgram,
        entry: &str,
        max_parameters: usize,
        recursion_frames: Option<usize>,
    ) -> Result<Self, Diagnostic> {
        Self::lower_for_target(
            resolved,
            entry,
            max_parameters,
            recursion_frames,
            NativeProfile::Scalar,
        )
    }

    pub(in crate::scalar_ir) fn lower_platform(
        resolved: &ResolvedProgram,
        entry: &str,
        max_parameters: usize,
        profile: NativeProfile,
    ) -> Result<Self, Diagnostic> {
        Self::lower_for_target(resolved, entry, max_parameters, None, profile)
    }

    fn lower_for_target(
        resolved: &ResolvedProgram,
        entry: &str,
        max_parameters: usize,
        recursion_frames: Option<usize>,
        profile: NativeProfile,
    ) -> Result<Self, Diagnostic> {
        let signatures = resolved.functions();
        if let Some(step) = resolved.statements.first() {
            return Err(unsupported(
                step.span,
                "a native entry object requires function declarations only",
            ));
        }
        if signatures.len() > MAX_FUNCTIONS {
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
        if let Some(frames) = recursion_frames {
            if !(1..=RECURSIVE_INTEGER_ENTRY_MAX_FRAMES).contains(&frames) {
                return Err(unsupported(
                    signatures[entry].span,
                    "native recursion requires a frame allowance in 1 through 64",
                ));
            }
        }
        if !signatures[entry].type_parameters.is_empty() {
            return Err(unsupported(
                signatures[entry].span,
                "the native entry needs an ordinary pure integer wrapper for a generic function",
            ));
        }
        if profile != NativeProfile::Scalar && !signatures[entry].parameters.is_empty() {
            return Err(unsupported(
                signatures[entry].span,
                "this native platform entry must have zero parameters",
            ));
        }
        let mut targets = CallTargets {
            indices: vec![None; resolved.functions.len()],
            arities: Vec::new(),
            profile,
        };
        let mut source_ids = Vec::new();
        let mut spans = Vec::new();
        for id in 0..resolved.functions.len() {
            let signature = resolved.function_metadata(id);
            if !signature.type_parameters.is_empty() {
                continue;
            }
            if source_ids.len() >= MAX_FUNCTIONS {
                return Err(unsupported(
                    signature.span,
                    "a native entry object supports up to 32 concrete bodies including generic instances",
                ));
            }
            if signature.parameters.len() > max_parameters {
                return Err(unsupported(
                    signature.span,
                    "this native call convention supports at most sixteen integer arguments",
                ));
            }
            targets.indices[id] = Some(source_ids.len());
            targets.arities.push(signature.parameters.len());
            source_ids.push(id);
            spans.push(signature.span);
        }

        let entry = targets.indices[entry].expect("checked concrete entry");
        let mut bodies = Vec::with_capacity(source_ids.len());
        let mut edges = Vec::with_capacity(source_ids.len());
        let mut remaining = MAX_IR_STEPS;
        for (id, &span) in source_ids.into_iter().zip(&spans) {
            let body = ScalarProgramIr::lower_selected_function(
                resolved,
                id,
                max_parameters,
                true,
                Some(&targets),
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
        let mode = match recursion_frames {
            Some(max_frames) => GraphMode::Recursive { max_frames },
            None => acyclic_order(entry, &edges, &spans)?,
        };
        Ok(Self {
            bodies,
            entry,
            edges,
            mode,
            spans,
        })
    }

    pub(in crate::scalar_ir) fn entry(&self) -> usize {
        self.entry
    }

    pub(in crate::scalar_ir) fn recursion_frames(&self) -> Option<usize> {
        match self.mode {
            GraphMode::Acyclic { .. } => None,
            GraphMode::Recursive { max_frames } => Some(max_frames),
        }
    }

    pub(in crate::scalar_ir) fn bodies(&self) -> &[ScalarProgramIr] {
        &self.bodies
    }

    /// Visit each reachable body once, including back edges, then retain
    /// source order for deterministic output.
    pub(in crate::scalar_ir) fn emission_order(&self) -> Vec<usize> {
        let mut reachable = vec![false; self.bodies.len()];
        reachable[self.entry] = true;
        let mut pending = vec![self.entry];
        while let Some(id) = pending.pop() {
            for &(callee, _) in &self.edges[id] {
                if !reachable[callee] {
                    reachable[callee] = true;
                    pending.push(callee);
                }
            }
        }
        std::iter::once(self.entry)
            .chain((0..self.bodies.len()).filter(|&id| id != self.entry && reachable[id]))
            .collect()
    }

    /// The longest active path includes each frame's actual allocation and
    /// its saved return address. Sequential calls do not accumulate stack.
    pub(in crate::scalar_ir) fn verify_stack(
        &self,
        frames: &[u32],
        max_stack_bytes: u32,
        entry_overhead: u32,
    ) -> Result<StackBound, Diagnostic> {
        if frames.len() != self.bodies.len() {
            return Err(super::invalid(Span::default()));
        }
        let check_hard_limit = |bytes, id| {
            if bytes > BOUNDED_INTEGER_ENTRY_MAX_STACK_BYTES {
                return Err(unsupported(
                    self.spans[id],
                    "native call chain exceeds the 64 KiB stack limit",
                ));
            }
            Ok(())
        };
        let mut bound = match &self.mode {
            GraphMode::Acyclic { postorder, depth } => {
                let mut peak = vec![0; frames.len()];
                for &id in postorder {
                    let child = self.edges[id].iter().map(|&(callee, _)| peak[callee]).max();
                    let bytes = extend_stack(frames[id], child.unwrap_or(0))?;
                    check_hard_limit(bytes, id)?;
                    peak[id] = bytes;
                }
                StackBound {
                    bytes: peak[self.entry],
                    frames: *depth,
                }
            }
            GraphMode::Recursive { max_frames } => {
                let bound = self.recursive_stack_bound(frames, *max_frames)?;
                check_hard_limit(bound.bytes, self.entry)?;
                bound
            }
        };
        bound.bytes = bound
            .bytes
            .checked_add(entry_overhead)
            .ok_or_else(|| super::invalid(self.spans[self.entry]))?;
        check_hard_limit(bound.bytes, self.entry)?;
        if bound.bytes > max_stack_bytes {
            return Err(unsupported(
                self.spans[self.entry],
                &format!(
                    "native entry requires {} stack bytes, exceeding the caller budget of {max_stack_bytes}",
                    bound.bytes
                ),
            ));
        }
        Ok(bound)
    }

    /// Each iteration permits one additional active frame. Read only the
    /// previous layer so a cycle cannot add frames in the same iteration.
    fn recursive_stack_bound(
        &self,
        frames: &[u32],
        max_frames: usize,
    ) -> Result<StackBound, Diagnostic> {
        let empty = StackBound {
            bytes: 0,
            frames: 0,
        };
        let mut previous = vec![empty; frames.len()];
        let mut current = previous.clone();
        for remaining in 1..=max_frames {
            for (id, &frame) in frames.iter().enumerate() {
                let mut child = empty;
                if remaining > 1 {
                    for &(callee, _) in &self.edges[id] {
                        child.bytes = child.bytes.max(previous[callee].bytes);
                        child.frames = child.frames.max(previous[callee].frames);
                    }
                }
                current[id] = StackBound {
                    bytes: extend_stack(frame, child.bytes)?,
                    frames: 1 + child.frames,
                };
            }
            std::mem::swap(&mut previous, &mut current);
        }
        Ok(previous[self.entry])
    }
}

fn extend_stack(frame: u32, child: u32) -> Result<u32, Diagnostic> {
    frame
        .checked_add(RETURN_ADDRESS_BYTES)
        .and_then(|bytes| bytes.checked_add(child))
        .ok_or_else(|| super::invalid(Span::default()))
}

fn acyclic_order(
    entry: usize,
    edges: &[Vec<(usize, Span)>],
    spans: &[Span],
) -> Result<GraphMode, Diagnostic> {
    let mut postorder = Vec::with_capacity(edges.len());
    let mut state = vec![Visit::New; edges.len()];
    for id in 0..edges.len() {
        postorder_visit(id, edges, &mut state, &mut postorder)?;
    }
    let mut depth = vec![0usize; edges.len()];
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
    Ok(GraphMode::Acyclic {
        postorder,
        depth: depth[entry],
    })
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
        let graph = ScalarCallGraph::lower(&resolved, "top", 6, None).unwrap();
        let limit = BOUNDED_INTEGER_ENTRY_MAX_STACK_BYTES;
        assert_eq!(
            graph.verify_stack(&[32_760, 32_760], limit, 0).unwrap(),
            StackBound {
                bytes: limit,
                frames: 2
            },
        );
        assert!(graph.verify_stack(&[32_776, 32_760], u32::MAX, 0).is_err());
        assert!(graph.verify_stack(&[32_760], limit, 0).is_err());
    }

    #[test]
    fn recursive_stack_bound_accounts_for_different_frames_and_exits() {
        let resolved = crate::compile(
            "pure funcra top(n: integer) -> integer { return looped(n); }
             pure funcra looped(n: integer) -> integer {
                 return if n > 0 { top(n - 1) } else { leaf(n) };
             }
             pure funcra leaf(n: integer) -> integer { return n; }",
        )
        .unwrap();
        let widths = [24, 40, 200];
        let limit = BOUNDED_INTEGER_ENTRY_MAX_STACK_BYTES;
        for (cap, bytes) in [(1, 32), (2, 80), (3, 288), (4, 288), (5, 368)] {
            let graph = ScalarCallGraph::lower(&resolved, "top", 6, Some(cap)).unwrap();
            assert_eq!(graph.emission_order(), [0, 1, 2]);
            assert_eq!(
                graph.verify_stack(&widths, limit, 0).unwrap(),
                StackBound { bytes, frames: cap },
            );
            assert!(graph.verify_stack(&widths, bytes - 1, 0).is_err());
        }
        let graph = ScalarCallGraph::lower(&resolved, "top", 6, Some(5)).unwrap();
        assert_eq!(
            graph
                .verify_stack(&[16_376, 16_376, 0], limit, 0)
                .unwrap_err()
                .kind,
            DiagnosticKind::NativeUnsupported
        );
    }

    #[test]
    fn verifier_rejects_forged_callee_and_missing_argument() {
        let resolved = crate::compile(CHAIN).unwrap();
        let graph = ScalarCallGraph::lower(&resolved, "top", 6, None).unwrap();
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
