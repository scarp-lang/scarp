//! Primitive evaluation with checked integers and finite binary64 arithmetic.

use crate::{
    ast::{BinaryOperator, Expression, ExpressionKind, LogicalOperator},
    diagnostics::{Diagnostic, DiagnosticKind, Span},
    input::IoBuiltin,
    limits::Budget,
    numeric::{
        self,
        source_tensor::{self, TensorCall},
        Builtin,
    },
    resolver::{
        self, ArrayIntrinsic, ListIntrinsic, MapIntrinsic, ResolvedCall, ResolvedExpression,
    },
    types::{SourceType, MAX_LIST_LENGTH, MAX_MAP_ENTRIES},
    value::{
        finite_number, key_compare, key_work, type_mismatch, ArrayValue, ListValue, MapValue,
        RecordValue, TupleValue, Value, ValueType, VariantValue,
    },
};

/// Evaluate a standalone integer expression, preserving the v0.0.1 return type.
/// Use `evaluate_value` when any value result is allowed.
pub fn evaluate(expression: &Expression) -> Result<i64, Diagnostic> {
    evaluate_value(expression)?.require_integer(expression.nodes()[expression.root()].span)
}

/// Evaluate a standalone expression without a variable environment. This legacy
/// helper has an unbounded work allowance; capped program/native APIs are
/// preferable for controlled execution of numerical workloads.
pub fn evaluate_value(expression: &Expression) -> Result<Value, Diagnostic> {
    Workspace::default().evaluate(
        &resolver::resolve_standalone(expression)?,
        &[],
        &mut Budget::new(u64::MAX),
    )
}

/// Scratch storage belongs to one execution, never to the immutable program.
#[derive(Default)]
pub(crate) struct Workspace {
    values: Vec<Value>,
    tasks: Vec<Task>,
    tensor_tape: Option<Box<numeric::tensor_graph::TapeWorkspace>>,
}

enum Task {
    BitNot(Span),
    Vector(usize, bool, Span),
    Numeric(NumericTask, Span),
    Tuple(usize, Span),
    Array(usize, Span),
    List(usize, Span),
    Map(usize, Span),
    Record(usize, Span),
    Variant(usize, bool, Span),
    Field(usize, Span),
    Index(usize, Span),
    Builtin(Builtin, usize, Span),
    BigIntDivMod(usize, Span),
    ArrayIntrinsic(ArrayIntrinsic, usize, Span),
    ListIntrinsic(ListIntrinsic, usize, Span),
    MapIntrinsic(MapIntrinsic, usize, Span),
    UserCall(usize, usize, Span),
    Io(IoBuiltin, Span),
    Visit(usize),
    Negate(Span),
    Not(Span),
    Boolean(Span),
    Binary(BinaryOperator, Span),
    Logical(LogicalOperator, usize, Span),
    Conditional(usize, usize, Span),
}

pub(crate) enum Evaluation {
    Complete(Value),
    Io {
        builtin: IoBuiltin,
        prompt: Option<Value>,
        fallback: Option<Value>,
        span: Span,
    },
    Call {
        node: usize,
        arguments: Vec<Value>,
        span: Span,
    },
}

// Keep numerical materialization and VJP temporaries out of the general
// evaluator frame, including in Windows debug builds with a small host stack.
enum NumericTask {
    Construct(usize),
    Tensor(TensorCall, usize),
    Optimizer(numeric::source_optimizer::TensorOptimizer, usize),
    ScalarUnary(numeric::autodiff::Unary),
    FixedCast(ValueType),
}

impl Workspace {
    pub(crate) fn clear(&mut self) {
        self.values.clear();
        self.tasks.clear();
        if let Some(tape) = &mut self.tensor_tape {
            tape.clear();
        }
    }

    pub(crate) fn retire_for_reuse(&mut self, max_items: usize) {
        self.clear();
        if self.values.capacity() > max_items {
            self.values = Vec::new();
        }
        if self.tasks.capacity() > max_items {
            self.tasks = Vec::new();
        }
        if let Some(tape) = &mut self.tensor_tape {
            tape.retire_for_reuse(max_items);
        }
    }

    pub(crate) fn evaluate(
        &mut self,
        expression: &ResolvedExpression,
        environment: &[Value],
        budget: &mut Budget,
    ) -> Result<Value, Diagnostic> {
        self.start(expression);
        match self.resume(expression, environment, budget)? {
            Evaluation::Complete(value) => Ok(value),
            Evaluation::Io {
                builtin,
                prompt,
                span,
                ..
            } => Err(if builtin.output_target().is_some() {
                Diagnostic::new(
                    DiagnosticKind::FunctionUnsupported,
                    span,
                    "stream flush requires a program execution frame",
                )
            } else if prompt.is_some() {
                crate::input::prompt_unavailable(span)
            } else {
                crate::input::unavailable(span)
            }),
            Evaluation::Call { span, .. } => Err(Diagnostic::new(
                DiagnosticKind::FunctionUnsupported,
                span,
                "user calls require a program execution frame",
            )),
        }
    }

    pub(crate) fn start(&mut self, expression: &ResolvedExpression) {
        self.clear();
        self.tasks.push(Task::Visit(expression.syntax.root()));
    }

    pub(crate) fn accept_input_tuple(
        &mut self,
        present: bool,
        line: Value,
        budget: &mut Budget,
        span: Span,
    ) -> Result<(), Diagnostic> {
        self.values.extend([Value::Boolean(present), line]);
        finish_tuple(&mut self.values, 2, budget, span)
    }

    pub(crate) fn accept(&mut self, value: Value) {
        self.values.push(value);
    }

