//! The syntax tree for the currently supported language slice.

use crate::decimal::Decimal;
use crate::diagnostics::Span;
use crate::types::SourceType;
use crate::value::FiniteFloat;

mod rebase;
pub(crate) use rebase::relocate;

/// Maximum decoded UTF-8 bytes in a native assertion's static message.
pub const MAX_ASSERT_MESSAGE_BYTES: usize = 256;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Program {
    pub statements: Vec<Statement>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Statement {
    Import {
        path: String,
        path_span: Span,
        alias: String,
        alias_span: Span,
        span: Span,
    },
    Export {
        declaration: Box<Statement>,
        span: Span,
    },
    Function(FunctionDeclaration),
    Record(Box<RecordDeclaration>),
    Variant(Box<VariantDeclaration>),
    Match(Box<MatchStatement>),
    Return {
        value: Option<Expression>,
        span: Span,
    },
    Call {
        expression: Expression,
        span: Span,
    },
    /// A checked boolean condition, optionally with a bounded static message.
    Assert {
        condition: Expression,
        message: Option<String>,
        span: Span,
    },
    Repeat {
        count: Expression,
        index: String,
        index_span: Span,
        statements: Vec<Statement>,
        span: Span,
    },
    While {
        condition: Expression,
        statements: Vec<Statement>,
        span: Span,
    },
    Break {
        span: Span,
    },
    Continue {
        span: Span,
    },
    If {
        branches: Vec<ConditionalBranch>,
        otherwise: Vec<Statement>,
        span: Span,
    },
    Block {
        statements: Vec<Statement>,
        span: Span,
    },
    Println {
        argument: PrintArgument,
        span: Span,
    },
    /// Zero/multiple arguments, a raw write, or explicit stderr output.
    /// Single-argument stdout line forms retain `Println` for compatibility.
    Output {
        arguments: Vec<PrintArgument>,
        target: OutputTarget,
        newline: bool,
        span: Span,
    },
    /// An immutable bind or mutable var declaration.
    Declaration {
        name: String,
        name_span: Span,
        mutable: bool,
        annotation: Option<TypeAnnotation>,
        initializer: Expression,
        /// Question-mark postfix on the initializer: explicitly propagate
        /// absence or failure from a binding inside a compatible funcra.
        propagate: Option<Span>,
        span: Span,
    },
    /// Structural binding from one initializer.
    Destructure {
        pattern: BindingPattern,
        mutable: bool,
        initializer: Expression,
        span: Span,
    },
    Assign {
        name: String,
        name_span: Span,
        /// A source index expression for persistent tuple element replacement.
        /// Only compile-time nonnegative integer literals are currently accepted.
        index: Option<Expression>,
        operator: AssignmentOperator,
        operator_span: Span,
        value: Expression,
        span: Span,
    },
}

/// A binding pattern stored in preorder, with each group followed
/// by its child patterns. The parser validates the shape before construction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BindingPattern {
    pub(crate) nodes: Vec<PatternNode>,
}

impl BindingPattern {
    pub fn nodes(&self) -> &[PatternNode] {
        &self.nodes
    }

