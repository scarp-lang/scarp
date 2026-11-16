//! Primitive evaluation with checked integers and finite binary64 arithmetic.

use crate::{
    ast::{BinaryOperator, Expression, ExpressionKind, LogicalOperator},
    diagnostics::{Diagnostic, DiagnosticKind, Span},
    input::IoBuiltin,
    limits::Budget,
    numeric::{self, Builtin},
    resolver::{self, ListIntrinsic, MapIntrinsic, ResolvedCall, ResolvedExpression},
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
}

enum Task {
    BitNot(Span),
    Vector(usize, bool, Span),
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

impl Workspace {
    pub(crate) fn clear(&mut self) {
        self.values.clear();
        self.tasks.clear();
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
        let Self { values, tasks } = self;
        while let Some(task) = tasks.pop() {
            let span = match &task {
                Task::Visit(index) => expression.syntax.nodes()[*index].span,
                Task::UserCall(_, _, span)
                | Task::Tuple(_, span)
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
                            tasks.push(match &expression.calls[index] {
                                Some(ResolvedCall::Io(input)) => Task::Io(*input, node.span),
                                Some(ResolvedCall::Builtin(builtin)) => {
                                    Task::Builtin(*builtin, index, node.span)
                                }
                                Some(ResolvedCall::BigIntDivMod) => {
                                    Task::BigIntDivMod(index, node.span)
                                }
                                Some(ResolvedCall::List(op)) => {
                                    Task::ListIntrinsic(*op, index, node.span)
                                }
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
                            tasks.push(Task::Negate(*operator_span));
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
                            tasks.push(Task::Binary(*operator, *operator_span));
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
                    let resolver::CompositeOperation::Variant { kind, tag } =
                        &expression.composites[&node]
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
                    let resolver::CompositeOperation::Record { kind, order } =
                        &expression.composites[&node]
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
                    let resolver::CompositeOperation::Field(offset) = expression.composites[&node]
                    else {
                        unreachable!("resolved record field");
                    };
                    let Some(Value::Record(value)) = values.pop() else {
                        unreachable!("field receiver was resolved as a record");
                    };
                    values.push(value.fields()[offset].clone());
                }
                Task::Tuple(count, span) => finish_tuple(values, count, budget, span)?,
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
                        items.extend_from_slice(list.elements());
                        match operation {
                            ListIntrinsic::Push => items.push(values[start + 1].clone()),
                            ListIntrinsic::Set => {
                                items[index.expect("checked list index")] =
                                    values[start + 2].clone();
                            }
                            ListIntrinsic::Remove => {
                                items.remove(index.expect("checked list index"));
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
                    let key_index = if operation == MapIntrinsic::Len {
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
                            entries.extend_from_slice(map.entries());
                            match operation {
                                MapIntrinsic::Remove => {
                                    entries.remove(found.expect("validated map removal index"));
                                }
                                MapIntrinsic::Set => {
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
                    let value = values.pop().unwrap().require_integer(span)?;
                    values.push(Value::Integer(!value));
                }
                Task::Negate(span) => {
                    let value = values.pop().unwrap();
                    values.push(match value {
                        Value::Float(value) => finite(-value.get(), span)?,
                        Value::BigInt(value) => Value::BigInt(value.negated()),
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

pub(crate) fn apply_binary_with_budget(
    operator: BinaryOperator,
    left: &Value,
    right: &Value,
    budget: &mut Budget,
    span: Span,
) -> Result<Value, Diagnostic> {
    if let (Some(left), Some(right)) = (left.as_bigint(), right.as_bigint()) {
        return crate::bigint::binary(operator, left, right, budget, span);
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

fn finish_tuple(
    values: &mut Vec<Value>,
    count: usize,
    budget: &mut Budget,
    span: Span,
) -> Result<(), Diagnostic> {
    budget.charge(count as u64, span)?;
    let start = values.len() - count;
    let bytes = TupleValue::allocation_bytes(&values[start..]).ok_or_else(|| {
        Diagnostic::new(
            DiagnosticKind::TupleAllocation,
            span,
            "tuple allocation size overflow",
        )
    })?;
    budget.charge_tuple_allocation(bytes, span)?;
    let mut elements = Vec::new();
    elements.try_reserve_exact(count).map_err(|_| {
        Diagnostic::new(
            DiagnosticKind::TupleAllocation,
            span,
            "could not reserve tuple storage",
        )
    })?;
    elements.extend(values.drain(start..));
    let tuple = TupleValue::new(elements)
        .ok_or_else(|| Diagnostic::new(DiagnosticKind::TupleLimit, span, "invalid tuple shape"))?;
    values.push(Value::Tuple(tuple));
    Ok(())
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
