//! Experimental C11 scalar emission. Not a native compile CLI or a stable ABI.
//!
//! This is deliberately opt-in through the Rust library. Unsupported modern
//! language features fail before any generated artifact is returned.

use crate::{
    ast::{BinaryOperator, ExpressionKind, LogicalOperator},
    diagnostics::{Diagnostic, Span},
    resolver::{ResolvedArgument, ResolvedExpression, ResolvedProgram, ResolvedStatement},
    value::ValueType,
};
use std::fmt::Write;

/// Emit a complete C translation unit. This does not invoke a C compiler.
pub fn emit_c(source: &str) -> Result<String, Diagnostic> {
    emit_program(&crate::compile(source)?)
}

/// Emit an opt-in, freestanding C11 translation unit from a statically
/// resolved plan. All unsupported forms are rejected before emission.
pub fn emit_program(program: &ResolvedProgram) -> Result<String, Diagnostic> {
    let requirements = crate::native_preflight::inspect(program)?;
    crate::scalar_ir::verify_program(program)?;
    let mut out = format!(
        "/* Scarp freestanding C11 unit. Caller supplies workspace and output. */\n#define SCARP_SLOT_COUNT {}\n#define SCARP_TEMP_COUNT {}\n",
        requirements.slot_count,
        requirements.temporary_count
    );
    out.push_str(include_str!("c_runtime.h"));
    for (index, operation) in program.statements.iter().enumerate() {
        match &operation.kind {
            ResolvedStatement::Store { value, .. }
            | ResolvedStatement::Update { value, .. }
            | ResolvedStatement::RepeatInit { count: value, .. } => {
                emit_expression(&mut out, index, value)
            }
            ResolvedStatement::JumpIfFalse { condition, .. } => {
                emit_expression(&mut out, index, condition)
            }
            ResolvedStatement::Output { arguments, .. } => {
                if let ResolvedArgument::Expression(value) = &arguments[0] {
                    emit_expression(&mut out, index, value);
                }
            }
            _ => {}
        }
    }
    out.push_str("\nstatic int scarp_pass(struct scarp_workspace *w, const struct scarp_host *host, int capture, uint64_t steps, size_t max_prints) {\nsize_t pc=0, i; int64_t value=0; (void)value; (void)host; (void)capture; (void)max_prints;\n(void)scarp_step; (void)scarp_binary; (void)scarp_print;\nw->remaining=steps; w->prints=0; w->error_code=0; w->error_start=0; w->error_end=0;\nfor(i=0;i<SCARP_SLOT_COUNT;++i) w->slots[i]=0;\nfor(;;) switch(pc) {\n");
    for (index, operation) in program.statements.iter().enumerate() {
        writeln!(out, "case {index}: {{").unwrap();
        // Storage retirement is not charged as an execution step.
        if !matches!(operation.kind, ResolvedStatement::Retire { .. }) {
            step(&mut out, operation.span);
        }
        match &operation.kind {
            ResolvedStatement::Store { slot, .. } => {
                writeln!(
                    out,
                    "if(!scarp_e{index}(w,&w->slots[{slot}])) return 0;\npc={};",
                    index + 1
                )
                .unwrap();
            }
            ResolvedStatement::RepeatInit {
                count_slot,
                index_slot,
                ..
            } => {
                writeln!(out, "if(!scarp_e{index}(w,&value)) return 0;").unwrap();
                writeln!(
                    out,
                    "if(value<0) return scarp_fail(w,19u,{},{});\nw->slots[{count_slot}]=value;\nw->slots[{index_slot}]=0;\npc={};",
                    operation.span.start, operation.span.end, index + 1
                )
                .unwrap();
            }
            ResolvedStatement::RepeatGuard {
                count_slot,
                index_slot,
                target,
            } => {
                writeln!(
                    out,
                    "pc=w->slots[{index_slot}]>=w->slots[{count_slot}]?{target}:{};",
                    index + 1
                )
                .unwrap();
            }
            ResolvedStatement::RepeatNext { index_slot } => {
                writeln!(
                    out,
                    "if(w->slots[{index_slot}]==INT64_MAX) return scarp_fail(w,7u,{},{});\n++w->slots[{index_slot}];\npc={};",
                    operation.span.start, operation.span.end, index + 1
                )
                .unwrap();
            }

            ResolvedStatement::Update {
                slot,
                operator,
                operator_span,
                ..
            } => {
                writeln!(out, "if(!scarp_e{index}(w,&value)) return 0;").unwrap();
                writeln!(out,"if(!scarp_binary(w,{},w->slots[{slot}],value,&w->slots[{slot}],{},{})) return 0;\npc={};",opcode(*operator),operator_span.start,operator_span.end,index+1).unwrap();
            }
            ResolvedStatement::JumpIfFalse { target, .. } => {
                writeln!(
                    out,
                    "if(!scarp_e{index}(w,&value)) return 0;\npc=value?{}:{target};",
                    index + 1
                )
                .unwrap();
            }
            ResolvedStatement::Jump { target } | ResolvedStatement::LoopControl { target, .. } => {
                writeln!(out, "pc={target};").unwrap();
            }
            ResolvedStatement::Retire { .. } => {
                // Only scalar slots exist. A retired slot is never read again
                // without an intervening Store, so no C memory action is needed.
                writeln!(out, "pc={};", index + 1).unwrap();
            }
            ResolvedStatement::Output { arguments, .. } => {
                writeln!(
                    out,
                    "if(w->prints>=max_prints) return scarp_fail(w,18u,{},{});\n++w->prints;",
                    operation.span.start, operation.span.end
                )
                .unwrap();
                match &arguments[0] {
                    ResolvedArgument::Expression(expression) => {
                        writeln!(out, "if(!scarp_e{index}(w,&value)) return 0;").unwrap();
                        writeln!(
                            out,
                            "if(capture && !scarp_print(w,host,value,{}, {},{})) return 0;",
                            usize::from(expression.kind == ValueType::Boolean),
                            operation.span.start,
                            operation.span.end
                        )
                        .unwrap();
                    }
                    ResolvedArgument::String(value) => {
                        out.push_str("static const uint8_t text[]={");
                        for byte in value.as_bytes() {
                            write!(out, "{byte},").unwrap();
                        }
                        out.push_str("0}; static const uint8_t lf[]={10};\n");
                        writeln!(out,"if(capture && (!scarp_write(w,host,text,{}, {},{}) || !scarp_write(w,host,lf,1,{},{}))) return 0;",value.len(),operation.span.start,operation.span.end,operation.span.start,operation.span.end).unwrap();
                    }
                }
                writeln!(out, "pc={};", index + 1).unwrap();
            }
            _ => unreachable!("unsupported operation rejected before emission"),
        }
        out.push_str("break; }\n");
    }
    out.push_str("default: return 1;\n}\n}\nuint32_t scarp_validate(struct scarp_workspace *w, uint64_t steps, size_t prints) {\nif(!w) return SCARP_OUTPUT_ERROR;\n(void)scarp_pass(w,0,0,steps,prints); return w->error_code;\n}\nuint32_t scarp_run(struct scarp_workspace *w, const struct scarp_host *host, uint64_t steps, size_t prints) {\nif(!w) return SCARP_OUTPUT_ERROR;\nif(!scarp_pass(w,0,0,steps,prints)) return w->error_code;\n(void)scarp_pass(w,host,1,steps,prints); return w->error_code;\n}\n");
    Ok(out)
}