    pub(crate) fn resume(
        &mut self,
        expression: &ResolvedExpression,
        environment: &[Value],
        budget: &mut Budget,
    ) -> Result<Evaluation, Diagnostic> {
        let Self {
            values,
            tasks,
            tensor_tape,
        } = self;
        while let Some(task) = tasks.pop() {
            let span = match &task {
                Task::Visit(index) => expression.syntax.nodes()[*index].span,
                Task::UserCall(_, _, span)
                | Task::Tuple(_, span)
                | Task::Numeric(_, span)
                | Task::Array(_, span)
                | Task::List(_, span)
                | Task::Map(_, span)
                | Task::Record(_, span)
                | Task::Variant(_, _, span)
                | Task::Field(_, span)
                | Task::Io(_, span)
                | Task::Vector(_, _, span)
                | Task::BitNot(span)
                | Task::Index(_, span)
                | Task::Builtin(_, _, span)
                | Task::BigIntDivMod(_, span)
                | Task::ArrayIntrinsic(_, _, span)
                | Task::ListIntrinsic(_, _, span)
                | Task::MapIntrinsic(_, _, span)
                | Task::Negate(span)
                | Task::Not(span)
                | Task::Boolean(span)
                | Task::Binary(_, span)
                | Task::Logical(_, _, span)
                | Task::Conditional(_, _, span) => *span,
            };
            budget.charge(1, span)?;
            match task {
                Task::Visit(index) => {
                    let node = &expression.syntax.nodes()[index];
                    match &node.kind {
                        ExpressionKind::UserCall { arguments, .. } => {
                            if let Some(ResolvedCall::ScalarGraph(graph)) = &expression.calls[index]
                            {
                                values.push(graph.evaluate(environment, budget, node.span)?);
                                continue;
                            }
                            if let Some(ResolvedCall::TensorGraph(graph)) = &expression.calls[index]
                            {
                                values.push(graph.evaluate(
                                    environment,
                                    budget,
                                    node.span,
                                    tensor_tape.get_or_insert_with(Box::default),
                                )?);
                                continue;
                            }
                            tasks.push(match &expression.calls[index] {
                                Some(ResolvedCall::X86Pause) => {
                                    return Err(Diagnostic::new(
                                        DiagnosticKind::NativeUnsupported,
                                        node.span,
                                        "x86_pause cannot run in the interpreter",
                                    ));
                                }
                                Some(ResolvedCall::Io(input)) => Task::Io(*input, node.span),
                                Some(ResolvedCall::Builtin(builtin)) => {
                                    Task::Builtin(*builtin, index, node.span)
                                }
                                Some(ResolvedCall::BigIntDivMod) => {
                                    Task::BigIntDivMod(index, node.span)
                                }
                                Some(ResolvedCall::Array(op)) => {
                                    Task::ArrayIntrinsic(*op, index, node.span)
                                }
                                Some(ResolvedCall::List(op)) => {
                                    Task::ListIntrinsic(*op, index, node.span)
                                }
                                Some(ResolvedCall::FixedCast(kind)) => {
                                    Task::Numeric(NumericTask::FixedCast(*kind), node.span)
                                }
                                Some(ResolvedCall::ScalarUnary(op)) => {
                                    Task::Numeric(NumericTask::ScalarUnary(*op), node.span)
                                }
                                Some(ResolvedCall::TensorOptimizer(op)) => Task::Numeric(
                                    NumericTask::Optimizer(*op, arguments.len()),
                                    node.span,
                                ),
                                Some(ResolvedCall::Tensor(op)) => Task::Numeric(
                                    NumericTask::Tensor(*op, arguments.len()),
                                    node.span,
                                ),
                                Some(ResolvedCall::Map(op)) => {
                                    Task::MapIntrinsic(*op, index, node.span)
                                }
                                _ => Task::UserCall(index, arguments.len(), node.span),
                            });
                            for argument in arguments.iter().rev() {
                                tasks.push(Task::Visit(argument.expression));
                            }
                        }
                        ExpressionKind::Variant(constructor) => {
                            tasks.push(Task::Variant(
                                index,
                                constructor.payload.is_some(),
                                node.span,
                            ));
                            if let Some(payload) = constructor.payload {
                                tasks.push(Task::Visit(payload));
                            }
                        }
                        ExpressionKind::Record { fields, .. } => {
                            tasks.push(Task::Record(index, node.span));
                            for field in fields.iter().rev() {
                                tasks.push(Task::Visit(field.expression));
                            }
                        }
                        ExpressionKind::Field { receiver, .. } => {
                            tasks.push(Task::Field(index, node.span));
                            tasks.push(Task::Visit(*receiver));
                        }
                        ExpressionKind::Tuple { elements } => {
                            tasks.push(Task::Tuple(elements.len(), node.span));
                            for &element in elements.iter().rev() {
                                tasks.push(Task::Visit(element));
                            }
                        }
                        ExpressionKind::Tensor { elements, .. } => {
                            tasks.push(Task::Numeric(NumericTask::Construct(index), node.span));
                            for &element in elements.iter().rev() {
                                tasks.push(Task::Visit(element));
                            }
                        }
                        ExpressionKind::Array { elements, .. } => {
                            tasks.push(Task::Array(index, node.span));
                            for &element in elements.iter().rev() {
                                tasks.push(Task::Visit(element));
                            }
                        }
                        ExpressionKind::List { elements, .. } => {
                            tasks.push(Task::List(index, node.span));
                            for &element in elements.iter().rev() {
                                tasks.push(Task::Visit(element));
                            }
                        }
                        ExpressionKind::Map { entries, .. } => {
                            tasks.push(Task::Map(index, node.span));
                            for &(key, value) in entries.iter().rev() {
                                tasks.push(Task::Visit(value));
                                tasks.push(Task::Visit(key));
                            }
                        }
                        ExpressionKind::Vector { elements }
                        | ExpressionKind::Integers { elements } => {
                            tasks.push(Task::Vector(
                                elements.len(),
                                matches!(&node.kind, ExpressionKind::Integers { .. }),
                                node.span,
                            ));
                            for &element in elements.iter().rev() {
                                tasks.push(Task::Visit(element));
                            }
                        }
                        ExpressionKind::Index {
                            vector,
                            index,
                            operator_span,
                            ..
                        } => {
                            tasks.push(Task::Index(*index, *operator_span));
                            tasks.push(Task::Visit(*index));
                            tasks.push(Task::Visit(*vector));
                        }
                        ExpressionKind::Integer(value) => values.push(Value::Integer(*value)),
                        ExpressionKind::Float(value) => values.push(Value::Float(*value)),
                        ExpressionKind::Decimal(value) => values.push(Value::Decimal(*value)),
                        ExpressionKind::Boolean(value) => values.push(Value::Boolean(*value)),
                        ExpressionKind::String(value) => {
                            budget.charge_text_allocation(value.len(), node.span)?;
                            values.push(Value::from(value.as_str()));
                        }
                        ExpressionKind::Name(name) => {
                            let value = expression.slots[index]
                                .and_then(|slot| environment.get(slot))
                                .cloned()
                                .ok_or_else(|| resolver::unknown_name(name, node.span))?;
                            values.push(value);
                        }
                        ExpressionKind::Group { expression } => {
                            tasks.push(Task::Visit(*expression))
                        }
                        ExpressionKind::Conditional { branches, .. } => {
                            let condition = branches[0].condition;
                            let span = expression.syntax.nodes()[condition].span;
                            tasks.push(Task::Conditional(index, 0, span));
                            tasks.push(Task::Visit(condition));
                        }
                        ExpressionKind::BitNot {
                            operand,
                            operator_span,
                        } => {
                            tasks.push(Task::BitNot(*operator_span));
                            tasks.push(Task::Visit(*operand));
                        }
                        ExpressionKind::Negate {
                            operand,
                            operator_span,
                        } => {
                            tasks.push(match expression.calls[index] {
                                Some(ResolvedCall::Tensor(operation)) => {
                                    Task::Numeric(NumericTask::Tensor(operation, 1), *operator_span)
                                }
                                _ => Task::Negate(*operator_span),
                            });
                            tasks.push(Task::Visit(*operand));
                        }
                        ExpressionKind::Not {
                            operand,
                            operator_span,
                        } => {
                            tasks.push(Task::Not(*operator_span));
                            tasks.push(Task::Visit(*operand));
                        }
                        ExpressionKind::Binary {
                            left,
                            operator,
                            right,
                            operator_span,
                        } => {
                            tasks.push(match expression.calls[index] {
                                Some(ResolvedCall::Tensor(operation)) => {
                                    Task::Numeric(NumericTask::Tensor(operation, 2), *operator_span)
                                }
                                _ => Task::Binary(*operator, *operator_span),
                            });
                            tasks.push(Task::Visit(*right));
                            tasks.push(Task::Visit(*left));
                        }
                        ExpressionKind::Logical {
                            left,
                            operator,
                            right,
                            operator_span,
                        } => {
                            tasks.push(Task::Logical(*operator, *right, *operator_span));
                            tasks.push(Task::Visit(*left));
                        }
                    }
                }
                Task::Variant(node, has_payload, span) => {
                    let resolver::NodeMetadata::Variant { kind, tag } =
                        &expression.node_metadata[&node]
                    else {
                        unreachable!("resolved variant constructor");
                    };
                    let payload = if has_payload {
                        budget.charge_nominal_allocation(
                            VariantValue::allocation_bytes(),
                            span,
                            DiagnosticKind::VariantAllocation,
                        )?;
                        Some(values.pop().expect("resolved variant payload"))
                    } else {
                        None
                    };
                    values.push(Value::Variant(VariantValue::from_checked(
                        kind.clone(),
                        *tag,
                        payload,
                    )));
                }
                Task::Record(node, span) => {
                    let resolver::NodeMetadata::Record { kind, order } =
                        &expression.node_metadata[&node]
                    else {
                        unreachable!("resolved record constructor");
                    };
                    budget.charge(order.len() as u64, span)?;
                    budget.charge_nominal_allocation(
                        RecordValue::allocation_bytes(order.len()),
                        span,
                        DiagnosticKind::RecordAllocation,
                    )?;
                    let start = values.len() - order.len();
                    let mut fields = Vec::new();
                    fields.try_reserve_exact(order.len()).map_err(|_| {
                        Diagnostic::new(
                            DiagnosticKind::RecordAllocation,
                            span,
                            "could not reserve record storage",
                        )
                    })?;
                    for &input in order.iter() {
                        fields.push(std::mem::replace(&mut values[start + input], Value::Unit));
                    }
                    values.truncate(start);
                    values.push(Value::Record(RecordValue::from_checked(
                        kind.clone(),
                        fields,
                    )));
                }
                Task::Field(node, _) => {
                    let resolver::NodeMetadata::Field(offset) = expression.node_metadata[&node]
                    else {
                        unreachable!("resolved record field");
                    };
                    let Some(Value::Record(value)) = values.pop() else {
                        unreachable!("field receiver was resolved as a record");
                    };
                    values.push(value.fields()[offset].clone());
                }
                Task::Tuple(count, span) => finish_tuple(values, count, budget, span)?,
                Task::Numeric(operation, span) => {
                    finish_numeric(operation, expression, values, budget, span)?;
                }
                Task::Array(node, span) => {
                    let ExpressionKind::Array {
                        elements,
                        empty_element_type,
                    } = &expression.syntax.nodes()[node].kind
                    else {
                        unreachable!("fixed array task");
                    };
                    let count = elements.len();
                    let start = values.len() - count;
                    let element = empty_element_type
                        .clone()
                        .unwrap_or_else(|| values[start].kind());
                    budget.charge((count + element.node_count() + 1) as u64, span)?;
                    let bytes = ArrayValue::allocation_bytes(&element, count).ok_or_else(|| {
                        Diagnostic::new(
                            DiagnosticKind::ArrayAllocation,
                            span,
                            "fixed array allocation size overflow",
                        )
                    })?;
                    budget.charge_array_allocation(bytes, span)?;
                    let kind =
                        SourceType::array(element, count).expect("resolved fixed array shape");
                    let mut elements = Vec::new();
                    elements.try_reserve_exact(count).map_err(|_| {
                        Diagnostic::new(
                            DiagnosticKind::ArrayAllocation,
                            span,
                            "could not reserve fixed array storage",
                        )
                    })?;
                    elements.extend(values.drain(start..));
                    values.push(Value::Array(ArrayValue::from_checked(kind, elements)));
                }
                Task::List(node, span) => {
                    let ExpressionKind::List {
                        elements,
                        empty_element_type,
                    } = &expression.syntax.nodes()[node].kind
                    else {
                        unreachable!("typed list task");
                    };
                    let count = elements.len();
                    let start = values.len() - count;
                    let element = empty_element_type
                        .clone()
                        .unwrap_or_else(|| values[start].kind());
                    budget.charge((count + element.node_count() + 1) as u64, span)?;
                    let bytes = ListValue::allocation_bytes(&element, count).ok_or_else(|| {
                        Diagnostic::new(
                            DiagnosticKind::ListAllocation,
                            span,
                            "dynamic list allocation size exceeds configured limits",
                        )
                    })?;
                    budget.charge_list_allocation(bytes, span)?;
                    let kind = SourceType::list(element).expect("resolved list element type");
                    let mut items = Vec::new();
                    items.try_reserve_exact(count).map_err(|_| {
                        Diagnostic::new(
                            DiagnosticKind::ListAllocation,
                            span,
                            "could not reserve dynamic list storage",
                        )
                    })?;
                    items.extend(values.drain(start..));
                    values.push(Value::List(ListValue::from_checked(kind, items)));
                }
                Task::Map(node, span) => {
                    let ExpressionKind::Map {
                        entries,
                        empty_types,
                    } = &expression.syntax.nodes()[node].kind
                    else {
                        unreachable!("checked map literal task");
                    };
                    let count = entries.len();
                    let start = values.len() - 2 * count;
                    let (key, value) = empty_types
                        .clone()
                        .unwrap_or_else(|| (values[start].kind(), values[start + 1].kind()));
                    let kind =
                        SourceType::map(&key, &value).expect("resolved map key and value types");
                    budget.charge(
                        (2 * count + key.node_count() + value.node_count() + 1) as u64,
                        span,
                    )?;
                    let bytes =
                        MapValue::allocation_bytes(&key, &value, count).ok_or_else(|| {
                            Diagnostic::new(
                                DiagnosticKind::MapAllocation,
                                span,
                                "map storage size exceeds its configured limits",
                            )
                        })?;
                    budget.charge_map_allocation(bytes, span)?;
                    let mut sorted: Vec<(Value, Value)> = Vec::new();
                    sorted.try_reserve_exact(count).map_err(|_| {
                        Diagnostic::new(
                            DiagnosticKind::MapAllocation,
                            span,
                            "cannot reserve ordered map storage",
                        )
                    })?;
                    let mut incoming = values.drain(start..);
                    for _ in 0..count {
                        let entry_key = incoming.next().expect("resolved key");
                        let entry_value = incoming.next().expect("resolved value");
                        let mut low = 0;
                        let mut high = sorted.len();
                        while low < high {
                            let mid = low + (high - low) / 2;
                            budget.charge(key_work(&entry_key, &sorted[mid].0), span)?;
                            match key_compare(&entry_key, &sorted[mid].0) {
                                std::cmp::Ordering::Less => high = mid,
                                std::cmp::Ordering::Equal => {
                                    return Err(Diagnostic::new(
                                        DiagnosticKind::MapKey,
                                        span,
                                        "duplicate key in map literal",
                                    ));
                                }
                                std::cmp::Ordering::Greater => low = mid + 1,
                            }
                        }
                        // Shifting a contiguous suffix is bounded and charged,
                        // even when keys arrive in descending order.
                        budget.charge((sorted.len() - low) as u64, span)?;
                        sorted.insert(low, (entry_key, entry_value));
                    }
                    drop(incoming);
                    values.push(Value::Map(MapValue::from_checked(kind, sorted)));
                }
                Task::Vector(count, integers, span) => {
                    budget.charge(count as u64, span)?;
                    let start = values.len() - count;
                    if integers {
                        let mut elements = numeric::buffer(count, budget, span)?;
                        for value in &values[start..] {
                            elements.push(value.require_integer(span)?);
                        }
                        values.truncate(start);
                        values.push(Value::Integers(
                            crate::value::IntegerVector::from_values(elements)
                                .expect("bounded collection"),
                        ));
                        continue;
                    }
                    let mut elements = numeric::buffer(count, budget, span)?;
                    for value in &values[start..] {
                        elements.push(finite_number(value.require_float(span)?, span)?);
                    }
                    values.truncate(start);
                    values.push(numeric::vector(elements, span)?);
                }
                Task::Index(index_node, span) => {
                    let index = values.pop().unwrap().require_integer(span)?;
                    let value = values.pop().unwrap();
                    if let Some(tensor) = value.as_tensor() {
                        let index_span = expression.syntax.nodes()[index_node].span;
                        values.push(select_tensor(tensor, index, index_span)?);
                        continue;
                    }
                    if let Some(tuple) = value.as_tuple() {
                        let element = usize::try_from(index)
                            .ok()
                            .and_then(|i| tuple.elements().get(i))
                            .cloned()
                            .ok_or_else(|| {
                                Diagnostic::new(
                                    DiagnosticKind::TupleIndex,
                                    span,
                                    "tuple index outside arity",
                                )
                            })?;
                        values.push(element);
                        continue;
                    }
                    if let Some(array) = value.as_array() {
                        let element = usize::try_from(index)
                            .ok()
                            .and_then(|index| array.elements().get(index))
                            .ok_or_else(|| {
                                Diagnostic::new(
                                    DiagnosticKind::ArrayIndex,
                                    expression.syntax.nodes()[index_node].span,
                                    format!(
                                        "index {index} is outside fixed array length {}",
                                        array.len()
                                    ),
                                )
                            })?;
                        values.push(element.clone());
                        continue;
                    }
                    if let Some(list) = value.as_list() {
                        let element = usize::try_from(index)
                            .ok()
                            .and_then(|index| list.elements().get(index))
                            .ok_or_else(|| {
                                Diagnostic::new(
                                    DiagnosticKind::ListIndex,
                                    expression.syntax.nodes()[index_node].span,
                                    format!(
                                        "index {index} is outside dynamic list length {}",
                                        list.len()
                                    ),
                                )
                            })?;
                        values.push(element.clone());
                        continue;
                    }
                    if let Some(array) = value.as_integers() {
                        let element = usize::try_from(index)
                            .ok()
                            .and_then(|index| array.as_slice().get(index))
                            .ok_or_else(|| {
                                Diagnostic::new(
                                    DiagnosticKind::VectorIndex,
                                    span,
                                    format!(
                                        "index {index} is outside integer collection length {}",
                                        array.len()
                                    ),
                                )
                            })?;
                        values.push(Value::Integer(*element));
                        continue;
                    }
                    let vector = value.require_vector(span)?;
                    let element = usize::try_from(index)
                        .ok()
                        .and_then(|i| vector.as_slice().get(i))
                        .ok_or_else(|| {
                            Diagnostic::new(
                                DiagnosticKind::VectorIndex,
                                span,
                                format!("index {index} is outside vector length {}", vector.len()),
                            )
                        })?;
                    values.push(Value::Float(*element));
                }
                Task::Builtin(builtin, node, span) => {
                    let ExpressionKind::UserCall { arguments, .. } =
                        &expression.syntax.nodes()[node].kind
                    else {
                        unreachable!("resolved builtin call")
                    };
                    let count = arguments.len();
                    let start = values.len() - count;
                    let value = numeric::evaluate_with_spans(
                        builtin,
                        &values[start..],
                        budget,
                        span,
                        |i| expression.syntax.nodes()[arguments[i].expression].span,
                    )?;
                    values.truncate(start);
                    values.push(value);
                }
                Task::BigIntDivMod(node, span) => {
                    let ExpressionKind::UserCall { arguments, .. } =
                        &expression.syntax.nodes()[node].kind
                    else {
                        unreachable!("resolved bigint_divmod call")
                    };
                    debug_assert_eq!(arguments.len(), 2);
                    let start = values.len() - 2;
                    let (quotient, remainder) = crate::bigint::divmod(
                        values[start].as_bigint().expect("typed bigint"),
                        values[start + 1].as_bigint().expect("typed bigint"),
                        budget,
                        expression.syntax.nodes()[arguments[1].expression].span,
                    )?;
                    values.truncate(start);
                    values.extend([Value::BigInt(quotient), Value::BigInt(remainder)]);
                    finish_tuple(values, 2, budget, span)?;
                }
                Task::ArrayIntrinsic(operation, node, span) => {
                    let start = values.len() - operation.arity();
                    let array = values[start].as_array().expect("checked array argument");
                    let result = match operation {
                        ArrayIntrinsic::Len => Value::Integer(array.len() as i64),
                        ArrayIntrinsic::Concat => {
                            let right = values[start + 1]
                                .as_array()
                                .expect("checked array argument");
                            if array.is_empty() {
                                Value::Array(right.clone())
                            } else if right.is_empty() {
                                Value::Array(array.clone())
                            } else {
                                let length = array.len() + right.len();
                                let mut elements =
                                    reserve_array_snapshot(array, length, budget, span)?;
                                elements.extend_from_slice(array.elements());
                                elements.extend_from_slice(right.elements());
                                let kind = SourceType::array(array.element_type(), length)
                                    .expect("resolved concatenated array shape");
                                Value::Array(ArrayValue::from_checked(kind, elements))
                            }
                        }
                        ArrayIntrinsic::GetOr => {
                            let index = values[start + 1].require_integer(span)?;
                            usize::try_from(index)
                                .ok()
                                .and_then(|index| array.elements().get(index))
                                .unwrap_or(&values[start + 2])
                                .clone()
                        }
                        ArrayIntrinsic::Set | ArrayIntrinsic::Swap => {
                            let ExpressionKind::UserCall { arguments, .. } =
                                &expression.syntax.nodes()[node].kind
                            else {
                                unreachable!("resolved array operation");
                            };
                            let check_index = |argument: usize| -> Result<usize, Diagnostic> {
                                let position = values[start + argument].require_integer(span)?;
                                usize::try_from(position)
                                    .ok()
                                    .filter(|&index| index < array.len())
                                    .ok_or_else(|| {
                                        Diagnostic::new(
                                            DiagnosticKind::ArrayIndex,
                                            expression.syntax.nodes()
                                                [arguments[argument].expression]
                                                .span,
                                            format!(
                                                "index {position} is outside fixed array length {}",
                                                array.len()
                                            ),
                                        )
                                    })
                            };
                            let first = check_index(1)?;
                            if operation == ArrayIntrinsic::Swap {
                                let second = check_index(2)?;
                                snapshot_array(array, budget, span, |items| {
                                    items.swap(first, second);
                                })?
                            } else {
                                snapshot_array(array, budget, span, |items| {
                                    items[first] = values[start + 2].clone();
                                })?
                            }
                        }
                        ArrayIntrinsic::Reverse => snapshot_array(array, budget, span, |items| {
                            items.reverse();
                        })?,
                    };
                    values.truncate(start);
                    values.push(result);
                }
                Task::ListIntrinsic(operation, node, span) => {
                    let ExpressionKind::UserCall { arguments, .. } =
                        &expression.syntax.nodes()[node].kind
                    else {
                        unreachable!("resolved list operation");
                    };
                    let count = arguments.len();
                    let start = values.len() - count;
                    let list = values[start]
                        .as_list()
                        .expect("list operation has checked first argument");
                    let result = if operation == ListIntrinsic::Len {
                        Value::Integer(list.len() as i64)
                    } else {
                        let index = if operation == ListIntrinsic::Set
                            || operation == ListIntrinsic::Remove
                            || operation == ListIntrinsic::Swap
                        {
                            let n = values[start + 1].require_integer(span)?;
                            Some(
                                usize::try_from(n)
                                    .ok()
                                    .filter(|&n| n < list.len())
                                    .ok_or_else(|| {
                                        Diagnostic::new(
                                            DiagnosticKind::ListIndex,
                                            expression.syntax.nodes()[arguments[1].expression].span,
                                            format!(
                                                "index {n} is outside dynamic list length {}",
                                                list.len()
                                            ),
                                        )
                                    })?,
                            )
                        } else {
                            None
                        };
                        let second_index = if operation == ListIntrinsic::Swap {
                            let position = values[start + 2].require_integer(span)?;
                            Some(
                                usize::try_from(position)
                                    .ok()
                                    .filter(|&index| index < list.len())
                                    .ok_or_else(|| {
                                        Diagnostic::new(
                                            DiagnosticKind::ListIndex,
                                            expression.syntax.nodes()[arguments[2].expression].span,
                                            format!(
                                                "index {position} is outside dynamic list length {}",
                                                list.len()
                                            ),
                                        )
                                    })?,
                            )
                        } else {
                            None
                        };
                        let length = if operation == ListIntrinsic::Push {
                            list.len().checked_add(1)
                        } else if operation == ListIntrinsic::Remove {
                            list.len().checked_sub(1)
                        } else {
                            Some(list.len())
                        }
                        .filter(|&length| length <= MAX_LIST_LENGTH)
                        .ok_or_else(|| {
                            Diagnostic::new(
                                DiagnosticKind::ListLimit,
                                span,
                                "dynamic list update would exceed its 4096-element limit",
                            )
                        })?;
                        budget.charge((length + list.kind().node_count() + 1) as u64, span)?;
                        let bytes = ListValue::allocation_bytes(list.element_type(), length)
                            .ok_or_else(|| {
                                Diagnostic::new(
                                    DiagnosticKind::ListAllocation,
                                    span,
                                    "list update allocation size overflow",
                                )
                            })?;
                        budget.charge_list_allocation(bytes, span)?;
                        let mut items = Vec::new();
                        items.try_reserve_exact(length).map_err(|_| {
                            Diagnostic::new(
                                DiagnosticKind::ListAllocation,
                                span,
                                "could not reserve new immutable list storage",
                            )
                        })?;
                        let elements = list.elements();
                        match operation {
                            ListIntrinsic::Push => {
                                items.extend_from_slice(elements);
                                items.push(values[start + 1].clone());
                            }
                            ListIntrinsic::Set => {
                                items.extend_from_slice(elements);
                                items[index.expect("checked list index")] =
                                    values[start + 2].clone();
                            }
                            ListIntrinsic::Remove => {
                                let index = index.expect("checked list index");
                                items.extend_from_slice(&elements[..index]);
                                items.extend_from_slice(&elements[index + 1..]);
                            }
                            ListIntrinsic::Reverse => {
                                items.extend_from_slice(elements);
                                items.reverse();
                            }
                            ListIntrinsic::Swap => {
                                items.extend_from_slice(elements);
                                items.swap(
                                    index.expect("checked first list index"),
                                    second_index.expect("checked second list index"),
                                );
                            }
                            ListIntrinsic::Len => unreachable!("length needs no allocation"),
                        }
                        Value::List(ListValue::from_checked(list.kind().clone(), items))
                    };
                    values.truncate(start);
                    values.push(result);
                }
                Task::MapIntrinsic(operation, node, span) => {
                    let ExpressionKind::UserCall { arguments, .. } =
                        &expression.syntax.nodes()[node].kind
                    else {
                        unreachable!("typed map intrinsic");
                    };
                    let count = arguments.len();
                    let start = values.len() - count;
                    let map = values[start]
                        .as_map()
                        .expect("first argument checked as a map");
                    let key_index = if matches!(
                        operation,
                        MapIntrinsic::Len | MapIntrinsic::Keys | MapIntrinsic::Values
                    ) {
                        None
                    } else {
                        let query = &values[start + 1];
                        let mut low = 0;
                        let mut high = map.len();
                        let mut found = None;
                        while low < high {
                            let middle = low + (high - low) / 2;
                            let existing = &map.entries()[middle].0;
                            budget.charge(key_work(query, existing), span)?;
                            match key_compare(query, existing) {
                                std::cmp::Ordering::Less => high = middle,
                                std::cmp::Ordering::Greater => low = middle + 1,
                                std::cmp::Ordering::Equal => {
                                    found = Some(middle);
                                    break;
                                }
                            }
                        }
                        Some((found, low))
                    };
                    let result = match operation {
                        MapIntrinsic::Len => Value::Integer(map.len() as i64),
                        MapIntrinsic::Keys | MapIntrinsic::Values => {
                            // A map has at most 1024 entries, so its canonical
                            // ordered projection fits the list length limit.
                            let projected = if operation == MapIntrinsic::Keys {
                                map.key_type()
                            } else {
                                map.value_type()
                            };
                            let length = map.len();
                            budget.charge((length + projected.node_count() + 1) as u64, span)?;
                            let bytes = ListValue::allocation_bytes(projected, length).ok_or_else(
                                || {
                                    Diagnostic::new(
                                        DiagnosticKind::ListAllocation,
                                        span,
                                        "map projection storage size exceeds list bounds",
                                    )
                                },
                            )?;
                            budget.charge_list_allocation(bytes, span)?;
                            let mut items = Vec::new();
                            items.try_reserve_exact(length).map_err(|_| {
                                Diagnostic::new(
                                    DiagnosticKind::ListAllocation,
                                    span,
                                    "could not reserve ordered map projection",
                                )
                            })?;
                            for (key, value) in map.entries() {
                                items.push(if operation == MapIntrinsic::Keys {
                                    key.clone()
                                } else {
                                    value.clone()
                                });
                            }
                            let kind = SourceType::list(projected)
                                .expect("resolver checked projected list type");
                            Value::List(ListValue::from_checked(kind, items))
                        }
                        MapIntrinsic::Contains => {
                            Value::Boolean(key_index.expect("lookup").0.is_some())
                        }
                        MapIntrinsic::GetOr => {
                            if let Some(found) = key_index.expect("lookup").0 {
                                map.entries()[found].1.clone()
                            } else {
                                values[start + 2].clone()
                            }
                        }
                        MapIntrinsic::Remove if key_index.expect("lookup").0.is_none() => {
                            // Removing an absent key is an immutable no-op.
                            values[start].clone()
                        }
                        MapIntrinsic::Remove | MapIntrinsic::Set => {
                            let (found, insertion) = key_index.expect("checked key lookup");
                            let next_len = if operation == MapIntrinsic::Remove {
                                map.len() - 1
                            } else if found.is_some() {
                                map.len()
                            } else {
                                map.len().checked_add(1).ok_or_else(|| {
                                    Diagnostic::new(
                                        DiagnosticKind::MapLimit,
                                        span,
                                        "map size arithmetic overflow",
                                    )
                                })?
                            };
                            if next_len > MAX_MAP_ENTRIES {
                                return Err(Diagnostic::new(
                                    DiagnosticKind::MapLimit,
                                    span,
                                    "map update exceeds its 1024-entry limit",
                                ));
                            }
                            budget.charge((next_len + map.kind().node_count() + 1) as u64, span)?;
                            let bytes = MapValue::allocation_bytes(
                                map.key_type(),
                                map.value_type(),
                                next_len,
                            )
                            .ok_or_else(|| {
                                Diagnostic::new(
                                    DiagnosticKind::MapAllocation,
                                    span,
                                    "map update allocation size overflow",
                                )
                            })?;
                            budget.charge_map_allocation(bytes, span)?;
                            let mut entries = Vec::new();
                            entries.try_reserve_exact(next_len).map_err(|_| {
                                Diagnostic::new(
                                    DiagnosticKind::MapAllocation,
                                    span,
                                    "could not reserve new immutable map storage",
                                )
                            })?;
                            match operation {
                                MapIntrinsic::Remove => {
                                    let index = found.expect("validated map removal index");
                                    let previous = map.entries();
                                    entries.extend_from_slice(&previous[..index]);
                                    entries.extend_from_slice(&previous[index + 1..]);
                                }
                                MapIntrinsic::Set => {
                                    entries.extend_from_slice(map.entries());
                                    let new_pair =
                                        (values[start + 1].clone(), values[start + 2].clone());
                                    if let Some(index) = found {
                                        entries[index] = new_pair;
                                    } else {
                                        // Count the suffix shift, including the
                                        // worst-case descending-key insertion.
                                        budget.charge((entries.len() - insertion) as u64, span)?;
                                        entries.insert(insertion, new_pair);
                                    }
                                }
                                _ => unreachable!("map update operation"),
                            }
                            Value::Map(MapValue::from_checked(map.kind().clone(), entries))
                        }
                    };
                    values.truncate(start);
                    values.push(result);
                }
                Task::Io(input, span) => {
                    let fallback = input
                        .has_fallback()
                        .then(|| values.pop().expect("typed fallback"));
                    let prompt = input.prompts().then(|| values.pop().expect("typed prompt"));
                    return Ok(Evaluation::Io {
                        builtin: input,
                        prompt,
                        fallback,
                        span,
                    });
                }
                Task::UserCall(node, count, span) => {
                    let arguments = values.split_off(values.len() - count);
                    return Ok(Evaluation::Call {
                        node,
                        arguments,
                        span,
                    });
                }
                Task::BitNot(span) => {
                    let value = values.pop().unwrap();
                    values.push(match value.as_fixed_integer() {
                        Some(value) => value.invert(),
                        None => Value::Integer(!value.require_integer(span)?),
                    });
                }
                Task::Negate(span) => {
                    let value = values.pop().unwrap();
                    values.push(match value {
                        Value::Float(value) => finite(-value.get(), span)?,
                        Value::BigInt(value) => Value::BigInt(value.negated()),
                        Value::FixedInteger(value) => value.negate(span)?,
                        Value::Decimal(value) => Value::Decimal(
                            crate::decimal::Decimal::new(-value.coefficient(), value.scale())
                                .expect("bounded canonical decimal negation"),
                        ),
                        _ => Value::Integer(
                            value
                                .require_integer(span)?
                                .checked_neg()
                                .ok_or_else(|| overflow(span))?,
                        ),
                    });
                }
                Task::Not(span) => {
                    let value = values.pop().unwrap().require_boolean(span)?;
                    values.push(Value::Boolean(!value));
                }
                Task::Boolean(span) => {
                    let value = values.pop().unwrap().require_boolean(span)?;
                    values.push(Value::Boolean(value));
                }
                Task::Binary(operator, span) => {
                    let right = values.pop().unwrap();
                    let left = values.pop().unwrap();
                    if let (Some(a), Some(b)) = (left.as_vector(), right.as_vector()) {
                        budget.charge(a.len().max(b.len()) as u64, span)?;
                    }
                    if let (Some(a), Some(b)) = (left.as_tile(), right.as_tile()) {
                        budget.charge(a.len().max(b.len()) as u64, span)?;
                    }
                    if let (Some(a), Some(b)) = (left.as_integers(), right.as_integers()) {
                        budget.charge(a.len().max(b.len()) as u64, span)?;
                    }
                    if let (Some(a), Some(b)) = (left.composite_work(), right.composite_work()) {
                        budget.charge(a.max(b), span)?;
                    }
                    values.push(apply_binary_with_budget(
                        operator, &left, &right, budget, span,
                    )?);
                }
                Task::Logical(operator, right, span) => {
                    let left = values.pop().unwrap().require_boolean(span)?;
                    if (operator == LogicalOperator::And && !left)
                        || (operator == LogicalOperator::Or && left)
                    {
                        values.push(Value::Boolean(left));
                    } else {
                        tasks.push(Task::Boolean(span));
                        tasks.push(Task::Visit(right));
                    }
                }
                Task::Conditional(node, branch, span) => {
                    let ExpressionKind::Conditional {
                        branches,
                        otherwise,
                    } = &expression.syntax.nodes()[node].kind
                    else {
                        unreachable!("conditional expression task")
                    };
                    if values.pop().unwrap().require_boolean(span)? {
                        tasks.push(Task::Visit(branches[branch].result));
                    } else if let Some(next) = branches.get(branch + 1) {
                        let span = expression.syntax.nodes()[next.condition].span;
                        tasks.push(Task::Conditional(node, branch + 1, span));
                        tasks.push(Task::Visit(next.condition));
                    } else {
                        tasks.push(Task::Visit(*otherwise));
                    }
                }
            }
        }
        Ok(Evaluation::Complete(
            values
                .pop()
                .expect("a resolved expression produces one value"),
        ))
    }
}

