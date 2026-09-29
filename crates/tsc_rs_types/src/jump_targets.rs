//! Resolve statement jump targets during the normal checker traversal.
use super::*;

#[derive(Clone)]
pub(crate) struct JumpTarget {
    label: Option<String>,
    iteration: bool,
    function_depth: u32,
    async_context_depth: usize,
}

impl TypeChecker {
    pub(crate) fn enter_jump_target(&mut self, statement: &Stmt) -> bool {
        let (label, iteration) = match &statement.kind {
            StmtKind::Block(statements) if self.ambient_depth > 0 && self.check_index_grammar => {
                // Each nested ambient block reports TS1036 once. That first
                // statement's ambient error takes precedence over its jump error.
                if let Some(first) = statements.iter().find(|statement| {
                    !matches!(
                        statement.kind,
                        StmtKind::Var(_)
                            | StmtKind::FnDecl(_)
                            | StmtKind::ClassDecl(_)
                            | StmtKind::InterfaceDecl(_)
                            | StmtKind::TypeAlias(_)
                            | StmtKind::EnumDecl(_)
                            | StmtKind::ModuleDecl(_)
                            | StmtKind::Import(_)
                            | StmtKind::ImportEquals(_)
                            | StmtKind::Export(_)
                            | StmtKind::ExportAssign(_)
                    )
                }) {
                    if self.ambient_statement_starts.insert(first.span.start) {
                        let (span, _) = self.jump_first_token(first.span, "");
                        self.report_jump_error(
                            1036,
                            "Statements are not allowed in ambient contexts.".to_owned(),
                            span,
                        );
                    }
                }
                return false;
            }
            StmtKind::Labeled(labeled) => {
                let label = Self::jump_label_key(&labeled.label);
                if self.check_index_grammar
                    && !self
                        .ambient_statement_starts
                        .contains(&statement.span.start)
                    && self
                        .jump_targets
                        .iter()
                        .rev()
                        .take_while(|target| target.function_depth == self.jump_function_depth)
                        .any(|target| target.label.as_deref() == Some(label.as_ref()))
                {
                    let (span, spelling) = self.jump_first_token(statement.span, &labeled.label);
                    self.report_jump_error(1114, format!("Duplicate label '{spelling}'."), span);
                }
                let mut body = &*labeled.body;
                while let StmtKind::Labeled(inner) = &body.kind {
                    body = &inner.body;
                }
                (Some(label.into_owned()), Self::jump_is_iteration(body))
            }
            StmtKind::Switch(_) => (None, false),
            StmtKind::While(_)
            | StmtKind::DoWhile(_)
            | StmtKind::For(_)
            | StmtKind::ForIn(_)
            | StmtKind::ForOf(_) => (None, true),
            StmtKind::Break(label) | StmtKind::Continue(label) => {
                if self.check_index_grammar
                    && !self
                        .ambient_statement_starts
                        .contains(&statement.span.start)
                {
                    let label = label.as_deref().map(Self::jump_label_key);
                    self.check_jump_target(
                        statement,
                        label.as_deref(),
                        matches!(statement.kind, StmtKind::Continue(_)),
                    );
                }
                return false;
            }
            _ => return false,
        };
        self.jump_targets.push(JumpTarget {
            label,
            iteration,
            function_depth: self.jump_function_depth,
            async_context_depth: self.return_is_async_stack.len(),
        });
        true
    }

    fn jump_label_key(label: &str) -> std::borrow::Cow<'_, str> {
        if !label.contains('\\') {
            return std::borrow::Cow::Borrowed(label);
        }
        let mut scanner = tsc_rs_scanner::TsScanner::new(label);
        scanner.scan();
        std::borrow::Cow::Owned(scanner.token_value().to_owned())
    }

    fn jump_is_iteration(statement: &Stmt) -> bool {
        matches!(
            statement.kind,
            StmtKind::While(_)
                | StmtKind::DoWhile(_)
                | StmtKind::For(_)
                | StmtKind::ForIn(_)
                | StmtKind::ForOf(_)
        )
    }

    fn jump_first_token(&self, statement: Span, fallback: &str) -> (Span, String) {
        if let Some(source) = self
            .current_source
            .as_deref()
            .and_then(|source| source.get(statement.start as usize..))
        {
            let mut scanner = tsc_rs_scanner::TsScanner::new(source);
            scanner.scan();
            let end = scanner.text_pos();
            return (
                Span::new(statement.start, statement.start + end as u32),
                source[..end].to_owned(),
            );
        }
        (
            Span::new(statement.start, statement.start + fallback.len() as u32),
            fallback.to_owned(),
        )
    }

    fn check_jump_target(&mut self, statement: &Stmt, label: Option<&str>, is_continue: bool) {
        for target in self.jump_targets.iter().rev() {
            if target.function_depth != self.jump_function_depth
                || target.async_context_depth != self.return_is_async_stack.len()
            {
                break;
            }
            if let Some(label) = label {
                if target.label.as_deref() == Some(label) {
                    if is_continue && !target.iteration {
                        self.report_jump_error(1115, "A 'continue' statement can only jump to a label of an enclosing iteration statement.".to_owned(), statement.span);
                    }
                    return;
                }
            } else if target.label.is_none() && (!is_continue || target.iteration) {
                return;
            }
        }
        let (code, message) = if self.jump_function_depth > 0
            || !self.return_is_async_stack.is_empty()
        {
            (1107, "Jump target cannot cross function boundary.")
        } else if label.is_some() {
            if is_continue {
                (1115, "A 'continue' statement can only jump to a label of an enclosing iteration statement.")
            } else {
                (
                    1116,
                    "A 'break' statement can only jump to a label of an enclosing statement.",
                )
            }
        } else if is_continue {
            (
                1104,
                "A 'continue' statement can only be used within an enclosing iteration statement.",
            )
        } else {
            (1105, "A 'break' statement can only be used within an enclosing iteration or switch statement.")
        };
        self.report_jump_error(code, message.to_owned(), statement.span);
    }

    fn report_jump_error(&mut self, code: u32, message: String, span: Span) {
        self.diagnostics.push(Diagnostic {
            code,
            message,
            category: DiagnosticCategory::Error,
            file_name: None,
            span: Some(span),
            related: None,
        });
    }
}
