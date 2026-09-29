//! JSX parsing methods.

use super::*;

/// Compare semantic JSX name tokens, ignoring trivia and decoding escapes.
/// Lookahead may include the rest of a closing tag after the name.
fn jsx_names_match(opening: &str, closing: &str, closing_suffix: bool) -> bool {
    if !closing_suffix && opening == closing {
        return true;
    }
    let mut left = TsScanner::new(opening);
    let mut right = TsScanner::new(closing);
    loop {
        let left_kind = left.scan();
        let right_kind = right.scan();
        if left_kind == TokenKind::EndOfFile {
            return right_kind == TokenKind::EndOfFile
                || (closing_suffix
                    && matches!(
                        right_kind,
                        TokenKind::GreaterThan
                            | TokenKind::GreaterGreater
                            | TokenKind::GreaterGreaterGreater
                            | TokenKind::GreaterEqual
                            | TokenKind::GreaterGreaterEquals
                            | TokenKind::GreaterGreaterGreaterEquals
                    ));
        }
        if left_kind.is_identifier_name() && right_kind.is_identifier_name() {
            left.scan_jsx_identifier();
            right.scan_jsx_identifier();
            if left.token_value() != right.token_value() {
                return false;
            }
        } else if left_kind != right_kind {
            return false;
        }
    }
}

impl<'a> Parser<'a> {
    // -----------------------------------------------------------------------
    // JSX parsing
    // -----------------------------------------------------------------------

    /// Check if current `<` looks like the start of a JSX element.
    /// Only returns true in `.tsx` files — in `.ts` files, `<Type>expr` is a
    /// type assertion, not JSX.
    pub(crate) fn is_jsx_start(&self) -> bool {
        if !self.is_tsx {
            return false;
        }
        if !self.at(TokenKind::LessThan) {
            return false;
        }
        if let Some(next) = self.tokens.get(self.pos + 1) {
            matches!(
                next.kind,
                TokenKind::Identifier | TokenKind::GreaterThan | TokenKind::Slash
            ) || next.kind.is_contextual_keyword()
                || next.kind.is_keyword()
        } else {
            false
        }
    }

    pub(crate) fn parse_jsx_element_or_fragment(&mut self, start: u32) -> Expr {
        self.parse_jsx_with_parent(start, None)
    }

    fn parse_jsx_with_parent(&mut self, start: u32, parent: Option<&Expr>) -> Expr {
        // JSX elements create a fresh expression context — outer ternary
        // state must not leak into JSX children or attributes.
        let saved_ternary = self.in_ternary_consequent;
        self.in_ternary_consequent = false;
        let result = self.parse_jsx_element_or_fragment_inner(start, parent);
        self.in_ternary_consequent = saved_ternary;
        result
    }

    fn parse_jsx_element_or_fragment_inner(&mut self, start: u32, parent: Option<&Expr>) -> Expr {
        let full_start = self
            .pos
            .checked_sub(1)
            .map_or(0, |pos| self.tokens[pos].span.end);
        self.bump(); // <

        // Fragment: <>...</>
        if self.at(TokenKind::GreaterThan) {
            self.bump(); // >
            let opening_span = Span::new(full_start, self.tokens[self.pos - 1].span.end);
            let children = self.parse_jsx_children(None);
            // Expect </>
            if self.is_eof() {
                self.error_at_span(
                    17014,
                    "JSX fragment has no corresponding closing tag.".into(),
                    opening_span,
                );
                self.error_jsx_missing_closing_delimiter();
            } else {
                self.expect(TokenKind::LessThan);
                self.expect(TokenKind::Slash);
                if matches!(
                    self.cur(),
                    TokenKind::GreaterThan
                        | TokenKind::GreaterGreater
                        | TokenKind::GreaterGreaterGreater
                        | TokenKind::GreaterEqual
                        | TokenKind::GreaterGreaterEquals
                        | TokenKind::GreaterGreaterGreaterEquals
                ) {
                    self.expect_jsx_greater_than();
                } else {
                    self.error_code(
                        17015,
                        "Expected corresponding closing tag for JSX fragment.".into(),
                    );
                }
            }
            return Expr {
                kind: ExprKind::JsxFragment(Box::new(JsxFragment { children })),
                span: self.span_from(start),
            };
        }

        // Parse tag name (identifier, or member expression like Foo.Bar).
        // For a bare `<` followed by another tag, synthesize the missing name
        // without consuming that following tag; it belongs to the parent JSX
        // element's children/recovery stream.
        let missing_name = self.at(TokenKind::LessThan) || self.is_eof();
        let name = if missing_name {
            self.error_code(1003, "Identifier expected.".into());
            Expr {
                kind: ExprKind::Ident("<error>".into()),
                span: Span::new(self.cur_span().start, self.cur_span().start),
            }
        } else {
            self.parse_jsx_tag_name()
        };

        // Parse type arguments (e.g. <MyComponent<string> />)
        let type_args = if missing_name {
            None
        } else {
            self.try_parse_type_args()
        };

        // Parse attributes
        let attributes = self.parse_jsx_attributes();

        // Self-closing: />
        if self.at(TokenKind::Slash) {
            self.bump(); // /
            self.expect_jsx_greater_than();
            return Expr {
                kind: ExprKind::JsxSelfClosing(Box::new(JsxSelfClosingElement {
                    name: Box::new(name),
                    type_args,
                    attributes,
                })),
                span: self.span_from(start),
            };
        }

        // A new JSX tag before this opening tag's `>` terminates the malformed
        // tag as self-closing.  Do not consume the next `<`: it may be the
        // ancestor's closing tag or a following sibling.
        if self.at(TokenKind::LessThan) {
            return Expr {
                kind: ExprKind::JsxSelfClosing(Box::new(JsxSelfClosingElement {
                    name: Box::new(name),
                    type_args,
                    attributes,
                })),
                span: self.span_from(start),
            };
        }

        // Computed access is not permitted in a JSX tag name. Recover the tag
        // parsed so far as self-closing and leave `[expr]` to the surrounding
        // expression parser.
        if self.at(TokenKind::OpenBracket) {
            return Expr {
                kind: ExprKind::JsxSelfClosing(Box::new(JsxSelfClosingElement {
                    name: Box::new(name),
                    type_args,
                    attributes,
                })),
                span: self.span_from(start),
            };
        }

        // Scanner-recovery path for multiline quoted JSX attribute values:
        // some token streams collapse `></tag>` into a synthetic StringLiteral
        // token after the attribute value. Treat it as an empty-children element.
        if self.at(TokenKind::StringLiteral) {
            let sp = self.cur_span();
            let text = self.text(sp).trim_start();
            let text = text
                .strip_prefix('"')
                .or_else(|| text.strip_prefix('\''))
                .unwrap_or(text);
            if text.starts_with("></") {
                self.bump();
                return Expr {
                    kind: ExprKind::JsxElement(Box::new(JsxElement {
                        name: Box::new(name),
                        opening_span: Span::new(start, sp.end - text.len() as u32 + 1),
                        closing: None,
                        type_args,
                        attributes,
                        children: Vec::new(),
                        closing_tag_missing: false,
                    })),
                    span: self.span_from(start),
                };
            }
            // A standalone string after the parsed attributes belongs to the
            // outer recovery stream, not to JSX children.  Treat the malformed
            // opening tag as self-closing without consuming the string.
            if !attributes.is_empty() {
                return Expr {
                    kind: ExprKind::JsxSelfClosing(Box::new(JsxSelfClosingElement {
                        name: Box::new(name),
                        type_args,
                        attributes,
                    })),
                    span: self.span_from(start),
                };
            }
        }

        // Opening tag: >
        self.expect_jsx_greater_than();
        let opening_span = self.span_from(start);

        // Parse children
        let children = self.parse_jsx_children(Some(&name));

        let closes_parent = parent.is_some_and(|parent| {
            !jsx_names_match(self.text(name.span), self.text(parent.span), false)
                && self.at(TokenKind::LessThan)
                && self.peek_is(TokenKind::Slash)
                && jsx_names_match(
                    self.text(parent.span),
                    &self.source[self.tokens[self.pos + 1].span.end as usize..],
                    true,
                )
        });
        if self.is_eof() || closes_parent {
            // Diagnose the missing inner close without consuming a parent's
            // closing tag. At EOF, report the shared missing delimiter too.
            self.error_at_span(
                17008,
                format!(
                    "JSX element '{}' has no corresponding closing tag.",
                    self.text(name.span)
                ),
                name.span,
            );
            if self.is_eof() {
                self.error_jsx_missing_closing_delimiter();
            }
            return Expr {
                kind: ExprKind::JsxElement(Box::new(JsxElement {
                    name: Box::new(name),
                    opening_span,
                    closing: None,
                    type_args,
                    attributes,
                    children,
                    closing_tag_missing: true,
                })),
                span: self.span_from(start),
            };
        }

        // Closing tag: </tagName>
        let closing_start = self.cur_span().start;
        self.expect(TokenKind::LessThan);
        self.expect(TokenKind::Slash);
        // Parse and validate the closing tag while retaining recovery tokens.
        let closing_name = if self.cur() == TokenKind::Identifier
            || self.cur().is_keyword()
            || self.cur().is_contextual_keyword()
        {
            Some(self.parse_jsx_tag_name())
        } else {
            None
        };
        if let Some(closing_name) = &closing_name {
            if !jsx_names_match(self.text(name.span), self.text(closing_name.span), false) {
                self.error_at_span(
                    17002,
                    format!(
                        "Expected corresponding JSX closing tag for '{}'.",
                        self.text(name.span)
                    ),
                    closing_name.span,
                );
            }
        }
        // Recovery: malformed namespace closing tags like `</a:ele:ment>` should
        // leave `, ment` for outer parsing (matching tsc recovery output).
        if self.at(TokenKind::Colon)
            && self.tokens.get(self.pos + 1).is_some_and(|t| {
                t.kind == TokenKind::Identifier
                    || t.kind.is_keyword()
                    || t.kind.is_contextual_keyword()
            })
        {
            if closing_name
                .as_ref()
                .is_some_and(|name| matches!(name.kind, ExprKind::Member(_)))
            {
                self.remove_token(self.pos);
            } else {
                self.set_token_kind(self.pos, TokenKind::Comma);
            }
        }
        self.expect_jsx_greater_than();

        Expr {
            kind: ExprKind::JsxElement(Box::new(JsxElement {
                name: Box::new(name),
                opening_span,
                closing: closing_name.map(|name| JsxClosingElement {
                    name: Box::new(name),
                    span: self.span_from(closing_start),
                }),
                type_args,
                attributes,
                children,
                closing_tag_missing: false,
            })),
            span: self.span_from(start),
        }
    }

