//! Test-only scalar IR execution oracle.
//!
//! Runs *lowered instructions* rather than the resolver or interpreter
//! state machine. Checked arithmetic and work-budget helpers are shared
//! intentionally so differential tests isolate control flow, metering,
//! output, source spans and storage semantics, not duplicate numeric rules.

use super::*;
use crate::{
    diagnostics::{DiagnosticKind, Span},
    evaluator,
    limits::{Budget, ExecutionOptions},
    value::Value,
};

fn register(values: &[Value], index: usize, span: Span) -> Result<Value, Diagnostic> {
    values.get(index).cloned().ok_or_else(|| invalid(span))
}

impl ScalarExpressionIr {
    fn execute(&self, storage: &[Value], budget: &mut Budget) -> Result<Value, Diagnostic> {
        self.verify()?;
        let mut values = vec![Value::Unit; self.registers.len()];
        let mut pc = 0;
        while pc < self.steps.len() {
            let instruction = &self.steps[pc];
            pc += 1;
            let span = instruction.span;
            match &instruction.kind {
                super::super::Operation::Work => budget.charge(1, span)?,
                super::super::Operation::Integer { dst, value } => {
                    values[*dst] = Value::Integer(*value);
                }
                super::super::Operation::Boolean { dst, value } => {
                    values[*dst] = Value::Boolean(*value);
                }
                super::super::Operation::Load { dst, slot } => {
                    values[*dst] = storage.get(*slot).cloned().ok_or_else(|| invalid(span))?;
                }
                super::super::Operation::Move { dst, src } => {
                    values[*dst] = register(&values, *src, span)?;
                }
                super::super::Operation::Unary { dst, src, op } => {
                    let input = register(&values, *src, span)?;
                    values[*dst] = match op {
                        super::super::Unary::Negate => {
                            let value = input.require_integer(span)?;
                            Value::Integer(value.checked_neg().ok_or_else(|| {
                                Diagnostic::new(
                                    DiagnosticKind::ArithmeticOverflow,
                                    span,
                                    "integer arithmetic overflow",
                                )
                                .with_help("each result must fit in a signed 64-bit integer")
                            })?)
                        }
                        super::super::Unary::BitNot => {
                            Value::Integer(!input.require_integer(span)?)
                        }
                        super::super::Unary::Not => Value::Boolean(!input.require_boolean(span)?),
                    };
                }
                super::super::Operation::Binary {
                    dst,
                    left,
                    right,
                    op,
                } => {
                    let left = register(&values, *left, span)?;
                    let right = register(&values, *right, span)?;
                    values[*dst] = evaluator::apply_binary(*op, &left, &right, span)?;
                }
                super::super::Operation::Branch {
                    condition,
                    when_true,
                    target,
                } => {
                    if register(&values, *condition, span)?.require_boolean(span)? == *when_true {
                        pc = *target;
                    }
                }
            }
        }
        register(&values, self.root, Span::default())
    }
}

