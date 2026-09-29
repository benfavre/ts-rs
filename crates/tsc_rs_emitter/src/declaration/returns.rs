use super::*;

impl DeclarationEmitter<'_> {
    pub(super) fn no_value_return_type(
        body: Option<&[Stmt]>,
        is_async: bool,
        is_generator: bool,
    ) -> Option<&'static str> {
        let body = body?;
        if is_generator || body.iter().any(Self::stmt_has_value_return) {
            None
        } else if is_async {
            Some("Promise<void>")
        } else {
            Some("void")
        }
    }

    // A single return has no preceding assignments or narrowing that could
    // change a parameter's declared type. More complex bodies use the existing
    // inference path until declaration emission has full checker information.
    pub(super) fn single_return_type(
        &self,
        body: Option<&[Stmt]>,
        params: &[Param],
        is_async: bool,
        is_generator: bool,
    ) -> Option<String> {
        if is_generator {
            return None;
        }
        let [statement] = body? else {
            return None;
        };
        let StmtKind::Return(Some(expression)) = &statement.kind else {
            return None;
        };
        let mut expression = expression.as_ref();
        while let ExprKind::Paren(inner) = &expression.kind {
            expression = inner;
        }
        let ty = match &expression.kind {
            ExprKind::Ident(name) => {
                let param = params.iter().find(|param| matches!(&param.name.kind, PatKind::Ident(param_name) if param_name == name))?;
                if param.initializer.is_some() {
                    return None;
                }
                if let Some(ty) = &param.type_ann {
                    // Awaiting non-primitive types requires Promise/thenable
                    // resolution; keep those on the existing inference path.
                    if is_async && !matches!(ty.kind, TypeNodeKind::Keyword(_)) {
                        return None;
                    }
                    let mut renderer = Self::new(self.source);
                    renderer.indent = self.indent;
                    renderer.at_line_start = false;
                    renderer.strict_null_checks = self.strict_null_checks;
                    renderer.emit_parameter_type(ty, param.optional && self.strict_null_checks);
                    renderer.output.strip_prefix(": ")?.to_string()
                } else if param.dotdotdot {
                    if is_async {
                        return None;
                    }
                    "any[]".to_string()
                } else {
                    "any".to_string()
                }
            }
            ExprKind::NumLit(_) => "number".to_string(),
            ExprKind::BigIntLit(_) => "bigint".to_string(),
            ExprKind::StrLit(_) | ExprKind::NoSubstTemplate(_) => "string".to_string(),
            ExprKind::BoolLit(_) => "boolean".to_string(),
            ExprKind::NullLit => if self.strict_null_checks {
                "null"
            } else {
                "any"
            }
            .to_string(),
            _ => return None,
        };
        Some(if is_async {
            format!("Promise<{ty}>")
        } else {
            ty
        })
    }

    fn stmt_has_value_return(stmt: &Stmt) -> bool {
        match &stmt.kind {
            StmtKind::Return(value) => value.is_some(),
            StmtKind::Block(body) => body.iter().any(Self::stmt_has_value_return),
            StmtKind::If(branch) => {
                Self::stmt_has_value_return(&branch.consequent)
                    || branch
                        .alternate
                        .as_deref()
                        .is_some_and(Self::stmt_has_value_return)
            }
            StmtKind::While(loop_stmt) => Self::stmt_has_value_return(&loop_stmt.body),
            StmtKind::DoWhile(loop_stmt) => Self::stmt_has_value_return(&loop_stmt.body),
            StmtKind::For(loop_stmt) => Self::stmt_has_value_return(&loop_stmt.body),
            StmtKind::ForIn(loop_stmt) => Self::stmt_has_value_return(&loop_stmt.body),
            StmtKind::ForOf(loop_stmt) => Self::stmt_has_value_return(&loop_stmt.body),
            StmtKind::Switch(switch) => switch
                .cases
                .iter()
                .any(|case| case.consequent.iter().any(Self::stmt_has_value_return)),
            StmtKind::Try(try_stmt) => {
                try_stmt.block.iter().any(Self::stmt_has_value_return)
                    || try_stmt
                        .handler
                        .as_ref()
                        .is_some_and(|handler| handler.body.iter().any(Self::stmt_has_value_return))
                    || try_stmt
                        .finalizer
                        .as_ref()
                        .is_some_and(|body| body.iter().any(Self::stmt_has_value_return))
            }
            StmtKind::Labeled(label) => Self::stmt_has_value_return(&label.body),
            StmtKind::With(with) => Self::stmt_has_value_return(&with.body),
            // Returns in nested functions, classes, or expressions belong to
            // those declarations, not this function's signature.
            _ => false,
        }
    }
}
