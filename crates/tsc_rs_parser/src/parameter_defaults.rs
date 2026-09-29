use super::*;

impl Parser<'_> {
    pub(crate) fn check_parameter_grammar(&mut self, params: &[Param]) {
        let mut seen_optional = false;
        for (index, param) in params.iter().enumerate() {
            let diagnostic = if param.dotdotdot {
                if index + 1 != params.len() {
                    let span = self
                        .tokens
                        .iter()
                        .rev()
                        .find(|token| {
                            token.span.start >= param.span.start
                                && token.span.end <= param.name.span.start
                                && token.kind == TokenKind::DotDotDot
                        })
                        .map(|token| token.span)
                        .unwrap_or(param.name.span);
                    Some((
                        1014,
                        "A rest parameter must be last in a parameter list.",
                        span,
                    ))
                } else if param.optional {
                    let span = self
                        .tokens
                        .iter()
                        .find(|token| {
                            token.span.start >= param.name.span.end
                                && token.span.end <= param.span.end
                                && token.kind == TokenKind::Question
                        })
                        .map(|token| token.span)
                        .unwrap_or(param.name.span);
                    Some((1047, "A rest parameter cannot be optional.", span))
                } else if param.initializer.is_some() {
                    Some((
                        1048,
                        "A rest parameter cannot have an initializer.",
                        param.name.span,
                    ))
                } else {
                    None
                }
            } else if param.optional {
                seen_optional = true;
                param.initializer.as_ref().map(|_| {
                    (
                        1015,
                        "Parameter cannot have question mark and initializer.",
                        param.name.span,
                    )
                })
            } else if seen_optional && param.initializer.is_none() {
                Some((
                    1016,
                    "A required parameter cannot follow an optional parameter.",
                    param.name.span,
                ))
            } else {
                None
            };
            if let Some((code, message, span)) = diagnostic {
                self.error_at_span(code, message.to_string(), span);
                break;
            }
        }
    }

    pub(crate) fn check_signature_parameter_defaults(&mut self, params: &[Param]) {
        for param in params {
            self.check_signature_binding_defaults(&param.name);
            if param.initializer.is_some() {
                self.report_signature_parameter_default(param.span);
            }
        }
    }

    fn report_signature_parameter_default(&mut self, span: Span) {
        self.error_at_span(
            2371,
            "A parameter initializer is only allowed in a function or constructor implementation."
                .to_string(),
            span,
        );
    }

    fn check_signature_binding_defaults(&mut self, pattern: &Pat) {
        match &pattern.kind {
            PatKind::Assign(inner, _) => {
                self.check_signature_binding_defaults(inner);
                self.report_signature_parameter_default(inner.span);
            }
            PatKind::Array(elements) => {
                for element in elements.iter().flatten() {
                    let (ArrayPatElem::Pat(inner) | ArrayPatElem::Rest(inner)) = element;
                    self.check_signature_binding_defaults(inner);
                }
            }
            PatKind::Object(properties) => {
                for property in properties {
                    match property {
                        ObjPatProp::KeyValue(_, inner) | ObjPatProp::Rest(inner) => {
                            self.check_signature_binding_defaults(inner);
                        }
                        ObjPatProp::ShorthandAssign(_, _, span) => {
                            self.report_signature_parameter_default(*span);
                        }
                        ObjPatProp::Shorthand(_, _) => {}
                    }
                }
            }
            PatKind::Rest(inner) => self.check_signature_binding_defaults(inner),
            PatKind::Ident(_) => {}
        }
    }
}
