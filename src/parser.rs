//! Parse declarations, assignments, blocks, and print statements without execution.

use crate::types::{
    SourceType, MAX_ARRAY_LENGTH, MAX_COMPOSITE_DEPTH, MAX_SOURCE_TYPE_NODES, MAX_TUPLE_ARITY,
};
use std::collections::VecDeque;

use crate::{
    ast::{
        AssignmentOperator, BinaryOperator, CallArgument, ConditionalBranch,
        ConditionalExpressionBranch, Contract, Expression, ExpressionKind, ExpressionNode,
        FunctionDeclaration, LogicalOperator, OutputTarget, Parameter, PrintArgument, Program,
        Statement, MAX_ASSERT_MESSAGE_BYTES,
    },
    diagnostics::{Diagnostic, DiagnosticKind, Span},
    lexer::{Token, TokenKind},
    value::{FiniteFloat, ValueType, MAX_VECTOR_LENGTH},
};

/// Maximum expression nesting, excluding the output call itself.
pub const MAX_EXPRESSION_DEPTH: usize = 64;

/// Maximum nested statement blocks, independent of expression parentheses.
pub const MAX_BLOCK_DEPTH: usize = 64;

/// Maximum source-bound names in one flat array destructuring pattern.
pub const MAX_DESTRUCTURE_BINDINGS: usize = 128;

/// Parse a token stream. An omitted EOF token is treated as the end of input.
pub fn parse(tokens: &[Token]) -> Result<Program, Diagnostic> {
    Parser {
        tokens,
        cursor: 0,
        pending: VecDeque::new(),
        type_end: 0,
    }
    .program()
    .map_err(|error| *error.0)
}

// Keep recursive success paths' Result values small. Public diagnostics retain
// their original representation; only failures allocate this internal box.
#[derive(Debug)]
struct ParseError(Box<Diagnostic>);

impl ParseError {
    fn new(kind: DiagnosticKind, span: Span, message: impl Into<String>) -> Self {
        Self(Box::new(Diagnostic::new(kind, span, message)))
    }
    fn with_help(self, help: impl Into<String>) -> Self {
        Self(Box::new((*self.0).with_help(help)))
    }
}

struct Parser<'a> {
    tokens: &'a [Token],
    cursor: usize,
    pending: VecDeque<Token>,
    type_end: usize,
}