impl ScalarProgramIr {
    /// Evaluates the private IR into one transactionally committed UTF-8
    /// stdout buffer, mirroring interpreter validation-before-write behavior.
    fn execute(&self, options: ExecutionOptions) -> Result<Vec<u8>, Diagnostic> {
        self.verify()?;
        if self.slots.len() > options.max_live_slots {
            return Err(Diagnostic::new(
                DiagnosticKind::CallLimit,
                Span::default(),
                format!("live binding slot limit of {} exceeded", options.max_live_slots),
            )
            .with_help(
                "check recursion and frame size, or raise --max-live-slots / ExecutionOptions.max_live_slots",
            ));
        }
        let mut budget = Budget::new(options.limits.max_steps);
        let mut storage = vec![Value::Unit; self.slots.len()];
        let mut output = Vec::new();
        let mut captured = 0usize;
        let mut prints = 0usize;
        let mut pc = 0usize;

        while pc < self.steps.len() {
            let instruction = &self.steps[pc];
            let span = instruction.span;
            if instruction.charge_work {
                budget.charge(1, span)?;
            }
            pc += 1;
            match &instruction.kind {
                ProgramOp::Store { slot, value } => {
                    storage[*slot] = value.execute(&storage, &mut budget)?;
                }
                ProgramOp::Update {
                    slot,
                    operator,
                    operator_span,
                    value,
                } => {
                    let previous = storage[*slot].clone();
                    let next = value.execute(&storage, &mut budget)?;
                    storage[*slot] =
                        evaluator::apply_binary(*operator, &previous, &next, *operator_span)?;
                }
                ProgramOp::RepeatInit {
                    count,
                    count_slot,
                    index_slot,
                } => {
                    let count = count
                        .execute(&storage, &mut budget)?
                        .require_integer(span)?;
                    if count < 0 {
                        return Err(Diagnostic::new(
                            DiagnosticKind::RepeatCount,
                            span,
                            "repeat count must be nonnegative",
                        )
                        .with_help("use zero to skip the loop or a positive integer count"));
                    }
                    storage[*count_slot] = Value::Integer(count);
                    storage[*index_slot] = Value::Integer(0);
                }
                ProgramOp::RepeatGuard {
                    count_slot,
                    index_slot,
                    target,
                } => {
                    let index = storage[*index_slot].require_integer(span)?;
                    let count = storage[*count_slot].require_integer(span)?;
                    if index >= count {
                        pc = *target;
                    }
                }
                ProgramOp::RepeatNext { index_slot } => {
                    let index = storage[*index_slot].require_integer(span)?;
                    let next = index.checked_add(1).ok_or_else(|| {
                        Diagnostic::new(
                            DiagnosticKind::ArithmeticOverflow,
                            span,
                            "repeat index overflow",
                        )
                    })?;
                    storage[*index_slot] = Value::Integer(next);
                }
                ProgramOp::JumpIfFalse { condition, target } => {
                    if !condition
                        .execute(&storage, &mut budget)?
                        .require_boolean(span)?
                    {
                        pc = *target;
                    }
                }
                ProgramOp::Jump { target } => pc = *target,
                ProgramOp::LoopControl { target, retire } => {
                    for &slot in retire {
                        storage[slot] = Value::Unit;
                    }
                    pc = *target;
                }
                ProgramOp::Retire { slots } => {
                    for &slot in slots {
                        storage[slot] = Value::Unit;
                    }
                }
                ProgramOp::PrintValue { value } => {
                    if prints >= options.limits.max_prints {
                        return Err(print_limit(span, options.limits.max_prints));
                    }
                    prints += 1;
                    let value = value.execute(&storage, &mut budget)?;
                    charge_snapshot(&mut captured, 8, options, span)?;
                    output.extend_from_slice(value.to_string().as_bytes());
                    output.push(b'\n');
                }
                ProgramOp::PrintLiteral { text } => {
                    if prints >= options.limits.max_prints {
                        return Err(print_limit(span, options.limits.max_prints));
                    }
                    prints += 1;
                    charge_snapshot(&mut captured, text.len(), options, span)?;
                    output.extend_from_slice(text);
                    output.push(b'\n');
                }
            }
        }
        Ok(output)
    }
}

fn print_limit(span: Span, count: usize) -> Diagnostic {
    Diagnostic::new(
        DiagnosticKind::OutputLimit,
        span,
        format!("print count limit of {count} exceeded"),
    )
    .with_help("reduce output calls or raise --max-prints / ExecutionLimits.max_prints")
}