    fn error_jsx_missing_closing_delimiter(&mut self) {
        // Nested unclosed tags share the same EOF insertion point. Retain
        // each opening-tag error, but report that missing delimiter once.
        let span = self.cur_span();
        if !self.diagnostics.iter().rev().any(|diagnostic| {
            diagnostic.code == 1005
                && diagnostic.span == Some(span)
                && diagnostic.message == "'</' expected."
        }) {
            self.error_code(1005, "'</' expected.".into());
        }
    }

    fn parse_jsx_identifier_name(&mut self) -> Option<(AstString, Span)> {
        if !self.cur().is_identifier_name() {
            return None;
        }
        let start = self.cur_span().start;
        let mut end = self.cur_span().end;
        if matches!(self.source.as_bytes().get(end as usize), Some(b'-' | b'\\')) {
            let mut scanner = TsScanner::new(&self.source[start as usize..]);
            scanner.scan();
            scanner.scan_jsx_identifier();
            end = start + scanner.text_pos() as u32;
        }
        while !self.is_eof() && self.cur_span().end <= end {
            self.bump();
        }
        if self.cur_span().start < end {
            // A JS numeric/operator token can straddle the JSX name boundary,
            // e.g. `a-1.2`. Retain its consumed prefix and rescan the suffix.
            let mut prefix = self.tokens[self.pos].clone();
            prefix.kind = TokenKind::Identifier;
            prefix.span.end = end;
            self.replace_token(self.pos, prefix);
            self.bump();
            self.rescan_tail_from(end);
        }
        let span = Span::new(start, end);
        let name = self.text(span);
        if name.contains('\\') {
            self.error_at_span(
                17021,
                "Unicode escape sequence cannot appear here.".into(),
                span,
            );
        }
        Some((name.into(), span))
    }

    pub(crate) fn parse_jsx_tag_name(&mut self) -> Expr {
        let start = self.cur_span().start;
        let this_token = self.at(TokenKind::This).then(|| self.cur_span());
        let (name, span) = if let Some(name) = self.parse_jsx_identifier_name() {
            name
        } else {
            let span = self.cur_span();
            self.error_code(1003, "Identifier expected.".into());
            if !self.is_eof() {
                self.bump();
            }
            ("<error>".into(), span)
        };
        // Each side of a namespace colon is a JSX identifier. Only one colon
        // is allowed; member access is handled only for non-namespaced tags.
        if self.eat(TokenKind::Colon).is_some() {
            let mut full_name = name.clone();
            full_name.push(':');
            if let Some((part, _)) = self.parse_jsx_identifier_name() {
                full_name.push_str(&part);
            } else {
                self.error_code(1003, "Identifier expected.".into());
            }
            return Expr {
                kind: ExprKind::Ident(full_name.into()),
                span: self.span_from(start),
            };
        }
        let mut expr = Expr {
            // JSX `this` is an expression, including when spelled with an
            // escape. A hyphenated extension or namespace colon makes it an
            // ordinary JSX name instead.
            kind: if this_token == Some(span) {
                ExprKind::This
            } else {
                ExprKind::Ident(name.into())
            },
            span,
        };
        // Handle member expression tag names: Foo.Bar.Baz
        while self.eat(TokenKind::Dot).is_some() {
            let prop = if let Some((name, span)) = self.parse_identifier_name() {
                if name.contains('\\') {
                    self.error_at_span(
                        17021,
                        "Unicode escape sequence cannot appear here.".into(),
                        span,
                    );
                }
                name
            } else {
                self.error_code(1003, "Identifier expected.".into());
                "<error>".into()
            };
            expr = Expr {
                kind: ExprKind::Member(Box::new(MemberExpr {
                    object: Box::new(expr),
                    property: prop.into(),
                    optional: false,
                })),
                span: self.span_from(start),
            };
        }
        expr
    }