#[inline(never)]
fn finish_numeric(
    operation: NumericTask,
    expression: &ResolvedExpression,
    values: &mut Vec<Value>,
    budget: &mut Budget,
    span: Span,
) -> Result<(), Diagnostic> {
    let count = match operation {
        NumericTask::Construct(node) => {
            let ExpressionKind::Tensor { kind, elements } = &expression.syntax.nodes()[node].kind
            else {
                unreachable!("resolved tensor literal");
            };
            let shape = kind.as_tensor().expect("resolved tensor type").dimensions();
            let count = elements.len();
            budget.charge(count as u64, span)?;
            let mut cells = numeric::buffer(count, budget, span)?;
            for value in values.drain(values.len() - count..) {
                let Value::Float(cell) = value else {
                    unreachable!("typed tensor cell");
                };
                cells.push(cell);
            }
            let vector =
                crate::value::NumericVector::from_finite(cells).expect("bounded tensor buffer");
            let tensor = crate::value::NumericTensor::from_vector(vector, shape)
                .expect("typed tensor literal shape");
            values.push(source_tensor::wrap(tensor));
            return Ok(());
        }
        NumericTask::ScalarUnary(_) | NumericTask::FixedCast(_) => 1,
        NumericTask::Tensor(_, count) | NumericTask::Optimizer(_, count) => count,
    };
    let start = values.len() - count;
    let args = &values[start..];
    let value = match operation {
        NumericTask::Tensor(operation, _) => operation.evaluate(args, budget, span)?,
        NumericTask::Optimizer(operation, _) => operation.evaluate(args, budget, span)?,
        NumericTask::FixedCast(kind) => {
            crate::value::fixed_integer::cast(kind, &args[0], budget, span)?
        }
        NumericTask::ScalarUnary(operation) => {
            numeric::source_autodiff::unary(operation, &args[0], budget, span)?
        }
        NumericTask::Construct(_) => unreachable!("handled tensor construction"),
    };
    values.truncate(start);
    values.push(value);
    Ok(())
}

