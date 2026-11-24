//! Stack-slot reuse for verified scalar expressions.
use super::{failure, Diagnostic, Operation, ScalarExpressionIr};
use crate::scalar_ir::MAX_IR_REGISTERS;
use std::collections::BTreeSet;

#[derive(Debug, PartialEq, Eq)]
pub(super) struct TemporaryLayout {
    slots: Vec<usize>,
    count: usize,
}

#[derive(Clone, Copy)]
struct Interval {
    first: usize,
    last: usize,
    register: usize,
}

#[derive(Clone, Copy)]
struct RegisterSet([u64; MAX_IR_REGISTERS.div_ceil(64)]);

impl RegisterSet {
    const EMPTY: Self = Self([0; MAX_IR_REGISTERS.div_ceil(64)]);

    fn insert(&mut self, register: usize) {
        self.0[register / 64] |= 1 << (register % 64);
    }

    fn remove(&mut self, register: usize) {
        self.0[register / 64] &= !(1 << (register % 64));
    }

    fn union(&mut self, other: Self) {
        for (word, other_word) in self.0.iter_mut().zip(other.0) {
            *word |= other_word;
        }
    }

    fn members(self) -> impl Iterator<Item = usize> {
        self.0
            .into_iter()
            .enumerate()
            .flat_map(|(index, mut word)| {
                std::iter::from_fn(move || {
                    if word == 0 {
                        return None;
                    }
                    let bit = word.trailing_zeros() as usize;
                    word &= word - 1;
                    Some(index * 64 + bit)
                })
            })
    }

    fn first_free(self) -> Option<usize> {
        self.0.iter().enumerate().find_map(|(index, word)| {
            (*word != u64::MAX).then_some(index * 64 + word.trailing_ones() as usize)
        })
    }
}

fn accesses(operation: &Operation) -> (Option<usize>, [Option<usize>; 2]) {
    match *operation {
        Operation::Work => (None, [None, None]),
        Operation::Integer { dst, .. }
        | Operation::Boolean { dst, .. }
        | Operation::Load { dst, .. } => (Some(dst), [None, None]),
        Operation::Move { dst, src } | Operation::Unary { dst, src, .. } => {
            (Some(dst), [Some(src), None])
        }
        Operation::Binary {
            dst, left, right, ..
        } => (Some(dst), [Some(left), Some(right)]),
        Operation::Branch { condition, .. } => (None, [Some(condition), None]),
    }
}

fn register_sets(len: usize) -> Result<Vec<RegisterSet>, Diagnostic> {
    let mut sets = Vec::new();
    sets.try_reserve_exact(len).map_err(|_| failure())?;
    sets.resize(len, RegisterSet::EMPTY);
    Ok(sets)
}

impl TemporaryLayout {
    pub(super) fn plan(expression: &ScalarExpressionIr) -> Result<Self, Diagnostic> {
        expression.verify()?;
        if expression
            .steps
            .iter()
            .any(|step| matches!(step.kind, Operation::Branch { .. }))
        {
            return Self::plan_branches(expression);
        }

        let mut intervals = vec![None; expression.registers.len()];
        for (pc, step) in expression.steps.iter().enumerate() {
            let mut touch = |register: usize| -> Result<(), Diagnostic> {
                let interval = intervals.get_mut(register).ok_or_else(failure)?;
                match interval {
                    Some(Interval { last, .. }) => *last = pc,
                    None => {
                        *interval = Some(Interval {
                            first: pc,
                            last: pc,
                            register,
                        });
                    }
                }
                Ok(())
            };
            let (write, reads) = accesses(&step.kind);
            for register in reads.into_iter().flatten().chain(write) {
                touch(register)?;
            }
        }
        // The emitter reloads the root after the last IR operation.
        intervals
            .get_mut(expression.root)
            .and_then(Option::as_mut)
            .ok_or_else(failure)?
            .last = expression.steps.len();

        let mut ordered = intervals.into_iter().flatten().collect::<Vec<_>>();
        ordered.sort_unstable_by_key(|interval| (interval.first, interval.register));
        let mut slots = vec![usize::MAX; expression.registers.len()];
        // Active intervals ordered by their last use, then physical slot.
        let mut active = BTreeSet::<(usize, usize)>::new();
        let mut available = BTreeSet::<usize>::new();
        let mut count = 0usize;
        for interval in ordered {
            while let Some(&(last, slot)) = active.first() {
                // Keep both source and destination live at the same IR
                // instruction: allocation never assumes an operand order.
                if last >= interval.first {
                    break;
                }
                active.remove(&(last, slot));
                available.insert(slot);
            }
            let slot = if let Some(slot) = available.pop_first() {
                slot
            } else {
                let slot = count;
                count = count.checked_add(1).ok_or_else(failure)?;
                slot
            };
            slots[interval.register] = slot;
            active.insert((interval.last, slot));
        }
        Ok(Self { slots, count })
    }

