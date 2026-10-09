//! Type annotation and type parameter parsing methods.

use super::*;

impl<'a> Parser<'a> {
    // -----------------------------------------------------------------------
    // Type annotations
    // -----------------------------------------------------------------------

    /// After bumping a keyword token that could be a parameter name,
    /// check if `is` follows and parse as a type predicate if so.
    fn try_type_predicate(&mut self, name: &str, start: u32) -> Option<TypeNode> {
        if self.at(TokenKind::Is) {
            self.bump();
            // Disable nested type predicates in the predicate's type.
            let saved_pred = self.allow_type_predicate;
            self.allow_type_predicate = false;
            let ty = self.parse_type();
            self.allow_type_predicate = saved_pred;
            Some(TypeNode {
                kind: TypeNodeKind::Predicate(Box::new(PredicateTypeNode {
                    param_name: name.to_string(),
                    type_ann: Some(Box::new(ty)),
                    asserts: false,
                })),
                span: self.span_from(start),
            })
        } else {
            None
        }
    }

    /// Parse a type in return-type position where type predicates are allowed.
    pub(crate) fn parse_return_type(&mut self) -> TypeNode {
        let saved = self.allow_type_predicate;
        let saved_dcl = self.disallow_conditional_types;
        self.allow_type_predicate = true;
        // Re-allow conditional types in return type position — the return
        // type of a function type is a distinct syntactic context.
        self.disallow_conditional_types = false;
        let ty = self.parse_type();
        self.allow_type_predicate = saved;
        self.disallow_conditional_types = saved_dcl;
        ty
    }

    pub(crate) fn parse_type(&mut self) -> TypeNode {
        let start = self.cur_span().start;
        let ty = self.parse_union_or_intersection_type();

        // Check for conditional type: T extends U ? V : W
        // When disallow_conditional_types is set (e.g. inside `infer T extends <constraint>`),
        // do not consume extends/? as a conditional type.
        if !self.disallow_conditional_types && self.eat(TokenKind::Extends).is_some() {
            // The extends type disallows nested conditional types so that the
            // `?` terminator is unambiguous.
            let saved_dcl = self.disallow_conditional_types;
            self.disallow_conditional_types = true;
            let extends_type = self.parse_type();
            self.disallow_conditional_types = saved_dcl;
            self.expect(TokenKind::Question);
            let true_type = self.parse_type();
            self.expect(TokenKind::Colon);
            let false_type = self.parse_type();
            return TypeNode {
                kind: TypeNodeKind::Conditional(Box::new(ConditionalTypeNode {
                    check: Box::new(ty),
                    extends: Box::new(extends_type),
                    true_type: Box::new(true_type),
                    false_type: Box::new(false_type),
                })),
                span: self.span_from(start),
            };
        }

        ty
    }

    pub(crate) fn parse_union_or_intersection_type(&mut self) -> TypeNode {
        let start = self.cur_span().start;
        self.eat(TokenKind::Bar); // leading |
        let first = self.parse_intersection_type();

        if self.at(TokenKind::Bar) {
            let mut types = vec![first];
            while self.eat(TokenKind::Bar).is_some() {
                // A predicate is only valid as the entire return type, not as
                // a union constituent. Leave a following `is T` for ordinary
                // error recovery (`A | b is T` -> erased function, then
                // `is; T; ...`).
                let saved_predicate = self.allow_type_predicate;
                self.allow_type_predicate = false;
                types.push(self.parse_intersection_type());
                self.allow_type_predicate = saved_predicate;
            }
            return TypeNode {
                kind: TypeNodeKind::Union(types),
                span: self.span_from(start),
            };
        }

        first
    }

    pub(crate) fn parse_intersection_type(&mut self) -> TypeNode {
        let start = self.cur_span().start;
        self.eat(TokenKind::Ampersand); // leading &
        let first = self.parse_type_operator_or_primary();

        if self.at(TokenKind::Ampersand) {
            let mut types = vec![first];
            while self.eat(TokenKind::Ampersand).is_some() {
                types.push(self.parse_type_operator_or_primary());
            }
            return TypeNode {
                kind: TypeNodeKind::Intersection(types),
                span: self.span_from(start),
            };
        }

        first
    }

    pub(crate) fn parse_type_operator_or_primary(&mut self) -> TypeNode {
        let start = self.cur_span().start;

        match self.cur() {
            TokenKind::Question => {
                self.bump();
                let inner = if self.is_jsdoc_nullable_type_terminator() {
                    None
                } else {
                    Some(Box::new(self.parse_type_operator_or_primary()))
                };
                TypeNode {
                    kind: TypeNodeKind::JSDocNullable(inner),
                    span: self.span_from(start),
                }
            }
            TokenKind::Excl => {
                self.bump();
                if self.is_recoverable_prefix_type_operator_terminator() {
                    TypeNode {
                        kind: TypeNodeKind::Keyword(KeywordTypeKind::Any),
                        span: self.span_from(start),
                    }
                } else {
                    let mut ty = self.parse_type_operator_or_primary();
                    ty.span = self.span_from(start);
                    ty
                }
            }
            TokenKind::Keyof => {
                self.bump();
                let ty = self.parse_type_operator_or_primary();
                TypeNode {
                    kind: TypeNodeKind::Keyof(Box::new(ty)),
                    span: self.span_from(start),
                }
            }
            TokenKind::Unique => {
                self.bump();
                let ty = self.parse_type_operator_or_primary();
                TypeNode {
                    kind: TypeNodeKind::Unique(Box::new(ty)),
                    span: self.span_from(start),
                }
            }
            TokenKind::Readonly => {
                self.bump();
                let ty = self.parse_type_operator_or_primary();
                TypeNode {
                    kind: TypeNodeKind::Readonly(Box::new(ty)),
                    span: self.span_from(start),
                }
            }
            TokenKind::Infer => {
                self.bump();
                let (name, _) = self.parse_identifier();
                // Speculative lookahead for `infer T extends U ?`:
                // When conditional types are allowed (the normal case), if
                // `extends TYPE` is followed by `?`, treat it as a conditional
                // type rather than an infer constraint — backtrack so the
                // outer `parse_type` sees `extends TYPE ? TRUE : FALSE`.
                //
                // When `disallow_conditional_types` is set (inside the extends
                // clause of a conditional type), `?` cannot start a nested
                // conditional, so `extends TYPE` is always the infer constraint.
                let constraint = if self.at(TokenKind::Extends) {
                    if self.disallow_conditional_types {
                        // Conditional types not allowed here, so `extends TYPE`
                        // is always the infer constraint.
                        self.bump(); // extends
                        Some(Box::new(self.parse_type()))
                    } else {
                        // Speculative parse: try `extends TYPE` then check for `?`.
                        let saved = self.pos;
                        self.bump(); // extends
                                     // Parse the constraint type with conditional types
                                     // disallowed (same as the extends clause of a
                                     // conditional type).
                        let saved_dcl = self.disallow_conditional_types;
                        self.disallow_conditional_types = true;
                        let ty = self.parse_type();
                        self.disallow_conditional_types = saved_dcl;
                        if self.at(TokenKind::Question) {
                            // `infer U extends TYPE ?` — conditional, not constraint.
                            self.rewind_to(saved);
                            None
                        } else {
                            Some(Box::new(ty))
                        }
                    }
                } else {
                    None
                };
                TypeNode {
                    kind: TypeNodeKind::Infer(name.into(), constraint),
                    span: self.span_from(start),
                }
            }
            _ => self.parse_postfix_type(),
        }
    }