fn charge_snapshot(
    used: &mut usize,
    payload: usize,
    options: ExecutionOptions,
    span: Span,
) -> Result<(), Diagnostic> {
    // Mirror the interpreter's logical snapshot accounting, not the actual
    // vector allocation of this test oracle.
    let amount = payload.saturating_add(64);
    if amount > options.max_capture_bytes.saturating_sub(*used) {
        return Err(Diagnostic::new(
            DiagnosticKind::CaptureLimit,
            span,
            format!(
                "output snapshot storage limit of {} exceeded",
                options.max_capture_bytes
            ),
        )
        .with_help(
            "reduce selected output or raise --max-capture-bytes / ExecutionOptions.max_capture_bytes",
        ));
    }
    *used += amount;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{compile, interpreter, limits::ExecutionLimits, native_preflight, RunError};

    fn outcomes(
        source: &str,
        options: ExecutionOptions,
    ) -> (Result<Vec<u8>, Diagnostic>, Result<Vec<u8>, Diagnostic>) {
        let program = compile(source).unwrap();
        native_preflight::inspect(&program).unwrap();
        let ir = ScalarProgramIr::lower(&program).unwrap();

        let mut interpreted = Vec::new();
        let expected = interpreter::execute_with_options(&program, &mut interpreted, options)
            .map(|_| interpreted)
            .map_err(|error| match error {
                RunError::Diagnostic(error) => error,
                RunError::Output(error) => panic!("unexpected writer failure: {error}"),
            });
        let actual = ir.execute(options);
        (expected, actual)
    }

    fn same(source: &str, options: ExecutionOptions) {
        let (expected, actual) = outcomes(source, options);
        match (expected, actual) {
            (Ok(expected), Ok(actual)) => assert_eq!(
                actual, expected,
                "IR output differs from interpreter for {source:?}"
            ),
            (Err(expected), Err(actual)) => {
                assert_eq!(actual.kind, expected.kind, "kind for {source:?}");
                assert_eq!(actual.span, expected.span, "span for {source:?}");
                assert_eq!(actual.message, expected.message, "message for {source:?}");
            }
            (expected, actual) => panic!(
                "IR/interpreter success or failure differs for {source:?}: expected {expected:?}, got {actual:?}"
            ),
        }
    }

    #[test]
    fn differential_success_including_repeated_work_and_short_circuit() {
        for source in [
            "",
            "print(1 + 2 * 3); print(false);",
            "print(true == false); print(not false); print(~0);",
            "print(false and (1 / 0 == 1)); print(true or (1 / 0 == 1));",
            "print(true and (false or true)); print((4 + 3) == 7);",
            "var n=0; while n<4 { print(n); n+=1; }",
            "var n=0; while n<4 { n+=1; if n==2 { continue; } print(n); }",
            "repeat 3 as i { if i == 1 { continue; } print(i); }",
            "repeat 4 as i { if i == 2 { break; } print(i); }",
            "var total=0; repeat 5 as i { total+=i; } print(total);",
            "if false { print(1/0); } else { print(\"मिलान\"); }",
            "print(9223372036854775807); print(1 << 63);",
            "print(-1 >>> 1); print(8 & 3); print(6 ^ 2);",
            "{ bind inner=true; if inner { print(\"नमस्ते 🌍\"); } }",
            "print(\"\"); print(\"é\tदेवनागरी\");",
        ] {
            same(source, ExecutionOptions::default());
        }
    }

    #[test]
    fn differential_execution_and_print_budgets_at_boundaries() {
        let sources = [
            "print(1);",
            "print(false and (1/0==1));",
            "print(true and false);",
            "var x=0; while x<2 { x+=1; } print(x);",
            "repeat 2 as i { print(i); }",
            "repeat 0 as i { print(i); }",
            "print(\"नमस्ते\"); print(true);",
        ];
        for source in sources {
            for steps in 0..=60u64 {
                let mut options = ExecutionOptions::default();
                options.limits.max_steps = steps;
                same(source, options);
            }
            for prints in 0..=3 {
                let mut options = ExecutionOptions::default();
                options.limits.max_prints = prints;
                same(source, options);
            }
        }
    }

    #[test]
    fn differential_runtime_error_diagnostics_and_source_byte_spans() {
        for source in [
            "print(9223372036854775807 + 1);",
            "print(1 / 0);",
            "print(1 % 0);",
            "print(1 << 64);",
            "print(1 >> -1);",
            "print(-(0 - 9223372036854775807 - 1));",
            "var n=9223372036854775807; n+=1;",
            "repeat -1 as n { print(n); }",
            "print(\"हिन्दी\"); print(1 / 0);",
            "if true { print(1/0); } else { print(3); }",
        ] {
            same(source, ExecutionOptions::default());
        }
    }

    #[test]
    fn differential_capture_and_slot_budget_boundaries() {
        for source in [
            "print(1);",
            "print(\"\");",
            "print(\"नमस्ते\");",
            "print(true); print(false);",
            "var x=1; print(x);",
        ] {
            for bytes in [0, 63, 64, 65, 71, 72, 73, 100, 144, 1024] {
                let mut options = ExecutionOptions::default();
                options.max_capture_bytes = bytes;
                same(source, options);
            }
            for slots in [0, 1, 2, 256] {
                let mut options = ExecutionOptions::default();
                options.max_live_slots = slots;
                same(source, options);
            }
        }
    }

    #[test]
    fn verified_ir_rejects_mutation_before_any_execution() {
        let resolved = compile("print(false and (1/0==1));").unwrap();
        let mut ir = ScalarProgramIr::lower(&resolved).unwrap();
        if let ProgramOp::PrintValue { value } = &mut ir.steps[0].kind {
            for instruction in &mut value.steps {
                if let super::super::super::Operation::Branch { target, .. } = &mut instruction.kind
                {
                    *target = 0;
                    break;
                }
            }
        }
        assert_eq!(
            ir.execute(ExecutionOptions::default()).unwrap_err().kind,
            DiagnosticKind::NativeUnsupported
        );
    }

    #[test]
    fn interpreter_and_ir_keep_selected_output_transactional() {
        let source = "print(1); print(1/0);";
        let (expected, actual) = outcomes(source, ExecutionOptions::default());
        assert_eq!(actual.unwrap_err().span, expected.unwrap_err().span);
    }

    #[test]
    fn exact_print_count_is_enforced_for_repeat_runs() {
        let mut options = ExecutionOptions::default();
        options.limits = ExecutionLimits {
            max_steps: 1_000_000,
            max_prints: 3,
        };
        same("repeat 3 as i { print(i); }", options);
        options.limits.max_prints = 2;
        same("repeat 3 as i { print(i); }", options);
    }
}