    fn plan_branches(expression: &ScalarExpressionIr) -> Result<Self, Diagnostic> {
        let registers = expression.registers.len();
        let mut live = register_sets(expression.steps.len() + 1)?;
        let mut conflicts = register_sets(registers)?;
        let mut written = RegisterSet::EMPTY;
        // Include the implicit read performed when the emitter loads the result.
        live[expression.steps.len()].insert(expression.root);
        for (pc, step) in expression.steps.iter().enumerate().rev() {
            let mut outgoing = live[pc + 1];
            if let Operation::Branch { target, .. } = step.kind {
                // Verification guarantees target > pc, including the exit.
                outgoing.union(live[target]);
            }

            let (write, reads) = accesses(&step.kind);
            if let Some(dst) = write {
                written.insert(dst);
                outgoing.remove(dst);
                // A store cannot overwrite anything still live after it.
                // Keep same-instruction operands separate even at last use.
                let mut neighbors = outgoing;
                for src in reads.into_iter().flatten() {
                    if src != dst {
                        neighbors.insert(src);
                    }
                }
                conflicts[dst].union(neighbors);
                for neighbor in neighbors.members() {
                    conflicts[neighbor].insert(dst);
                }
            }
            if let [Some(left), Some(right)] = reads {
                if left != right {
                    conflicts[left].insert(right);
                    conflicts[right].insert(left);
                }
            }
            for src in reads.into_iter().flatten() {
                outgoing.insert(src);
            }
            live[pc] = outgoing;
        }

        let mut slots = vec![usize::MAX; registers];
        let mut count = 0;
        // Color deterministically by register number using the lowest
        // available slot. Repeated definitions of one register share a slot.
        for register in written.members() {
            let mut occupied = RegisterSet::EMPTY;
            for neighbor in conflicts[register].members() {
                let assigned = slots[neighbor];
                if assigned != usize::MAX {
                    occupied.insert(assigned);
                }
            }
            let slot = occupied.first_free().ok_or_else(failure)?;
            slots[register] = slot;
            count = count.max(slot + 1);
        }
        Ok(Self { slots, count })
    }

    pub(super) fn count(&self) -> usize {
        self.count
    }

    pub(super) fn slot(&self, register: usize) -> Result<usize, Diagnostic> {
        self.slots
            .get(register)
            .copied()
            .filter(|slot| *slot < self.count)
            .ok_or_else(failure)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        diagnostics::Span,
        scalar_ir::{
            program::{ProgramOp, ScalarProgramIr},
            Instruction,
        },
        value::ValueType,
    };

    fn expression(source: &str) -> ScalarExpressionIr {
        let program = crate::compile(source).unwrap();
        let ir = ScalarProgramIr::lower_integer_function(&program, 16).unwrap();
        ir.operations()
            .iter()
            .find_map(|step| match &step.kind {
                ProgramOp::Return { value } => Some(value.clone()),
                _ => None,
            })
            .unwrap()
    }

