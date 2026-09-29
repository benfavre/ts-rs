use super::*;

fn after(seen: ModifierFlags, order: &[(ModifierFlags, &'static str)]) -> Option<&'static str> {
    order
        .iter()
        .find_map(|(flag, name)| (seen & flag != 0).then_some(*name))
}
fn order(name: &str, previous: &str) -> (u32, String) {
    (
        1029,
        format!("'{name}' modifier must precede '{previous}' modifier."),
    )
}
fn incompatible(name: &str, previous: &str) -> (u32, String) {
    (
        1243,
        format!("'{name}' modifier cannot be used with '{previous}' modifier."),
    )
}

impl Parser<'_> {
    pub(crate) fn check_class_member_modifiers(
        &mut self,
        member: &ClassMember,
        range: std::ops::Range<usize>,
        abstract_class: bool,
    ) {
        if matches!(member.kind, ClassMemberKind::StaticBlock(_)) {
            return;
        }
        let property = matches!(member.kind, ClassMemberKind::Property(_));
        let method = matches!(member.kind, ClassMemberKind::Method(_));
        let accessor = matches!(
            member.kind,
            ClassMemberKind::GetAccessor(_) | ClassMemberKind::SetAccessor(_)
        );
        let index_signature = matches!(member.kind, ClassMemberKind::IndexSignature(_));
        let constructor = matches!(member.kind, ClassMemberKind::Constructor(_));
        let name = match &member.kind {
            ClassMemberKind::Property(p) => Some(&p.name),
            ClassMemberKind::Method(m) => Some(&m.name),
            ClassMemberKind::GetAccessor(a) | ClassMemberKind::SetAccessor(a) => Some(&a.name),
            _ => None,
        };
        let private_name = name.is_some_and(|name| matches!(name, PropName::Private(_, _)));
        let name_span = name.map(PropName::span).unwrap_or_else(|| {
            Span::new(
                member.span.start,
                self.tokens
                    .get(range.end)
                    .map_or(member.span.end, |token| token.span.end),
            )
        });
        let mut seen = MOD_NONE;
        let mut last_async = None;
        let mut constructor_forbidden = Vec::new();
        for position in range {
            let token = &self.tokens[position];
            let (text, flag) = match token.kind {
                TokenKind::Public => ("public", MOD_PUBLIC),
                TokenKind::Private => ("private", MOD_PRIVATE),
                TokenKind::Protected => ("protected", MOD_PROTECTED),
                TokenKind::Static => ("static", MOD_STATIC),
                TokenKind::Readonly => ("readonly", MOD_READONLY),
                TokenKind::Abstract => ("abstract", MOD_ABSTRACT),
                TokenKind::Override => ("override", MOD_OVERRIDE),
                TokenKind::Async => ("async", MOD_ASYNC),
                TokenKind::Accessor => ("accessor", MOD_ACCESSOR),
                TokenKind::Declare => ("declare", MOD_DECLARE),
                TokenKind::Const => ("const", MOD_CONST),
                TokenKind::Export => ("export", MOD_EXPORT),
                _ => continue,
            };
            let mut span = token.span;
            let diagnostic = if index_signature
                && !matches!(token.kind, TokenKind::Readonly | TokenKind::Static)
            {
                Some((
                    1071,
                    format!("'{text}' modifier cannot appear on an index signature."),
                ))
            } else if flag & (MOD_PUBLIC | MOD_PRIVATE | MOD_PROTECTED) != 0 {
                if seen & (MOD_PUBLIC | MOD_PRIVATE | MOD_PROTECTED) != 0 {
                    Some((1028, "Accessibility modifier already seen.".into()))
                } else if let Some(previous) = after(
                    seen,
                    &[
                        (MOD_OVERRIDE, "override"),
                        (MOD_STATIC, "static"),
                        (MOD_ACCESSOR, "accessor"),
                        (MOD_READONLY, "readonly"),
                        (MOD_ASYNC, "async"),
                    ],
                ) {
                    Some(order(text, previous))
                } else if seen & MOD_ABSTRACT != 0 {
                    Some(if flag == MOD_PRIVATE {
                        incompatible(text, "abstract")
                    } else {
                        order(text, "abstract")
                    })
                } else if private_name {
                    Some((
                        18010,
                        "An accessibility modifier cannot be used with a private identifier."
                            .into(),
                    ))
                } else {
                    None
                }
            } else if seen & flag != 0 && flag != MOD_CONST {
                Some((1030, format!("'{text}' modifier already seen.")))
            } else {
                match token.kind {
                    TokenKind::Const => {
                        span = name_span;
                        Some((
                            1248,
                            "A class member cannot have the 'const' keyword.".into(),
                        ))
                    }
                    TokenKind::Static => {
                        if let Some(previous) = after(
                            seen,
                            &[
                                (MOD_READONLY, "readonly"),
                                (MOD_ASYNC, "async"),
                                (MOD_ACCESSOR, "accessor"),
                            ],
                        ) {
                            Some(order(text, previous))
                        } else if seen & MOD_ABSTRACT != 0 {
                            Some(incompatible("static", "abstract"))
                        } else if seen & MOD_OVERRIDE != 0 {
                            Some(order(text, "override"))
                        } else {
                            None
                        }
                    }
                    TokenKind::Override => {
                        if seen & MOD_DECLARE != 0 {
                            Some(incompatible(text, "declare"))
                        } else {
                            after(
                                seen,
                                &[
                                    (MOD_READONLY, "readonly"),
                                    (MOD_ACCESSOR, "accessor"),
                                    (MOD_ASYNC, "async"),
                                ],
                            )
                            .map(|previous| order(text, previous))
                        }
                    }
                    TokenKind::Readonly => {
                        if !property && !index_signature {
                            Some((1024,"'readonly' modifier can only appear on a property declaration or index signature.".into()))
                        } else if seen & MOD_ACCESSOR != 0 {
                            Some(incompatible(text, "accessor"))
                        } else {
                            None
                        }
                    }
                    TokenKind::Accessor => {
                        if let Some(previous) = after(
                            seen,
                            &[(MOD_READONLY, "readonly"), (MOD_DECLARE, "declare")],
                        ) {
                            Some(incompatible(text, previous))
                        } else if !property {
                            Some((
                                1275,
                                "'accessor' modifier can only appear on a property declaration."
                                    .into(),
                            ))
                        } else {
                            None
                        }
                    }
                    TokenKind::Export => {
                        if let Some(previous) = after(
                            seen,
                            &[
                                (MOD_DECLARE, "declare"),
                                (MOD_ABSTRACT, "abstract"),
                                (MOD_ASYNC, "async"),
                            ],
                        ) {
                            Some(order(text, previous))
                        } else {
                            Some((
                                1031,
                                "'export' modifier cannot appear on class elements of this kind."
                                    .into(),
                            ))
                        }
                    }
                    TokenKind::Declare => {
                        if let Some(previous) =
                            after(seen, &[(MOD_ASYNC, "async"), (MOD_OVERRIDE, "override")])
                        {
                            Some((
                                1040,
                                format!(
                                    "'{previous}' modifier cannot be used in an ambient context."
                                ),
                            ))
                        } else if !property {
                            Some((
                                1031,
                                "'declare' modifier cannot appear on class elements of this kind."
                                    .into(),
                            ))
                        } else if private_name {
                            Some((
                                18019,
                                "'declare' modifier cannot be used with a private identifier."
                                    .into(),
                            ))
                        } else if seen & MOD_ACCESSOR != 0 {
                            Some(incompatible(text, "accessor"))
                        } else {
                            None
                        }
                    }
                    TokenKind::Abstract => {
                        if !property && !method && !accessor {
                            Some((1242,"'abstract' modifier can only appear on a class, method, or property declaration.".into()))
                        } else if !abstract_class {
                            Some(if property {
                                (
                                    1253,
                                    "Abstract properties can only appear within an abstract class."
                                        .into(),
                                )
                            } else {
                                (
                                    1244,
                                    "Abstract methods can only appear within an abstract class."
                                        .into(),
                                )
                            })
                        } else if let Some(previous) =
                            after(seen, &[(MOD_STATIC, "static"), (MOD_PRIVATE, "private")])
                        {
                            Some(incompatible(previous, "abstract"))
                        } else if seen & MOD_ASYNC != 0 {
                            span = last_async.unwrap_or(span);
                            Some(incompatible("async", "abstract"))
                        } else if let Some(previous) = after(
                            seen,
                            &[(MOD_OVERRIDE, "override"), (MOD_ACCESSOR, "accessor")],
                        ) {
                            Some(order(text, previous))
                        } else if private_name {
                            Some((
                                18019,
                                "'abstract' modifier cannot be used with a private identifier."
                                    .into(),
                            ))
                        } else {
                            None
                        }
                    }
                    TokenKind::Async => {
                        if seen & MOD_DECLARE != 0 || self.ambient_depth > 0 {
                            Some((
                                1040,
                                "'async' modifier cannot be used in an ambient context.".into(),
                            ))
                        } else if seen & MOD_ABSTRACT != 0 {
                            Some(incompatible("async", "abstract"))
                        } else {
                            None
                        }
                    }
                    _ => None,
                }
            };
            if let Some((code, message)) = diagnostic {
                self.grammar_error_at_span(code, message, span);
                return;
            }
            seen |= flag;
            if flag == MOD_ASYNC {
                last_async = Some(span);
            }
            if constructor && matches!(flag, MOD_STATIC | MOD_OVERRIDE | MOD_ASYNC) {
                constructor_forbidden.push((flag, text, span));
            }
        }
        if constructor {
            for flag in [MOD_STATIC, MOD_OVERRIDE, MOD_ASYNC] {
                if let Some((_, text, span)) = constructor_forbidden
                    .iter()
                    .find(|(found, _, _)| *found == flag)
                {
                    self.grammar_error_at_span(
                        1089,
                        format!("'{text}' modifier cannot appear on a constructor declaration."),
                        *span,
                    );
                    return;
                }
            }
        } else if !method {
            if let Some(span) = last_async {
                self.grammar_error_at_span(
                    1042,
                    "'async' modifier cannot be used here.".into(),
                    span,
                );
                return;
            }
        }
        // A readonly property without a type annotation can declare a literal
        // or enum value. Other ambient properties cannot have initializers.
        if let ClassMemberKind::Property(property) = &member.kind {
            let ambient = self.ambient_depth > 0 || property.modifiers & MOD_DECLARE != 0;
            let infer_readonly =
                property.modifiers & MOD_READONLY != 0 && property.type_ann.is_none();
            // Decorator validity depends on compiler options. The checker
            // handles decorated properties after that validation.
            if ambient && !infer_readonly && property.decorators.is_empty() {
                if let Some(initializer) = &property.initializer {
                    self.grammar_error_at_span(
                        1039,
                        "Initializers are not allowed in ambient contexts.".into(),
                        initializer.span,
                    );
                }
            }
        }
    }
}