#[inline(never)]
fn select_tensor(
    tensor: &crate::value::NumericTensor,
    index: i64,
    span: Span,
) -> Result<Value, Diagnostic> {
    let length = tensor.shape()[0];
    let index = usize::try_from(index)
        .ok()
        .filter(|&i| i < length)
        .ok_or_else(|| {
            Diagnostic::new(
                DiagnosticKind::TensorIndex,
                span,
                format!("index {index} is outside tensor axis length {length}"),
            )
        })?;
    Ok(if tensor.rank() == 1 {
        Value::Float(tensor.cells()[index])
    } else {
        source_tensor::wrap(
            tensor
                .slice_first(index)
                .expect("tensor slice shape checked during resolution"),
        )
    })
}

pub(crate) fn apply_binary_with_budget(
    operator: BinaryOperator,
    left: &Value,
    right: &Value,
    budget: &mut Budget,
    span: Span,
) -> Result<Value, Diagnostic> {
    // Reuse the bounded text kernels, independent of source-level builtin shadowing.
    let text_operator = match operator {
        BinaryOperator::Add if left.as_str().is_some() && right.as_str().is_some() => {
            Some(Builtin::TextConcat)
        }
        BinaryOperator::Multiply if left.as_str().is_some() && right.as_integer().is_some() => {
            Some(Builtin::TextRepeat)
        }
        _ => None,
    };
    if let Some(function) = text_operator {
        return numeric::evaluate_with_budget(
            function,
            &[left.clone(), right.clone()],
            budget,
            span,
        );
    }
    if left.as_tensor().is_some() && right.as_tensor().is_some() {
        if let Some(operation) = TensorCall::operator(operator) {
            return operation.evaluate(&[left.clone(), right.clone()], budget, span);
        }
    }
    if let Some(left) = left.as_fixed_integer() {
        return left.binary(operator, right, span);
    }
    if let (Some(left), Some(right)) = (left.as_bigint(), right.as_bigint()) {
        return crate::bigint::binary(operator, left, right, budget, span);
    }
    if let (Some(left), Some(right)) = (left.as_str(), right.as_str()) {
        // String equality can reject different lengths without scanning. For
        // equal lengths, prepay for all bytes regardless of where they differ.
        if left.len() == right.len() {
            budget.charge(left.len().div_ceil(64) as u64, span)?;
        }
    }
    if left.as_decimal().is_some() && right.as_decimal().is_some() {
        // All decimal arithmetic is fixed-width, with at most 18 digit steps.
        // Charge a conservative work fee before invoking the exact kernel.
        budget.charge(64, span)?;
    }
    apply_binary(operator, left, right, span)
}