impl Parser<'_> {
    fn current(&self) -> Option<&Token> {
        self.pending
            .front()
            .or_else(|| self.tokens.get(self.cursor))
    }

    fn advance(&mut self) {
        if self.pending.pop_front().is_none() {
            self.cursor += 1;
        }
    }
    fn type_close(&mut self) -> Result<Span, ParseError> {
        let token = self
            .current()
            .cloned()
            .ok_or_else(|| self.expected("'>' after a type"))?;
        let remainder = match token.kind {
            TokenKind::Greater => Vec::new(),
            TokenKind::ShiftRight => vec![TokenKind::Greater],
            TokenKind::UnsignedRight => vec![TokenKind::Greater, TokenKind::Greater],
            TokenKind::GreaterEqual => vec![TokenKind::Equal],
            TokenKind::RightEqual => vec![TokenKind::Greater, TokenKind::Equal],
            TokenKind::UnsignedEqual => {
                vec![TokenKind::Greater, TokenKind::Greater, TokenKind::Equal]
            }
            _ => return Err(self.expected("'>' after a type")),
        };
        self.advance();
        for (offset, kind) in remainder.into_iter().enumerate().rev() {
            self.pending.push_front(Token {
                kind,
                span: Span::new(token.span.start + offset + 1, token.span.start + offset + 2),
            });
        }
        self.type_end = token.span.start + 1;
        Ok(Span::new(token.span.start, self.type_end))
    }

    fn expected(&self, expected: &str) -> ParseError {
        let (found, span) = self.current().map_or_else(
            || {
                let end = self.tokens.last().map_or(0, |token| token.span.end);
                ("<eof>".to_string(), Span::new(end, end))
            },
            |token| (token.kind.to_string(), token.span),
        );
        ParseError::new(
            DiagnosticKind::UnexpectedToken,
            span,
            format!("expected {expected}, found {found}"),
        )
        .with_help("use print(\"text\"); or print(1 + 2);")
    }

    fn punctuation(&mut self, kind: TokenKind, label: &str) -> Result<Span, ParseError> {
        if let Some(token) = self.current() {
            if token.kind == kind {
                let span = token.span;
                self.advance();
                return Ok(span);
            }
        }
        Err(self.expected(label))
    }

    fn program(mut self) -> Result<Program, ParseError> {
        Ok(Program {
            statements: self.statement_list(0, false)?,
        })
    }

    // Only compact container state survives recursive block parsing. Leaf
    // declarations/output have larger temporaries and finish in a separate call.
    fn statement_list(
        &mut self,
        depth: usize,
        in_block: bool,
    ) -> Result<Vec<Statement>, ParseError> {
        let mut statements = Vec::new();
        while let Some(token) = self.current() {
            if self.module_start() {
                self.module_into(&mut statements, in_block)?;
                continue;
            }
            match &token.kind {
                TokenKind::Eof => {
                    if in_block {
                        return Err(self
                            .expected("'}'")
                            .with_help("close the statement block with '}'"));
                    }
                    if self.cursor + 1 != self.tokens.len() {
                        return Err(self.expected("the end of the token stream"));
                    }
                    break;
                }
                TokenKind::RightBrace if in_block => break,
                _ if self.container_start() => {
                    statements.push(self.container_statement(depth, in_block)?)
                }
                _ => self.leaf_statements(in_block, &mut statements)?,
            }
        }
        if in_block && self.current().is_none() {
            return Err(self
                .expected("'}'")
                .with_help("close the statement block with '}'"));
        }
        Ok(statements)
    }

    fn container_start(&self) -> bool {
        match self.current().map(|token| &token.kind) {
            Some(TokenKind::LeftBrace) => true,
            Some(TokenKind::Identifier(name)) => match name.as_str() {
                "funcra" | "pure" | "if" | "while" => true,
                "repeat" => !self
                    .tokens
                    .get(self.cursor + 1)
                    .is_some_and(|token| assignment_operator(&token.kind).is_some()),
                _ => false,
            },
            _ => false,
        }
    }

    fn module_start(&self) -> bool {
        (self.keyword("import") || self.keyword("pub"))
            && !self.tokens.get(self.cursor + 1).is_some_and(|token| {
                matches!(token.kind, TokenKind::LeftParen | TokenKind::Dot)
                    || assignment_operator(&token.kind).is_some()
            })
    }

    fn module_statement(&mut self, in_block: bool) -> Result<Statement, ParseError> {
        let start = self.current().unwrap().span;
        if in_block {
            return Err(ParseError::new(
                DiagnosticKind::ModuleSyntax,
                start,
                "imports and public declarations are allowed only at file scope",
            )
            .with_help("move this declaration outside the block"));
        }
        let import = self.keyword("import");
        self.advance();
        if import {
            let (path, path_span) = match self.current() {
                Some(Token {
                    kind: TokenKind::String(path),
                    span,
                }) => (path.clone(), *span),
                _ => {
                    return Err(ParseError::new(
                        DiagnosticKind::ModuleSyntax,
                        start,
                        "import requires a source path string and an explicit alias",
                    )
                    .with_help("use import \"library.srp\" as library;"))
                }
            };
            self.advance();
            if !self.keyword("as") {
                return Err(ParseError::new(
                    DiagnosticKind::ModuleSyntax,
                    path_span,
                    "import requires an explicit as alias",
                )
                .with_help("use import \"library.srp\" as library;"));
            }
            self.advance();
            let (alias, alias_span) = self.identifier()?;
            let end = self.punctuation(TokenKind::Semicolon, "';'")?.end;
            Ok(Statement::Import {
                path,
                path_span,
                alias,
                alias_span,
                span: Span::new(start.start, end),
            })
        } else {
            let declaration = if self.keyword("funcra") || self.keyword("pure") {
                Statement::Function(self.function()?)
            } else if self.keyword("bind") {
                self.declaration()?
            } else {
                return Err(ParseError::new(
                    DiagnosticKind::ModuleSyntax,
                    start,
                    "pub requires a funcra declaration or an immutable scalar bind",
                )
                .with_help("export a function or write pub bind name = scalar_literal;"));
            };
            let end = declaration.span().end;
            Ok(Statement::Export {
                declaration: Box::new(declaration),
                span: Span::new(start.start, end),
            })
        }
    }

    // Keep an extra Statement-sized temporary out of every recursive block frame.
    fn module_into(
        &mut self,
        statements: &mut Vec<Statement>,
        in_block: bool,
    ) -> Result<(), ParseError> {
        statements.push(self.module_statement(in_block)?);
        Ok(())
    }

    fn qualified_identifier(&mut self) -> Result<(String, Span), ParseError> {
        let (mut name, mut span) = self.identifier()?;
        if self.at(&TokenKind::Dot) {
            self.advance();
            let (member, member_span) = self.identifier()?;
            name.push('.');
            name.push_str(&member);
            span.end = member_span.end;
        }
        Ok((name, span))
    }

    // Qualified names add owned text only on this leaf path, not on each nested
    // grouping/call frame in atom.
    fn qualified_atom(
        &mut self,
        nodes: &mut Vec<ExpressionNode>,
        depth: usize,
    ) -> Result<usize, ParseError> {
        if self
            .tokens
            .get(self.cursor + 3)
            .is_some_and(|t| t.kind == TokenKind::LeftParen)
        {
            self.call_expression(nodes, depth)
        } else {
            let (name, span) = self.qualified_identifier()?;
            Ok(push(nodes, ExpressionKind::Name(name), span))
        }
    }

    /// Collapse a consecutive brace-only chain into one physical parse frame.
    ///
    /// Every intermediate block contains exactly one child block and no
    /// sibling statements. Their AST and byte spans remain identical to the
    /// ordinary recursive parser, but 64 nested braces do not retain 64
    /// statement-list frames while parsing a deeply nested expression.
    ///
    /// A preflight of matching braces is required: if a parent has further
    /// statements after the child block, fall back to the general parser.
    fn compact_block_chain(&mut self, depth: usize) -> Result<Option<Statement>, ParseError> {
        if !self.pending.is_empty() {
            return Ok(None);
        }
        let start = self.cursor;
        let mut count = 0usize;
        while matches!(
            self.tokens.get(start + count).map(|t| &t.kind),
            Some(TokenKind::LeftBrace)
        ) {
            count += 1;
            // Stop before bounds that would violate the normal depth limit.
            if count > MAX_BLOCK_DEPTH {
                break;
            }
        }
        if count < 2 {
            return Ok(None);
        }
        let mut balance = 0usize;
        let mut inner_close = None;
        for (position, token) in self.tokens.iter().enumerate().skip(start + count - 1) {
            match token.kind {
                TokenKind::LeftBrace => balance += 1,
                TokenKind::RightBrace => {
                    balance -= 1;
                    if balance == 0 {
                        inner_close = Some(position);
                        break;
                    }
                }
                TokenKind::Eof => return Ok(None),
                _ => {}
            }
        }
        let Some(inner_close) = inner_close else {
            return Ok(None);
        };
        // A string with an unmatched parent block is still handled by the
        // standard parser; optimization does not change syntax diagnostics.
        if (1..count).any(|offset| {
            !matches!(
                self.tokens.get(inner_close + offset).map(|t| &t.kind),
                Some(TokenKind::RightBrace)
            )
        }) {
            return Ok(None);
        }
        // Check the same nesting limit as one-by-one block() calls, before
        // advancing cursor or allocating any AST nodes.
        for offset in 0..count {
            if depth + offset >= MAX_BLOCK_DEPTH {
                let opening = self.tokens[start + offset].span;
                return Err(ParseError::new(
                    DiagnosticKind::BlockNesting,
                    opening,
                    format!("block nesting exceeds {MAX_BLOCK_DEPTH} levels"),
                )
                .with_help("reduce the number of nested statement blocks"));
            }
        }
        self.cursor += count;
        let mut body = self.statement_list(depth + count, true)?;
        for offset in (0..count).rev() {
            let end = self.punctuation(TokenKind::RightBrace, "'}'")?.end;
            let span = Span::new(self.tokens[start + offset].span.start, end);
            let node = Statement::Block {
                statements: body,
                span,
            };
            body = vec![node];
        }
        Ok(body.pop())
    }

    fn container_statement(
        &mut self,
        depth: usize,
        in_block: bool,
    ) -> Result<Statement, ParseError> {
        if let Some(compact) = self.compact_block_chain(depth)? {
            return Ok(compact);
        }
        match &self.current().expect("container start").kind {
            TokenKind::LeftBrace => {
                let (statements, span) = self.block(depth)?;
                Ok(Statement::Block { statements, span })
            }
            TokenKind::Identifier(name) if name == "funcra" || name == "pure" => {
                if in_block {
                    return Err(ParseError::new(
                        DiagnosticKind::FunctionUnsupported,
                        self.current().unwrap().span,
                        "nested function declarations are not supported",
                    ));
                }
                Ok(Statement::Function(self.function()?))
            }
            TokenKind::Identifier(name) if name == "if" => self.conditional(depth),
            TokenKind::Identifier(name) if name == "repeat" => self.repeat_loop(depth),
            TokenKind::Identifier(name) if name == "while" => self.while_loop(depth),
            _ => unreachable!("checked container start"),
        }
    }

    fn repeat_loop(&mut self, depth: usize) -> Result<Statement, ParseError> {
        let start = self.current().unwrap().span.start;
        self.advance();
        let mut nodes = Vec::new();
        let root = self.logical_or(&mut nodes, 0)?;
        if !self.keyword("as") {
            return Err(self
                .expected("as followed by an index name")
                .with_help("use repeat count as index { statements }"));
        }
        self.advance();
        let (index, index_span) = match self.current() {
            Some(Token {
                kind: TokenKind::Identifier(name),
                span,
            }) => (name.clone(), *span),
            _ => {
                return Err(self
                    .expected("an immutable repeat index name")
                    .with_help("use repeat count as index { statements }"))
            }
        };
        self.advance();
        if !matches!(self.current().map(|t| &t.kind), Some(TokenKind::LeftBrace)) {
            return Err(self.expected("'{'").with_help(
                "enclose the repeat body in braces: repeat count as index { statements }",
            ));
        }
        let (body, body_span) = self.block(depth)?;
        Ok(Statement::Repeat {
            count: Expression { nodes, root },
            index,
            index_span,
            statements: body,
            span: Span::new(start, body_span.end),
        })
    }

    fn while_loop(&mut self, depth: usize) -> Result<Statement, ParseError> {
        let start = self.current().unwrap().span.start;
        self.advance();
        let mut nodes = Vec::new();
        let root = self.logical_or(&mut nodes, 0).map_err(|error| {
            if error.0.kind == DiagnosticKind::UnexpectedToken {
                error.with_help("use while boolean_condition { statements }")
            } else {
                error
            }
        })?;
        if !matches!(self.current().map(|t| &t.kind), Some(TokenKind::LeftBrace)) {
            return Err(self
                .expected("'{'")
                .with_help("enclose the while body in braces: while condition { statements }"));
        }
        let (body, span) = self.block(depth)?;
        Ok(Statement::While {
            condition: Expression { nodes, root },
            statements: body,
            span: Span::new(start, span.end),
        })
    }

    fn leaf_statements(
        &mut self,
        in_block: bool,
        statements: &mut Vec<Statement>,
    ) -> Result<(), ParseError> {
        while let Some(token) = self.current() {
            if self.container_start() || self.module_start() {
                break;
            }
            match &token.kind {
                TokenKind::Eof => {
                    if in_block {
                        return Err(self
                            .expected("'}'")
                            .with_help("close the statement block with '}'"));
                    }
                    if self.cursor + 1 != self.tokens.len() {
                        return Err(self.expected("the end of the token stream"));
                    }
                    break;
                }
                TokenKind::RightBrace if in_block => break,
                TokenKind::Identifier(name) if name == "return" => {
                    let start = token.span.start;
                    self.advance();
                    let value = if self.at(&TokenKind::Semicolon) {
                        None
                    } else {
                        Some(self.expression()?)
                    };
                    let end = self.punctuation(TokenKind::Semicolon, "';'")?.end;
                    statements.push(Statement::Return {
                        value,
                        span: Span::new(start, end),
                    });
                }
                TokenKind::Identifier(name) if name == "break" || name == "continue" => {
                    let start = token.span.start;
                    let is_break = name == "break";
                    self.advance();
                    let end = self.punctuation(TokenKind::Semicolon, "';'")?.end;
                    let span = Span::new(start, end);
                    statements.push(if is_break {
                        Statement::Break { span }
                    } else {
                        Statement::Continue { span }
                    });
                }
                TokenKind::Identifier(name)
                    if matches!(
                        name.as_str(),
                        "print" | "println" | "write" | "eprint" | "ewrite"
                    ) =>
                {
                    statements.push(self.output_statement()?);
                }
                TokenKind::Identifier(name) if name == "assert" => {
                    statements.push(self.assertion()?);
                }
                TokenKind::Identifier(name) if name == "let" => {
                    let modifier = self.tokens.get(self.cursor + 1).filter(
                        |next| matches!(&next.kind, TokenKind::Identifier(name) if name == "mut"),
                    );
                    let (span, old, replacement) = if let Some(modifier) = modifier {
                        (
                            Span::new(token.span.start, modifier.span.end),
                            "let mut",
                            "var",
                        )
                    } else {
                        (token.span, "let", "bind")
                    };
                    return Err(ParseError::new(
                        DiagnosticKind::UnknownStatement,
                        span,
                        format!("legacy declaration {old:?} is not supported"),
                    )
                    .with_help(format!(
                        "replace {old:?} with {replacement:?}: {replacement} name = expression;"
                    )));
                }
                TokenKind::Identifier(name) if name == "bind" || name == "var" => {
                    statements.push(self.declaration()?);
                }
                TokenKind::Identifier(name) => {
                    if self
                        .tokens
                        .get(self.cursor + 1)
                        .is_some_and(|t| t.kind == TokenKind::Dot)
                        && self
                            .tokens
                            .get(self.cursor + 3)
                            .is_some_and(|t| assignment_operator(&t.kind).is_some())
                    {
                        let (_, span) = self.qualified_identifier()?;
                        return Err(ParseError::new(
                            DiagnosticKind::ModuleSyntax,
                            span,
                            "imported module members are not assignment targets",
                        )
                        .with_help(
                            "read the exported value, or copy it into a local var before assigning",
                        ));
                    }
                    if self
                        .tokens
                        .get(self.cursor + 1)
                        .is_some_and(|t| matches!(t.kind, TokenKind::LeftParen | TokenKind::Dot))
                    {
                        let start = token.span.start;
                        let expression = self.expression()?;
                        if !matches!(
                            expression.nodes[expression.root].kind,
                            ExpressionKind::UserCall { .. }
                        ) {
                            return Err(self.expected("a standalone function call"));
                        }
                        let end = self.punctuation(TokenKind::Semicolon, "';'")?.end;
                        statements.push(Statement::Call {
                            expression,
                            span: Span::new(start, end),
                        });
                        continue;
                    }

                    if let Some(operator) = self
                        .tokens
                        .get(self.cursor + 1)
                        .and_then(|token| assignment_operator(&token.kind))
                    {
                        statements.push(self.assignment(operator)?);
                        continue;
                    }
                    return Err(ParseError::new(
                        DiagnosticKind::UnknownStatement,
                        token.span,
                        format!("unknown statement {name:?}"),
                    )
                    .with_help(
                        "use bind, var, print, write, or an assignment to a declared mutable variable",
                    ));
                }
                _ => return Err(self.expected("an output statement or binding declaration")),
            }
        }
        if in_block && self.current().is_none() {
            return Err(self
                .expected("'}'")
                .with_help("close the statement block with '}'"));
        }
        Ok(())
    }

    /// Parse flat structural bindings: bind (a, b) = tuple; or
    /// var [x, _, z] = fixed_array; Nested patterns remain future work.
    fn destructure(&mut self, start: usize, mutable: bool) -> Result<Statement, ParseError> {
        let array = self.at(&TokenKind::LeftBracket);
        let close = if array {
            TokenKind::RightBracket
        } else {
            TokenKind::RightParen
        };
        let opener = self.current().unwrap().span;
        self.advance();
        let mut bindings = Vec::new();
        if self.at(&close) {
            return Err(ParseError::new(
                DiagnosticKind::UnexpectedToken,
                opener,
                "a destructuring pattern requires at least one name",
            )
            .with_help("use bind (first, second) = tuple; or bind [item] = array;"));
        }
        let trailing_comma = loop {
            let (name, span) = self.identifier().map_err(|error| {
                error.with_help("destructuring accepts names or '_' placeholders, not expressions")
            })?;
            let max = if array {
                MAX_DESTRUCTURE_BINDINGS
            } else {
                MAX_TUPLE_ARITY
            };
            if bindings.len() >= max {
                return Err(ParseError::new(
                    if array {
                        DiagnosticKind::ArrayLimit
                    } else {
                        DiagnosticKind::TupleLimit
                    },
                    span,
                    format!("destructuring pattern exceeds {max} elements"),
                ));
            }
            bindings.push((name, span));
            if !self.at(&TokenKind::Comma) {
                break false;
            }
            self.advance();
            if self.at(&close) {
                break true;
            }
        };
        let close_label = if array { "']'" } else { "')'" };
        self.punctuation(close, close_label)?;
        if !array && bindings.len() == 1 && !trailing_comma {
            return Err(ParseError::new(
                DiagnosticKind::UnexpectedToken,
                opener,
                "a one-element tuple pattern requires a trailing comma",
            )
            .with_help("write bind (value,) = tuple;"));
        }
        self.punctuation(TokenKind::Equal, "'=' after the destructuring pattern")?;
        let initializer = self.expression()?;
        let end = self
            .punctuation(TokenKind::Semicolon, "';' after destructuring")
            .map_err(|error| error.with_help("finish the destructuring declaration with ';'"))?
            .end;
        Ok(Statement::Destructure {
            bindings,
            mutable,
            array,
            initializer,
            span: Span::new(start, end),
        })
    }

    // Keep declaration temporaries out of recursive statement-list frames.
    fn declaration(&mut self) -> Result<Statement, ParseError> {
        let token = self.current().expect("declaration keyword is present");
        let TokenKind::Identifier(name) = &token.kind else {
            unreachable!()
        };
        let start = token.span.start;
        let mutable = name == "var";
        let keyword = if mutable { "var" } else { "bind" };
        self.advance();
        if self.at(&TokenKind::LeftParen) || self.at(&TokenKind::LeftBracket) {
            return self.destructure(start, mutable);
        }
        if self.keyword("mut")
            && matches!(
                self.tokens.get(self.cursor + 1).map(|token| &token.kind),
                Some(TokenKind::Identifier(_))
            )
        {
            return Err(self
                .expected("a binding name followed by '='")
                .with_help("use var name = expression; for mutation; remove mut"));
        }
        let (name, name_span) = match self.current() {
            Some(Token {
                kind: TokenKind::Identifier(name),
                span,
            }) => (name.clone(), *span),
            _ => {
                return Err(self
                    .expected("a binding name")
                    .with_help(format!("use {keyword} name = expression;")))
            }
        };
        self.advance();
        let annotation = if self.at(&TokenKind::Colon) {
            self.advance();
            let span = self
                .current()
                .map_or_else(Span::default, |token| token.span);
            let kind = self.value_type(false).map_err(|mut error| {
                if error.0.kind == DiagnosticKind::FunctionDefinition {
                    error.0.kind = DiagnosticKind::UnexpectedToken;
                    error.0.message = error.0.message.replace("parameter", "binding");
                }
                error.with_help("binding types are integer, float, bigint, vector, integers, tile, boolean or string; an initializer is required")
            })?;
            Some(crate::ast::TypeAnnotation {
                kind,
                span: Span::new(span.start, self.type_end),
            })
        } else {
            None
        };
        self.punctuation(TokenKind::Equal, "'='").map_err(|error|
            error.with_help("declarations require an initializer: bind name = expression; or var name = expression;"))?;
        let mut nodes = Vec::new();
        let root = self.logical_or(&mut nodes, 0).map_err(|error| {
            if error.0.kind == DiagnosticKind::UnexpectedToken {
                error.with_help(format!(
                    "provide an initializer after '=': {keyword} name = expression;"
                ))
            } else {
                error
            }
        })?;
        let end = self
            .punctuation(TokenKind::Semicolon, "';'")
            .map_err(|error| error.with_help(format!("end the {keyword} declaration with ';'")))?
            .end;
        Ok(Statement::Declaration {
            name,
            name_span,
            mutable,
            annotation,
            initializer: Expression { nodes, root },
            span: Span::new(start, end),
        })
    }

    fn assignment(&mut self, operator: AssignmentOperator) -> Result<Statement, ParseError> {
        let token = self.current().expect("assignment name is present");
        let TokenKind::Identifier(name) = &token.kind else {
            unreachable!()
        };
        let name = name.clone();
        let name_span = token.span;
        let operator_span = self.tokens[self.cursor + 1].span;
        self.advance();
        self.advance();
        let mut nodes = Vec::new();
        let root = self.logical_or(&mut nodes, 0).map_err(|error| {
            if error.0.kind == DiagnosticKind::UnexpectedToken {
                error.with_help("provide a value expression after the assignment operator")
            } else {
                error
            }
        })?;
        let end = self
            .punctuation(TokenKind::Semicolon, "';'")
            .map_err(|error| error.with_help("end the assignment with ';'"))?
            .end;
        Ok(Statement::Assign {
            name,
            name_span,
            operator,
            operator_span,
            value: Expression { nodes, root },
            span: Span::new(name_span.start, end),
        })
    }

    fn keyword(&self, expected: &str) -> bool {
        matches!(self.current().map(|token| &token.kind), Some(TokenKind::Identifier(name)) if name == expected)
    }

    fn at(&self, kind: &TokenKind) -> bool {
        self.current().is_some_and(|token| &token.kind == kind)
    }

    fn expression(&mut self) -> Result<Expression, ParseError> {
        let mut nodes = Vec::new();
        let root = self.logical_or(&mut nodes, 0)?;
        Ok(Expression { nodes, root })
    }

    fn identifier(&mut self) -> Result<(String, Span), ParseError> {
        match self.current() {
            Some(Token {
                kind: TokenKind::Identifier(name),
                span,
            }) => {
                let result = (name.clone(), *span);
                self.advance();
                Ok(result)
            }
            _ => Err(self.expected("an identifier")),
        }
    }

    fn value_type(&mut self, result: bool) -> Result<SourceType, ParseError> {
        let mut remaining = MAX_SOURCE_TYPE_NODES;
        self.source_type(result, 0, &mut remaining, false)
    }

    fn source_type(
        &mut self,
        result: bool,
        depth: usize,
        remaining: &mut usize,
        inside_array: bool,
    ) -> Result<SourceType, ParseError> {
        let (name, span) = self.identifier()?;
        self.type_end = span.end;
        let inside_array = inside_array || name == "array";
        let limit = if inside_array {
            DiagnosticKind::ArrayLimit
        } else {
            DiagnosticKind::TupleLimit
        };
        *remaining = remaining
            .checked_sub(1)
            .ok_or_else(|| ParseError::new(limit, span, "type exceeds 4096 structural nodes"))?;
        if matches!(name.as_str(), "tuple" | "array") && depth >= MAX_COMPOSITE_DEPTH {
            return Err(ParseError::new(
                limit,
                span,
                "composite type nesting exceeds 32 levels",
            ));
        }
        if name == "array" {
            self.punctuation(TokenKind::Less, "'<' after array")?;
            let element = self.source_type(false, depth + 1, remaining, true)?;
            self.punctuation(TokenKind::Comma, "',' before the array length")?;
            let length = self.array_length()?;
            let end = self.type_close()?.end;
            return SourceType::array(element, length).ok_or_else(|| {
                ParseError::new(
                    DiagnosticKind::ArrayLimit,
                    Span::new(span.start, end),
                    "array type exceeds shape limits",
                )
            });
        }
        if name == "tuple" {
            self.punctuation(TokenKind::Less, "'<' after tuple")?;
            let mut elements = Vec::new();
            loop {
                if elements.len() >= MAX_TUPLE_ARITY {
                    return Err(ParseError::new(
                        DiagnosticKind::TupleLimit,
                        span,
                        "tuple type arity exceeds 32",
                    ));
                }
                elements.push(self.source_type(false, depth + 1, remaining, inside_array)?);
                if !self.at(&TokenKind::Comma) {
                    break;
                }
                self.advance();
                if self.current().is_some_and(|t| is_type_close(&t.kind)) {
                    break;
                }
            }
            let end = self.type_close()?.end;
            let limit = if elements.iter().any(SourceType::contains_array) {
                DiagnosticKind::ArrayLimit
            } else {
                limit
            };
            return SourceType::tuple(elements).ok_or_else(|| {
                ParseError::new(
                    limit,
                    Span::new(span.start, end),
                    "tuple type exceeds shape limits",
                )
            });
        }
        if let Some(kind) =
            ValueType::from_name(&name).filter(|kind| result || *kind != ValueType::Unit)
        {
            return Ok(kind.into());
        }
        match name.as_str() {
            "funcra" | "pure" => Err(ParseError::new(
                DiagnosticKind::FunctionUnsupported,
                span,
                "function types and first-class functions are not supported yet",
            )),
            _ => Err(ParseError::new(
                DiagnosticKind::FunctionDefinition,
                span,
                format!(
                    "invalid {} type {name:?}",
                    if result { "result" } else { "parameter" }
                ),
            )
            .with_help(
                "use integer, float, bigint, vector, integers, tile, boolean, string, or unit for a result",
            )),
        }
    }

    fn array_length(&mut self) -> Result<usize, ParseError> {
        let Some(Token {
            kind: TokenKind::Integer(digits),
            span,
        }) = self.current()
        else {
            return Err(self.array_length_error(self.current().map(|token| token.span)));
        };
        let mut span = *span;
        let mut length = digits.parse::<usize>().ok();
        self.advance();
        // The ordinary decimal lexer deliberately leaves separators unsupported.
        // Join only an immediately adjacent suffix in this type position.
        if let Some(Token {
            kind: TokenKind::Identifier(suffix),
            span: suffix_span,
        }) = self.current()
        {
            if span.end == suffix_span.start && suffix.starts_with('_') {
                span.end = suffix_span.end;
                length = length.and_then(|initial| {
                    suffix[1..].split('_').try_fold(initial, |n, group| {
                        if group.is_empty() {
                            return None;
                        }
                        group.bytes().try_fold(n, |n, digit| {
                            if !digit.is_ascii_digit() {
                                return None;
                            }
                            n.checked_mul(10)?.checked_add(usize::from(digit - b'0'))
                        })
                    })
                });
                self.advance();
            }
        }
        if !self
            .current()
            .is_some_and(|token| is_type_close(&token.kind))
        {
            return Err(self.array_length_error(Some(span)));
        }
        length
            .filter(|&n| n <= MAX_ARRAY_LENGTH)
            .ok_or_else(|| self.array_length_error(Some(span)))
    }

    fn array_length_error(&self, span: Option<Span>) -> ParseError {
        let mut span = span.unwrap_or_else(|| {
            let end = self.tokens.last().map_or(0, |token| token.span.end);
            Span::new(end, end)
        });
        if self.pending.is_empty() {
            for token in &self.tokens[self.cursor..] {
                if is_type_close(&token.kind)
                    || matches!(
                        token.kind,
                        TokenKind::Comma
                            | TokenKind::RightBracket
                            | TokenKind::RightParen
                            | TokenKind::Semicolon
                            | TokenKind::Equal
                            | TokenKind::Eof
                    )
                {
                    break;
                }
                span.end = token.span.end;
            }
        }
        ParseError::new(
            DiagnosticKind::ArrayLimit,
            span,
            "array length must be a decimal integer literal from 0 through 4096",
        )
        .with_help("write a nonnegative base-10 length, such as array<integer, 2>")
    }

    fn function(&mut self) -> Result<FunctionDeclaration, ParseError> {
        let start = self.current().unwrap().span.start;
        let pure = self.keyword("pure");
        if pure {
            self.advance();
        }
        if !self.keyword("funcra") {
            return Err(self.expected("funcra"));
        }
        self.advance();
        let (name, name_span) = self.identifier()?;
        self.punctuation(TokenKind::LeftParen, "'('")?;
        let mut parameters = Vec::new();
        while !self.at(&TokenKind::RightParen) {
            let (name, span) = self.identifier()?;
            self.punctuation(TokenKind::Colon, "':' before the parameter type")?;
            let kind = self.value_type(false)?;
            let default = if self.at(&TokenKind::Equal) {
                self.advance();
                Some(self.expression()?)
            } else {
                None
            };
            parameters.push(Parameter {
                name,
                span,
                kind,
                default,
            });
            if !self.at(&TokenKind::Comma) {
                break;
            }
            self.advance();
        }
        self.punctuation(TokenKind::RightParen, "')'")?;
        self.punctuation(TokenKind::Arrow, "'->' before the result type")?;
        let result = self.value_type(true)?;
        let mut contracts = Vec::new();
        while self.keyword("needs") || self.keyword("ensures") {
            let start = self.current().unwrap().span.start;
            let postcondition = self.keyword("ensures");
            self.advance();
            let expression = self.expression()?;
            let end = self
                .punctuation(TokenKind::Semicolon, "';' after the contract")?
                .end;
            contracts.push(Contract {
                postcondition,
                expression,
                span: Span::new(start, end),
            });
        }
        let (statements, body) = self.block(0)?;
        Ok(FunctionDeclaration {
            name,
            name_span,
            parameters,
            result,
            pure,
            contracts,
            statements,
            span: Span::new(start, body.end),
        })
    }

    /// `assert(boolean_expression);` or `assert(condition, "message");`.
    /// The message is static, bounded and never treated as executable code.
    fn assertion(&mut self) -> Result<Statement, ParseError> {
        let start = self
            .current()
            .expect("assert keyword is present")
            .span
            .start;
        self.advance();
        self.punctuation(TokenKind::LeftParen, "'(' after assert")?;
        let condition = self.expression()?;
        let message = if self.at(&TokenKind::Comma) {
            self.advance();
            let text = match self.current() {
                Some(Token {
                    kind: TokenKind::String(text),
                    ..
                }) => text.clone(),
                _ => return Err(self.expected("a quoted assertion message")),
            };
            if text.len() > MAX_ASSERT_MESSAGE_BYTES {
                return Err(ParseError::new(
                    DiagnosticKind::CallArguments,
                    self.current().expect("quoted message is present").span,
                    format!("assertion message exceeds {MAX_ASSERT_MESSAGE_BYTES} UTF-8 bytes"),
                ));
            }
            self.advance();
            Some(text)
        } else {
            None
        };
        self.punctuation(TokenKind::RightParen, "')' after assert")?;
        let end = self
            .punctuation(TokenKind::Semicolon, "';' after assert")?
            .end;
        Ok(Statement::Assert {
            condition,
            message,
            span: Span::new(start, end),
        })
    }

    fn output_statement(&mut self) -> Result<Statement, ParseError> {
        let token = self.current().expect("output keyword is present");
        let TokenKind::Identifier(name) = &token.kind else {
            unreachable!()
        };
        let start = token.span.start;
        let target = if name.starts_with('e') {
            OutputTarget::Stderr
        } else {
            OutputTarget::Stdout
        };
        let newline = name != "write" && name != "ewrite";
        self.advance();
        self.punctuation(TokenKind::LeftParen, "'('")?;
        let mut arguments = Vec::new();
        // Keep ordinary one-argument line output free of list allocations.
        let mut first_argument = None;
        if !matches!(
            self.current().map(|token| &token.kind),
            Some(TokenKind::RightParen)
        ) {
            loop {
                let direct_string = self.tokens.get(self.cursor + 1).is_some_and(|next| {
                    matches!(next.kind, TokenKind::Comma | TokenKind::RightParen)
                });
                let argument = if let Some(Token {
                    kind: TokenKind::String(value),
                    ..
                }) = self.current().filter(|_| direct_string)
                {
                    let argument = PrintArgument::String(value.clone());
                    self.advance();
                    argument
                } else {
                    if !matches!(
                        self.current().map(|token| &token.kind),
                        Some(
                            TokenKind::Integer(_)
                                | TokenKind::Float(_)
                                | TokenKind::String(_)
                                | TokenKind::Identifier(_)
                                | TokenKind::BitNot
                                | TokenKind::Minus
                                | TokenKind::LeftParen
                                | TokenKind::LeftBracket
                        )
                    ) {
                        return Err(self.expected("a string literal or value expression"));
                    }
                    let mut nodes = Vec::new();
                    let root = self.logical_or(&mut nodes, 0)?;
                    PrintArgument::Expression(Expression { nodes, root })
                };
                if arguments.is_empty() && first_argument.is_none() {
                    first_argument = Some(argument);
                } else {
                    if let Some(first) = first_argument.take() {
                        arguments.push(first);
                    }
                    arguments.push(argument);
                }
                if !matches!(
                    self.current().map(|token| &token.kind),
                    Some(TokenKind::Comma)
                ) {
                    break;
                }
                self.advance();
                if matches!(
                    self.current().map(|token| &token.kind),
                    Some(TokenKind::RightParen)
                ) {
                    break;
                }
            }
        }
        self.punctuation(TokenKind::RightParen, "')'")?;
        let end = self.punctuation(TokenKind::Semicolon, "';'")?.end;
        let span = Span::new(start, end);
        if target == OutputTarget::Stdout && newline && arguments.is_empty() {
            if let Some(argument) = first_argument.take() {
                return Ok(Statement::Println { argument, span });
            }
        }
        if let Some(argument) = first_argument {
            arguments.push(argument);
        }
        Ok(Statement::Output {
            arguments,
            target,
            newline,
            span,
        })
    }

    fn block(&mut self, depth: usize) -> Result<(Vec<Statement>, Span), ParseError> {
        let opening = self.punctuation(TokenKind::LeftBrace, "'{'")?;
        if depth >= MAX_BLOCK_DEPTH {
            return Err(ParseError::new(
                DiagnosticKind::BlockNesting,
                opening,
                format!("block nesting exceeds {MAX_BLOCK_DEPTH} levels"),
            )
            .with_help("reduce the number of nested statement blocks"));
        }
        let inner = self.statement_list(depth + 1, true)?;
        let end = self.punctuation(TokenKind::RightBrace, "'}'")?.end;
        Ok((inner, Span::new(opening.start, end)))
    }

    fn conditional(&mut self, depth: usize) -> Result<Statement, ParseError> {
        let start = self.current().unwrap().span.start;
        let mut branches = Vec::new();
        let mut end;
        loop {
            self.advance(); // if or elif
            let mut nodes = Vec::new();
            let root = self.logical_or(&mut nodes, 0)?;
            let (statements, span) = self.block(depth)?;
            end = span.end;
            branches.push(ConditionalBranch {
                condition: Expression { nodes, root },
                statements,
                span,
            });
            if !self.keyword("elif") {
                break;
            }
        }
        let otherwise = if self.keyword("else") {
            self.advance();
            let (statements, span) = self.block(depth)?;
            end = span.end;
            statements
        } else {
            Vec::new()
        };
        Ok(Statement::If {
            branches,
            otherwise,
            span: Span::new(start, end),
        })
    }

    // Keep the nested expression stack bounded even with 64 nested groups.
    #[inline(never)]
    fn logical_or(
        &mut self,
        nodes: &mut Vec<ExpressionNode>,
        depth: usize,
    ) -> Result<usize, ParseError> {
        let mut left = self.logical_and(nodes, depth)?;
        while self.keyword("or") {
            let operator_span = self.current().unwrap().span;
            self.advance();
            let right = self.logical_and(nodes, depth)?;
            left = logical(nodes, left, LogicalOperator::Or, right, operator_span);
        }
        Ok(left)
    }

    fn logical_and(
        &mut self,
        nodes: &mut Vec<ExpressionNode>,
        depth: usize,
    ) -> Result<usize, ParseError> {
        let mut left = self.logical_not(nodes, depth)?;
        while self.keyword("and") {
            let operator_span = self.current().unwrap().span;
            self.advance();
            let right = self.logical_not(nodes, depth)?;
            left = logical(nodes, left, LogicalOperator::And, right, operator_span);
        }
        Ok(left)
    }

    fn logical_not(
        &mut self,
        nodes: &mut Vec<ExpressionNode>,
        depth: usize,
    ) -> Result<usize, ParseError> {
        let mut operators = Vec::new();
        while self.keyword("not") {
            operators.push(self.current().unwrap().span);
            self.advance();
        }
        let mut operand = self.comparison(nodes, depth)?;
        for operator_span in operators.into_iter().rev() {
            let span = Span::new(operator_span.start, nodes[operand].span.end);
            operand = push(
                nodes,
                ExpressionKind::Not {
                    operand,
                    operator_span,
                },
                span,
            );
        }
        Ok(operand)
    }

    fn comparison(
        &mut self,
        nodes: &mut Vec<ExpressionNode>,
        depth: usize,
    ) -> Result<usize, ParseError> {
        let left = self.arithmetic(nodes, depth)?;
        if let Some(operator) = self
            .current()
            .and_then(|token| comparison_operator(&token.kind))
        {
            let operator_span = self.current().unwrap().span;
            self.advance();
            let right = self.arithmetic(nodes, depth)?;
            if self
                .current()
                .and_then(|token| comparison_operator(&token.kind))
                .is_some()
            {
                return Err(self
                    .expected("'and' between separate comparisons")
                    .with_help("write a < b and b < c; chained comparisons are not supported"));
            }
            return Ok(binary(nodes, left, operator, right, operator_span));
        }
        Ok(left)
    }

    fn arithmetic(
        &mut self,
        nodes: &mut Vec<ExpressionNode>,
        depth: usize,
    ) -> Result<usize, ParseError> {
        let first = self.unary(nodes, depth)?;
        if self
            .current()
            .and_then(|token| infix_operator(&token.kind))
            .is_none()
        {
            return Ok(first);
        }
        self.infix_chain(nodes, depth, first)
    }

    // Keep operator-stack locals off the recursive path for groups, calls and
    // collection literals that contain no infix operator at this level.
    fn infix_chain(
        &mut self,
        nodes: &mut Vec<ExpressionNode>,
        depth: usize,
        first: usize,
    ) -> Result<usize, ParseError> {
        let mut operands = vec![first];
        let mut operators: Vec<(BinaryOperator, Span, u8)> = Vec::new();
        while let Some((operator, precedence)) =
            self.current().and_then(|token| infix_operator(&token.kind))
        {
            let span = self.current().unwrap().span;
            while operators
                .last()
                .is_some_and(|(_, _, previous)| *previous >= precedence)
            {
                let (operator, span, _) = operators.pop().unwrap();
                reduce_binary(nodes, &mut operands, operator, span);
            }
            operators.push((operator, span, precedence));
            self.advance();
            operands.push(self.unary(nodes, depth)?);
        }
        while let Some((operator, span, _)) = operators.pop() {
            reduce_binary(nodes, &mut operands, operator, span);
        }
        Ok(operands.pop().unwrap())
    }

    fn unary(
        &mut self,
        nodes: &mut Vec<ExpressionNode>,
        depth: usize,
    ) -> Result<usize, ParseError> {
        if matches!(
            self.current().map(|token| &token.kind),
            Some(TokenKind::BitNot)
        ) {
            let operator_span = self.current().unwrap().span;
            self.advance();
            let operand = self.postfix(nodes, depth)?;
            let span = Span::new(operator_span.start, nodes[operand].span.end);
            return Ok(push(
                nodes,
                ExpressionKind::BitNot {
                    operand,
                    operator_span,
                },
                span,
            ));
        }
        if let Some(Token {
            kind: TokenKind::Minus,
            span,
        }) = self.current()
        {
            let operator_span = *span;
            self.advance();
            // The minimum integer has no representable positive i64 magnitude.
            // Only a direct literal after '-' gets this special treatment.
            if let Some(Token {
                kind: TokenKind::Integer(digits),
                span,
            }) = self.current()
            {
                if integer_magnitude(digits) == Some(1_u64 << 63) {
                    let span = Span::new(operator_span.start, span.end);
                    self.advance();
                    return Ok(push(nodes, ExpressionKind::Integer(i64::MIN), span));
                }
            }
            let operand = self.postfix(nodes, depth)?;
            let span = Span::new(operator_span.start, nodes[operand].span.end);
            Ok(push(
                nodes,
                ExpressionKind::Negate {
                    operand,
                    operator_span,
                },
                span,
            ))
        } else {
            self.postfix(nodes, depth)
        }
    }

    fn expression_depth(&self, depth: usize, span: Span) -> Result<(), ParseError> {
        if depth >= MAX_EXPRESSION_DEPTH {
            return Err(ParseError::new(
                DiagnosticKind::ExpressionNesting,
                span,
                "expression nesting exceeds 64 levels",
            ));
        }
        Ok(())
    }

    fn postfix(
        &mut self,
        nodes: &mut Vec<ExpressionNode>,
        depth: usize,
    ) -> Result<usize, ParseError> {
        let mut vector = self.primary(nodes, depth)?;
        while self.at(&TokenKind::LeftBracket) {
            let opening = self.current().unwrap().span;
            self.expression_depth(depth, opening)?;
            self.advance();
            let index = self.logical_or(nodes, depth + 1)?;
            let end = self.punctuation(TokenKind::RightBracket, "']'")?.end;
            let span = Span::new(nodes[vector].span.start, end);
            let mut literal = index;
            while let ExpressionKind::Group { expression } = nodes[literal].kind {
                literal = expression;
            }
            let source_literal =
                matches!(nodes[literal].kind, ExpressionKind::Integer(n) if n >= 0);
            vector = push(
                nodes,
                ExpressionKind::Index {
                    vector,
                    index,
                    operator_span: Span::new(opening.start, end),
                    source_literal,
                },
                span,
            );
        }
        Ok(vector)
    }

    // Keep list/call buffers out of every parenthesized primary's debug frame.
    // Combined block/expression limits must also fit small native thread stacks.
    fn vector_literal(
        &mut self,
        nodes: &mut Vec<ExpressionNode>,
        depth: usize,
    ) -> Result<usize, ParseError> {
        let opening = self.punctuation(TokenKind::LeftBracket, "'['")?;
        self.expression_depth(depth, opening)?;
        // This prefix extends previously invalid syntax; [integer] still reads
        // a normal binding named integer and does not change float vectors.
        let tagged = self
            .tokens
            .get(self.cursor + 1)
            .is_some_and(|token| token.kind == TokenKind::Colon);
        if tagged && self.keyword("array") {
            self.advance();
            self.advance();
            return self.array_literal(nodes, depth, opening);
        }
        let integers = tagged && self.keyword("integer");
        if integers {
            self.advance();
            self.advance();
        }
        let mut elements = Vec::new();
        while !self.at(&TokenKind::RightBracket) {
            let element = self.logical_or(nodes, depth + 1)?;
            if elements.len() == MAX_VECTOR_LENGTH {
                return Err(ParseError::new(
                    DiagnosticKind::VectorLength,
                    nodes[element].span,
                    "vector literal exceeds 65536 elements",
                ));
            }
            elements.push(element);
            if !self.at(&TokenKind::Comma) {
                break;
            }
            self.advance();
        }
        let end = self.punctuation(TokenKind::RightBracket, "']'")?.end;
        Ok(push(
            nodes,
            if integers {
                ExpressionKind::Integers { elements }
            } else {
                ExpressionKind::Vector { elements }
            },
            Span::new(opening.start, end),
        ))
    }

    fn array_literal(
        &mut self,
        nodes: &mut Vec<ExpressionNode>,
        depth: usize,
        opening: Span,
    ) -> Result<usize, ParseError> {
        let mut elements = Vec::new();
        let empty_element_type = if self.at(&TokenKind::Less) {
            self.advance();
            let mut remaining = MAX_SOURCE_TYPE_NODES - 1;
            let element = self.source_type(false, 1, &mut remaining, true)?;
            self.type_close()?;
            Some(element)
        } else {
            if self.at(&TokenKind::RightBracket) {
                return Err(self
                    .expected("an array element or '<type>'")
                    .with_help("write a typed empty array such as [array: <integer>]"));
            }
            loop {
                if elements.len() == MAX_ARRAY_LENGTH {
                    return Err(ParseError::new(
                        DiagnosticKind::ArrayLimit,
                        self.current().map_or(opening, |token| token.span),
                        "array literal exceeds 4096 elements",
                    ));
                }
                elements.push(self.logical_or(nodes, depth + 1)?);
                if !self.at(&TokenKind::Comma) {
                    break;
                }
                self.advance();
                if self.at(&TokenKind::RightBracket) {
                    break;
                }
            }
            None
        };
        let end = self
            .punctuation(TokenKind::RightBracket, "']' after array literal")?
            .end;
        Ok(push(
            nodes,
            ExpressionKind::Array {
                elements,
                empty_element_type,
            },
            Span::new(opening.start, end),
        ))
    }

    fn call_expression(
        &mut self,
        nodes: &mut Vec<ExpressionNode>,
        depth: usize,
    ) -> Result<usize, ParseError> {
        let (name, function_span) = self.qualified_identifier()?;
        let opening = self.punctuation(TokenKind::LeftParen, "'('")?;
        self.expression_depth(depth, opening)?;
        let mut arguments = Vec::new();
        while !self.at(&TokenKind::RightParen) {
            let label = if self
                .tokens
                .get(self.cursor + 1)
                .is_some_and(|t| t.kind == TokenKind::Colon)
            {
                let label = self.identifier()?;
                self.advance();
                Some(label)
            } else {
                None
            };
            let expression = self.logical_or(nodes, depth + 1)?;
            arguments.push(CallArgument { label, expression });
            if !self.at(&TokenKind::Comma) {
                break;
            }
            self.advance();
        }
        let end = self.punctuation(TokenKind::RightParen, "')'")?.end;
        Ok(push(
            nodes,
            ExpressionKind::UserCall {
                name,
                arguments,
                function_span,
            },
            Span::new(function_span.start, end),
        ))
    }

    // Keep the nested expression stack bounded even with 64 nested groups.
    #[inline(never)]
    fn primary(
        &mut self,
        nodes: &mut Vec<ExpressionNode>,
        depth: usize,
    ) -> Result<usize, ParseError> {
        match self.current().map(|token| &token.kind) {
            Some(TokenKind::Identifier(_))
                if self
                    .tokens
                    .get(self.cursor + 1)
                    .is_some_and(|t| t.kind == TokenKind::Dot) =>
            {
                self.qualified_atom(nodes, depth)
            }
            Some(TokenKind::LeftBracket) => self.vector_literal(nodes, depth),
            Some(TokenKind::LeftParen) => self.group_expression(nodes, depth),
            Some(TokenKind::Identifier(name)) if name == "if" => {
                self.conditional_expression(nodes, depth)
            }
            Some(TokenKind::Identifier(name))
                if !matches!(
                    name.as_str(),
                    "funcra" | "pure" | "print" | "println" | "write" | "eprint" | "ewrite"
                ) && self
                    .tokens
                    .get(self.cursor + 1)
                    .is_some_and(|t| t.kind == TokenKind::LeftParen) =>
            {
                self.call_expression(nodes, depth)
            }
            _ => self.atom(nodes),
        }
    }

    fn conditional_expression(
        &mut self,
        nodes: &mut Vec<ExpressionNode>,
        depth: usize,
    ) -> Result<usize, ParseError> {
        let opening = self.current().unwrap().span;
        self.expression_depth(depth, opening)?;
        let depth = depth + 1;
        let mut branches = Vec::new();
        loop {
            self.advance(); // if or elif
            let condition = self.logical_or(nodes, depth)?;
            let (result, _) = self.conditional_result(nodes, depth)?;
            branches.push(ConditionalExpressionBranch { condition, result });
            if !self.keyword("elif") {
                break;
            }
        }
        if !self.keyword("else") {
            return Err(self
                .expected("'else' in a conditional expression")
                .with_help("finish the conditional expression with else { expression }"));
        }
        self.advance();
        let (otherwise, end) = self.conditional_result(nodes, depth)?;
        Ok(push(
            nodes,
            ExpressionKind::Conditional {
                branches,
                otherwise,
            },
            Span::new(opening.start, end),
        ))
    }

    fn conditional_result(
        &mut self,
        nodes: &mut Vec<ExpressionNode>,
        depth: usize,
    ) -> Result<(usize, usize), ParseError> {
        self.punctuation(TokenKind::LeftBrace, "'{' before conditional result")?;
        let result = self.logical_or(nodes, depth)?;
        let end = self
            .punctuation(
                TokenKind::RightBrace,
                "'}' after one conditional result expression",
            )?
            .end;
        Ok((result, end))
    }

    // Keep the nested expression stack bounded even with 64 nested groups.
    #[inline(never)]
    /// Parse deeply nested parenthesized integer infix expressions with a
    /// bounded *heap* operator stack instead of recursively consuming every
    /// logical/arithmetic precedence frame for each pair of parentheses.
    ///
    /// This is a conservative fast path. It requires a balanced, syntactically
    /// valid region consisting exclusively of integer literals, known binary
    /// arithmetic/bitwise operators, and parentheses. All mixed expressions,
    /// tuples, function calls, conditions, unary signs and invalid syntax
    /// continue through the existing complete grammar without reinterpretation.
    ///
    /// Only four-or-more nested groups use this path; ordinary source remains
    /// on the original parser. Each Group retains its original byte spans and
    /// every operator is reduced using the existing precedence table.
    fn grouped_integer_infix(
        &mut self,
        nodes: &mut Vec<ExpressionNode>,
        depth: usize,
    ) -> Result<Option<usize>, ParseError> {
        if !self.pending.is_empty() {
            return Ok(None);
        }
        let start = self.cursor;
        let mut openings = 0usize;
        let mut max_openings = 0usize;
        let mut expecting_operand = true;
        let mut final_position = None;
        for (position, token) in self.tokens.iter().enumerate().skip(start) {
            match &token.kind {
                TokenKind::LeftParen if expecting_operand => {
                    openings += 1;
                    max_openings = max_openings.max(openings);
                }
                TokenKind::Integer(_) if expecting_operand => expecting_operand = false,
                TokenKind::RightParen if !expecting_operand && openings > 0 => {
                    openings -= 1;
                    if openings == 0 {
                        final_position = Some(position);
                        break;
                    }
                }
                kind if !expecting_operand && infix_operator(kind).is_some() => {
                    expecting_operand = true;
                }
                _ => return Ok(None),
            }
        }
        let Some(final_position) = final_position else {
            return Ok(None);
        };
        if max_openings < 4 {
            return Ok(None);
        }
        enum Operator {
            Opening(Span),
            Binary(BinaryOperator, Span, u8),
        }
        let mut operators: Vec<Operator> = Vec::new();
        let mut operands: Vec<usize> = Vec::new();
        while self.cursor <= final_position {
            let token = self.current().expect("preflight validated expression");
            match &token.kind {
                TokenKind::LeftParen => {
                    let span = token.span;
                    self.expression_depth(depth + operators.iter()
                        .filter(|op| matches!(op, Operator::Opening(_))).count(), span)?;
                    operators.push(Operator::Opening(span));
                    self.advance();
                }
                TokenKind::Integer(_) => {
                    operands.push(self.atom(nodes)?);
                }
                TokenKind::RightParen => {
                    let closing = token.span;
                    self.advance();
                    loop {
                        match operators.pop().expect("preflight checked balanced groups") {
                            Operator::Opening(opening) => {
                                let expression = operands.pop()
                                    .expect("preflight checked group operand");
                                operands.push(push(
                                    nodes,
                                    ExpressionKind::Group { expression },
                                    Span::new(opening.start, closing.end),
                                ));
                                break;
                            }
                            Operator::Binary(operator, span, _) => {
                                reduce_binary(nodes, &mut operands, operator, span);
                            }
                        }
                    }
                }
                kind => {
                    let (operator, precedence) =
                        infix_operator(kind).expect("preflight validated infix operator");
                    let span = token.span;
                    self.advance();
                    while matches!(
                        operators.last(),
                        Some(Operator::Binary(_, _, old)) if *old >= precedence
                    ) {
                        let Some(Operator::Binary(operator, span, _)) = operators.pop() else {
                            unreachable!("checked binary operator");
                        };
                        reduce_binary(nodes, &mut operands, operator, span);
                    }
                    operators.push(Operator::Binary(operator, span, precedence));
                }
            }
        }
        debug_assert!(operators.is_empty());
        debug_assert_eq!(operands.len(), 1);
        Ok(operands.pop())
    }

    fn group_expression(
        &mut self,
        nodes: &mut Vec<ExpressionNode>,
        depth: usize,
    ) -> Result<usize, ParseError> {
        if let Some(compact) = self.grouped_integer_infix(nodes, depth)? {
            return Ok(compact);
        }
        let opening = self.current().unwrap().span;
        self.expression_depth(depth, opening)?;
        self.advance();
        let first = self.logical_or(nodes, depth + 1)?;
        if !self.at(&TokenKind::Comma) {
            let end = self.punctuation(TokenKind::RightParen, "')'")?.end;
            return Ok(push(
                nodes,
                ExpressionKind::Group { expression: first },
                Span::new(opening.start, end),
            ));
        }
        // Keep list storage out of the ordinary grouping stack frame.
        self.tuple_literal(nodes, depth, opening, first)
    }

    fn tuple_literal(
        &mut self,
        nodes: &mut Vec<ExpressionNode>,
        depth: usize,
        opening: Span,
        first: usize,
    ) -> Result<usize, ParseError> {
        let mut elements = vec![first];
        while self.at(&TokenKind::Comma) {
            self.advance();
            if self.at(&TokenKind::RightParen) {
                break;
            }
            if elements.len() >= MAX_TUPLE_ARITY {
                return Err(ParseError::new(
                    DiagnosticKind::TupleLimit,
                    opening,
                    "tuple literal arity exceeds 32",
                ));
            }
            elements.push(self.logical_or(nodes, depth + 1)?);
        }
        let end = self
            .punctuation(TokenKind::RightParen, "')' after tuple elements")?
            .end;
        Ok(push(
            nodes,
            ExpressionKind::Tuple { elements },
            Span::new(opening.start, end),
        ))
    }

    fn atom(&mut self, nodes: &mut Vec<ExpressionNode>) -> Result<usize, ParseError> {
        match self.current() {
            Some(Token {
                kind: TokenKind::Float(digits),
                span,
            }) => {
                let value = digits
                    .parse::<f64>()
                    .ok()
                    .and_then(FiniteFloat::new)
                    .ok_or_else(|| {
                        ParseError::new(
                            DiagnosticKind::InvalidFloat,
                            *span,
                            "float literal is not a finite binary64 value",
                        )
                    })?;
                let span = *span;
                self.advance();
                Ok(push(nodes, ExpressionKind::Float(value), span))
            }
            Some(Token {
                kind: TokenKind::String(value),
                span,
            }) => {
                let kind = ExpressionKind::String(value.clone());
                let span = *span;
                self.advance();
                Ok(push(nodes, kind, span))
            }
            Some(Token {
                kind: TokenKind::Identifier(name),
                span,
            }) => {
                if name == "funcra" || name == "pure" {
                    return Err(ParseError::new(
                        DiagnosticKind::FunctionUnsupported,
                        *span,
                        "anonymous funcra expressions are not supported yet",
                    ));
                }
                let kind = match name.as_str() {
                    "true" => ExpressionKind::Boolean(true),
                    "false" => ExpressionKind::Boolean(false),
                    _ => ExpressionKind::Name(name.clone()),
                };
                let span = *span;
                self.advance();
                Ok(push(nodes, kind, span))
            }
            Some(Token {
                kind: TokenKind::Integer(digits),
                span,
            }) => {
                let span = *span;
                let radix = ["0x", "0X", "0b", "0B"]
                    .iter()
                    .any(|prefix| digits.starts_with(prefix));
                let message = if radix {
                    "radix integer literal requires valid digits and a signed 64-bit magnitude"
                } else {
                    "integer literal is outside the signed 64-bit range"
                };
                let value = integer_magnitude(digits)
                    .and_then(|value| i64::try_from(value).ok())
                    .ok_or_else(|| {
                        ParseError::new(DiagnosticKind::InvalidInteger, span, message).with_help(
                            "use a value from -9223372036854775808 to 9223372036854775807",
                        )
                    })?;
                self.advance();
                Ok(push(nodes, ExpressionKind::Integer(value), span))
            }
            _ => Err(self.expected("a literal, variable name, or '('")),
        }
    }
}