    fn is_start_of_type(&self) -> bool {
        self.is_identifier()
            || matches!(
                self.cur(),
                TokenKind::Void
                    | TokenKind::Null
                    | TokenKind::This
                    | TokenKind::TypeOf
                    | TokenKind::OpenBrace
                    | TokenKind::OpenBracket
                    | TokenKind::OpenParen
                    | TokenKind::LessThan
                    | TokenKind::Bar
                    | TokenKind::Ampersand
                    | TokenKind::New
                    | TokenKind::StringLiteral
                    | TokenKind::NumericLiteral
                    | TokenKind::BigIntLiteral
                    | TokenKind::True
                    | TokenKind::False
                    | TokenKind::Asterisk
                    | TokenKind::Question
                    | TokenKind::Excl
                    | TokenKind::DotDotDot
                    | TokenKind::Import
                    | TokenKind::Function
                    | TokenKind::NoSubstitutionTemplate
                    | TokenKind::TemplateHead
            )
            || self.at(TokenKind::Minus)
                && self.tokens.get(self.pos + 1).is_some_and(|token| {
                    matches!(
                        token.kind,
                        TokenKind::NumericLiteral | TokenKind::BigIntLiteral
                    )
                })
    }

    pub(crate) fn parse_postfix_type(&mut self) -> TypeNode {
        let start = self.cur_span().start;
        let mut ty = self.parse_primary_type();

        loop {
            if self.at(TokenKind::OpenBracket) && !self.is_on_new_line() {
                self.bump();
                if !self.is_start_of_type() {
                    // An invalid index start (including a private name) belongs
                    // to the surrounding recovery stream. This is an array
                    // suffix with a missing `]`, not an indexed access.
                    self.expect(TokenKind::CloseBracket);
                    ty = TypeNode {
                        kind: TypeNodeKind::Array(Box::new(ty)),
                        span: self.span_from(start),
                    };
                } else {
                    let index = self.parse_type();
                    self.expect(TokenKind::CloseBracket);
                    ty = TypeNode {
                        kind: TypeNodeKind::IndexedAccess(Box::new(ty), Box::new(index)),
                        span: self.span_from(start),
                    };
                }
            } else if self.at(TokenKind::Question)
                && !self.is_on_new_line()
                && !self.disallow_conditional_types
                && self.next_token_can_end_recoverable_postfix_type_operator()
            {
                self.bump();
                ty = TypeNode {
                    kind: TypeNodeKind::JSDocNullable(Some(Box::new(ty))),
                    span: self.span_from(start),
                };
            } else if self.at(TokenKind::Excl)
                && !self.is_on_new_line()
                && self.next_token_can_end_recoverable_postfix_type_operator()
            {
                self.bump();
                ty.span = self.span_from(start);
            } else {
                break;
            }
        }

        ty
    }