    pub(crate) fn parse_jsx_attributes(&mut self) -> Vec<JsxAttribute> {
        let mut attrs = Vec::new();
        loop {
            let start = self.cur_span().start;
            if self.at(TokenKind::GreaterThan)
                || self.at(TokenKind::Slash)
                || self.at(TokenKind::LessThan)
                || self.at(TokenKind::OpenBracket)
                || self.is_eof()
            {
                break;
            }
            // `<b:c.x>` — a stray `.` after a namespaced tag name: tsc skips
            // it with an error and parses `x` as an attribute.
            if self.at(TokenKind::Dot) {
                self.error_code(1003, "Identifier expected.".into());
                self.bump();
                continue;
            }
            // Compound `>` tokens (`>>`, `>>>`, `>>=`, `>>>=`) are produced when
            // a tag close is immediately followed by JSX text starting with `>`
            // (e.g. `<div>>...`). Break here so expect_jsx_greater_than can
            // split the leading `>` off, leaving the rest as JSX text.
            if matches!(
                self.cur(),
                TokenKind::GreaterGreater
                    | TokenKind::GreaterGreaterGreater
                    | TokenKind::GreaterGreaterEquals
                    | TokenKind::GreaterGreaterGreaterEquals
            ) {
                break;
            }
            // `>=` after a string attribute: split into `>` (closes tag) + `=` (text)
            // e.g., className="text-[#6c7086]">= → `>` closes tag, `=` is content
            if self.at(TokenKind::GreaterEqual) {
                let span = self.cur_span();
                let mid = span.start + 1;
                // The `>` half keeps the original token's start, so it keeps whatever
                // trivia preceded it; the `=` half abuts it and never can.
                let nl = self.tokens[self.pos].preceded_by_line_break;
                // Mutate current token to `>` and push `=` after
                self.replace_token(
                    self.pos,
                    Token {
                        kind: TokenKind::GreaterThan,
                        span: Span::new(span.start, mid),
                        preceded_by_line_break: nl,
                    },
                );
                // Insert `=` token after current position
                self.insert_token(
                    self.pos + 1,
                    Token {
                        kind: TokenKind::Equals,
                        span: Span::new(mid, span.end),
                        preceded_by_line_break: false,
                    },
                );
                break;
            }
            if self.at(TokenKind::StringLiteral) {
                let text = self.text(self.cur_span()).trim_start();
                let text = text
                    .strip_prefix('"')
                    .or_else(|| text.strip_prefix('\''))
                    .unwrap_or(text);
                if text.starts_with("></") || text.starts_with("/>") || text.starts_with('>') {
                    break;
                }
                // A quoted token without a preceding `=` cannot be an
                // attribute.  End recovery for this opening tag here and let
                // the string be reparsed by the surrounding expression, as in
                // `<div className"app">`.
                if !attrs.is_empty() {
                    break;
                }
            }
            // Spread attribute: {...expr}
            if self.at(TokenKind::OpenBrace) {
                self.bump(); // {
                self.expect(TokenKind::DotDotDot);
                let expr = self.parse_expression();
                self.expect(TokenKind::CloseBrace);
                attrs.push(JsxAttribute::Spread(Box::new(expr), self.span_from(start)));
                continue;
            }
            // Normal attribute: name or name="value" or name={expr}
            // JSX attribute names can be any identifier or keyword (e.g.
            // `extends`, `class`).  Use is_identifier_name() which is broader.
            if self.is_identifier() || self.cur().is_keyword() {
                let mut attr_name = self.parse_jsx_identifier_name().unwrap().0;
                if self.eat(TokenKind::Colon).is_some() {
                    attr_name.push(':');
                    if let Some((part, _)) = self.parse_jsx_identifier_name() {
                        attr_name.push_str(&part);
                    } else {
                        self.error_code(1003, "Identifier expected.".into());
                        if !matches!(
                            self.cur(),
                            TokenKind::Equals
                                | TokenKind::OpenBrace
                                | TokenKind::Slash
                                | TokenKind::GreaterThan
                        ) {
                            attr_name.push_str("<error>");
                        }
                    }
                }
                let value = if self.eat(TokenKind::Equals).is_some() {
                    if self.at(TokenKind::StringLiteral) {
                        let str_span = self.cur_span();
                        let raw = self.text(str_span);
                        let quote = raw.as_bytes().first().copied();
                        // In JSX attribute strings, backslash is NOT an escape
                        // character. The scanner may mis-tokenize `attr="...\"`
                        // by treating `\"` as an escape and extending the string
                        // across lines. Detect this by checking for newlines.
                        let terminated = quote.is_some_and(|q| {
                            (q == b'"' || q == b'\'')
                                && raw.len() >= 2
                                && raw.as_bytes().last().copied() == Some(q)
                                && !raw.contains('\n')
                        });
                        if terminated {
                            let str_span = self.bump();
                            let raw = self.text(str_span);
                            let content = if raw.len() >= 2 {
                                &raw[1..raw.len() - 1]
                            } else {
                                ""
                            };
                            Some(Box::new(Expr {
                                kind: ExprKind::StrLit(content.to_string().into()),
                                span: str_span,
                            }))
                        } else if let Some(raw_quoted) = self.try_parse_jsx_raw_quoted_attr_value()
                        {
                            Some(Box::new(raw_quoted))
                        } else {
                            let str_span = self.bump();
                            let raw = self.text(str_span);
                            let content = if raw.len() >= 2 {
                                &raw[1..raw.len() - 1]
                            } else {
                                ""
                            };
                            Some(Box::new(Expr {
                                kind: ExprKind::StrLit(content.to_string().into()),
                                span: str_span,
                            }))
                        }
                    } else if let Some(raw_quoted) = self.try_parse_jsx_raw_quoted_attr_value() {
                        Some(Box::new(raw_quoted))
                    } else if self.at(TokenKind::OpenBrace) {
                        let opening = self.bump(); // {
                        let expr = if self.at(TokenKind::CloseBrace) {
                            let closing = self.bump();
                            // Keep an empty container distinct from a boolean
                            // attribute, including its braces for TS17000.
                            Expr {
                                kind: ExprKind::Omitted,
                                span: Span::new(opening.start, closing.end),
                            }
                        } else {
                            // Fresh expression context for JSX attribute value.
                            let saved_ternary = self.in_ternary_consequent;
                            self.in_ternary_consequent = false;
                            let expr = self.parse_expression();
                            self.in_ternary_consequent = saved_ternary;
                            self.expect(TokenKind::CloseBrace);
                            expr
                        };
                        Some(Box::new(expr))
                    } else if self.at(TokenKind::GreaterThan) || self.at(TokenKind::Slash) {
                        // Missing initializer after `=` (e.g. `<div foo= >`).
                        // Don't consume the closing `>` or `/>`; treat as
                        // boolean attribute with no value.
                        self.error_code(1145, "'{' or JSX element expected.".into());
                        None
                    } else if self.is_jsx_start() {
                        let start = self.cur_span().start;
                        Some(Box::new(self.parse_jsx_element_or_fragment(start)))
                    } else {
                        // JSX only permits string literals, expression
                        // containers, or JSX elements after `=`.  For an
                        // unbraced identifier (`<a b=d />`), retain it for the
                        // attribute loop so it recovers as a second boolean
                        // attribute instead of accepting a JavaScript
                        // expression initializer.
                        self.error_code(1145, "'{' or JSX element expected.".into());
                        None
                    }
                } else {
                    None // boolean attribute like `disabled`
                };
                attrs.push(JsxAttribute::Normal {
                    name: attr_name.into(),
                    value,
                    span: self.span_from(start),
                });
            } else {
                // Unknown token in attributes, skip
                self.error_code(1003, "Identifier expected.".into());
                if self.at(TokenKind::StringLiteral) {
                    // A quoted token without `=` is ordinary scanner input,
                    // not a JSX attribute value. Preserve escape diagnostics
                    // while consuming it during recovery.
                    self.bump_string_literal();
                } else {
                    self.bump();
                }
            }
        }
        attrs
    }