fn is_type_close(kind: &TokenKind) -> bool {
    matches!(
        kind,
        TokenKind::Greater
            | TokenKind::ShiftRight
            | TokenKind::UnsignedRight
            | TokenKind::GreaterEqual
            | TokenKind::RightEqual
            | TokenKind::UnsignedEqual
    )
}

fn assignment_operator(kind: &TokenKind) -> Option<AssignmentOperator> {
    Some(match kind {
        TokenKind::Equal => AssignmentOperator::Set,
        TokenKind::PlusEqual => AssignmentOperator::Compound(BinaryOperator::Add),
        TokenKind::MinusEqual => AssignmentOperator::Compound(BinaryOperator::Subtract),
        TokenKind::StarEqual => AssignmentOperator::Compound(BinaryOperator::Multiply),
        TokenKind::SlashEqual => AssignmentOperator::Compound(BinaryOperator::Divide),
        TokenKind::PercentEqual => AssignmentOperator::Compound(BinaryOperator::Remainder),
        TokenKind::AndEqual => AssignmentOperator::Compound(BinaryOperator::BitAnd),
        TokenKind::OrEqual => AssignmentOperator::Compound(BinaryOperator::BitOr),
        TokenKind::XorEqual => AssignmentOperator::Compound(BinaryOperator::BitXor),
        TokenKind::LeftEqual => AssignmentOperator::Compound(BinaryOperator::ShiftLeft),
        TokenKind::RightEqual => AssignmentOperator::Compound(BinaryOperator::ShiftRight),
        TokenKind::UnsignedEqual => AssignmentOperator::Compound(BinaryOperator::UnsignedRight),
        _ => return None,
    })
}