pub(crate) fn apply_binary(
    operator: BinaryOperator,
    left: &Value,
    right: &Value,
    span: Span,
) -> Result<Value, Diagnostic> {
    if matches!(
        operator,
        BinaryOperator::Less
            | BinaryOperator::LessEqual
            | BinaryOperator::Greater
            | BinaryOperator::GreaterEqual
    ) {
        if let (Some(left_tuple), Some(right_tuple)) = (left.as_tuple(), right.as_tuple()) {
            if left_tuple.kind() != right_tuple.kind() {
                return Err(type_mismatch(span, left.kind(), right.kind()));
            }
            let order = numeric_tuple_cmp(left_tuple, right_tuple).ok_or_else(|| {
                Diagnostic::new(
                    DiagnosticKind::TypeMismatch,
                    span,
                    "tuple ordering requires numeric elements, including within nested tuples",
                )
            })?;
            return Ok(Value::Boolean(match operator {
                BinaryOperator::Less => order.is_lt(),
                BinaryOperator::LessEqual => order.is_le(),
                BinaryOperator::Greater => order.is_gt(),
                BinaryOperator::GreaterEqual => order.is_ge(),
                _ => unreachable!("only ordering operators reach tuple comparison"),
            }));
        }
    }
    if matches!(operator, BinaryOperator::Equal | BinaryOperator::NotEqual) {
        if left.kind() != right.kind() {
            return Err(type_mismatch(span, left.kind(), right.kind()));
        }
        return Ok(Value::Boolean(if operator == BinaryOperator::Equal {
            left == right
        } else {
            left != right
        }));
    }
    if let (Some(left), Some(right)) = (left.as_decimal(), right.as_decimal()) {
        return numeric::decimals::binary(operator, left, right, span);
    }
    if left.kind() == ValueType::Float || right.kind() == ValueType::Float {
        let left = left.require_float(span)?;
        let right = right.require_float(span)?;
        return match operator {
            BinaryOperator::Add => finite(left + right, span),
            BinaryOperator::Subtract => finite(left - right, span),
            BinaryOperator::Multiply => finite(left * right, span),
            BinaryOperator::Divide => {
                if right == 0.0 {
                    return Err(Diagnostic::new(
                        DiagnosticKind::DivisionByZero,
                        span,
                        "division by zero",
                    )
                    .with_help("the float divisor must be nonzero, including signed zero"));
                }
                finite(left / right, span)
            }
            BinaryOperator::Less => Ok(Value::Boolean(left < right)),
            BinaryOperator::LessEqual => Ok(Value::Boolean(left <= right)),
            BinaryOperator::Greater => Ok(Value::Boolean(left > right)),
            BinaryOperator::GreaterEqual => Ok(Value::Boolean(left >= right)),
            _ => Err(type_mismatch(span, ValueType::Integer, ValueType::Float)),
        };
    }
    let left = left.require_integer(span)?;
    let right = right.require_integer(span)?;
    let result = match operator {
        BinaryOperator::BitAnd => return Ok(Value::Integer(left & right)),
        BinaryOperator::BitOr => return Ok(Value::Integer(left | right)),
        BinaryOperator::BitXor => return Ok(Value::Integer(left ^ right)),
        BinaryOperator::ShiftLeft | BinaryOperator::ShiftRight | BinaryOperator::UnsignedRight => {
            if !(0..64).contains(&right) {
                return Err(Diagnostic::new(
                    DiagnosticKind::InvalidShift,
                    span,
                    "shift count must be between 0 and 63",
                ).with_help("supply an integer count from 0 through 63; shifts use a fixed 64-bit bit pattern"));
            }
            return Ok(Value::Integer(match operator {
                BinaryOperator::ShiftLeft => left.wrapping_shl(right as u32),
                BinaryOperator::ShiftRight => left >> right,
                _ => ((left as u64) >> right) as i64,
            }));
        }
        BinaryOperator::Add => left.checked_add(right),
        BinaryOperator::Subtract => left.checked_sub(right),
        BinaryOperator::Multiply => left.checked_mul(right),
        BinaryOperator::Divide | BinaryOperator::Remainder => {
            if right == 0 {
                return Err(Diagnostic::new(
                    DiagnosticKind::DivisionByZero,
                    span,
                    "division by zero",
                )
                .with_help("the divisor must evaluate to a nonzero integer"));
            }
            if operator == BinaryOperator::Divide {
                left.checked_div(right)
            } else {
                left.checked_rem(right)
            }
        }
        BinaryOperator::Less => return Ok(Value::Boolean(left < right)),
        BinaryOperator::LessEqual => return Ok(Value::Boolean(left <= right)),
        BinaryOperator::Greater => return Ok(Value::Boolean(left > right)),
        BinaryOperator::GreaterEqual => return Ok(Value::Boolean(left >= right)),
        BinaryOperator::Equal | BinaryOperator::NotEqual => unreachable!("equality handled above"),
    };
    result.map(Value::Integer).ok_or_else(|| overflow(span))
}