    /// JSX string attribute fallback for scanner recovery:
    /// handles quoted attribute values where the scanner mis-tokenized
    /// `\"` as an escape (in JSX, `\` is NOT an escape character).
    fn try_parse_jsx_raw_quoted_attr_value(&mut self) -> Option<Expr> {
        let start = self.cur_span().start as usize;
        if start >= self.source.len() {
            return None;
        }
        let quote = self.source.as_bytes()[start];
        if quote != b'"' && quote != b'\'' {
            return None;
        }

        let bytes = self.source.as_bytes();
        let mut i = start + 1;
        while i < bytes.len() {
            if bytes[i] == quote {
                // In JSX attribute strings, backslash is NOT an escape character.
                // `\"` means literal `\` followed by the closing quote `"`.
                break;
            }
            i += 1;
        }
        let end = if i < bytes.len() { i + 1 } else { bytes.len() };
        let content_end = i.min(bytes.len());
        let content = self.source[start + 1..content_end].to_string();
        let end_u32 = end as u32;

        // The scanner may have produced a single token that extends past our
        // raw string end (because it treated `\"` as an escape and kept scanning).
        // We need to: (1) remove that oversized token, (2) re-scan the remainder
        // to recover the tokens the scanner consumed into the string.
        if !self.is_eof() && self.cur_span().end > end_u32 {
            let token_end = self.cur_span().end as usize;
            // Remove the current (oversized) token
            self.remove_token(self.pos);
            // Re-scan the source from end of our raw string to end of the
            // removed token, and insert the recovered tokens.
            if end < token_end {
                let remainder = &self.source[end..token_end];
                let sub_tokens = tsc_rs_scanner::Scanner::new(remainder).scan_all();
                let offset = end as u32;
                let mut j = 0;
                for t in sub_tokens.into_iter() {
                    // Skip EndOfFile — the sub-scanner adds it but it would
                    // terminate the parser prematurely in the middle of the file.
                    if t.kind == TokenKind::EndOfFile {
                        continue;
                    }
                    self.insert_token(
                        self.pos + j,
                        tsc_rs_scanner::Token {
                            kind: t.kind,
                            span: Span::new(t.span.start + offset, t.span.end + offset),
                            // The sub-scanner saw the same trivia, so its flag carries over.
                            preceded_by_line_break: t.preceded_by_line_break,
                        },
                    );
                    j += 1;
                }
            }
        } else {
            // Normal case: advance past tokens covered by the raw literal.
            while !self.is_eof() && self.cur_span().end <= end_u32 {
                self.bump();
            }
        }

        Some(Expr {
            kind: ExprKind::StrLit(content.into()),
            span: Span::new(start as u32, end_u32),
        })
    }

    /// Like `expect(GreaterThan)` but splits a compound greater token (`>>`,
    /// `>>>`, `>>=`, `>>>=`) when needed. JSX uses `>` to terminate tags, but
    /// the scanner greedily produces `>>` for `<div>>` (open tag close + JSX
    /// text `>`) which would otherwise be lost. Mirrors `eat_greater_than` but
    /// emits a diagnostic when no `>`-shaped token is available.
    fn expect_jsx_greater_than(&mut self) {
        let pos = self.pos;
        let preceded_by_line_break = self.tokens[pos].preceded_by_line_break;
        if let Some(span) = self.eat_greater_than() {
            if self.pos == pos {
                // Keep the consumed prefix in the token stream: JSX text
                // starts at the previous token's end. Without this, splitting
                // `<div>>` would also capture the opening tag's delimiter.
                self.insert_token(
                    pos,
                    Token {
                        kind: TokenKind::GreaterThan,
                        span,
                        preceded_by_line_break,
                    },
                );
                self.bump();
            }
        } else {
            self.expect_error(TokenKind::GreaterThan);
        }
    }

    /// Re-tokenize the source from `boundary` and replace everything the parser
    /// has not consumed yet.
    ///
    /// Used when a token is found to straddle a JSX boundary, which means the
    /// batch scanner lexed that region in the wrong context and every token
    /// after it is suspect. `boundary` always sits on a `<` or `{`, a token
    /// start in any correct tokenization, so re-scanning from there is
    /// well-defined. This is the batch-scanner equivalent of what TypeScript
    /// gets from `reScanJsxToken`.
    fn rescan_tail_from(&mut self, boundary: u32) {
        let source = self.source;
        let base = boundary as usize;
        debug_assert!(base <= source.len());
        let rescanned = Scanner::new(&source[base..]).scan_all();
        self.tokens.truncate(self.pos);
        self.token_kinds.truncate(self.pos);
        self.tokens.extend(rescanned.iter().map(|token| Token {
            kind: token.kind,
            span: Span::new(token.span.start + boundary, token.span.end + boundary),
            preceded_by_line_break: token.preceded_by_line_break,
        }));
        self.token_kinds
            .extend(rescanned.iter().map(|token| token.kind));
        self.debug_assert_token_cursor_invariants();
    }