fn reduce_binary(
    nodes: &mut Vec<ExpressionNode>,
    operands: &mut Vec<usize>,
    operator: BinaryOperator,
    span: Span,
) {
    let right = operands.pop().unwrap();
    let left = operands.pop().unwrap();
    operands.push(binary(nodes, left, operator, right, span));
}

fn integer_magnitude(digits: &str) -> Option<u64> {
    if let Some(digits) = digits
        .strip_prefix("0x")
        .or_else(|| digits.strip_prefix("0X"))
    {
        u64::from_str_radix(digits, 16).ok()
    } else if let Some(digits) = digits
        .strip_prefix("0b")
        .or_else(|| digits.strip_prefix("0B"))
    {
        u64::from_str_radix(digits, 2).ok()
    } else {
        digits.parse().ok()
    }
}

fn infix_operator(kind: &TokenKind) -> Option<(BinaryOperator, u8)> {
    Some(match kind {
        TokenKind::BitOr => (BinaryOperator::BitOr, 1),
        TokenKind::BitXor => (BinaryOperator::BitXor, 2),
        TokenKind::BitAnd => (BinaryOperator::BitAnd, 3),
        TokenKind::ShiftLeft => (BinaryOperator::ShiftLeft, 4),
        TokenKind::ShiftRight => (BinaryOperator::ShiftRight, 4),
        TokenKind::UnsignedRight => (BinaryOperator::UnsignedRight, 4),
        TokenKind::Plus => (BinaryOperator::Add, 5),
        TokenKind::Minus => (BinaryOperator::Subtract, 5),
        TokenKind::Star => (BinaryOperator::Multiply, 6),
        TokenKind::Slash => (BinaryOperator::Divide, 6),
        TokenKind::Percent => (BinaryOperator::Remainder, 6),
        _ => return None,
    })
}