    pub(crate) fn bindings(&self) -> impl Iterator<Item = (&str, Span)> {
        self.nodes.iter().filter_map(|node| match &node.kind {
            PatternKind::Name(name) => Some((name.as_str(), node.span)),
            _ => None,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatternNode {
    pub kind: PatternKind,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PatternKind {
    Tuple(usize),
    Array(usize),
    Record(Vec<PatternField>),
    Name(String),
    Discard,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatternField {
    pub name: String,
    pub span: Span,
}

/// Optional explicit binding type, retaining its source token location.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeAnnotation {
    pub kind: crate::types::SourceType,
    pub span: Span,
}

impl Statement {
    pub fn span(&self) -> Span {
        match self {
            Self::Import { span, .. } | Self::Export { span, .. } => *span,
            Self::Function(function) => function.span,
            Self::Record(record) => record.span,
            Self::Variant(variant) => variant.span,
            Self::Match(statement) => statement.span,
            Self::Return { span, .. } | Self::Call { span, .. } | Self::Assert { span, .. } => {
                *span
            }
            Self::Repeat { span, .. }
            | Self::While { span, .. }
            | Self::Break { span }
            | Self::Continue { span }
            | Self::If { span, .. }
            | Self::Block { span, .. }
            | Self::Println { span, .. }
            | Self::Output { span, .. }
            | Self::Declaration { span, .. }
            | Self::Destructure { span, .. }
            | Self::Assign { span, .. } => *span,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordDeclaration {
    pub name: String,
    pub name_span: Span,
    pub fields: Vec<(String, Span, TypeAnnotation)>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VariantDeclaration {
    pub name: String,
    pub name_span: Span,
    pub cases: Vec<VariantCase>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VariantCase {
    pub name: String,
    pub span: Span,
    pub payload: Option<TypeAnnotation>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatchStatement {
    pub subject: Expression,
    pub arms: Vec<MatchArm>,
    pub span: Span,
}

/// An exact scalar case used only by a type-checked match, not a dynamic
/// expression or an unbounded pattern evaluator.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MatchLiteral {
    Integer(i64),
    Text(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatchArm {
    /// The named variant/boolean case or wildcard; empty for scalar literals.
    pub tag: String,
    /// Literal case, if any; matches never interpret a string as an identifier.
    pub literal: Option<MatchLiteral>,
    pub tag_span: Span,
    pub payload: Option<BindingPattern>,
    pub statements: Vec<Statement>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FunctionDeclaration {
    pub name: String,
    pub name_span: Span,
    pub parameters: Vec<Parameter>,
    pub result: crate::types::SourceType,
    pub pure: bool,
    pub contracts: Vec<Contract>,
    pub statements: Vec<Statement>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Parameter {
    pub name: String,
    pub span: Span,
    pub kind: crate::types::SourceType,
    pub default: Option<Expression>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Contract {
    pub postcondition: bool,
    pub expression: Expression,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallArgument {
    pub label: Option<(String, Span)>,
    pub expression: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConditionalBranch {
    pub condition: Expression,
    pub statements: Vec<Statement>,
    /// The branch's brace-delimited body, including both braces.
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssignmentOperator {
    Set,
    Compound(BinaryOperator),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrintArgument {
    String(String),
    Expression(Expression),
}

/// The explicit destination of program output, distinct from diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputTarget {
    Stdout,
    Stderr,
}

/// A flat, postorder syntax tree. Children always precede their parent.
/// Keeping nodes in a vector avoids recursive evaluation and destruction of
/// long left-associative expressions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Expression {
    pub(crate) nodes: Vec<ExpressionNode>,
    pub(crate) root: usize,
}

impl Expression {
    pub fn nodes(&self) -> &[ExpressionNode] {
        &self.nodes
    }

    pub fn root(&self) -> usize {
        self.root
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExpressionNode {
    pub kind: ExpressionKind,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConditionalExpressionBranch {
    /// Index of the boolean condition in the containing expression.
    pub condition: usize,
    /// Index of the result expression in the containing expression.
    pub result: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VariantConstructor {
    pub name: String,
    pub name_span: Span,
    pub tag: String,
    pub tag_span: Span,
    pub payload: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExpressionKind {
    Variant(Box<VariantConstructor>),
    Record {
        name: String,
        name_span: Span,
        fields: Vec<CallArgument>,
    },
    Field {
        receiver: usize,
        name: String,
        name_span: Span,
    },
    Conditional {
        branches: Vec<ConditionalExpressionBranch>,
        otherwise: usize,
    },
    Tuple {
        elements: Vec<usize>,
    },
    Array {
        elements: Vec<usize>,
        /// Present for a typed empty literal, absent for inferred elements.
        empty_element_type: Option<SourceType>,
    },
    /// Homogeneous persistent dynamic-list literal, with typed empty forms.
    List {
        elements: Vec<usize>,
        empty_element_type: Option<SourceType>,
    },
    /// Ordered immutable keyed collection. Each pair is evaluated exactly
    /// once in source order, and stored in canonical key order at runtime.
    Map {
        entries: Vec<(usize, usize)>,
        empty_types: Option<(SourceType, SourceType)>,
    },
    Integers {
        elements: Vec<usize>,
    },
    Vector {
        elements: Vec<usize>,
    },
    Index {
        vector: usize,
        index: usize,
        operator_span: Span,
        /// Original syntax was a nonnegative integer literal (possibly grouped).
        /// Retained across constant folding so tuple projection stays literal-only.
        source_literal: bool,
    },
    UserCall {
        name: String,
        arguments: Vec<CallArgument>,
        function_span: Span,
    },
    Integer(i64),
    Float(FiniteFloat),
    Decimal(Decimal),
    Boolean(bool),
    String(String),
    Name(String),
    Negate {
        operand: usize,
        operator_span: Span,
    },
    BitNot {
        operand: usize,
        operator_span: Span,
    },
    Not {
        operand: usize,
        operator_span: Span,
    },
    Logical {
        left: usize,
        operator: LogicalOperator,
        right: usize,
        operator_span: Span,
    },
    Binary {
        left: usize,
        operator: BinaryOperator,
        right: usize,
        operator_span: Span,
    },
    Group {
        expression: usize,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOperator {
    Add,
    Subtract,
    Multiply,
    Divide,
    Remainder,
    BitAnd,
    BitOr,
    BitXor,
    ShiftLeft,
    ShiftRight,
    UnsignedRight,
    Equal,
    NotEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
}

impl BinaryOperator {
    pub(crate) fn is_comparison(self) -> bool {
        matches!(
            self,
            Self::Equal
                | Self::NotEqual
                | Self::Less
                | Self::LessEqual
                | Self::Greater
                | Self::GreaterEqual
        )
    }
    pub(crate) fn is_integer_only(self) -> bool {
        matches!(
            self,
            Self::Remainder
                | Self::BitAnd
                | Self::BitOr
                | Self::BitXor
                | Self::ShiftLeft
                | Self::ShiftRight
                | Self::UnsignedRight
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogicalOperator {
    And,
    Or,
}