    fn parse_jsx_children(&mut self, parent: Option<&Expr>) -> Vec<JsxChild> {
        let mut children = Vec::new();
        loop {
            if self.is_eof() {
                // The scanner may have consumed JSX structure as a trailing
                // comment (e.g. `<p>// not a comment</p>` with no newline —
                // `//...</p>;` ran to EOF as a line comment). If there is
                // un-tokenized source between the previous token and the EOF
                // token's position, fall through to text-mode recovery so we
                // can re-scan and find the closing tag.
                let prev_end = if self.pos > 0 {
                    self.tokens[self.pos - 1].span.end
                } else {
                    0
                };
                let cur_start = self.cur_span().start;
                if cur_start <= prev_end {
                    break;
                }
                // Fall through to text-mode block below.
            }
            // Check for closing tag or end of fragment
            if self.at(TokenKind::LessThan) {
                // Look ahead for </ (closing tag)
                if self
                    .tokens
                    .get(self.pos + 1)
                    .is_some_and(|t| t.kind == TokenKind::Slash)
                {
                    // Check for whitespace-only text that the scanner skipped.
                    // This handles cases like `<div>   </div>` where there are
                    // no tokens between `>` and `</`.
                    // Only emit if the last child is NOT a Text node (text capture
                    // already extends to cover trailing whitespace).
                    let last_is_text = matches!(children.last(), Some(JsxChild::Text(..)));
                    if !last_is_text && self.pos > 0 {
                        let prev_end = self.tokens[self.pos - 1].span.end;
                        let cur_start = self.cur_span().start;
                        if cur_start > prev_end {
                            let gap = &self.source[prev_end as usize..cur_start as usize];
                            // Keep non-whitespace text the batch scanner skipped
                            // as a comment, including comments spanning lines.
                            // Multiline indentation alone has no JSX text value.
                            if !gap.is_empty()
                                && (!gap.contains('\n')
                                    || gap
                                        .bytes()
                                        .any(|byte| !matches!(byte, b' ' | b'\t' | b'\r' | b'\n')))
                            {
                                children.push(self.jsx_text_child(
                                    gap.to_string(),
                                    Span::new(prev_end, cur_start),
                                ));
                            }
                        }
                    }
                    break;
                }
                // Check for whitespace-only text gap before nested element
                let last_is_text = matches!(children.last(), Some(JsxChild::Text(..)));
                if !last_is_text && self.pos > 0 {
                    let prev_end = self.tokens[self.pos - 1].span.end;
                    let cur_start = self.cur_span().start;
                    if cur_start > prev_end {
                        let gap = &self.source[prev_end as usize..cur_start as usize];
                        if !gap.is_empty()
                            && (!gap.contains('\n')
                                || gap
                                    .bytes()
                                    .any(|byte| !matches!(byte, b' ' | b'\t' | b'\r' | b'\n')))
                        {
                            children.push(
                                self.jsx_text_child(
                                    gap.to_string(),
                                    Span::new(prev_end, cur_start),
                                ),
                            );
                        }
                    }
                }
                // Nested JSX element
                let start = self.cur_span().start;
                let child = self.parse_jsx_with_parent(start, parent);
                children.push(JsxChild::Element(Box::new(child)));
                continue;
            }
            // Expression container: {expr} or {/* comment */}
            if self.at(TokenKind::OpenBrace) {
                // Check for whitespace-only text gap before `{`
                // Only emit if the last child is NOT a Text node (text capture
                // already extends to cover trailing whitespace).
                let last_is_text = matches!(children.last(), Some(JsxChild::Text(..)));
                if !last_is_text && self.pos > 0 {
                    let prev_end = self.tokens[self.pos - 1].span.end;
                    let cur_start = self.cur_span().start;
                    if cur_start > prev_end {
                        let gap = &self.source[prev_end as usize..cur_start as usize];
                        if !gap.is_empty()
                            && (!gap.contains('\n')
                                || gap
                                    .bytes()
                                    .any(|byte| !matches!(byte, b' ' | b'\t' | b'\r' | b'\n')))
                        {
                            children.push(
                                self.jsx_text_child(
                                    gap.to_string(),
                                    Span::new(prev_end, cur_start),
                                ),
                            );
                        }
                    }
                }
                let start = self.cur_span().start;
                self.bump(); // {
                if self.at(TokenKind::CloseBrace) {
                    self.bump();
                    children.push(JsxChild::Expression(None, self.span_from(start)));
                } else if self.at(TokenKind::LessThan)
                    && self
                        .tokens
                        .get(self.pos + 1)
                        .is_some_and(|t| t.kind == TokenKind::Slash)
                {
                    // A closing tag immediately after `{` cannot begin the
                    // container expression. Recover an empty container and
                    // leave `</...>` for the surrounding JSX element.
                    children.push(JsxChild::Expression(None, self.span_from(start)));
                } else {
                    // JSX expression container creates a fresh expression context —
                    // reset in_ternary_consequent so arrow functions inside don't
                    // get confused by the outer ternary.
                    let saved_ternary = self.in_ternary_consequent;
                    self.in_ternary_consequent = false;
                    let expr = self.parse_expression();
                    self.in_ternary_consequent = saved_ternary;
                    self.expect(TokenKind::CloseBrace);
                    children.push(JsxChild::Expression(
                        Some(Box::new(expr)),
                        self.span_from(start),
                    ));
                }
                continue;
            }
            // Text content. In JSX text, `//` and `/* */` are NOT comments —
            // they are literal characters. The scanner pre-tokenizes the whole
            // source treating those as comments, which can swallow JSX
            // structure (e.g. `<code>https://x/y</code>` → `//x/y</code>` is
            // eaten as a line comment, dropping the closing tag from the token
            // stream). Defend by scanning raw source bytes for the next `<` or
            // `{`, then re-tokenize any byte gap that the original scanner
            // collapsed into a comment.
            let token_start = self.cur_span().start;
            let start = if self.pos > 0 {
                let prev_end = self.tokens[self.pos - 1].span.end;
                prev_end.min(token_start)
            } else {
                token_start
            };
            // Progress floor: when the token cursor can't advance (EOF, or a
            // compound token like `<<` whose first byte is the JSX boundary),
            // `prev_end` stays stale across iterations and text mode would
            // re-capture the same bytes forever (e.g. `const x = <div>` at EOF,
            // or conflict-marker `<<<<<<<` lines). Never re-capture bytes
            // already emitted as a Text child — the `text_end == start` break
            // below then terminates the loop.
            let start = match children.last() {
                Some(JsxChild::Text(_, sp)) => start.max(sp.end),
                _ => start,
            };
            let src_bytes = self.source.as_bytes();
            let src_len = src_bytes.len() as u32;
            let mut text_end = start;
            while text_end < src_len {
                let b = src_bytes[text_end as usize];
                if b == b'<' || b == b'{' {
                    break;
                }
                text_end += 1;
            }
            // Advance the token cursor past tokens that fall before text_end
            // (i.e. the JSX text region we just captured). The original scanner
            // may have produced normal tokens for the text body — we discard
            // them since we use the raw byte slice.
            // A token that STRADDLES text_end proves the scanner lexed this
            // region in the wrong context: no correct tokenization puts a token
            // across a `<` or `{` that opens JSX. It happens on an apostrophe in
            // JSX text — in `<p>Vous n'avez pas ?{' '}</p>` the `'` of `n'avez`
            // pairs with the opening quote of `{' '}`, so StringLiteral
            // `'avez pas ?{'` eats the `{`, and the closing quote opens another
            // literal that eats the rest of the FILE. Splicing just the byte gap
            // (below) cannot fix that: the gap stops at the next token start,
            // which cuts `' '` in half and leaves the runaway literal in place —
            // hence "expected CloseBrace, got StringLiteral", and recovery then
            // emitted invalid JavaScript. Re-scan the whole tail instead.
            let mut straddles_boundary = false;
            while self.pos < self.tokens.len() && self.tokens[self.pos].span.start < text_end {
                straddles_boundary |= self.tokens[self.pos].span.end > text_end;
                self.pos += 1;
            }
            if straddles_boundary {
                self.rescan_tail_from(text_end);
            }
            // If the next remaining token starts after text_end, the scanner
            // swallowed JSX structure as a comment (e.g. `//x</code>`). Re-scan
            // the gap and splice in the recovered tokens so closing tags are
            // visible to the parser.
            else if self.pos < self.tokens.len() {
                let next_tok_start = self.tokens[self.pos].span.start;
                if next_tok_start > text_end {
                    let segment_start = text_end as usize;
                    let segment_end = next_tok_start as usize;
                    let segment = &self.source[segment_start..segment_end];
                    let scanner = Scanner::new(segment);
                    let mut new_tokens = scanner.scan_all();
                    new_tokens.retain(|t| t.kind != TokenKind::EndOfFile);
                    for tok in &mut new_tokens {
                        tok.span = Span::new(
                            tok.span.start + segment_start as u32,
                            tok.span.end + segment_start as u32,
                        );
                    }
                    let insert_at = self.pos;
                    for (i, tok) in new_tokens.into_iter().enumerate() {
                        self.insert_token(insert_at + i, tok);
                    }
                }
            }
            if text_end > start {
                let text = self.source[start as usize..text_end as usize].to_string();
                let text_span = Span::new(start, text_end);
                self.suppress_eager_scanner_diagnostics_in_span(text_span);
                children.push(self.jsx_text_child(text, text_span));
            } else {
                // No text and no `<`/`{` found — break to avoid infinite loop
                // (e.g. malformed input at EOF).
                break;
            }
        }
        children
    }

    fn jsx_text_child(&mut self, text: String, span: Span) -> JsxChild {
        // Check text when captured so diagnostics retain streaming source
        // order across nested elements. This also covers text that the batch
        // scanner skipped as a comment, without decoding entity escapes.
        for (offset, byte) in text.bytes().enumerate() {
            let (code, message) = match byte {
                b'}' => (
                    1381,
                    "Unexpected token. Did you mean `{'}'}` or `&rbrace;`?",
                ),
                b'>' => (1382, "Unexpected token. Did you mean `{'>'}` or `&gt;`?"),
                _ => continue,
            };
            let start = span.start + offset as u32;
            self.error_at_span(code, message.into(), Span::new(start, start + 1));
        }
        JsxChild::Text(text, span)
    }

