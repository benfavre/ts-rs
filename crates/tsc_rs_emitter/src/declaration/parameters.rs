use super::*;

impl DeclarationEmitter<'_> {
    pub(super) fn emit_parameter_list(&mut self, params: &[Param], constructor: bool) {
        for (index, param) in params.iter().enumerate() {
            if index > 0 {
                self.write(", ");
            }
            if param.dotdotdot {
                self.write("...");
            }
            self.emit_binding_pattern(&param.name);
            let trailing_default = param.initializer.is_some()
                && params[index + 1..]
                    .iter()
                    .all(|next| next.optional || next.initializer.is_some() || next.dotdotdot);
            let optional = !param.dotdotdot && (param.optional || trailing_default);
            if optional {
                self.write("?");
            }
            let add_undefined = self.strict_null_checks
                && param.initializer.is_some()
                && !optional
                && !param.dotdotdot;
            if let Some(ty) = &param.type_ann {
                self.emit_parameter_type(ty, add_undefined);
            } else if constructor || matches!(param.name.kind, PatKind::Ident(_)) {
                let initializer = param.initializer.as_deref().map(|mut expr| {
                    while let ExprKind::Paren(inner) = &expr.kind {
                        expr = inner;
                    }
                    expr
                });
                let asserted_type = initializer.and_then(|expr| match &expr.kind {
                    ExprKind::As(assertion) => Some(&assertion.type_node),
                    ExprKind::TypeAssertion(assertion) => Some(&assertion.type_node),
                    _ => None,
                });
                if let Some(ty) = asserted_type.filter(|ty| !Self::is_const_assertion_type(ty)) {
                    self.emit_parameter_type(ty, add_undefined);
                    continue;
                }
                let inferred = initializer.and_then(|expr| match &expr.kind {
                    ExprKind::As(assertion)
                        if Self::is_const_assertion_type(&assertion.type_node) =>
                    {
                        Self::const_initializer_text(&assertion.expr)
                    }
                    ExprKind::TypeAssertion(assertion)
                        if Self::is_const_assertion_type(&assertion.type_node) =>
                    {
                        Self::const_initializer_text(&assertion.expr)
                    }
                    ExprKind::NullLit => Some(
                        if self.strict_null_checks {
                            "null"
                        } else {
                            "any"
                        }
                        .to_string(),
                    ),
                    ExprKind::Ident(name) if name == "undefined" => Some(
                        if self.strict_null_checks {
                            "undefined"
                        } else {
                            "any"
                        }
                        .to_string(),
                    ),
                    _ => self.infer_var_type_text(expr),
                });
                let ty =
                    inferred
                        .as_deref()
                        .unwrap_or(if param.dotdotdot { "any[]" } else { "any" });
                self.write(": ");
                self.write(ty);
                if add_undefined && !matches!(ty, "any" | "unknown" | "undefined") {
                    self.write(" | undefined");
                }
            }
        }
    }

    fn is_const_assertion_type(ty: &TypeNode) -> bool {
        matches!(&ty.kind, TypeNodeKind::Reference(reference) if matches!(&reference.name.kind, ExprKind::Ident(name) if name == "const"))
    }

    pub(super) fn emit_parameter_type(&mut self, ty: &TypeNode, add_undefined: bool) {
        let add_undefined = add_undefined && !Self::type_explicitly_allows_undefined(ty);
        let parenthesize = add_undefined
            && matches!(
                ty.kind,
                TypeNodeKind::Function(_)
                    | TypeNodeKind::Constructor(_)
                    | TypeNodeKind::Conditional(_)
            );
        self.write(": ");
        if parenthesize {
            self.write("(");
        }
        self.emit_type_node(ty);
        if parenthesize {
            self.write(")");
        }
        if add_undefined {
            self.write(" | undefined");
        }
    }

    fn type_explicitly_allows_undefined(ty: &TypeNode) -> bool {
        match &ty.kind {
            TypeNodeKind::Keyword(
                KeywordTypeKind::Any | KeywordTypeKind::Unknown | KeywordTypeKind::Undefined,
            ) => true,
            TypeNodeKind::Union(types) => types.iter().any(Self::type_explicitly_allows_undefined),
            TypeNodeKind::Paren(inner) => Self::type_explicitly_allows_undefined(inner),
            _ => false,
        }
    }
}