fn snapshot_array(
    array: &ArrayValue,
    budget: &mut Budget,
    span: Span,
    edit: impl FnOnce(&mut [Value]),
) -> Result<Value, Diagnostic> {
    let mut elements = reserve_array_snapshot(array, array.len(), budget, span)?;
    elements.extend_from_slice(array.elements());
    edit(&mut elements);
    Ok(Value::Array(ArrayValue::from_checked(
        array.kind().clone(),
        elements,
    )))
}

fn reserve_array_snapshot(
    array: &ArrayValue,
    length: usize,
    budget: &mut Budget,
    span: Span,
) -> Result<Vec<Value>, Diagnostic> {
    // Each snapshot owns a new immutable backing. Charge work and the shared
    // array/list/map allocation quota before attempting a fallible reservation.
    budget.charge((length + array.kind().node_count() + 1) as u64, span)?;
    let bytes = ArrayValue::allocation_bytes(array.element_type(), length).ok_or_else(|| {
        Diagnostic::new(
            DiagnosticKind::ArrayAllocation,
            span,
            "array snapshot allocation size overflow",
        )
    })?;
    budget.charge_array_allocation(bytes, span)?;
    let mut elements = Vec::new();
    elements.try_reserve_exact(length).map_err(|_| {
        Diagnostic::new(
            DiagnosticKind::ArrayAllocation,
            span,
            "could not reserve new immutable array storage",
        )
    })?;
    Ok(elements)
}