    pub(crate) fn parse_array_literal(&mut self, start: u32) -> Expr {
        let opening = self.bump(); // [
        let mut elements = Vec::with_capacity(4);
        while !self.at(TokenKind::CloseBracket)
            && !self.at(TokenKind::CloseBrace)
            && !self.at(TokenKind::CloseParen)
            && !self.is_eof()
        {
            if self.at(TokenKind::Comma) {
                elements.push(None);
                self.bump();
                continue;
            }
            elements.push(Some(Box::new(self.parse_assignment_expr())));
            if self.eat(TokenKind::Comma).is_none() {
                // A stray `:` between elements (recovered index-signature
                // text like `[name:string]`) reads as a missing comma in
                // tsc: `[name, string]` plus a "',' expected." error.
                if self.at(TokenKind::Colon) && self.speculation_depth == 0 {
                    self.error_code(1005, "',' expected.".into());
                    self.bump();
                    continue;
                }
                // Error recovery: if the next token could start a new element
                // (identifier, literal, [, {, etc.), insert an implied comma
                // and continue parsing. This matches TypeScript's recovery for
                // `[1, 2, 3\n4, 5, 6, 7]` → `[1, 2, 3, 4, 5, 6, 7]`.
                if !self.at(TokenKind::CloseBracket)
                    && !self.is_eof()
                    && self.cur_could_start_expr()
                {
                    continue;
                }
                break;
            }
        }
        // A mismatched closer stays available to the enclosing expression.
        self.expect_matching_delimiter(
            Some(opening),
            TokenKind::OpenBracket,
            TokenKind::CloseBracket,
        );
        Expr {
            kind: ExprKind::ArrayLit(elements),
            span: self.span_from(start),
        }
    }

    pub(crate) fn parse_object_literal(&mut self, start: u32) -> Expr {
        let opening = self.bump(); // {
        let mut props = Vec::with_capacity(8);
        while !self.at(TokenKind::CloseBrace) && !self.is_eof() {
            // Skip extra commas (error recovery: `{ x: 0,, }` keeps only the
            // trailing comma and drops the extra one).
            if self.at(TokenKind::Comma) {
                self.bump();
                continue;
            }
            // Template literals can't be property names — close the object
            // so the template becomes a tagged template on the object result.
            // E.g. `{ \`a\`: 321 }` → `{} \`a\`; 321;`
            if matches!(
                self.cur(),
                TokenKind::NoSubstitutionTemplate | TokenKind::TemplateHead
            ) {
                break;
            }
            let before = self.pos;
            if let Some(prop) = self.parse_object_lit_element() {
                props.push(prop);
            }

            // Error recovery: if we didn't advance, skip the current token
            if self.pos == before && !self.is_eof() {
                self.error_code(1136, "Property assignment expected.".into());
                self.bump();
            }

            // Accept both `,` and `;` as separators (`;` for error recovery —
            // TypeScript parses `{ a; b; c }` as shorthand properties separated
            // by semicolons and emits them with commas in JS output).
            // In TSX recovery around malformed `... />;` tails, a semicolon here
            // is usually the statement terminator (not an object member separator).
            if self.at(TokenKind::Semicolon)
                && self
                    .tokens
                    .get(self.pos.wrapping_sub(1))
                    .is_some_and(|t| t.kind == TokenKind::GreaterThan)
            {
                break;
            }
            if self.eat(TokenKind::Comma).is_none() && self.eat(TokenKind::Semicolon).is_none() {
                // TypeScript recovers from missing commas between object literal
                // members if the next token could start a new property (e.g.
                // `{ 2: 1  2: 1 }` is parsed as two properties with an implied comma).
                if self.cur_could_start_obj_prop() {
                    continue;
                }
                break;
            }
        }
        self.expect_matching_delimiter(Some(opening), TokenKind::OpenBrace, TokenKind::CloseBrace);
        Expr {
            kind: ExprKind::ObjectLit(props),
            span: self.span_from(start),
        }
    }

    pub(crate) fn parse_object_lit_element(&mut self) -> Option<ObjLitProp> {
        // Skip illegal access/class modifiers on object literal members.
        // TypeScript parses these (reports an error) and strips them on emit.
        while matches!(
            self.cur(),
            TokenKind::Public
                | TokenKind::Private
                | TokenKind::Protected
                | TokenKind::Abstract
                | TokenKind::Override
                | TokenKind::Readonly
                | TokenKind::Static
                | TokenKind::Export
        ) && self.peek_could_start_prop_name()
        {
            self.bump();
        }
        // Start span AFTER any skipped modifiers so they are excluded.
        let start = self.cur_span().start;

        // Spread: ...expr
        if self.at(TokenKind::DotDotDot) {
            self.bump();
            let expr = if self.at(TokenKind::Asterisk) {
                let operator = self.bump();
                let right = self.parse_unary_expr();
                Expr {
                    kind: ExprKind::Binary(BinaryExpr {
                        left: Box::new(Expr {
                            kind: ExprKind::Omitted,
                            span: Span::new(operator.start, operator.start),
                        }),
                        op: BinaryOp::Mul,
                        right: Box::new(right),
                    }),
                    span: self.span_from(operator.start),
                }
            } else {
                self.parse_assignment_expr()
            };
            return Some(ObjLitProp::Spread(Box::new(expr), self.span_from(start)));
        }

        // Get/Set accessor
        if (self.at(TokenKind::Get) || self.at(TokenKind::Set)) && self.peek_could_start_prop_name()
        {
            let is_get = self.at(TokenKind::Get);
            self.bump();
            let name = self
                .parse_property_name()
                .unwrap_or(PropName::Ident("<error>".into(), self.cur_span()));
            let missing_param_list = self.at(TokenKind::Comma);
            let params = if missing_param_list {
                Vec::new()
            } else {
                self.parse_param_list()
            };
            let return_type = if self.eat(TokenKind::Colon).is_some() {
                Some(self.parse_js_checked_return_type_annotation())
            } else {
                None
            };
            // Invalid comma-terminated accessors are emitted with an empty
            // body. Keep the separator in place so the following property is
            // not parsed as part of a synthetic block.
            let body = if missing_param_list || self.at(TokenKind::Comma) {
                Vec::new()
            } else {
                self.parse_block_body()
            };
            return if is_get {
                Some(ObjLitProp::Get(ObjAccessor {
                    name,
                    params,
                    return_type,
                    body,
                    span: self.span_from(start),
                }))
            } else {
                Some(ObjLitProp::Set(ObjAccessor {
                    name,
                    params,
                    return_type,
                    body,
                    span: self.span_from(start),
                }))
            };
        }

        // Async/generator method
        let is_async = self.at(TokenKind::Async)
            && (self.peek_could_start_prop_name() || self.peek_is(TokenKind::Asterisk))
            && !self.peek_is(TokenKind::Colon);
        if is_async {
            self.bump();
        }
        let is_generator = self.eat(TokenKind::Asterisk).is_some();

        let Some(name) = self.parse_property_name() else {
            // Failed to parse property name.
            return None;
        };

        // Definite-assignment markers are invalid in object literals, but the
        // following shorthand or method is still parsed normally.
        if self.at(TokenKind::Excl)
            && self.tokens.get(self.pos + 1).is_some_and(|token| {
                matches!(
                    token.kind,
                    TokenKind::Comma | TokenKind::OpenParen | TokenKind::LessThan
                )
            })
        {
            self.bump();
        }

        // Skip optional `?` marker on methods: `{ foo?() {} }` → `{ foo() {} }`
        if self.at(TokenKind::Question)
            && self
                .tokens
                .get(self.pos + 1)
                .is_some_and(|t| t.kind == TokenKind::OpenParen || t.kind == TokenKind::LessThan)
        {
            self.bump(); // consume `?`
        }

        // Method shorthand: { name() {} }
        if is_generator || is_async || self.at(TokenKind::OpenParen) || self.at(TokenKind::LessThan)
        {
            let type_params = self.try_parse_type_params();
            let params = self.parse_param_list();
            let return_type = if self.eat(TokenKind::Colon).is_some() {
                Some(self.parse_js_checked_return_type_annotation())
            } else {
                None
            };
            // A call signature is not valid in an object literal, but tsc
            // recovers a comma-terminated signature as an empty method. Do
            // not let block recovery consume the following method's body.
            let body = if self.at(TokenKind::Comma) {
                Vec::new()
            } else {
                self.parse_block_body()
            };
            return Some(ObjLitProp::Method(ObjMethod {
                name,
                type_params,
                params,
                return_type,
                body,
                is_generator,
                is_async,
                span: self.span_from(start),
            }));
        }

        // Shorthand property with default: { name = expr }
        if self.at(TokenKind::Equals) {
            if let PropName::Ident(ref n, _) = name {
                self.bump(); // =
                let default_expr = Box::new(self.parse_assignment_expr());
                return Some(ObjLitProp::ShorthandDefault(
                    n.clone(),
                    default_expr,
                    self.span_from(start),
                ));
            }
        }

        // Optional property: { x?: value } → { x: value } (strip `?` marker)
        let question_token = if self.at(TokenKind::Question)
            && self
                .tokens
                .get(self.pos + 1)
                .is_some_and(|t| t.kind == TokenKind::Colon)
        {
            self.bump(); // consume `?`
            true
        } else {
            false
        };

        // Shorthand property: { name } or { name? } (strip optional `?`)
        if !self.at(TokenKind::Colon) {
            // Eat optional `?` for shorthand properties (error recovery).
            // `{ name? }` → `{ name }` (TypeScript strips the `?`).
            // Record span before eating `?` so the span doesn't include it.
            let span_before_question = self.span_from(start);
            if !question_token && self.at(TokenKind::Question) {
                self.bump(); // consume `?`
            }
            if let PropName::Ident(ref n, _) = name {
                return Some(ObjLitProp::Shorthand(n.clone(), span_before_question));
            }
        }

        // Regular property: { name: value }
        let had_colon = self.eat(TokenKind::Colon).is_some();
        if !had_colon {
            self.expect_error(TokenKind::Colon);
        }
        // Recovery: `{ "x" }` should emit as `{ "x":  }` and keep `}` as the
        // object terminator (don't consume it as a value expression).
        let value = if !had_colon && self.at(TokenKind::CloseBrace) {
            Box::new(Expr {
                kind: ExprKind::Omitted,
                span: Span::new(self.cur_span().start, self.cur_span().start),
            })
        } else {
            Box::new(self.parse_assignment_expr())
        };
        let computed = matches!(&name, PropName::Computed(_, _));
        Some(ObjLitProp::Property(ObjProp {
            key: name,
            value,
            computed,
            question_token,
            span: self.span_from(start),
        }))
    }

