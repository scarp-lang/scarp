//! Definite scalar-slot initialization across all reachable statement edges.
//!
//! The type/shape verifier checks every operation first. This analysis adds
//! a bounded meet-over-paths check: a load is valid only when every possible
//! predecessor has initialized the slot since its most recent retirement.
//! It never executes user expressions or assumes a branch's runtime value.

use super::{ProgramOp, ScalarProgramIr};
use crate::{
    diagnostics::{Diagnostic, Span},
    scalar_ir::{invalid, Operation, ScalarExpressionIr},
};
use std::collections::VecDeque;

/// At most 256 scalar slots are permitted by the independent shape gate.
/// Four machine words bound memory and avoid source-controlled allocations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SlotSet([u64; 4]);

impl SlotSet {
    fn empty() -> Self {
        Self([0; 4])
    }

    fn contains(self, slot: usize) -> bool {
        self.0
            .get(slot / 64)
            .is_some_and(|word| word & (1_u64 << (slot % 64)) != 0)
    }

    fn insert(&mut self, slot: usize) {
        if let Some(word) = self.0.get_mut(slot / 64) {
            *word |= 1_u64 << (slot % 64);
        }
    }

    fn remove(&mut self, slot: usize) {
        if let Some(word) = self.0.get_mut(slot / 64) {
            *word &= !(1_u64 << (slot % 64));
        }
    }

    fn intersect(self, other: Self) -> Self {
        let mut result = self;
        for (word, next) in result.0.iter_mut().zip(other.0) {
            *word &= next;
        }
        result
    }
}

fn join(
    target: usize,
    incoming: SlotSet,
    states: &mut [Option<SlotSet>],
    queued: &mut [bool],
    work: &mut VecDeque<usize>,
) {
    let combined = match states[target] {
        Some(known) => known.intersect(incoming),
        None => incoming,
    };
    if states[target] != Some(combined) {
        states[target] = Some(combined);
        if !queued[target] {
            work.push_back(target);
            queued[target] = true;
        }
    }
}

fn needs_slot(known: SlotSet, slot: usize, span: Span) -> Result<(), Diagnostic> {
    if known.contains(slot) {
        Ok(())
    } else {
        Err(invalid(span))
    }
}

fn needs_expression(known: SlotSet, expression: &ScalarExpressionIr) -> Result<(), Diagnostic> {
    for instruction in &expression.steps {
        if let Operation::Load { slot, .. } = instruction.kind {
            needs_slot(known, slot, instruction.span)?;
        }
    }
    Ok(())
}