    pub(crate) fn parse_primary_type(&mut self) -> TypeNode {
        let start = self.cur_span().start;

        match self.cur() {
            // Keyword types
            TokenKind::Any if !self.peek_is(TokenKind::Dot) => {
                let name = self.text(self.cur_span()).to_string();
                self.bump();
                if let Some(pred) = self.try_type_predicate(&name, start) {
                    return pred;
                }
                TypeNode {
                    kind: TypeNodeKind::Keyword(KeywordTypeKind::Any),
                    span: self.span_from(start),
                }
            }
            TokenKind::UnknownKeyword if !self.peek_is(TokenKind::Dot) => {
                let name = self.text(self.cur_span()).to_string();
                self.bump();
                if let Some(pred) = self.try_type_predicate(&name, start) {
                    return pred;
                }
                TypeNode {
                    kind: TypeNodeKind::Keyword(KeywordTypeKind::Unknown),
                    span: self.span_from(start),
                }
            }
            TokenKind::Number if !self.peek_is(TokenKind::Dot) => {
                let name = self.text(self.cur_span()).to_string();
                self.bump();
                if let Some(pred) = self.try_type_predicate(&name, start) {
                    return pred;
                }
                TypeNode {
                    kind: TypeNodeKind::Keyword(KeywordTypeKind::Number),
                    span: self.span_from(start),
                }
            }
            TokenKind::BigInt if !self.peek_is(TokenKind::Dot) => {
                let name = self.text(self.cur_span()).to_string();
                self.bump();
                if let Some(pred) = self.try_type_predicate(&name, start) {
                    return pred;
                }
                TypeNode {
                    kind: TypeNodeKind::Keyword(KeywordTypeKind::BigInt),
                    span: self.span_from(start),
                }
            }
            TokenKind::String if !self.peek_is(TokenKind::Dot) => {
                let name = self.text(self.cur_span()).to_string();
                self.bump();
                if let Some(pred) = self.try_type_predicate(&name, start) {
                    return pred;
                }
                TypeNode {
                    kind: TypeNodeKind::Keyword(KeywordTypeKind::String),
                    span: self.span_from(start),
                }
            }
            TokenKind::Boolean if !self.peek_is(TokenKind::Dot) => {
                let name = self.text(self.cur_span()).to_string();
                self.bump();
                if let Some(pred) = self.try_type_predicate(&name, start) {
                    return pred;
                }
                TypeNode {
                    kind: TypeNodeKind::Keyword(KeywordTypeKind::Boolean),
                    span: self.span_from(start),
                }
            }
            TokenKind::Void => {
                let name = self.text(self.cur_span()).to_string();
                self.bump();
                if let Some(pred) = self.try_type_predicate(&name, start) {
                    return pred;
                }
                TypeNode {
                    kind: TypeNodeKind::Keyword(KeywordTypeKind::Void),
                    span: self.span_from(start),
                }
            }
            TokenKind::Undefined => {
                let name = self.text(self.cur_span()).to_string();
                self.bump();
                if let Some(pred) = self.try_type_predicate(&name, start) {
                    return pred;
                }
                TypeNode {
                    kind: TypeNodeKind::Keyword(KeywordTypeKind::Undefined),
                    span: self.span_from(start),
                }
            }
            TokenKind::Null if !self.peek_is(TokenKind::Dot) => {
                self.bump();
                TypeNode {
                    kind: TypeNodeKind::Keyword(KeywordTypeKind::Null),
                    span: self.span_from(start),
                }
            }
            TokenKind::Never if !self.peek_is(TokenKind::Dot) => {
                self.bump();
                TypeNode {
                    kind: TypeNodeKind::Keyword(KeywordTypeKind::Never),
                    span: self.span_from(start),
                }
            }
            TokenKind::Object if !self.peek_is(TokenKind::Dot) => {
                let name = self.text(self.cur_span()).to_string();
                self.bump();
                if let Some(pred) = self.try_type_predicate(&name, start) {
                    return pred;
                }
                TypeNode {
                    kind: TypeNodeKind::Keyword(KeywordTypeKind::Object),
                    span: self.span_from(start),
                }
            }
            TokenKind::Symbol if !self.peek_is(TokenKind::Dot) => {
                let name = self.text(self.cur_span()).to_string();
                self.bump();
                if let Some(pred) = self.try_type_predicate(&name, start) {
                    return pred;
                }
                TypeNode {
                    kind: TypeNodeKind::Keyword(KeywordTypeKind::Symbol),
                    span: self.span_from(start),
                }
            }
            TokenKind::Intrinsic if !self.peek_is(TokenKind::Dot) => {
                self.bump();
                TypeNode {
                    kind: TypeNodeKind::Keyword(KeywordTypeKind::Intrinsic),
                    span: self.span_from(start),
                }
            }
            TokenKind::True => {
                self.bump();
                TypeNode {
                    kind: TypeNodeKind::Literal(LiteralTypeKind::Boolean(true)),
                    span: self.span_from(start),
                }
            }
            TokenKind::False => {
                self.bump();
                TypeNode {
                    kind: TypeNodeKind::Literal(LiteralTypeKind::Boolean(false)),
                    span: self.span_from(start),
                }
            }
            TokenKind::NumericLiteral => {
                let span = self.bump();
                TypeNode {
                    kind: TypeNodeKind::Literal(LiteralTypeKind::Number(
                        self.text(span).to_string(),
                    )),
                    span: self.span_from(start),
                }
            }
            TokenKind::BigIntLiteral => {
                let span = self.bump();
                TypeNode {
                    kind: TypeNodeKind::Literal(LiteralTypeKind::BigInt(
                        self.text(span).to_string(),
                    )),
                    span: self.span_from(start),
                }
            }
            TokenKind::StringLiteral => {
                let span = self.bump_string_literal();
                let raw = self.text(span);
                let content = if raw.len() >= 2 {
                    &raw[1..raw.len() - 1]
                } else {
                    ""
                };
                TypeNode {
                    kind: TypeNodeKind::Literal(LiteralTypeKind::String(content.to_string())),
                    span: self.span_from(start),
                }
            }
            TokenKind::Minus if self.peek_is_numeric() => {
                self.bump();
                let span = self.bump();
                TypeNode {
                    kind: TypeNodeKind::Literal(LiteralTypeKind::Minus(
                        self.text(span).to_string(),
                    )),
                    span: self.span_from(start),
                }
            }
            TokenKind::This => {
                self.bump();
                if self.at(TokenKind::Is) {
                    // this is T (type predicate)
                    self.bump();
                    // Disable nested type predicates in the predicate's type.
                    let saved_pred = self.allow_type_predicate;
                    self.allow_type_predicate = false;
                    let ty = self.parse_type();
                    self.allow_type_predicate = saved_pred;
                    TypeNode {
                        kind: TypeNodeKind::Predicate(Box::new(PredicateTypeNode {
                            param_name: "this".into(),
                            type_ann: Some(Box::new(ty)),
                            asserts: false,
                        })),
                        span: self.span_from(start),
                    }
                } else {
                    TypeNode {
                        kind: TypeNodeKind::This,
                        span: self.span_from(start),
                    }
                }
            }
            TokenKind::TypeOf => {
                self.bump();
                if self.at(TokenKind::Import) {
                    // typeof import("foo").Bar
                    // typeof import.defer("foo").Bar
                    self.bump(); // import
                                 // Handle `import.defer` — consume the property access
                                 // but NOT the call arguments. The call `("./a").Foo`
                                 // should remain as a runtime expression statement.
                    if self.at(TokenKind::Dot) {
                        self.bump(); // .
                        if self.is_identifier() {
                            self.bump(); // defer/meta/etc
                        }
                        // Don't parse call arguments — they remain as
                        // a separate expression statement.
                        let query = TypeNode {
                            kind: TypeNodeKind::TypeQuery(Box::new(Expr {
                                kind: ExprKind::Ident("import".into()),
                                span: self.span_from(start),
                            })),
                            span: self.span_from(start),
                        };
                        return query;
                    }
                    self.expect(TokenKind::OpenParen);
                    let (argument, argument_span) = if self.at(TokenKind::StringLiteral) {
                        let span = self.bump_string_literal();
                        let raw = self.text(span);
                        let content = if raw.len() >= 2 {
                            &raw[1..raw.len() - 1]
                        } else {
                            ""
                        };
                        (
                            TypeNode {
                                kind: TypeNodeKind::Literal(LiteralTypeKind::String(
                                    content.to_string(),
                                )),
                                span: self.span_from(start),
                            },
                            span,
                        )
                    } else {
                        // Recover by parsing whatever type appears inside import(...)
                        let argument = self.parse_type();
                        let span = argument.span;
                        (argument, span)
                    };
                    // Skip optional second argument: import("mod", { with: { ... } })
                    let has_resolution_mode = self.skip_import_type_options();
                    self.expect(TokenKind::CloseParen);
                    let qualifier = if self.eat(TokenKind::Dot).is_some() {
                        // Use parse_identifier_name (accepts keywords like `default`)
                        // since import("mod").default is common
                        if let Some((first_name, first_span)) = self.parse_identifier_name() {
                            let mut qexpr = Expr {
                                kind: ExprKind::Ident(first_name.into()),
                                span: first_span,
                            };
                            while self.eat(TokenKind::Dot).is_some() {
                                if let Some((prop, _)) = self.parse_identifier_name() {
                                    qexpr = Expr {
                                        kind: ExprKind::Member(Box::new(MemberExpr {
                                            object: Box::new(qexpr),
                                            property: prop.into(),
                                            optional: false,
                                        })),
                                        span: self.span_from(start),
                                    };
                                } else {
                                    break;
                                }
                            }
                            Some(Box::new(qexpr))
                        } else {
                            None
                        }
                    } else {
                        None
                    };
                    let type_args = self.try_parse_type_args_in(true);
                    return TypeNode {
                        kind: TypeNodeKind::ImportType(Box::new(ImportTypeNode {
                            argument: Box::new(argument),
                            argument_span,
                            qualifier,
                            type_args,
                            is_typeof: true,
                            has_resolution_mode,
                        })),
                        span: self.span_from(start),
                    };
                }
                // `typeof this` or `typeof this.prop`
                let expr = if self.at(TokenKind::This) {
                    let this_span = self.bump();
                    let mut e = Expr {
                        kind: ExprKind::This,
                        span: this_span,
                    };
                    // Handle member access: typeof this.foo.bar
                    while self.eat(TokenKind::Dot).is_some() {
                        let (prop, _) = self.parse_identifier();
                        e = Expr {
                            kind: ExprKind::Member(Box::new(MemberExpr {
                                object: Box::new(e),
                                property: prop.into(),
                                optional: false,
                            })),
                            span: self.span_from(start),
                        };
                    }
                    e
                } else {
                    self.parse_entity_name()
                };
                // Consume optional type arguments (instantiation expression type).
                // e.g. `typeof Err<U>` — the type args are part of the type
                // query and don't need to be preserved (erased during emit).
                let _ = self.try_parse_type_args_in(true);
                TypeNode {
                    kind: TypeNodeKind::TypeQuery(Box::new(expr)),
                    span: self.span_from(start),
                }
            }
            TokenKind::Import => {
                // import("foo").Bar
                self.bump();
                self.expect(TokenKind::OpenParen);
                let (argument, argument_span) = if self.at(TokenKind::StringLiteral) {
                    let span = self.bump_string_literal();
                    let raw = self.text(span);
                    let content = if raw.len() >= 2 {
                        &raw[1..raw.len() - 1]
                    } else {
                        ""
                    };
                    (
                        TypeNode {
                            kind: TypeNodeKind::Literal(LiteralTypeKind::String(
                                content.to_string(),
                            )),
                            span: self.span_from(start),
                        },
                        span,
                    )
                } else {
                    // Recover by parsing whatever type appears inside import(...)
                    let argument = self.parse_type();
                    let span = argument.span;
                    (argument, span)
                };
                // Skip optional second argument: import("mod", { with: { ... } })
                let has_resolution_mode = self.skip_import_type_options();
                self.expect(TokenKind::CloseParen);
                let qualifier = if self.eat(TokenKind::Dot).is_some() {
                    Some(Box::new(self.parse_entity_name()))
                } else {
                    None
                };
                let type_args = self.try_parse_type_args_in(true);
                TypeNode {
                    kind: TypeNodeKind::ImportType(Box::new(ImportTypeNode {
                        argument: Box::new(argument),
                        argument_span,
                        qualifier,
                        type_args,
                        is_typeof: false,
                        has_resolution_mode,
                    })),
                    span: self.span_from(start),
                }
            }
            TokenKind::Asserts if self.is_asserts_keyword() => {
                self.bump();
                // `asserts this` / `asserts this is T`
                let param = if self.at(TokenKind::This) {
                    self.bump();
                    "this".to_string()
                } else {
                    self.parse_identifier().0.to_string()
                };
                let type_ann = if self.eat(TokenKind::Is).is_some() {
                    Some(Box::new(self.parse_type()))
                } else {
                    None
                };
                TypeNode {
                    kind: TypeNodeKind::Predicate(Box::new(PredicateTypeNode {
                        param_name: param.into(),
                        type_ann,
                        asserts: true,
                    })),
                    span: self.span_from(start),
                }
            }
            TokenKind::OpenParen => {
                // Parenthesized type or function type
                let saved = self.pos;
                // Try function type: (params) => returnType
                self.bump(); // (
                if self.try_parse_as_function_type(start) {
                    // It was a function type, already returned
                    // Actually let me handle this differently
                }
                self.rewind_to(saved);
                self.bump(); // (
                             // Could be function type if followed by params and =>
                             // For simplicity, try to detect
                if self.at(TokenKind::CloseParen) {
                    self.bump();
                    self.expect(TokenKind::FatArrow);
                    let return_type = self.parse_return_type();
                    return TypeNode {
                        kind: TypeNodeKind::Function(Box::new(FnTypeNode {
                            is_abstract: false,
                            type_params: None,
                            params: Vec::new(),
                            return_type: Box::new(return_type),
                        })),
                        span: self.span_from(start),
                    };
                }
                // Try as function params
                let params_saved = self.pos;
                let maybe_params = self.try_parse_param_list_for_fn_type();
                if let Some(params) = maybe_params {
                    if self.eat(TokenKind::FatArrow).is_some() {
                        self.check_parameter_grammar(&params);
                        self.check_signature_parameter_defaults(&params);
                        let return_type = self.parse_return_type();
                        return TypeNode {
                            kind: TypeNodeKind::Function(Box::new(FnTypeNode {
                                is_abstract: false,
                                type_params: None,
                                params,
                                return_type: Box::new(return_type),
                            })),
                            span: self.span_from(start),
                        };
                    }
                }
                // It's a parenthesized type — re-allow conditional types
                // inside parens (they were disallowed in the extends clause
                // of an outer conditional type, but parens reset that).
                self.rewind_to(params_saved);
                let saved_dcl = self.disallow_conditional_types;
                self.disallow_conditional_types = false;
                let ty = self.parse_type();
                self.disallow_conditional_types = saved_dcl;
                self.expect(TokenKind::CloseParen);
                TypeNode {
                    kind: TypeNodeKind::Paren(Box::new(ty)),
                    span: self.span_from(start),
                }
            }
            TokenKind::Function => {
                // JSDoc function type syntax is invalid in a .ts file, but the
                // parser still consumes the complete annotation for recovery:
                // `function(new: T, U)` / `function(this: T, U): R`.
                let function_pos = self.pos;
                self.bump();
                if self.eat(TokenKind::OpenParen).is_some() {
                    while !self.at(TokenKind::CloseParen) && !self.is_eof() {
                        if matches!(self.cur(), TokenKind::New | TokenKind::This)
                            && self.peek_is(TokenKind::Colon)
                        {
                            self.bump();
                            self.bump();
                        }
                        self.parse_type();
                        if self.eat(TokenKind::Comma).is_none() {
                            break;
                        }
                    }
                    self.expect(TokenKind::CloseParen);
                    if self.eat(TokenKind::Colon).is_some() {
                        self.parse_type();
                    }
                }
                // In arrow-function speculation, `(x): function() {}` is an
                // ordinary ternary tail whose `function() {}` must remain an
                // expression. Leaving the parens unconsumed makes speculation
                // fail and lets the expression parser handle it.
                if self.at(TokenKind::OpenBrace) {
                    self.rewind_to(function_pos + 1);
                }
                TypeNode {
                    kind: TypeNodeKind::Keyword(KeywordTypeKind::Any),
                    span: self.span_from(start),
                }
            }
            TokenKind::OpenBracket => {
                // Tuple type
                self.bump();
                let mut elements = Vec::new();
                while !self.at(TokenKind::CloseBracket) && !self.is_eof() {
                    let _elem_start = self.cur_span().start;
                    let dotdotdot = self.eat(TokenKind::DotDotDot).is_some();
                    // Labeled tuple element: `label: type`, `label?: type`, `...label: type`
                    // Detect by lookahead: identifier/keyword followed by `:` or `?:`
                    let is_labeled = {
                        let cur_is_ident = self.is_identifier();
                        if cur_is_ident {
                            let next = self.tokens.get(self.pos + 1).map(|t| t.kind);
                            matches!(next, Some(TokenKind::Colon))
                                || (matches!(next, Some(TokenKind::Question))
                                    && self
                                        .tokens
                                        .get(self.pos + 2)
                                        .is_some_and(|t| t.kind == TokenKind::Colon))
                        } else {
                            false
                        }
                    };
                    let (label, optional) = if is_labeled {
                        let (lbl, _) = self.parse_identifier();
                        let opt = self.eat(TokenKind::Question).is_some();
                        self.expect(TokenKind::Colon);
                        (Some(lbl.into()), opt)
                    } else {
                        (None, false)
                    };
                    let ty = self.parse_type();
                    // Non-labeled optional: `T?` (only when no label)
                    let optional = if !is_labeled && self.eat(TokenKind::Question).is_some() {
                        true
                    } else {
                        optional
                    };
                    elements.push(TupleElement {
                        label,
                        type_node: ty,
                        optional,
                        dotdotdot,
                    });
                    if self.eat(TokenKind::Comma).is_none() {
                        break;
                    }
                }
                self.expect(TokenKind::CloseBracket);
                TypeNode {
                    kind: TypeNodeKind::Tuple(elements),
                    span: self.span_from(start),
                }
            }
            TokenKind::OpenBrace => {
                // Object literal type or mapped type
                self.bump();
                if self.is_mapped_type_start() {
                    return self.parse_mapped_type(start);
                }
                let members = self.parse_type_literal_members();
                self.expect(TokenKind::CloseBrace);
                TypeNode {
                    kind: TypeNodeKind::TypeLit(members),
                    span: self.span_from(start),
                }
            }
            TokenKind::New => {
                // Constructor type: new (...) => T
                self.bump();
                let type_params = self.try_parse_type_params();
                let params = self.parse_param_list();
                self.check_signature_parameter_defaults(&params);
                self.expect(TokenKind::FatArrow);
                let return_type = self.parse_return_type();
                TypeNode {
                    kind: TypeNodeKind::Constructor(Box::new(FnTypeNode {
                        is_abstract: false,
                        type_params,
                        params,
                        return_type: Box::new(return_type),
                    })),
                    span: self.span_from(start),
                }
            }
            TokenKind::Abstract if self.peek_is(TokenKind::New) => {
                // Abstract constructor type: abstract new (...) => T
                self.bump(); // abstract
                self.bump(); // new
                let type_params = self.try_parse_type_params();
                let params = self.parse_param_list();
                self.check_signature_parameter_defaults(&params);
                self.expect(TokenKind::FatArrow);
                let return_type = self.parse_return_type();
                TypeNode {
                    kind: TypeNodeKind::Constructor(Box::new(FnTypeNode {
                        is_abstract: true,
                        type_params,
                        params,
                        return_type: Box::new(return_type),
                    })),
                    span: self.span_from(start),
                }
            }
            TokenKind::Asterisk => {
                // JSDoc's all type is still parsed in a TypeScript annotation;
                // rejecting its use is a semantic check, not token recovery.
                let span = self.bump();
                TypeNode {
                    kind: TypeNodeKind::Keyword(KeywordTypeKind::Any),
                    span,
                }
            }
            TokenKind::NoSubstitutionTemplate | TokenKind::TemplateHead => {
                // Template literal type
                self.parse_template_literal_type(start)
            }
            TokenKind::DotDotDot => {
                self.bump();
                let ty = self.parse_type();
                TypeNode {
                    kind: TypeNodeKind::Rest(Box::new(ty)),
                    span: self.span_from(start),
                }
            }
            TokenKind::LessThan => {
                // Generic function type: <T>(...) => R
                let type_params = self.try_parse_type_params();
                let params = self.parse_param_list();
                self.check_signature_parameter_defaults(&params);
                self.expect(TokenKind::FatArrow);
                let return_type = self.parse_return_type();
                TypeNode {
                    kind: TypeNodeKind::Function(Box::new(FnTypeNode {
                        is_abstract: false,
                        type_params,
                        params,
                        return_type: Box::new(return_type),
                    })),
                    span: self.span_from(start),
                }
            }
            _ if self.cur().is_identifier_name() => {
                // Type references accept keyword names as well as identifiers,
                // including `const` in angle-bracket const assertions.
                let expr = self.parse_type_entity_name();
                let type_args = self.try_parse_type_args_in(true);
                // Check for type predicate: paramName is Type
                // Only in return type context (allow_type_predicate flag)
                if self.at(TokenKind::Is) && self.allow_type_predicate {
                    if let ExprKind::Ident(ref name) = expr.kind {
                        let param_name = name.to_string();
                        self.bump();
                        // Disable nested type predicates: `x is T` is valid
                        // but `x is y is T` is not.
                        let saved_pred = self.allow_type_predicate;
                        self.allow_type_predicate = false;
                        let ty = self.parse_type();
                        self.allow_type_predicate = saved_pred;
                        return TypeNode {
                            kind: TypeNodeKind::Predicate(Box::new(PredicateTypeNode {
                                param_name,
                                type_ann: Some(Box::new(ty)),
                                asserts: false,
                            })),
                            span: self.span_from(start),
                        };
                    }
                }
                TypeNode {
                    kind: TypeNodeKind::Reference(Box::new(TypeRef {
                        name: Box::new(expr),
                        type_args,
                    })),
                    span: self.span_from(start),
                }
            }
            _ => {
                self.error_code(1110, "Type expected.".into());
                // A missing type is an insertion. Keep the next token for its
                // owner (a parameter list, tuple, declaration, or statement).
                // Consuming it here loses delimiters and causes cascading errors.
                TypeNode {
                    kind: TypeNodeKind::Keyword(KeywordTypeKind::Any),
                    span: Span::new(start, start),
                }
            }
        }
    }

