//! Experimental bounded scalar reverse-mode automatic differentiation.
//! Graph construction is append-only; each evaluation is an isolated tape.
//! This API is for Rust hosts, not yet source-language automatic differentiation.

use super::{buffer, domain};
use crate::{
    diagnostics::{Diagnostic, DiagnosticKind, Span},
    limits::Budget,
    value::{finite_number, FiniteFloat, MAX_VECTOR_LENGTH},
};
use std::sync::Arc;

/// An opaque graph-local node handle. A handle retains its identity token,
/// preventing a stale handle from being accepted by a later graph.
#[derive(Debug, Clone)]
pub struct NodeId {
    owner: Arc<()>,
    index: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unary {
    Negate,
    Square,
    Exp,
    Log,
    Relu,
    Sigmoid,
    Tanh,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Binary {
    Add,
    Subtract,
    Multiply,
    Divide,
}

#[derive(Debug, Clone, Copy)]
enum Node {
    Input(usize),
    Constant(FiniteFloat),
    Unary(Unary, usize),
    Binary(Binary, usize, usize),
}

/// A bounded append-only graph. Edges always reference earlier nodes.
/// The graph is reusable with new input values; it owns no evaluation tape.
#[derive(Debug)]
pub struct Graph {
    owner: Arc<()>,
    nodes: Vec<Node>,
    max_nodes: usize,
    inputs: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GraphLimits {
    pub max_steps: u64,
    pub max_vector_bytes: usize,
}

/// Evaluation snapshot. Unreachable nodes have a zero gradient and no value.
/// Later graph edits cannot change a snapshot or make new nodes visible in it.
#[derive(Debug)]
pub struct Evaluation {
    owner: Arc<()>,
    output: usize,
    values: Vec<FiniteFloat>,
    gradients: Vec<FiniteFloat>,
    reachable: Vec<bool>,
}

fn span() -> Span {
    Span::default()
}

fn invalid_node() -> Diagnostic {
    Diagnostic::new(
        DiagnosticKind::VectorIndex,
        span(),
        "node does not belong to this computation graph",
    )
}

fn node_limit() -> Diagnostic {
    Diagnostic::new(
        DiagnosticKind::VectorLength,
        span(),
        "computation graph node capacity exceeded",
    )
}

fn checked(value: f64) -> Result<FiniteFloat, Diagnostic> {
    finite_number(value, span())
}

fn sigmoid(x: f64) -> f64 {
    if x >= 0.0 {
        1.0 / (1.0 + (-x).exp())
    } else {
        let e = x.exp();
        e / (1.0 + e)
    }
}

fn unary_value(op: Unary, x: f64) -> Result<FiniteFloat, Diagnostic> {
    let y = match op {
        Unary::Negate => -x,
        Unary::Square => x * x,
        Unary::Exp => x.exp(),
        Unary::Log => {
            if x <= 0.0 {
                return Err(domain(span(), "graph log requires a positive input"));
            }
            x.ln()
        }
        Unary::Relu => {
            if x > 0.0 {
                x
            } else {
                0.0
            }
        }
        Unary::Sigmoid => sigmoid(x),
        Unary::Tanh => x.tanh(),
    };
    checked(y)
}

fn binary_value(op: Binary, a: f64, b: f64) -> Result<FiniteFloat, Diagnostic> {
    let value = match op {
        Binary::Add => a + b,
        Binary::Subtract => a - b,
        Binary::Multiply => a * b,
        Binary::Divide => {
            if b == 0.0 {
                return Err(domain(span(), "graph division by zero"));
            }
            a / b
        }
    };
    checked(value)
}

fn unary_vjp(op: Unary, x: f64, y: f64, g: f64) -> Result<FiniteFloat, Diagnostic> {
    let value = match op {
        Unary::Negate => -g,
        Unary::Square => checked(g * x)?.get() * 2.0,
        Unary::Exp => g * y,
        Unary::Log => g / x,
        Unary::Relu => {
            if x > 0.0 {
                g
            } else {
                0.0
            }
        }
        Unary::Sigmoid => {
            let e = (-x.abs()).exp();
            let d = 1.0 + e;
            g * (e / (d * d))
        }
        Unary::Tanh => {
            let exponent = -2.0 * x.abs();
            let d = 1.0 + exponent.exp();
            g * ((exponent + 2.0 * std::f64::consts::LN_2).exp() / (d * d))
        }
    };
    checked(value)
}

fn binary_vjp(
    op: Binary,
    a: f64,
    b: f64,
    y: f64,
    g: f64,
) -> Result<(FiniteFloat, FiniteFloat), Diagnostic> {
    let (da, db) = match op {
        Binary::Add => (g, g),
        Binary::Subtract => (g, -g),
        Binary::Multiply => (g * b, g * a),
        Binary::Divide => {
            let left = checked(g / b)?.get();
            (left, -left * y)
        }
    };
    Ok((checked(da)?, checked(db)?))
}

fn add_gradient(
    gradients: &mut [FiniteFloat],
    index: usize,
    contribution: FiniteFloat,
) -> Result<(), Diagnostic> {
    gradients[index] = checked(gradients[index].get() + contribution.get())?;
    Ok(())
}

impl Graph {
    /// Reserve graph metadata up front, capped at 65,536 nodes.
    /// This construction limit is separate from evaluation payload budgets.
    pub fn new(max_nodes: usize) -> Result<Self, Diagnostic> {
        if max_nodes > MAX_VECTOR_LENGTH {
            return Err(node_limit());
        }
        let mut nodes = Vec::new();
        nodes.try_reserve_exact(max_nodes).map_err(|_| {
            Diagnostic::new(
                DiagnosticKind::VectorAllocation,
                span(),
                "cannot reserve computation graph metadata",
            )
        })?;
        Ok(Self {
            owner: Arc::new(()),
            nodes,
            max_nodes,
            inputs: 0,
        })
    }

    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    pub fn input_count(&self) -> usize {
        self.inputs
    }

    fn index(&self, node: &NodeId) -> Result<usize, Diagnostic> {
        if Arc::ptr_eq(&self.owner, &node.owner) && node.index < self.nodes.len() {
            Ok(node.index)
        } else {
            Err(invalid_node())
        }
    }

    fn append(&mut self, node: Node) -> Result<NodeId, Diagnostic> {
        if self.nodes.len() == self.max_nodes {
            return Err(node_limit());
        }
        let index = self.nodes.len();
        self.nodes.push(node);
        Ok(NodeId {
            owner: Arc::clone(&self.owner),
            index,
        })
    }

    /// Inputs are supplied in their creation order, even if disconnected.
    pub fn input(&mut self) -> Result<NodeId, Diagnostic> {
        let node = self.append(Node::Input(self.inputs))?;
        self.inputs += 1;
        Ok(node)
    }

    pub fn constant(&mut self, value: f64) -> Result<NodeId, Diagnostic> {
        self.append(Node::Constant(checked(value)?))
    }

    pub fn unary(&mut self, op: Unary, x: &NodeId) -> Result<NodeId, Diagnostic> {
        let x = self.index(x)?;
        self.append(Node::Unary(op, x))
    }

    pub fn binary(&mut self, op: Binary, a: &NodeId, b: &NodeId) -> Result<NodeId, Diagnostic> {
        let a = self.index(a)?;
        let b = self.index(b)?;
        self.append(Node::Binary(op, a, b))
    }

    /// Evaluate one scalar output and its reverse-mode derivatives.
    /// Only ancestors of the output execute; disconnected operations do not.
    /// Work is prepaid as 16 units per graph node plus one per supplied input.
    /// Numerical tape payload is 17 logical bytes per node: two finite
    /// scalar buffers and one reachability byte. Metadata is separately bounded.
    pub fn evaluate(
        &self,
        output: &NodeId,
        inputs: &[f64],
        seed: f64,
        limits: GraphLimits,
    ) -> Result<Evaluation, Diagnostic> {
        let output = self.index(output)?;
        if inputs.len() != self.inputs {
            return Err(Diagnostic::new(
                DiagnosticKind::CallArguments,
                span(),
                "computation graph input count mismatch",
            ));
        }
        let seed = checked(seed)?;
        let n = self.nodes.len();
        let mut budget = Budget::new(limits.max_steps).with_vector_bytes(limits.max_vector_bytes);
        budget.charge(16 * n as u64 + inputs.len() as u64, span())?;
        for &input in inputs {
            checked(input)?;
        }
        let zero = checked(0.0)?;
        let mut reachable = buffer(n, &mut budget, span())?;
        let mut values = buffer(n, &mut budget, span())?;
        let mut gradients = buffer(n, &mut budget, span())?;
        reachable.resize(n, false);
        values.resize(n, zero);
        gradients.resize(n, zero);
        reachable[output] = true;

        // Reverse topological reachability pass, no recursion or edge stack.
        for index in (0..=output).rev() {
            if !reachable[index] {
                continue;
            }
            match self.nodes[index] {
                Node::Unary(_, x) => reachable[x] = true,
                Node::Binary(_, a, b) => {
                    reachable[a] = true;
                    reachable[b] = true;
                }
                Node::Input(_) | Node::Constant(_) => {}
            }
        }
        for (index, &node) in self.nodes.iter().enumerate().take(output + 1) {
            if !reachable[index] {
                continue;
            }
            values[index] = match node {
                Node::Input(slot) => checked(inputs[slot])?,
                Node::Constant(value) => value,
                Node::Unary(op, x) => unary_value(op, values[x].get())?,
                Node::Binary(op, a, b) => binary_value(op, values[a].get(), values[b].get())?,
            };
        }
        gradients[output] = seed;
        for index in (0..=output).rev() {
            if !reachable[index] {
                continue;
            }
            let g = gradients[index].get();
            // A zero cotangent contributes nothing; avoid unnecessary local
            // derivatives that would overflow despite a zero contribution.
            if g == 0.0 {
                continue;
            }
            let y = values[index].get();
            match self.nodes[index] {
                Node::Unary(op, x) => {
                    let contribution = unary_vjp(op, values[x].get(), y, g)?;
                    add_gradient(&mut gradients, x, contribution)?;
                }
                Node::Binary(op, a, b) => {
                    let (da, db) = binary_vjp(op, values[a].get(), values[b].get(), y, g)?;
                    // A repeated edge contributes twice, not once.
                    add_gradient(&mut gradients, a, da)?;
                    add_gradient(&mut gradients, b, db)?;
                }
                Node::Input(_) | Node::Constant(_) => {}
            }
        }
        Ok(Evaluation {
            owner: Arc::clone(&self.owner),
            output,
            values,
            gradients,
            reachable,
        })
    }
}

impl Evaluation {
    fn index(&self, node: &NodeId) -> Option<usize> {
        (Arc::ptr_eq(&self.owner, &node.owner) && node.index < self.values.len())
            .then_some(node.index)
    }

    pub fn output(&self) -> f64 {
        self.values[self.output].get()
    }

    /// None for a foreign, later-added or unreachable node.
    pub fn value(&self, node: &NodeId) -> Option<f64> {
        let index = self.index(node)?;
        self.reachable[index].then(|| self.values[index].get())
    }

    /// Returns zero for a disconnected node in this evaluation snapshot.
    /// Constants also have cotangents, even though they are not parameters.
    pub fn gradient(&self, node: &NodeId) -> Option<f64> {
        self.index(node).map(|index| self.gradients[index].get())
    }
}
