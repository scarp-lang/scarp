//! Shared, bounded parser for irrefutable binding patterns.
use super::*;
use crate::{
    ast::{BindingPattern, PatternField, PatternKind, PatternNode},
    types::{MAX_EXPANDED_VALUE_NODES, MAX_RECORD_FIELDS},
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum GroupKind {
    Tuple,
    Array,
    Record,
}

impl GroupKind {
    fn closing(self) -> (TokenKind, &'static str) {
        match self {
            Self::Tuple => (TokenKind::RightParen, "')'"),
            Self::Array => (TokenKind::RightBracket, "']'"),
            Self::Record => (TokenKind::RightBrace, "'}'"),
        }
    }

    fn limit(self) -> (usize, DiagnosticKind) {
        match self {
            Self::Tuple => (MAX_TUPLE_ARITY, DiagnosticKind::TupleLimit),
            Self::Array => (MAX_DESTRUCTURE_BINDINGS, DiagnosticKind::ArrayLimit),
            Self::Record => (MAX_RECORD_FIELDS, DiagnosticKind::RecordField),
        }
    }
}

struct Group {
    node: usize,
    kind: GroupKind,
    arity: usize,
    expect_member: bool,
}

impl Parser<'_> {
    pub(super) fn pattern_group_kind(&self) -> Option<GroupKind> {
        match self.current()?.kind {
            TokenKind::LeftParen => Some(GroupKind::Tuple),
            TokenKind::LeftBracket => Some(GroupKind::Array),
            TokenKind::LeftBrace => Some(GroupKind::Record),
            _ => None,
        }
    }

    fn open_pattern_group(&mut self, kind: GroupKind, nodes: &mut Vec<PatternNode>) -> Group {
        let span = self.current().expect("pattern opener").span;
        let node = nodes.len();
        nodes.push(PatternNode {
            kind: match kind {
                GroupKind::Tuple => PatternKind::Tuple(0),
                GroupKind::Array => PatternKind::Array(0),
                GroupKind::Record => PatternKind::Record(Vec::new()),
            },
            span,
        });
        self.advance();
        Group {
            node,
            kind,
            arity: 0,
            expect_member: true,
        }
    }

    fn pattern_binding(&mut self) -> Result<PatternNode, ParseError> {
        let (name, span) = self.identifier().map_err(|error| {
            error.with_help("use a name, '_', or a nested tuple, array, or record pattern")
        })?;
        let kind = if name == "_" {
            PatternKind::Discard
        } else {
            PatternKind::Name(name)
        };
        Ok(PatternNode { kind, span })
    }

    #[inline(never)]
    pub(super) fn binding_pattern(&mut self) -> Result<BindingPattern, ParseError> {
        let mut nodes = Vec::new();
        let Some(kind) = self.pattern_group_kind() else {
            return Ok(BindingPattern {
                nodes: vec![self.pattern_binding()?],
            });
        };
        let mut groups = vec![self.open_pattern_group(kind, &mut nodes)];
        while let Some(group) = groups.last_mut() {
            let (close, close_label) = group.kind.closing();
            if self.at(&close) {
                let node = &mut nodes[group.node];
                if group.arity == 0 {
                    return Err(ParseError::new(
                        DiagnosticKind::UnexpectedToken,
                        node.span,
                        "a destructuring pattern requires at least one member",
                    ));
                }
                if group.kind == GroupKind::Tuple && group.arity == 1 && !group.expect_member {
                    return Err(ParseError::new(
                        DiagnosticKind::UnexpectedToken,
                        node.span,
                        "a one-element tuple pattern requires a trailing comma",
                    )
                    .with_help("write the pattern as (value,)"));
                }
                node.span.end = self.punctuation(close, close_label)?.end;
                match &mut node.kind {
                    PatternKind::Tuple(arity) | PatternKind::Array(arity) => *arity = group.arity,
                    _ => {}
                }
                groups.pop();
                continue;
            }
            if !group.expect_member {
                self.punctuation(
                    TokenKind::Comma,
                    "',' between destructuring pattern members",
                )?;
                group.expect_member = true;
                continue;
            }

            let token = self
                .current()
                .ok_or_else(|| self.expected("a pattern member"))?;
            let (max, limit) = group.kind.limit();
            if group.arity == max {
                return Err(ParseError::new(
                    limit,
                    token.span,
                    format!("destructuring pattern exceeds {max} elements"),
                ));
            }
            if nodes.len() == MAX_EXPANDED_VALUE_NODES {
                return Err(ParseError::new(
                    DiagnosticKind::TypeComplexity,
                    token.span,
                    format!("destructuring pattern exceeds {MAX_EXPANDED_VALUE_NODES} nodes"),
                ));
            }
            group.arity += 1;
            group.expect_member = false;
            if let PatternKind::Record(fields) = &mut nodes[group.node].kind {
                let (name, span) = self.identifier()?;
                self.punctuation(TokenKind::Colon, "':' after the record field name")?;
                fields.push(PatternField { name, span });
            }
            if let Some(kind) = self.pattern_group_kind() {
                if groups.len() == MAX_COMPOSITE_DEPTH {
                    return Err(ParseError::new(
                        limit,
                        self.current().unwrap().span,
                        format!("destructuring pattern exceeds {MAX_COMPOSITE_DEPTH} levels"),
                    ));
                }
                groups.push(self.open_pattern_group(kind, &mut nodes));
            } else {
                nodes.push(self.pattern_binding()?);
            }
        }
        Ok(BindingPattern { nodes })
    }
}