    #[test]
    fn long_arithmetic_chain_reuses_three_slots() {
        let source = format!(
            "pure funcra f(x: integer) -> integer {{ return x{}; }}",
            " + 1".repeat(200)
        );
        let expression = expression(&source);
        let layout = TemporaryLayout::plan(&expression).unwrap();
        assert_eq!(layout.count(), 3);
        assert!(expression.registers.len() > 400);
        assert_eq!(layout, TemporaryLayout::plan(&expression).unwrap());
        assert!(layout.slot(expression.root).is_ok());
        assert!(layout.slot(usize::MAX).is_err());
    }

    #[test]
    fn branch_expressions_reuse_slots_across_short_circuit_paths() {
        let program = crate::compile(
            "pure funcra f(x: integer) -> integer {
                if x > 0 and x < 10 { return 1; }
                return 0;
            }",
        )
        .unwrap();
        let ir = ScalarProgramIr::lower_integer_function(&program, 16).unwrap();
        let expression = ir
            .operations()
            .iter()
            .find_map(|step| match &step.kind {
                ProgramOp::JumpIfFalse { condition, .. } => Some(condition),
                _ => None,
            })
            .unwrap();
        let layout = TemporaryLayout::plan(expression).unwrap();
        assert_eq!(layout.count(), 3);
        assert_eq!(layout, TemporaryLayout::plan(expression).unwrap());
    }

    #[test]
    fn distinct_binary_operands_and_destination_do_not_alias() {
        let expression =
            expression("pure funcra f(x: integer) -> integer { return (x + 2) * (x - 3); }");
        let layout = TemporaryLayout::plan(&expression).unwrap();
        for step in expression.steps {
            if let Operation::Binary {
                dst, left, right, ..
            } = step.kind
            {
                assert_ne!(layout.slot(dst).unwrap(), layout.slot(left).unwrap());
                assert_ne!(layout.slot(dst).unwrap(), layout.slot(right).unwrap());
                assert_ne!(layout.slot(left).unwrap(), layout.slot(right).unwrap());
            }
        }
    }

    #[test]
    fn allocation_distinguishes_a_value_live_on_both_edges_from_a_killed_one() {
        let mut ir = ScalarExpressionIr {
            registers: vec![ValueType::Boolean; 5],
            slots: Vec::new(),
            root: 0,
            steps: [
                Operation::Boolean {
                    dst: 0,
                    value: true,
                },
                Operation::Boolean {
                    dst: 1,
                    value: true,
                },
                Operation::Branch {
                    condition: 1,
                    when_true: true,
                    target: 6,
                },
                Operation::Boolean {
                    dst: 2,
                    value: false,
                },
                Operation::Boolean {
                    dst: 3,
                    value: false,
                },
                Operation::Move { dst: 0, src: 3 },
            ]
            .into_iter()
            .map(|kind| Instruction {
                kind,
                span: Span::default(),
            })
            .collect(),
        };
        let layout = TemporaryLayout::plan(&ir).unwrap();
        // The original root is read on the taken edge. On the fallthrough
        // it is overwritten before the final read, so this dead store may
        // borrow its slot.
        assert_eq!(layout.slot(0).unwrap(), layout.slot(2).unwrap());
        assert_ne!(layout.slot(0).unwrap(), layout.slot(3).unwrap());

        ir.steps[5].kind = Operation::Move { dst: 4, src: 3 };
        let layout = TemporaryLayout::plan(&ir).unwrap();
        // With no redefinition, even the dead writes must preserve the root.
        assert_ne!(layout.slot(0).unwrap(), layout.slot(2).unwrap());
        assert_ne!(layout.slot(0).unwrap(), layout.slot(4).unwrap());

        let Operation::Branch { target, .. } = &mut ir.steps[2].kind else {
            unreachable!()
        };
        *target = usize::MAX;
        assert!(TemporaryLayout::plan(&ir).is_err());
    }
}