fn step(out: &mut String, span: Span) {
    writeln!(
        out,
        "if(!scarp_step(w,{},{})) return 0;",
        span.start, span.end
    )
    .unwrap();
}
fn integer(value: i64) -> String {
    if value == i64::MIN {
        "INT64_MIN".into()
    } else if value < 0 {
        format!("(-INT64_C({}))", -value)
    } else {
        format!("INT64_C({value})")
    }
}
fn opcode(operator: BinaryOperator) -> usize {
    use BinaryOperator::*;
    match operator {
        Add => 0,
        Subtract => 1,
        Multiply => 2,
        Divide => 3,
        Remainder => 4,
        BitAnd => 5,
        BitOr => 6,
        BitXor => 7,
        ShiftLeft => 8,
        ShiftRight => 9,
        UnsignedRight => 10,
        Equal => 11,
        NotEqual => 12,
        Less => 13,
        LessEqual => 14,
        Greater => 15,
        GreaterEqual => 16,
    }
}

enum Task {
    Visit(usize),
    Finish(usize),
    Logical(usize),
    Close(usize),
}
fn emit_expression(out: &mut String, id: usize, expression: &ResolvedExpression) {
    writeln!(
        out,
        "\nstatic int scarp_e{id}(struct scarp_workspace *w,int64_t *out) {{"
    )
    .unwrap();
    let mut tasks = vec![Task::Visit(expression.syntax.root())];
    while let Some(task) = tasks.pop() {
        match task {
            Task::Visit(index) => {
                let node = &expression.syntax.nodes()[index];
                step(out, node.span);
                match &node.kind {
                    ExpressionKind::Integer(value) => {
                        writeln!(out, "w->values[{index}]={};", integer(*value)).unwrap();
                    }
                    ExpressionKind::Boolean(value) => {
                        writeln!(out, "w->values[{index}]={};", usize::from(*value)).unwrap();
                    }
                    ExpressionKind::Name(_) => {
                        writeln!(
                            out,
                            "w->values[{index}]=w->slots[{}];",
                            expression.slots[index].unwrap()
                        )
                        .unwrap();
                    }
                    ExpressionKind::Group { expression: child } => {
                        tasks.push(Task::Finish(index));
                        tasks.push(Task::Visit(*child));
                    }
                    ExpressionKind::Negate { operand, .. }
                    | ExpressionKind::BitNot { operand, .. }
                    | ExpressionKind::Not { operand, .. } => {
                        tasks.push(Task::Finish(index));
                        tasks.push(Task::Visit(*operand));
                    }
                    ExpressionKind::Binary { left, right, .. } => {
                        tasks.push(Task::Finish(index));
                        tasks.push(Task::Visit(*right));
                        tasks.push(Task::Visit(*left));
                    }
                    ExpressionKind::Logical { left, .. } => {
                        tasks.push(Task::Logical(index));
                        tasks.push(Task::Visit(*left));
                    }
                    _ => unreachable!("unsupported syntax checked before emission"),
                }
            }
            Task::Finish(index) => match expression.syntax.nodes()[index].kind {
                ExpressionKind::Group { expression: child } => {
                    writeln!(out, "w->values[{index}]=w->values[{child}];").unwrap();
                }
                ExpressionKind::Negate {
                    operand,
                    operator_span,
                } => {
                    step(out, operator_span);
                    writeln!(out,"if(w->values[{operand}]==INT64_MIN) return scarp_fail(w,7u,{},{});\nw->values[{index}]=-w->values[{operand}];",operator_span.start,operator_span.end).unwrap();
                }
                ExpressionKind::BitNot {
                    operand,
                    operator_span,
                } => {
                    step(out, operator_span);
                    writeln!(
                        out,
                        "w->values[{index}]=scarp_from_bits(~(uint64_t)w->values[{operand}]);"
                    )
                    .unwrap();
                }
                ExpressionKind::Not {
                    operand,
                    operator_span,
                } => {
                    step(out, operator_span);
                    writeln!(out, "w->values[{index}]=!w->values[{operand}];").unwrap();
                }
                ExpressionKind::Binary {
                    left,
                    operator,
                    right,
                    operator_span,
                } => {
                    step(out, operator_span);
                    writeln!(out,"if(!scarp_binary(w,{},w->values[{left}],w->values[{right}],&w->values[{index}],{},{})) return 0;",opcode(operator),operator_span.start,operator_span.end).unwrap();
                }
                _ => unreachable!("finish task refers to a nonlogical parent"),
            },
            Task::Logical(index) => {
                let ExpressionKind::Logical {
                    left,
                    right,
                    operator,
                    operator_span,
                } = expression.syntax.nodes()[index].kind
                else {
                    unreachable!()
                };
                step(out, operator_span);
                writeln!(
                    out,
                    "if({}w->values[{left}]) {{ w->values[{index}]=w->values[{left}]; }} else {{",
                    if operator == LogicalOperator::And {
                        "!"
                    } else {
                        ""
                    }
                )
                .unwrap();
                tasks.push(Task::Close(index));
                tasks.push(Task::Visit(right));
            }
            Task::Close(index) => {
                let ExpressionKind::Logical {
                    right,
                    operator_span,
                    ..
                } = expression.syntax.nodes()[index].kind
                else {
                    unreachable!()
                };
                step(out, operator_span);
                writeln!(out, "w->values[{index}]=w->values[{right}]; }}").unwrap();
            }
        }
    }
    writeln!(
        out,
        "*out=w->values[{}]; return 1;\n}}",
        expression.syntax.root()
    )
    .unwrap();
}