    pub(crate) fn parse_function_expr(&mut self, start: u32) -> Expr {
        let is_async = self.eat(TokenKind::Async).is_some();
        self.expect(TokenKind::Function);
        let is_generator = self.eat(TokenKind::Asterisk).is_some();
        let (name, name_span) = if self.is_identifier() {
            let (n, s) = self.parse_identifier();
            (Some(n), Some(s))
        } else {
            (None, None)
        };
        let type_params = self.try_parse_js_checked_type_params();
        let params = self.parse_param_list();
        let return_type = if self.eat(TokenKind::Colon).is_some() {
            Some(self.parse_js_checked_return_type_annotation())
        } else {
            None
        };
        let body = if self.at(TokenKind::OpenBrace) {
            Some(self.parse_block_body())
        } else {
            None
        };
        Expr {
            kind: ExprKind::FnExpr(Box::new(FnDecl {
                name: name.map(Into::into),
                name_span,
                type_params,
                params,
                return_type,
                body,
                modifiers: if is_async { MOD_ASYNC } else { MOD_NONE },
                is_generator,
                is_async,
                decorators: Vec::new(),
                span: self.span_from(start),
            })),
            span: self.span_from(start),
        }
    }

    pub(crate) fn parse_template_literal(&mut self, is_tagged: bool) -> TemplateLit {
        let mut quasis = Vec::with_capacity(2);
        let mut exprs = Vec::with_capacity(2);

        // NoSubstitutionTemplate: just `content` with no ${} expressions
        if self.at(TokenKind::NoSubstitutionTemplate) {
            let span = self.bump_template_chunk(is_tagged);
            let raw = self.text(span);
            // Strip opening ` and closing `
            let content = if raw.len() >= 2 {
                &raw[1..raw.len() - 1]
            } else {
                ""
            };
            quasis.push(TemplateElement {
                raw: content.to_string(),
                cooked: Some(content.to_string()),
                tail: true,
                span,
            });
            return TemplateLit { quasis, exprs };
        }

        // Head (TemplateHead): `content${
        let head_span = self.bump_template_chunk(is_tagged);
        let head_raw = self.text(head_span);
        // Strip ` from start and ${ from end
        let head_content = if head_raw.len() >= 3 {
            &head_raw[1..head_raw.len() - 2]
        } else {
            ""
        };
        quasis.push(TemplateElement {
            raw: head_content.to_string(),
            cooked: Some(head_content.to_string()),
            tail: false,
            span: head_span,
        });

        loop {
            // Empty interpolation `${}` / `${ }`: the batch scanner has already
            // re-scanned the closing brace into a continuation token — don't
            // let parse_expression consume it as an error-recovery victim.
            if self.at(TokenKind::TemplateMiddle) || self.at(TokenKind::TemplateTail) {
                let pos = self.cur_span().start;
                exprs.push(Box::new(Expr {
                    kind: ExprKind::Omitted,
                    span: Span::new(pos, pos),
                }));
            } else {
                // Expression
                let expr = self.parse_expression();
                exprs.push(Box::new(expr));
            }

            // The batch scanner pre-rescans `}` into TemplateTail/TemplateMiddle,
            // so after the expression we directly see the continuation token.
            if self.at(TokenKind::TemplateTail) {
                let span = self.bump_template_chunk(is_tagged);
                let raw = self.text(span);
                let c = if raw.len() >= 2 {
                    &raw[1..raw.len() - 1]
                } else {
                    ""
                };
                quasis.push(TemplateElement {
                    raw: c.to_string(),
                    cooked: Some(c.to_string()),
                    tail: true,
                    span,
                });
                break;
            } else if self.at(TokenKind::TemplateMiddle) {
                let span = self.bump_template_chunk(is_tagged);
                let raw = self.text(span);
                let c = if raw.len() >= 3 {
                    &raw[1..raw.len() - 2]
                } else {
                    ""
                };
                quasis.push(TemplateElement {
                    raw: c.to_string(),
                    cooked: Some(c.to_string()),
                    tail: false,
                    span,
                });
            } else {
                // End of template (error recovery)
                quasis.push(TemplateElement {
                    raw: String::new(),
                    cooked: Some(String::new()),
                    tail: true,
                    span: self.cur_span(),
                });
                break;
            }
        }

        TemplateLit { quasis, exprs }
    }

