use super::*;

impl TypeChecker {
    fn identifier_is_nan(name: &str) -> bool {
        if name == "NaN" {
            return true;
        }
        if !name.contains('\\') {
            return false;
        }
        let mut scanner = tsc_rs_scanner::TsScanner::new(name);
        scanner.scan();
        scanner.text_pos() == name.len() && scanner.token_value() == "NaN"
    }

    fn equality_skip_parentheses(mut expression: &Expr) -> &Expr {
        while let ExprKind::Paren(inner) = &expression.kind {
            expression = inner;
        }
        expression
    }

    fn equality_operand_is_global_nan(&self, expression: &Expr) -> bool {
        let expression = Self::equality_skip_parentheses(expression);
        let ExprKind::Ident(name) = &expression.kind else {
            return false;
        };
        if !Self::identifier_is_nan(name)
            || self.function_parameter_name_is_active("NaN")
            || self
                .file_import_binding_names
                .iter()
                .any(|name| Self::identifier_is_nan(name))
            || (self.augmentation_file_is_external()
                && self
                    .local_value_names
                    .iter()
                    .any(|name| Self::identifier_is_nan(name)))
        {
            return false;
        }
        let mut scope = self.current_scope;
        while scope != 0 {
            if self.scopes[scope]
                .vars
                .keys()
                .any(|name| Self::identifier_is_nan(name))
            {
                return false;
            }
            let Some(parent) = self.scopes[scope].parent else {
                break;
            };
            scope = parent;
        }
        self.user_global_value_names.contains("NaN")
            || (!self.augmentation_file_is_external()
                && self.script_global_kinds.contains_key("NaN"))
            || stdlib::stdlib_member_availability(&self.compiler_options)
                .is_some_and(|library| library.declares_global_value("NaN"))
    }

    /// An entity-name suggestion preserves identifier spelling, discards
    /// optional-access punctuation, and accepts parentheses only around the
    /// whole operand. Other expressions use TypeScript's `...` placeholder.
    fn equality_entity_name(&self, expression: &Expr) -> Option<String> {
        match &expression.kind {
            ExprKind::Ident(name) => Some(
                self.current_source
                    .as_deref()
                    .and_then(|source| {
                        source.get(expression.span.start as usize..expression.span.end as usize)
                    })
                    .unwrap_or(name)
                    .to_string(),
            ),
            ExprKind::Member(member) if !member.property.starts_with('#') => Some(format!(
                "{}.{}",
                self.equality_entity_name(&member.object)?,
                member.property
            )),
            _ => None,
        }
    }

    pub(super) fn check_constant_equality_condition(&mut self, binary: &BinaryExpr, span: Span) {
        let unequal = matches!(binary.op, BinaryOp::Ne | BinaryOp::StrictNe);
        let object_literal = |expression: &Expr| {
            matches!(
                expression.kind,
                ExprKind::ObjectLit(_)
                    | ExprKind::ArrayLit(_)
                    | ExprKind::RegexpLit(_)
                    | ExprKind::FnExpr(_)
                    | ExprKind::ClassExpr(_)
            )
        };
        if (object_literal(&binary.left) || object_literal(&binary.right))
            && (!self.current_file_is_js()
                || matches!(binary.op, BinaryOp::StrictEq | BinaryOp::StrictNe))
        {
            self.diagnostics.push(Diagnostic {
                code: 2839,
                message: format!("This condition will always return '{unequal}' since JavaScript compares objects by reference, not value."),
                category: DiagnosticCategory::Error,
                file_name: None,
                span: Some(span),
                related: None,
            });
        }

        let left_nan = self.equality_operand_is_global_nan(&binary.left);
        let right_nan = self.equality_operand_is_global_nan(&binary.right);
        if !left_nan && !right_nan {
            return;
        }
        let related = if left_nan && right_nan {
            None
        } else {
            let operand = if left_nan {
                &binary.right
            } else {
                &binary.left
            };
            let argument = self
                .equality_entity_name(Self::equality_skip_parentheses(operand))
                .unwrap_or_else(|| "...".into());
            let negation = if unequal { "!" } else { "" };
            Some(vec![RelatedDiagnostic {
                code: 1369,
                message: format!("Did you mean '{negation}Number.isNaN({argument})'?"),
                file_name: self.current_file_name.clone(),
                span: Some(operand.span),
            }])
        };
        self.diagnostics.push(Diagnostic {
            code: 2845,
            message: format!("This condition will always return '{unequal}'."),
            category: DiagnosticCategory::Error,
            file_name: None,
            span: Some(span),
            related,
        });
    }
}
