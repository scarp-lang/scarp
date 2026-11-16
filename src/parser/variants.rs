use super::*;
use crate::ast::{
    MatchArm, MatchStatement, TypeAnnotation, VariantCase, VariantConstructor, VariantDeclaration,
};
use crate::types::MAX_VARIANT_CASES;

impl Parser<'_> {
    #[inline(never)]
    pub(super) fn variant_into(
        &mut self,
        statements: &mut Vec<Statement>,
        in_block: bool,
    ) -> Result<(), ParseError> {
        if in_block {
            return Err(ParseError::new(
                DiagnosticKind::VariantDefinition,
                self.current().unwrap().span,
                "variant declarations are allowed only at file scope",
            ));
        }
        statements.push(Statement::Variant(Box::new(self.variant_declaration()?)));
        Ok(())
    }

    pub(super) fn variant_declaration(&mut self) -> Result<VariantDeclaration, ParseError> {
        let start = self.current().unwrap().span.start;
        self.advance();
        let (name, name_span) = self.identifier()?;
        self.punctuation(TokenKind::LeftBrace, "'{' after the variant name")?;
        let mut cases = Vec::new();
        while !self.at(&TokenKind::RightBrace) {
            let (tag, span) = self.identifier()?;
            if cases.len() >= MAX_VARIANT_CASES {
                return Err(ParseError::new(
                    DiagnosticKind::VariantDefinition,
                    span,
                    "variants support at most 32 cases",
                ));
            }
            let payload = if self.at(&TokenKind::LeftParen) {
                let opening = self.punctuation(TokenKind::LeftParen, "'('")?;
                let kind = self.value_type(false)?;
                let span = Span::new(opening.end, self.type_end);
                self.punctuation(TokenKind::RightParen, "')' after the payload type")?;
                Some(TypeAnnotation { kind, span })
            } else {
                None
            };
            cases.push(VariantCase {
                name: tag,
                span,
                payload,
            });
            if !self.at(&TokenKind::Comma) {
                break;
            }
            self.advance();
        }
        let end = self.punctuation(TokenKind::RightBrace, "'}'")?.end;
        if cases.is_empty() {
            return Err(ParseError::new(
                DiagnosticKind::VariantDefinition,
                name_span,
                "a variant must declare at least one case",
            ));
        }
        Ok(VariantDeclaration {
            name,
            name_span,
            cases,
            span: Span::new(start, end),
        })
    }

    pub(super) fn variant_literal_start(&self) -> bool {
        if !self.keyword_then_identifier("new") {
            return false;
        }
        let mut after_name = self.cursor + 2;
        if self
            .tokens
            .get(after_name)
            .is_some_and(|t| t.kind == TokenKind::Dot)
        {
            after_name += 2;
        }
        self.tokens
            .get(after_name..after_name + 2)
            .is_some_and(|tokens| {
                tokens[0].kind == TokenKind::Colon
                    && tokens[1].kind == TokenKind::Colon
                    && tokens[0].span.end == tokens[1].span.start
            })
    }

    #[inline(never)]
    pub(super) fn variant_literal(
        &mut self,
        nodes: &mut Vec<ExpressionNode>,
        depth: usize,
    ) -> Result<usize, ParseError> {
        let opening = self.current().unwrap().span;
        self.expression_depth(depth, opening)?;
        self.advance();
        let (name, name_span) = self.qualified_identifier()?;
        self.punctuation(TokenKind::Colon, "'::' before the variant case")?;
        self.punctuation(TokenKind::Colon, "'::' before the variant case")?;
        let (tag, tag_span) = self.identifier()?;
        self.punctuation(TokenKind::LeftParen, "'(' after the variant case")?;
        let payload = if self.at(&TokenKind::RightParen) {
            None
        } else {
            Some(self.logical_or(nodes, depth + 1)?)
        };
        let end = self
            .punctuation(TokenKind::RightParen, "')' after the payload")?
            .end;
        Ok(push(
            nodes,
            ExpressionKind::Variant(Box::new(VariantConstructor {
                name,
                name_span,
                tag,
                tag_span,
                payload,
            })),
            Span::new(opening.start, end),
        ))
    }

    pub(super) fn match_start(&self) -> bool {
        self.keyword("match")
            && self
                .tokens
                .get(self.cursor + 1)
                .is_some_and(|t| matches!(&t.kind, TokenKind::Identifier(name) if name == "on"))
    }

    #[inline(never)]
    pub(super) fn match_into(
        &mut self,
        statements: &mut Vec<Statement>,
        depth: usize,
    ) -> Result<(), ParseError> {
        let start = self.current().unwrap().span.start;
        self.advance();
        self.advance();
        let subject = self.expression()?;
        self.punctuation(TokenKind::LeftBrace, "'{' before the match cases")?;
        let mut arms = Vec::new();
        while self.keyword("case") {
            let arm_start = self.current().unwrap().span.start;
            self.advance();
            let (tag, tag_span) = self.identifier()?;
            if arms.len() > MAX_VARIANT_CASES {
                return Err(ParseError::new(
                    DiagnosticKind::MatchCoverage,
                    tag_span,
                    "match supports at most 33 arms",
                ));
            }
            let payload = if self.at(&TokenKind::LeftParen) {
                self.advance();
                let name = self.identifier()?;
                self.punctuation(TokenKind::RightParen, "')' after the payload binding")?;
                Some(name)
            } else {
                None
            };
            let (body, body_span) = self.block(depth)?;
            arms.push(MatchArm {
                tag,
                tag_span,
                payload,
                statements: body,
                span: Span::new(arm_start, body_span.end),
            });
        }
        let end = self
            .punctuation(TokenKind::RightBrace, "'case' or '}' in match")?
            .end;
        if arms.is_empty() {
            return Err(ParseError::new(
                DiagnosticKind::MatchCoverage,
                Span::new(start, end),
                "match requires at least one case",
            ));
        }
        statements.push(Statement::Match(Box::new(MatchStatement {
            subject,
            arms,
            span: Span::new(start, end),
        })));
        Ok(())
    }
}
