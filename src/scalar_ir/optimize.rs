//! Constant propagation and dead definitions in verified native expressions.

use super::{invalid, Diagnostic, Operation, ScalarExpressionIr, Span, Unary};
use crate::{evaluator, value::Value};
use std::borrow::Cow;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Constant {
    Integer(i64),
    Boolean(bool),
}

impl Constant {
    fn value(self) -> Value {
        match self {
            Self::Integer(value) => Value::Integer(value),
            Self::Boolean(value) => Value::Boolean(value),
        }
    }

    fn definition(self, dst: usize) -> Operation {
        match self {
            Self::Integer(value) => Operation::Integer { dst, value },
            Self::Boolean(value) => Operation::Boolean { dst, value },
        }
    }
}

fn constant_result(
    operation: &Operation,
    known: &[Option<Constant>],
    span: Span,
) -> Option<Constant> {
    match *operation {
        Operation::Integer { value, .. } => Some(Constant::Integer(value)),
        Operation::Boolean { value, .. } => Some(Constant::Boolean(value)),
        Operation::Move { src, .. } => known[src],
        Operation::Unary { src, op, .. } => match (op, known[src]?) {
            (Unary::Negate, Constant::Integer(value)) => value.checked_neg().map(Constant::Integer),
            (Unary::BitNot, Constant::Integer(value)) => Some(Constant::Integer(!value)),
            (Unary::Not, Constant::Boolean(value)) => Some(Constant::Boolean(!value)),
            _ => None,
        },
        Operation::Binary {
            left, right, op, ..
        } => {
            // Leave a failing operation in place so the native status, span
            // and ordering relative to earlier work checks stay observable.
            match evaluator::apply_binary(op, &known[left]?.value(), &known[right]?.value(), span)
                .ok()?
            {
                Value::Integer(value) => Some(Constant::Integer(value)),
                Value::Boolean(value) => Some(Constant::Boolean(value)),
                _ => None,
            }
        }
        _ => None,
    }
}

fn literal(operation: &Operation) -> bool {
    matches!(
        operation,
        Operation::Integer { .. } | Operation::Boolean { .. }
    )
}

fn join(destination: &mut Option<Vec<Option<Constant>>>, incoming: Cow<'_, [Option<Constant>]>) {
    if let Some(existing) = destination {
        for (value, other) in existing.iter_mut().zip(incoming.iter()) {
            if value != other {
                *value = None;
            }
        }
    } else {
        *destination = Some(incoming.into_owned());
    }
}

impl ScalarExpressionIr {
    /// The original expression has passed preflight and IR verification.
    /// Keep both branch edges, even when the condition itself is constant.
    /// Statement slots and results of calls are treated as unknown.
    pub(super) fn optimize_verified(
        &self,
        call_arities: Option<&[usize]>,
    ) -> Result<Cow<'_, Self>, Diagnostic> {
        if self.steps.len() <= 2 || !self.steps.iter().any(|step| literal(&step.kind)) {
            return Ok(Cow::Borrowed(self));
        }

        let mut branch_inputs: Vec<Option<Vec<Option<Constant>>>> = Vec::new();
        let mut fallthrough = Some(vec![None; self.registers.len()]);
        let mut folded = Vec::new();
        for (pc, step) in self.steps.iter().enumerate() {
            // Every edge moves forward, so all incoming branch facts are
            // available here. Carry straight-line facts without snapshots.
            if let Some(incoming) = branch_inputs.get_mut(pc).and_then(Option::take) {
                join(&mut fallthrough, Cow::Owned(incoming));
            }
            let known = fallthrough.as_mut().ok_or_else(|| invalid(step.span))?;
            let (write, _, _) = step.kind.register_accesses();
            if let Some(dst) = write {
                let value = constant_result(&step.kind, known, step.span);
                if let Some(constant) = value.filter(|_| !literal(&step.kind)) {
                    folded.push((pc, constant.definition(dst)));
                }
                known[dst] = value;
            }
            let (next, alternative) = step.kind.successors(pc);
            let alternative = alternative.filter(|&target| target != next);
            if next == pc + 1 && alternative.is_none() {
                continue;
            }
            branch_inputs.resize_with(self.steps.len() + 1, || None);
            if let Some(target) = alternative {
                join(&mut branch_inputs[target], Cow::Borrowed(known));
            }
            if next != pc + 1 {
                let incoming = std::mem::take(known);
                fallthrough = None;
                join(&mut branch_inputs[next], Cow::Owned(incoming));
            }
        }
        if folded.is_empty() {
            return Ok(Cow::Borrowed(self));
        }

