//! Conservative stack-slot reuse for verified straight-line scalar expressions.
//! Logical short-circuit expressions retain the original disjoint layout.
use super::{failure, Diagnostic, Operation, ScalarExpressionIr};
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

impl TemporaryLayout {
    pub(super) fn identity(count: usize) -> Self {
        Self {
            slots: (0..count).collect(),
            count,
        }
    }

    pub(super) fn plan(expression: &ScalarExpressionIr) -> Result<Self, Diagnostic> {
        expression.verify()?;
        if expression
            .steps
            .iter()
            .any(|step| matches!(step.kind, Operation::Branch { .. }))
        {
            // Until CFG-sensitive allocation is implemented, preserve the
            // exact layout on all conditional expression paths.
            return Ok(Self::identity(expression.registers.len()));
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
            match step.kind {
                Operation::Work => {}
                Operation::Integer { dst, .. }
                | Operation::Boolean { dst, .. }
                | Operation::Load { dst, .. } => touch(dst)?,
                Operation::Move { dst, src } | Operation::Unary { dst, src, .. } => {
                    touch(src)?;
                    touch(dst)?;
                }
                Operation::Binary {
                    dst, left, right, ..
                } => {
                    touch(left)?;
                    touch(right)?;
                    touch(dst)?;
                }
                Operation::Branch { .. } => return Err(failure()),
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
    use crate::scalar_ir::program::{ProgramOp, ScalarProgramIr};

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
    fn branch_expressions_preserve_original_slot_identity() {
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
        assert_eq!(
            TemporaryLayout::plan(expression).unwrap(),
            TemporaryLayout::identity(expression.registers.len())
        );
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
}
