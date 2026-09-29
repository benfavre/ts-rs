use super::*;

impl Parser<'_> {
    pub(super) fn is_index_signature(&self) -> bool {
        let identifier = |kind: TokenKind| {
            kind == TokenKind::Identifier || kind.is_keyword() || kind.is_contextual_keyword()
        };
        let mut position = self.pos + 1;
        let Some(first) = self.tokens.get(position) else {
            return false;
        };
        if matches!(first.kind, TokenKind::DotDotDot | TokenKind::CloseBracket) {
            return true;
        }
        if matches!(
            first.kind,
            TokenKind::Abstract
                | TokenKind::Accessor
                | TokenKind::Async
                | TokenKind::Const
                | TokenKind::Declare
                | TokenKind::Default
                | TokenKind::Export
                | TokenKind::In
                | TokenKind::Public
                | TokenKind::Private
                | TokenKind::Protected
                | TokenKind::Readonly
                | TokenKind::Static
                | TokenKind::Out
                | TokenKind::Override
        ) {
            position += 1;
            if self
                .tokens
                .get(position)
                .is_some_and(|t| identifier(t.kind))
            {
                return true;
            }
        } else if identifier(first.kind) {
            position += 1;
        } else {
            return false;
        }
        match self.tokens.get(position).map(|t| t.kind) {
            Some(TokenKind::Colon | TokenKind::Comma) => true,
            Some(TokenKind::Question) => self.tokens.get(position + 1).is_some_and(|t| {
                matches!(
                    t.kind,
                    TokenKind::Colon | TokenKind::Comma | TokenKind::CloseBracket
                )
            }),
            _ => false,
        }
    }

    pub(super) fn parse_index_signature(
        &mut self,
        start: u32,
        modifiers: ModifierFlags,
    ) -> IndexSignature {
        self.expect(TokenKind::OpenBracket);
        let mut params = Vec::new();
        let mut trailing_comma = None;
        while !self.at(TokenKind::CloseBracket) && !self.is_eof() {
            let before = self.pos;
            params.push(self.parse_param());
            trailing_comma = self.eat(TokenKind::Comma);
            if trailing_comma.is_none() || self.pos == before {
                break;
            }
        }
        self.expect(TokenKind::CloseBracket);
        let type_ann = if self.eat(TokenKind::Colon).is_some() {
            Some(self.parse_js_checked_type_annotation())
        } else {
            None
        };
        self.eat_semicolon();
        let span = self.span_from(start);
        if modifiers & !(MOD_READONLY | MOD_STATIC) == 0 && !self.is_js_file {
            self.check_index_parameter_grammar(&params, trailing_comma, span);
        }
        self.check_signature_parameter_defaults(&params);
        IndexSignature {
            params,
            type_ann,
            modifiers,
        }
    }

    fn check_index_parameter_grammar(
        &mut self,
        params: &[Param],
        trailing_comma: Option<Span>,
        member_span: Span,
    ) {
        if params.len() != 1 {
            self.grammar_error_at_span(
                1096,
                "An index signature must have exactly one parameter.".into(),
                params.first().map_or(member_span, |param| param.name.span),
            );
            return;
        }
        if let Some(span) = trailing_comma {
            self.grammar_error_at_span(
                1025,
                "An index signature cannot have a trailing comma.".into(),
                span,
            );
        }
        let param = &params[0];
        let token_span = |kind, start, end| {
            self.tokens
                .iter()
                .find(|token| {
                    token.kind == kind && token.span.start >= start && token.span.end <= end
                })
                .map_or(param.name.span, |token| token.span)
        };
        let diagnostic = if param.dotdotdot {
            Some((
                1017,
                "An index signature cannot have a rest parameter.",
                token_span(
                    TokenKind::DotDotDot,
                    param.span.start,
                    param.name.span.start,
                ),
            ))
        } else if param.modifiers != MOD_NONE {
            Some((
                1018,
                "An index signature parameter cannot have an accessibility modifier.",
                param.name.span,
            ))
        } else if param.optional {
            Some((
                1019,
                "An index signature parameter cannot have a question mark.",
                token_span(TokenKind::Question, param.name.span.end, param.span.end),
            ))
        } else if param.initializer.is_some() {
            Some((
                1020,
                "An index signature parameter cannot have an initializer.",
                param.name.span,
            ))
        } else if param.type_ann.is_none() {
            Some((
                1022,
                "An index signature parameter must have a type annotation.",
                param.name.span,
            ))
        } else {
            None
        };
        if let Some((code, message, span)) = diagnostic {
            self.grammar_error_at_span(code, message.into(), span);
        }
    }
}