        let mut optimized = self.clone();
        for (pc, definition) in folded {
            optimized.steps[pc].kind = definition;
        }
        optimized.remove_unused_definitions();
        optimized.verify_with_calls(call_arities)?;
        Ok(Cow::Owned(optimized))
    }

    fn remove_unused_definitions(&mut self) {
        // Count uses across the entire expression, conservatively retaining
        // all definitions of a register if any can still be read.
        let mut uses = vec![0usize; self.registers.len()];
        uses[self.root] = 1;
        for step in &self.steps {
            let (_, reads, arguments) = step.kind.register_accesses();
            for source in reads.into_iter().flatten().chain(arguments.iter().copied()) {
                uses[source] += 1;
            }
        }

        let mut kept = vec![true; self.steps.len()];
        for (step, keep) in self.steps.iter().zip(&mut kept).rev() {
            let (write, reads, _) = step.kind.register_accesses();
            // Literals and moves have no runtime effects after verification.
            // Work checks, calls and possibly failing operators remain.
            if write.is_some_and(|dst| uses[dst] == 0)
                && (literal(&step.kind) || matches!(step.kind, Operation::Move { .. }))
            {
                *keep = false;
                for source in reads.into_iter().flatten() {
                    uses[source] -= 1;
                }
            }
        }

        // Map each original instruction boundary to its compacted boundary,
        // including jump labels on a removed definition or at the end.
        let mut offsets = Vec::with_capacity(kept.len() + 1);
        let mut offset = 0;
        for &keep in &kept {
            offsets.push(offset);
            offset += usize::from(keep);
        }
        offsets.push(offset);
        let mut pc = 0;
        self.steps.retain_mut(|step| {
            if let Operation::Branch { target, .. } | Operation::Jump { target } = &mut step.kind {
                *target = offsets[*target];
            }
            let keep = kept[pc];
            pc += 1;
            keep
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        ast::BinaryOperator,
        scalar_ir::program::{ProgramOp, ScalarProgramIr},
    };

    fn lower(expression: &str) -> ScalarExpressionIr {
        let program =
            crate::compile(&format!("// गणना\nbind x = 0; print({expression});")).unwrap();
        let ir = ScalarProgramIr::lower(&program).unwrap();
        ir.operations()
            .iter()
            .find_map(|step| match &step.kind {
                ProgramOp::PrintValue { value } => Some(value.clone()),
                _ => None,
            })
            .unwrap()
    }

    fn work_spans(ir: &ScalarExpressionIr) -> Vec<Span> {
        ir.steps
            .iter()
            .filter(|step| matches!(step.kind, Operation::Work))
            .map(|step| step.span)
            .collect()
    }

    #[test]
    fn folding_removes_dead_values_but_preserves_every_work_span() {
        let ir = lower("(6 * (7 + 2) - (~0)) >> (1 + 0)");
        let optimized = ir.optimize_verified(None).unwrap();
        let work = work_spans(&ir);
        assert_eq!(work_spans(&optimized), work);
        assert_eq!(optimized.steps.len(), work.len() + 1);
        assert_eq!(
            optimized.steps.last().unwrap().kind,
            Operation::Integer {
                dst: ir.root,
                value: 27
            }
        );
    }

    #[test]
    fn propagation_at_a_join_requires_matching_constants_on_all_edges() {
        for (alternative, expected) in [("1 + 5", Some(10)), ("1 + 6", None)] {
            let ir = lower(&format!(
                "(if x == 0 {{ 2 * 3 }} else {{ {alternative} }}) + 4"
            ));
            let optimized = ir.optimize_verified(None).unwrap();
            assert_eq!(work_spans(&optimized), work_spans(&ir));
            assert!(optimized
                .steps
                .iter()
                .any(|step| matches!(step.kind, Operation::Branch { .. })));
            match (expected, &optimized.steps.last().unwrap().kind) {
                (Some(expected), Operation::Integer { dst, value }) => {
                    assert_eq!((*dst, *value), (ir.root, expected));
                }
                (
                    None,
                    Operation::Binary {
                        dst,
                        op: BinaryOperator::Add,
                        ..
                    },
                ) => {
                    assert_eq!(*dst, ir.root);
                }
                (expected, operation) => panic!("expected {expected:?}, got {operation:?}"),
            }
        }
    }

    #[test]
    fn potentially_failing_operations_keep_their_original_instructions() {
        for (expression, operator) in [
            ("(1 / 0) + (2 * 3)", BinaryOperator::Divide),
            (
                "(-9223372036854775808 % -1) + (2 * 3)",
                BinaryOperator::Remainder,
            ),
            ("(9223372036854775807 + 1) * (2 * 3)", BinaryOperator::Add),
            ("(1 << 64) + (2 * 3)", BinaryOperator::ShiftLeft),
        ] {
            let ir = lower(expression);
            let optimized = ir.optimize_verified(None).unwrap();
            let original = ir
                .steps
                .iter()
                .find(|step| matches!(step.kind, Operation::Binary { op, .. } if op == operator))
                .unwrap();
            assert!(optimized.steps.contains(original), "{expression}");
            assert_eq!(work_spans(&optimized), work_spans(&ir));
        }
    }
}