impl ScalarProgramIr {
    /// Verify initializations without evaluating any branch or loop. A
    /// conditional's two successors are both possible; loops reach a fixed
    /// point by intersecting guaranteed slots on every incoming edge.
    ///
    /// A slot can become initialized only after Store/Update/RepeatInit,
    /// and becomes unavailable at Retire/LoopControl retirement. All reads
    /// use the *incoming* state, before the current operation's writes.
    pub(super) fn verify_definite_slots(&self) -> Result<(), Diagnostic> {
        let len = self.steps.len();
        let mut states = vec![None; len + 1];
        let mut queued = vec![false; len + 1];
        let mut pending = VecDeque::new();
        states[0] = Some(SlotSet::empty());
        queued[0] = true;
        pending.push_back(0);

        while let Some(pc) = pending.pop_front() {
            queued[pc] = false;
            if pc == len {
                continue;
            }
            let mut next = states[pc].ok_or_else(|| invalid(self.steps[pc].span))?;
            let branch = match &self.steps[pc].kind {
                ProgramOp::Store { slot, .. } | ProgramOp::Update { slot, .. } => {
                    next.insert(*slot);
                    None
                }
                ProgramOp::RepeatInit {
                    count_slot,
                    index_slot,
                    ..
                } => {
                    next.insert(*count_slot);
                    next.insert(*index_slot);
                    None
                }
                ProgramOp::RepeatNext { index_slot } => {
                    next.insert(*index_slot);
                    None
                }
                ProgramOp::Retire { slots } => {
                    for slot in slots {
                        next.remove(*slot);
                    }
                    None
                }
                ProgramOp::LoopControl { target, retire } => {
                    for slot in retire {
                        next.remove(*slot);
                    }
                    join(*target, next, &mut states, &mut queued, &mut pending);
                    continue;
                }
                ProgramOp::Jump { target } => {
                    join(*target, next, &mut states, &mut queued, &mut pending);
                    continue;
                }
                ProgramOp::JumpIfFalse { target, .. } | ProgramOp::RepeatGuard { target, .. } => {
                    Some(*target)
                }
                _ => None,
            };
            if let Some(target) = branch {
                join(target, next, &mut states, &mut queued, &mut pending);
            }
            join(pc + 1, next, &mut states, &mut queued, &mut pending);
        }

        // Reads are validated after the fixed point, not while states can
        // still shrink due to a backward edge. Unreachable instructions have
        // already passed the independent structural/type verifier.
        for (pc, instruction) in self.steps.iter().enumerate() {
            let Some(known) = states[pc] else {
                continue;
            };
            let span = instruction.span;
            match &instruction.kind {
                ProgramOp::Store { value, .. }
                | ProgramOp::RepeatInit { count: value, .. }
                | ProgramOp::PrintValue { value } => needs_expression(known, value)?,
                ProgramOp::Update { slot, value, .. } => {
                    needs_slot(known, *slot, span)?;
                    needs_expression(known, value)?;
                }
                ProgramOp::RepeatGuard {
                    count_slot,
                    index_slot,
                    ..
                } => {
                    needs_slot(known, *count_slot, span)?;
                    needs_slot(known, *index_slot, span)?;
                }
                ProgramOp::RepeatNext { index_slot } => {
                    needs_slot(known, *index_slot, span)?;
                }
                ProgramOp::JumpIfFalse { condition, .. } => {
                    needs_expression(known, condition)?;
                }
                ProgramOp::Jump { .. }
                | ProgramOp::LoopControl { .. }
                | ProgramOp::Retire { .. }
                | ProgramOp::PrintLiteral { .. } => {}
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{compile, diagnostics::DiagnosticKind, native_preflight};

    fn verified(source: &str) -> ScalarProgramIr {
        let program = compile(source).unwrap();
        native_preflight::inspect(&program).unwrap();
        ScalarProgramIr::lower(&program).unwrap()
    }

    fn assert_rejected(ir: &ScalarProgramIr) {
        assert_eq!(
            ir.verify().unwrap_err().kind,
            DiagnosticKind::NativeUnsupported,
        );
    }

    #[test]
    fn mask_handles_all_four_word_boundaries() {
        let mut known = SlotSet::empty();
        for slot in [0, 63, 64, 127, 128, 191, 192, 255] {
            known.insert(slot);
            assert!(known.contains(slot));
        }
        assert!(!known.contains(256));
        known.remove(64);
        known.remove(255);
        assert!(!known.contains(64));
        assert!(!known.contains(255));
        assert!(known.contains(63));
        assert!(known.contains(192));
        let only_63 = SlotSet([1_u64 << 63, 0, 0, 0]);
        assert_eq!(known.intersect(only_63), only_63);
    }

    #[test]
    fn valid_nested_control_flow_preserves_lexical_bindings() {
        for source in [
            "",
            "print(42);",
            "var n=0; while n<4 { n+=1; if n==2 { continue; } print(n); }",
            "var total=0; repeat 5 as i { total+=i; } print(total);",
            "repeat 3 as i { repeat i as j { print(i+j); } }",
            "var x=1; if true { x=2; } else { x=3; } print(x);",
            "repeat 3 as i { if i==1 { break; } print(i); }",
            "var x=1; { bind y=x; print(y); } print(x);",
            "var x=0; while x<2 { { bind y=x; print(y); } x+=1; }",
            "if false { bind skipped=1; print(skipped); } print(9);",
        ] {
            verified(source).verify().unwrap();
        }
    }

    #[test]
    fn skip_initial_store_is_rejected_at_later_read() {
        let mut ir = verified("var x=7; print(x);");
        if let Some(first) = ir.steps.first_mut() {
            first.kind = ProgramOp::Jump { target: 1 };
        }
        assert_rejected(&ir);
    }

    #[test]
    fn uninitialized_branch_join_is_rejected() {
        let mut ir = verified("var x=0; if true { x=1; } print(x);");
        if let Some(first) = ir.steps.first_mut() {
            first.kind = ProgramOp::Jump { target: 1 };
        }
        // One branch writes x, the other does not.
        assert_rejected(&ir);
    }

    #[test]
    fn retirement_cannot_clear_a_later_read_binding() {
        let mut ir = verified("var x=1; { bind y=x; print(y); } print(x);");
        let slot = ir
            .steps
            .iter()
            .find_map(|step| match step.kind {
                ProgramOp::Store { slot, .. } => Some(slot),
                _ => None,
            })
            .unwrap();
        let retired = ir
            .steps
            .iter_mut()
            .find_map(|step| match &mut step.kind {
                ProgramOp::Retire { slots } => Some(slots),
                _ => None,
            })
            .unwrap();
        retired.push(slot);
        assert_rejected(&ir);
    }

    #[test]
    fn repeat_guard_requires_prior_initialization() {
        let mut ir = verified("repeat 2 as i { print(i); }");
        let initialization = ir
            .steps
            .iter_mut()
            .find(|step| matches!(step.kind, ProgramOp::RepeatInit { .. }))
            .unwrap();
        initialization.kind = ProgramOp::Jump { target: 1 };
        assert_rejected(&ir);
    }

    #[test]
    fn update_requires_initialized_lhs_even_with_valid_rhs() {
        let mut ir = verified("var x=1; x+=1;");
        ir.steps[0].kind = ProgramOp::Jump { target: 1 };
        assert_rejected(&ir);
    }
}