fn finish_tuple(
    values: &mut Vec<Value>,
    count: usize,
    budget: &mut Budget,
    span: Span,
) -> Result<(), Diagnostic> {
    let start = values.len() - count;
    let mut elements = reserve_tuple(&values[start..], budget, span)?;
    elements.extend(values.drain(start..));
    values.push(checked_tuple(elements, span)?);
    Ok(())
}

pub(crate) fn make_tuple<const N: usize>(
    values: [Value; N],
    budget: &mut Budget,
    span: Span,
) -> Result<Value, Diagnostic> {
    let mut elements = reserve_tuple(&values, budget, span)?;
    elements.extend(values);
    checked_tuple(elements, span)
}

fn reserve_tuple(
    values: &[Value],
    budget: &mut Budget,
    span: Span,
) -> Result<Vec<Value>, Diagnostic> {
    budget.charge(values.len() as u64, span)?;
    let bytes = TupleValue::allocation_bytes(values).ok_or_else(|| {
        Diagnostic::new(
            DiagnosticKind::TupleAllocation,
            span,
            "tuple allocation size overflow",
        )
    })?;
    budget.charge_tuple_allocation(bytes, span)?;
    let mut elements = Vec::new();
    elements.try_reserve_exact(values.len()).map_err(|_| {
        Diagnostic::new(
            DiagnosticKind::TupleAllocation,
            span,
            "could not reserve tuple storage",
        )
    })?;
    Ok(elements)
}