fn comparison_operator(kind: &TokenKind) -> Option<BinaryOperator> {
    Some(match kind {
        TokenKind::EqualEqual => BinaryOperator::Equal,
        TokenKind::NotEqual => BinaryOperator::NotEqual,
        TokenKind::Less => BinaryOperator::Less,
        TokenKind::LessEqual => BinaryOperator::LessEqual,
        TokenKind::Greater => BinaryOperator::Greater,
        TokenKind::GreaterEqual => BinaryOperator::GreaterEqual,
        _ => return None,
    })
}

fn logical(
    nodes: &mut Vec<ExpressionNode>,
    left: usize,
    operator: LogicalOperator,
    right: usize,
    operator_span: Span,
) -> usize {
    let span = Span::new(nodes[left].span.start, nodes[right].span.end);
    push(
        nodes,
        ExpressionKind::Logical {
            left,
            operator,
            right,
            operator_span,
        },
        span,
    )
}

fn push(nodes: &mut Vec<ExpressionNode>, kind: ExpressionKind, span: Span) -> usize {
    let id = nodes.len();
    nodes.push(ExpressionNode { kind, span });
    id
}

fn binary(
    nodes: &mut Vec<ExpressionNode>,
    left: usize,
    operator: BinaryOperator,
    right: usize,
    operator_span: Span,
) -> usize {
    let span = Span::new(nodes[left].span.start, nodes[right].span.end);
    push(
        nodes,
        ExpressionKind::Binary {
            left,
            operator,
            right,
            operator_span,
        },
        span,
    )
}