    pub(crate) fn parse_entity_name(&mut self) -> Expr {
        let start = self.cur_span().start;
        let (name, span) = self.parse_identifier();
        self.parse_entity_name_rest(start, name, span)
    }

    fn parse_type_entity_name(&mut self) -> Expr {
        let start = self.cur_span().start;
        let (name, span) = self.bump_identifier_unchecked();
        self.parse_entity_name_rest(start, name, span)
    }

    fn parse_entity_name_rest(&mut self, start: u32, name: AstString, span: Span) -> Expr {
        let mut expr = Expr {
            kind: ExprKind::Ident(name.into()),
            span,
        };
        while self.eat(TokenKind::Dot).is_some() {
            // Error recovery: if the token after the dot is a statement keyword on
            // a new line, the dot was trailing error — stop the qualified name here
            // and let the keyword be parsed as a new statement.
            // e.g. `var x: TypeModule1.\nnamespace TypeModule2 { }`
            let stop = self.is_on_new_line()
                && matches!(
                    self.cur(),
                    TokenKind::Namespace
                        | TokenKind::Module
                        | TokenKind::Class
                        | TokenKind::Function
                        | TokenKind::Interface
                        | TokenKind::Enum
                        | TokenKind::Type
                        | TokenKind::Const
                        | TokenKind::Let
                        | TokenKind::Var
                        | TokenKind::Export
                        | TokenKind::Import
                        | TokenKind::Return
                        | TokenKind::If
                        | TokenKind::While
                        | TokenKind::For
                        | TokenKind::Switch
                        | TokenKind::Try
                        | TokenKind::Throw
                        | TokenKind::Break
                        | TokenKind::Continue
                );
            let prop = if stop {
                // Don't consume the keyword — leave it for statement parsing.
                break;
            } else if let Some((name, _)) = self.parse_identifier_name() {
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

    pub(crate) fn parse_type_list(&mut self) -> Vec<TypeNode> {
        let mut types = vec![self.parse_type()];
        while self.eat(TokenKind::Comma).is_some() {
            types.push(self.parse_type());
        }
        types
    }

    /// Like `parse_type_list` but does not parse conditional types, so `extends`
    /// keyword is not consumed.  Used for `implements` clause in class declarations.
    pub(crate) fn parse_heritage_type_list(&mut self) -> Vec<TypeNode> {
        let mut types = vec![self.parse_union_or_intersection_type()];
        while self.eat(TokenKind::Comma).is_some() {
            types.push(self.parse_union_or_intersection_type());
        }
        types
    }

    pub(crate) fn parse_mapped_type(&mut self, start: u32) -> TypeNode {
        let readonly = if self.eat(TokenKind::Readonly).is_some() {
            Some(MappedModifier::None)
        } else if self.eat(TokenKind::Plus).is_some() {
            self.expect(TokenKind::Readonly);
            Some(MappedModifier::Add)
        } else if self.at(TokenKind::Minus) && self.peek_is(TokenKind::Readonly) {
            self.bump();
            self.bump();
            Some(MappedModifier::Remove)
        } else {
            None
        };

        self.expect(TokenKind::OpenBracket);
        let (param_name, param_span) = self.parse_identifier();
        self.expect(TokenKind::In);
        let constraint = self.parse_type();
        let name_type = if self.eat(TokenKind::As).is_some() {
            Some(Box::new(self.parse_type()))
        } else {
            None
        };
        self.expect(TokenKind::CloseBracket);

        let optional = if self.eat(TokenKind::Question).is_some() {
            Some(MappedModifier::None)
        } else if self.eat(TokenKind::Plus).is_some() {
            self.eat(TokenKind::Question);
            Some(MappedModifier::Add)
        } else if self.at(TokenKind::Minus) && self.peek_is(TokenKind::Question) {
            self.bump();
            self.bump();
            Some(MappedModifier::Remove)
        } else {
            None
        };

        let type_ann = if self.eat(TokenKind::Colon).is_some() {
            Some(Box::new(self.parse_type()))
        } else {
            None
        };
        self.eat(TokenKind::Semicolon);
        // Error recovery: if there are extra members after the mapped clause
        // (e.g. `{ [K in T]: V; prop: T2 }` — invalid but TypeScript still
        // parses/erases the entire type), skip everything until the closing `}`.
        if !self.at(TokenKind::CloseBrace) && !self.is_eof() {
            let mut depth: u32 = 1;
            while depth > 0 && !self.is_eof() {
                match self.cur() {
                    TokenKind::OpenBrace => {
                        depth += 1;
                        self.bump();
                    }
                    TokenKind::CloseBrace => {
                        depth -= 1;
                        self.bump();
                    }
                    _ => {
                        self.bump();
                    }
                }
            }
        } else {
            self.expect(TokenKind::CloseBrace);
        }

        TypeNode {
            kind: TypeNodeKind::Mapped(Box::new(MappedTypeNode {
                type_param: TypeParam {
                    name: param_name.into(),
                    name_span: param_span,
                    constraint: Some(Box::new(constraint)),
                    default: None,
                    modifiers: MOD_NONE,
                    span: param_span,
                },
                name_type,
                type_ann,
                readonly,
                optional,
            })),
            span: self.span_from(start),
        }
    }

    pub(crate) fn parse_type_literal_members(&mut self) -> Vec<TypeMember> {
        let mut members = Vec::new();
        while !self.at(TokenKind::CloseBrace) && !self.is_eof() {
            let before = self.pos;
            members.push(self.parse_type_member(false));
            if self.eat(TokenKind::Semicolon).is_none() {
                self.eat(TokenKind::Comma);
            }
            // Error recovery: if we didn't advance, skip the current token to avoid
            // an infinite loop.
            if self.pos == before && !self.is_eof() {
                self.error_code(1131, "Property or signature expected.".into());
                self.bump();
            }
        }
        members
    }

    pub(crate) fn parse_template_literal_type(&mut self, start: u32) -> TypeNode {
        let mut quasis = Vec::new();
        let mut types = Vec::new();

        if self.at(TokenKind::NoSubstitutionTemplate) {
            // TypeScript accepts invalid escapes in a no-substitution
            // template-literal type. Once a `${...}` substitution is present,
            // the head/middle/tail chunks use the ordinary invalid-escape
            // diagnostics below.
            let span = self.bump();
            let raw = self.text(span);
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
        } else {
            // Template head
            let span = self.bump_template_chunk(false);
            let raw = self.text(span);
            let content = if raw.len() >= 3 {
                &raw[1..raw.len() - 2]
            } else {
                ""
            };
            quasis.push(TemplateElement {
                raw: content.to_string(),
                cooked: Some(content.to_string()),
                tail: false,
                span,
            });

            loop {
                let ty = self.parse_type();
                types.push(ty);

                // The batch scanner pre-rescans `}` into TemplateTail/TemplateMiddle.
                if self.at(TokenKind::TemplateTail) {
                    let span = self.bump_template_chunk(false);
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
                    let span = self.bump_template_chunk(false);
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
                    quasis.push(TemplateElement {
                        raw: String::new(),
                        cooked: Some(String::new()),
                        tail: true,
                        span: self.cur_span(),
                    });
                    break;
                }
            }
        }

        TypeNode {
            kind: TypeNodeKind::TemplateLit(Box::new(TemplateLitTypeNode { quasis, types })),
            span: self.span_from(start),
        }
    }

    // -----------------------------------------------------------------------
    // Type parameters
    // -----------------------------------------------------------------------

    /// Check if the current token starts with `>` (i.e. is `>`, `>>`, `>>>`,
    /// `>>=`, or `>>>=`). Used in type parameter/argument parsing to detect
    /// the closing `>` even when it has been merged with a following `>`.
    pub(crate) fn at_greater_than(&self) -> bool {
        matches!(
            self.cur(),
            TokenKind::GreaterThan
                | TokenKind::GreaterGreater
                | TokenKind::GreaterGreaterGreater
                | TokenKind::GreaterEqual
                | TokenKind::GreaterGreaterEquals
                | TokenKind::GreaterGreaterGreaterEquals
        )
    }

    pub(crate) fn try_parse_type_params(&mut self) -> Option<Vec<TypeParam>> {
        let saved = self.pos;
        let saved_token = self.tokens.get(self.pos).cloned();
        let split_mark = self.type_arg_split_rollback.len();
        self.type_arg_split_rollback_depth += 1;
        if self.eat_less_than().is_none() {
            self.type_arg_split_rollback_depth -= 1;
            return None;
        }
        let mut params = Vec::new();
        loop {
            if self.at_greater_than() || self.is_eof() {
                break;
            }
            let tp_start = self.cur_span().start;
            let modifiers = self.parse_type_param_modifiers();
            if !self.is_identifier() {
                self.rewind_to(saved);
                if let Some(tok) = saved_token {
                    self.replace_token(saved, tok);
                }
                let rollback: Vec<_> = self.type_arg_split_rollback.drain(split_mark..).collect();
                for (idx, tok) in rollback.into_iter().rev() {
                    self.replace_token(idx, tok);
                }
                self.type_arg_split_rollback_depth -= 1;
                return None;
            }
            let (name, name_span) = self.parse_identifier();
            let constraint = if self.eat(TokenKind::Extends).is_some() {
                Some(Box::new(self.parse_type()))
            } else {
                None
            };
            let default = if self.eat(TokenKind::Equals).is_some() {
                Some(Box::new(self.parse_type()))
            } else {
                None
            };
            params.push(TypeParam {
                name: name.into(),
                name_span,
                constraint,
                default,
                modifiers,
                span: self.span_from(tp_start),
            });
            if self.eat(TokenKind::Comma).is_none() {
                break;
            }
        }
        // Recovery used by TypeScript for a nested constraint immediately
        // followed by a parameter list. In `foo<U extends C<C<T>>(x: U)`,
        // the final `>` is shared by the nested type reference and the method
        // type-parameter list instead of causing the whole method parse to be
        // abandoned.
        let recovered_shared_constraint_close = self.at(TokenKind::OpenParen)
            && params
                .last()
                .is_some_and(|param| param.constraint.is_some());
        if self.eat_greater_than().is_none() && !recovered_shared_constraint_close {
            self.rewind_to(saved);
            if let Some(tok) = saved_token {
                self.replace_token(saved, tok);
            }
            let rollback: Vec<_> = self.type_arg_split_rollback.drain(split_mark..).collect();
            for (idx, tok) in rollback.into_iter().rev() {
                self.replace_token(idx, tok);
            }
            self.type_arg_split_rollback_depth -= 1;
            return None;
        }
        self.type_arg_split_rollback_depth -= 1;
        if self.type_arg_split_rollback_depth == 0 {
            self.type_arg_split_rollback.truncate(split_mark);
        }
        Some(params)
    }

    /// Type arguments after an expression (`f<T>(x)`), where `<` may also be
    /// a comparison.
    pub(crate) fn try_parse_type_args(&mut self) -> Option<Vec<TypeNode>> {
        self.try_parse_type_args_in(false)
    }

    /// `in_type`: after a type name, where `<` can only open a type argument
    /// list — the comparison heuristics below do not apply.
    pub(crate) fn try_parse_type_args_in(&mut self, in_type: bool) -> Option<Vec<TypeNode>> {
        let saved = self.pos;
        let saved_token = self.tokens.get(self.pos).cloned();
        let list_start = self.cur_span().start;
        let saved_diag_len = self.diagnostics.len();
        let split_mark = self.type_arg_split_rollback.len();
        self.type_arg_split_rollback_depth += 1;
        // Quick rejection: if `<` is followed by a statement keyword that
        // can NEVER start a type, this is a comparison, not type arguments.
        // Note: typeof, void, import, etc. CAN start types so are NOT rejected.
        if let Some(next) = self.tokens.get(self.pos + 1) {
            if matches!(
                next.kind,
                TokenKind::Delete
                    | TokenKind::Throw
                    | TokenKind::Return
                    | TokenKind::If
                    | TokenKind::Switch
                    | TokenKind::While
                    | TokenKind::For
                    | TokenKind::Try
                    | TokenKind::Catch
                    | TokenKind::Finally
                    | TokenKind::Break
                    | TokenKind::Continue
                    | TokenKind::Debugger
                    | TokenKind::Do
                    | TokenKind::Else
                    | TokenKind::With
                    // Unary operators that can't start a type
                    | TokenKind::Tilde
                | TokenKind::PlusPlus
                | TokenKind::MinusMinus
            ) {
                self.type_arg_split_rollback_depth -= 1;
                return None;
            }
            // `< ( identifier .` — member access in parens, not type args
            // e.g., `x < (product.price || 0)`. In a type, the same tokens
            // are a parenthesized qualified name: `Promise<(ns.T & U) | null>`.
            if !in_type && next.kind == TokenKind::OpenParen {
                if let (Some(id), Some(dot)) =
                    (self.tokens.get(self.pos + 2), self.tokens.get(self.pos + 3))
                {
                    if id.kind == TokenKind::Identifier
                        && matches!(dot.kind, TokenKind::Dot | TokenKind::QuestionDot)
                    {
                        self.type_arg_split_rollback_depth -= 1;
                        return None;
                    }
                }
            }
        }
        if self.eat_less_than().is_none() {
            self.type_arg_split_rollback_depth -= 1;
            return None;
        }
        let mut args = Vec::new();
        let mut trailing_comma = None;
        loop {
            if self.at_greater_than() || self.is_eof() {
                break;
            }
            args.push(self.parse_type());
            trailing_comma = self.eat(TokenKind::Comma);
            if trailing_comma.is_none() {
                break;
            }
        }
        let close = self.eat_greater_than();
        if close.is_none() {
            self.rewind_to(saved);
            if let Some(tok) = saved_token {
                self.replace_token(saved, tok);
            }
            let rollback: Vec<_> = self.type_arg_split_rollback.drain(split_mark..).collect();
            for (idx, tok) in rollback.into_iter().rev() {
                self.replace_token(idx, tok);
            }
            // Discard any errors emitted during speculative parsing
            self.diagnostics.truncate(saved_diag_len);
            self.type_arg_split_rollback_depth -= 1;
            return None;
        }
        self.type_arg_split_rollback_depth -= 1;
        if self.type_arg_split_rollback_depth == 0 {
            self.type_arg_split_rollback.truncate(split_mark);
        }
        if !self.is_js_file {
            if let Some(span) = trailing_comma {
                self.grammar_error_at_span(1009, "Trailing comma not allowed.".into(), span);
            } else if args.is_empty() {
                self.grammar_error_at_span(
                    1099,
                    "Type argument list cannot be empty.".into(),
                    Span::new(list_start, close.unwrap().end),
                );
            }
        }
        Some(args)
    }

    fn is_jsdoc_nullable_type_terminator(&self) -> bool {
        self.is_eof()
            || self.at_greater_than()
            || matches!(
                self.cur(),
                TokenKind::Comma
                    | TokenKind::CloseParen
                    | TokenKind::CloseBracket
                    | TokenKind::CloseBrace
                    | TokenKind::Semicolon
                    | TokenKind::Colon
                    | TokenKind::Equals
                    | TokenKind::FatArrow
            )
    }

    fn is_recoverable_prefix_type_operator_terminator(&self) -> bool {
        self.is_eof()
            || self.at_greater_than()
            || matches!(
                self.cur(),
                TokenKind::Comma
                    | TokenKind::CloseParen
                    | TokenKind::CloseBracket
                    | TokenKind::CloseBrace
                    | TokenKind::OpenBrace
                    | TokenKind::Semicolon
                    | TokenKind::Colon
                    | TokenKind::Equals
                    | TokenKind::FatArrow
            )
    }

    fn next_token_can_end_recoverable_postfix_type_operator(&self) -> bool {
        let next = self.tokens.get(self.pos + 1).map(|t| t.kind);
        next.is_none_or(|kind| {
            matches!(
                kind,
                TokenKind::Comma
                    | TokenKind::CloseParen
                    | TokenKind::CloseBracket
                    | TokenKind::CloseBrace
                    | TokenKind::OpenBrace
                    | TokenKind::Semicolon
                    | TokenKind::Colon
                    | TokenKind::Equals
                    | TokenKind::FatArrow
                    | TokenKind::Bar
                    | TokenKind::Ampersand
                    | TokenKind::Extends
                    | TokenKind::GreaterThan
                    | TokenKind::GreaterGreater
                    | TokenKind::GreaterGreaterGreater
                    | TokenKind::GreaterGreaterEquals
                    | TokenKind::GreaterGreaterGreaterEquals
            )
        })
    }

    pub(crate) fn parse_type_param_modifiers(&mut self) -> ModifierFlags {
        let mut flags = MOD_NONE;
        if self.eat(TokenKind::In).is_some() {
            flags |= MOD_IN;
        }
        if self.eat(TokenKind::Out).is_some() {
            flags |= MOD_OUT;
        }
        if self.eat(TokenKind::Const).is_some() {
            flags |= MOD_CONST;
        }
        flags
    }
}