    pub(crate) fn parse_regexp_literal(&mut self, start: u32) -> Expr {
        // The batch scanner tokenizes `/` as Slash. We need to walk the raw
        // source from the opening `/` to find the matching closing `/` and
        // any flags, then advance past all covered tokens.
        let regex_start = start as usize;
        let src = self.source.as_bytes();
        let mut pos = regex_start + 1; // skip opening /
        let mut in_char_class = false;
        let mut found_closing = false;
        let is_line_terminator = |position: usize| {
            matches!(src[position], b'\n' | b'\r')
                || src[position] == 0xe2
                    && src.get(position + 1) == Some(&0x80)
                    && matches!(src.get(position + 2), Some(0xa8 | 0xa9))
        };

        while pos < src.len() {
            let ch = src[pos];
            if is_line_terminator(pos) {
                break; // unterminated regex
            }
            if ch == b'\\' && pos + 1 < src.len() {
                if is_line_terminator(pos + 1) {
                    pos += 1;
                    break; // line continuations are not valid in regex literals
                }
                pos += 2; // skip escaped char
                continue;
            }
            if ch == b'[' {
                in_char_class = true;
            }
            if ch == b']' {
                in_char_class = false;
            }
            if ch == b'/' && !in_char_class {
                pos += 1; // skip closing /
                found_closing = true;
                // consume flags (g, i, m, s, u, y, d, v)
                // Also consume non-ASCII bytes for non-BMP Unicode flag chars.
                while pos < src.len() && (src[pos].is_ascii_alphabetic() || src[pos] >= 0x80) {
                    pos += 1;
                }
                break;
            }
            pos += 1;
        }

        // Unterminated regex: truncate at the first `)` or `;` that could
        // be part of the enclosing expression (e.g. call argument closing).
        // This preserves `/notregexp` as the regex body while letting `)` and
        // `;` be parsed normally as tokens.
        if !found_closing {
            // Find a reasonable end point: first `)` or `;` in the scanned range
            let body_end = {
                let mut p = regex_start + 1;
                while p < pos {
                    if src[p] == b')' || src[p] == b';' {
                        break;
                    }
                    p += 1;
                }
                p
            };
            // Only advance past tokens within the body range
            while self.pos < self.tokens.len()
                && (self.tokens[self.pos].span.start as usize) < body_end
            {
                self.pos += 1;
            }
            self.error_at_span(
                1161,
                "Unterminated regular expression literal.".to_string(),
                Span::new(start, body_end as u32),
            );
            self.suppress_eager_scanner_diagnostics_in_span(Span::new(start, body_end as u32));
            let body = &self.source[regex_start + 1..body_end];
            return Expr {
                kind: ExprKind::RegexpLit(Box::new(RegexpLitExpr {
                    pattern: body.to_string().into(),
                    flags: String::new().into(),
                })),
                span: Span::new(start, body_end as u32),
            };
        }

        // Advance the token-level position past all tokens covered by this regex
        while self.pos < self.tokens.len() && (self.tokens[self.pos].span.start as usize) < pos {
            self.pos += 1;
        }

        let raw = &self.source[regex_start..pos];
        // Split into body and flags at the last `/`
        let (body, flags) = if let Some(last_slash) = raw.rfind('/') {
            if last_slash > 0 {
                (&raw[1..last_slash], &raw[last_slash + 1..])
            } else {
                (&raw[1..], "")
            }
        } else {
            (&raw[1..], "")
        };

        let span = Span::new(start, pos as u32);
        self.suppress_eager_scanner_diagnostics_in_span(span);
        Expr {
            kind: ExprKind::RegexpLit(Box::new(RegexpLitExpr {
                pattern: body.to_string().into(),
                flags: flags.to_string().into(),
            })),
            span,
        }
    }

    #[allow(clippy::vec_box)] // matches AST types that use Vec<Box<Expr>>
    pub(crate) fn parse_arguments(&mut self) -> Vec<Box<Expr>> {
        self.expect(TokenKind::OpenParen);
        let mut args = Vec::with_capacity(6);
        while !self.at(TokenKind::CloseParen) && !self.is_eof() {
            // Skip omitted arguments: `foo(a,,b)` → skip the empty slot
            if self.at(TokenKind::Comma) {
                let sp = self.cur_span();
                args.push(Box::new(Expr {
                    kind: ExprKind::Omitted,
                    span: Span {
                        start: sp.start,
                        end: sp.start,
                    },
                }));
                self.bump(); // consume `,`
                continue;
            }
            // Error recovery: if the current token is a statement keyword
            // (return, break, continue, throw), close the argument list
            // instead of consuming the keyword as an argument expression.
            // E.g. `bar(\n   return x;` → `bar();\n   return x;`
            if matches!(
                self.cur(),
                TokenKind::Return | TokenKind::Break | TokenKind::Continue | TokenKind::Throw
            ) {
                break;
            }
            args.push(Box::new(self.parse_assignment_expr()));
            if self.eat(TokenKind::Comma).is_some() {
                continue;
            }
            if self.at(TokenKind::FatArrow) && self.peek_is(TokenKind::OpenBrace) {
                self.bump();
                args.push(Box::new(self.parse_recovered_arrow_block_object_literal()));
                if self.eat(TokenKind::Comma).is_some() {
                    continue;
                }
            }
            if self.at(TokenKind::CloseParen) || self.is_eof() {
                break;
            }
            // Recovery: treat adjacent expression starts as missing commas in
            // malformed calls like `foo(public blaz() {})`.
            if !self.can_start_argument_expression() {
                break;
            }
        }
        self.expect(TokenKind::CloseParen);
        args
    }

    fn parse_recovered_arrow_block_object_literal(&mut self) -> Expr {
        let start = self.cur_span().start;
        let stmts = self.parse_block_body();
        let props = stmts
            .into_iter()
            .filter_map(|stmt| {
                let span = stmt.span;
                match stmt.kind {
                    StmtKind::Return(Some(value)) => Some(ObjLitProp::Property(ObjProp {
                        key: PropName::Ident(
                            "return".into(),
                            Span::new(span.start, span.start + "return".len() as u32),
                        ),
                        value,
                        computed: false,
                        question_token: false,
                        span,
                    })),
                    _ => None,
                }
            })
            .collect();
        Expr {
            kind: ExprKind::ObjectLit(props),
            span: self.span_from(start),
        }
    }

    fn can_start_argument_expression(&self) -> bool {
        self.is_identifier()
            || matches!(
                self.cur(),
                TokenKind::NumericLiteral
                    | TokenKind::BigIntLiteral
                    | TokenKind::StringLiteral
                    | TokenKind::NoSubstitutionTemplate
                    | TokenKind::TemplateHead
                    | TokenKind::True
                    | TokenKind::False
                    | TokenKind::Null
                    | TokenKind::This
                    | TokenKind::Super
                    | TokenKind::OpenParen
                    | TokenKind::OpenBracket
                    | TokenKind::OpenBrace
                    | TokenKind::Function
                    | TokenKind::Class
                    | TokenKind::New
                    | TokenKind::Import
                    | TokenKind::Plus
                    | TokenKind::Minus
                    | TokenKind::Tilde
                    | TokenKind::Excl
                    | TokenKind::Await
                    | TokenKind::Yield
                    | TokenKind::Delete
                    | TokenKind::TypeOf
                    | TokenKind::Void
                    | TokenKind::LessThan
            )
    }
}