fn checked_tuple(elements: Vec<Value>, span: Span) -> Result<Value, Diagnostic> {
    TupleValue::new(elements)
        .map(Value::Tuple)
        .ok_or_else(|| Diagnostic::new(DiagnosticKind::TupleLimit, span, "invalid tuple shape"))
}

fn overflow(span: Span) -> Diagnostic {
    Diagnostic::new(
        DiagnosticKind::ArithmeticOverflow,
        span,
        "integer arithmetic overflow",
    )
    .with_help("each result must fit in a signed 64-bit integer")
}

pub(crate) fn finite(value: f64, span: Span) -> Result<Value, Diagnostic> {
    finite_number(value, span).map(Value::Float)
}

/// Compare numeric tuples from left to right without constructing new values.
/// The resolver checks all leaf types; the finite-float invariant excludes NaN.
/// Tuple nesting is bounded by the source type limit (32).
fn numeric_tuple_cmp(left: &TupleValue, right: &TupleValue) -> Option<std::cmp::Ordering> {
    use std::cmp::Ordering;
    for (a, b) in left.elements().iter().zip(right.elements()) {
        let order = match (a, b) {
            (Value::Integer(a), Value::Integer(b)) => a.cmp(b),
            (Value::FixedInteger(a), Value::FixedInteger(b)) => a.value().cmp(&b.value()),
            (Value::Float(a), Value::Float(b)) => a.get().partial_cmp(&b.get())?,
            (Value::BigInt(a), Value::BigInt(b)) => a.cmp(b),
            (Value::Decimal(a), Value::Decimal(b)) => a.cmp(b),
            (Value::Tuple(a), Value::Tuple(b)) => numeric_tuple_cmp(a, b)?,
            _ => return None,
        };
        if order != Ordering::Equal {
            return Some(order);
        }
    }
    Some(left.len().cmp(&right.len()))
}

#[cfg(test)]
mod workspace_retirement_tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn retirement_drops_values_and_only_keeps_buffers_within_the_cap() {
        let mut workspace = Workspace::default();
        workspace.tasks.reserve_exact(256);
        workspace.values.reserve_exact(8);
        let text: Arc<str> = Arc::from("temporary owned only by the evaluator");
        let weak = Arc::downgrade(&text);
        workspace.values.push(Value::String(text));
        let small = workspace.values.capacity();
        workspace.retire_for_reuse(128);
        assert!(weak.upgrade().is_none());
        assert_eq!(workspace.tasks.capacity(), 0);
        assert_eq!(workspace.values.capacity(), small);
        assert!(workspace.values.is_empty());

        workspace.values.reserve_exact(256);
        workspace.tasks.reserve_exact(8);
        workspace.tasks.push(Task::Visit(0));
        let small = workspace.tasks.capacity();
        workspace.retire_for_reuse(128);
        assert_eq!(workspace.values.capacity(), 0);
        assert_eq!(workspace.tasks.capacity(), small);
        assert!(workspace.tasks.is_empty());
    }
}
