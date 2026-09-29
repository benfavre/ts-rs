//! Preserve native loops without suspension; lower suspending loops to labels.
use super::*;

impl GeneratorPlan {
    pub(super) fn native_statements(
        &mut self,
        stmts: &[Stmt],
        context: &Emitter<'_>,
    ) -> Option<OperationBody> {
        // Share locals, temporary allocation, and enclosing control targets,
        // collecting the structured body outside the state blocks.
        let outer = std::mem::replace(&mut self.blocks, vec![Vec::new()]);
        let result = self.statements(stmts, context);
        let mut body = std::mem::replace(&mut self.blocks, outer);
        result?;
        if body.len() != 1 {
            return None;
        }
        Some(OperationBody {
            operations: body.pop()?,
            braced: true,
        })
    }

    pub(super) fn native_body(
        &mut self,
        stmt: &Stmt,
        context: &Emitter<'_>,
    ) -> Option<OperationBody> {
        let stmts = if let StmtKind::Block(stmts) = &stmt.kind {
            stmts.as_slice()
        } else {
            std::slice::from_ref(stmt)
        };
        let mut body = self.native_statements(stmts, context)?;
        body.braced = matches!(stmt.kind, StmtKind::Block(_));
        Some(body)
    }

    pub(super) fn statement_body(&mut self, stmt: &Stmt, context: &Emitter<'_>) -> Option<()> {
        // A lowered loop/conditional owns its body boundary. Only blocks
        // occurring as statements inside that body remain standalone blocks.
        if let StmtKind::Block(stmts) = &stmt.kind {
            self.statements(stmts, context)
        } else {
            self.statement(stmt, context)
        }
    }

    pub(super) fn control_statement(
        &mut self,
        is_continue: bool,
        label: &Option<String>,
    ) -> Option<()> {
        let scope = self.control_scopes.iter().rev().find(|scope| {
            (!is_continue || scope.is_loop)
                && (label.is_some() || !scope.label_only)
                && label
                    .as_ref()
                    .is_none_or(|label| scope.labels.contains(label))
        })?;
        self.push(match scope.targets {
            Some((exit, next)) => Operation::PendingJump(if is_continue { next } else { exit }),
            None => Operation::NativeControl(is_continue, label.clone()),
        });
        Some(())
    }

    pub(super) fn block_statement(
        &mut self,
        span: Span,
        stmts: &[Stmt],
        labels: Vec<String>,
        context: &Emitter<'_>,
    ) -> Option<()> {
        let suspends = context
            .source_between(span.start, span.end)
            .contains("await");
        if !suspends {
            if !labels.is_empty() {
                self.control_scopes.push(ControlScope {
                    is_loop: false,
                    label_only: true,
                    labels: labels.clone(),
                    targets: None,
                });
            }
            let body = self.native_statements(stmts, context)?;
            if !labels.is_empty() {
                self.control_scopes.pop();
            }
            self.push(Operation::Block(Box::new(NativeBlock { labels, body })));
        } else if labels.is_empty() {
            self.statements(stmts, context)?;
        } else {
            let exit = self.jump_targets.len();
            self.jump_targets.push(None);
            self.control_scopes.push(ControlScope {
                is_loop: false,
                label_only: true,
                labels,
                targets: Some((exit, exit)),
            });
            self.statements(stmts, context)?;
            self.control_scopes.pop();
            self.jump_targets[exit] = Some(self.label());
        }
        Some(())
    }

    fn for_initializer(&mut self, init: &ForInit, context: &Emitter<'_>) -> Option<Option<Expr>> {
        use super::expressions::{assignment, ident, synthetic};
        match init {
            ForInit::Expr(expr) => Some(Some(self.expression(expr, context)?)),
            ForInit::Var(vars) if vars.kind == VarKind::Var => {
                let mut values = Vec::new();
                for decl in &vars.declarations {
                    let PatKind::Ident(name) = &decl.name.kind else {
                        return None;
                    };
                    if !self.locals.contains(name) {
                        self.locals.push(name.clone());
                    }
                    if let Some(init) = &decl.init {
                        // Earlier initializers must execute before this await.
                        if Self::has_await(init, context.source) && !values.is_empty() {
                            self.push(Operation::Expr(synthetic(ExprKind::Comma(std::mem::take(
                                &mut values,
                            )))));
                        }
                        let value = self.expression(init, context)?;
                        values.push(Box::new(assignment(
                            ident(&self.catch_assignment_name(name)),
                            value,
                        )));
                    }
                }
                Some(match values.len() {
                    0 => None,
                    1 => values.pop().map(|value| *value),
                    _ => Some(synthetic(ExprKind::Comma(values))),
                })
            }
            _ => None,
        }
    }

    pub(super) fn loop_statement(&mut self, stmt: &Stmt, context: &Emitter<'_>) -> Option<()> {
        let mut labels = Vec::new();
        let mut loop_stmt = stmt;
        while let StmtKind::Labeled(label) = &loop_stmt.kind {
            labels.push(label.label.clone());
            loop_stmt = &label.body;
        }
        if let StmtKind::Block(stmts) = &loop_stmt.kind {
            return self.block_statement(loop_stmt.span, stmts, labels, context);
        }
        if let StmtKind::ForIn(loop_) = &loop_stmt.kind {
            return self.for_in_statement(loop_stmt.span, loop_, labels, context);
        }
        if let StmtKind::Switch(switch) = &loop_stmt.kind {
            return self.switch_statement(switch, labels, context);
        }
        let (body, test, update, init, is_do, is_for) = match &loop_stmt.kind {
            StmtKind::While(loop_) => (
                &loop_.body,
                Some(loop_.test.as_ref()),
                None,
                None,
                false,
                false,
            ),
            StmtKind::DoWhile(loop_) => (
                &loop_.body,
                Some(loop_.test.as_ref()),
                None,
                None,
                true,
                false,
            ),
            StmtKind::For(loop_) => (
                &loop_.body,
                loop_.test.as_deref(),
                loop_.update.as_deref(),
                loop_.init.as_ref(),
                false,
                true,
            ),
            _ => return None,
        };
        let suspends = context
            .source_between(loop_stmt.span.start, loop_stmt.span.end)
            .contains("await");
        let init = match init {
            Some(init) => self.for_initializer(init, context)?,
            None => None,
        };
        if !suspends {
            self.control_scopes.push(ControlScope {
                is_loop: true,
                label_only: false,
                labels: labels.clone(),
                targets: None,
            });
            let body = self.native_body(body, context)?;
            self.control_scopes.pop();
            let kind = if is_for {
                NativeLoopKind::For(init, test.cloned(), update.cloned())
            } else if is_do {
                NativeLoopKind::DoWhile(test?.clone())
            } else {
                NativeLoopKind::While(test?.clone())
            };
            self.push(Operation::Loop(Box::new(NativeLoop { kind, labels, body })));
            return Some(());
        }

        if let Some(init) = init {
            self.push(Operation::Expr(init));
        }
        // The first loop can reuse the initial empty state. A preceding
        // initializer or statement must not be repeated by a back edge.
        let start = if self.blocks.last()?.is_empty() {
            self.blocks.len() - 1
        } else {
            self.label()
        };
        let exit_slot = self.jump_targets.len();
        let continue_slot = exit_slot + 1;
        self.jump_targets.extend([None, None]);
        self.control_scopes.push(ControlScope {
            is_loop: true,
            label_only: false,
            labels,
            targets: Some((exit_slot, continue_slot)),
        });

        let branch = if !is_do {
            match test {
                Some(test) => {
                    let test = self.expression(test, context)?;
                    let location = (self.blocks.len() - 1, self.blocks.last()?.len());
                    self.push(Operation::Branch(test, 0));
                    Some(location)
                }
                None => None,
            }
        } else {
            None
        };
        self.statement_body(body, context)?;
        let next = if is_do || is_for { self.label() } else { start };
        self.jump_targets[continue_slot] = Some(next);
        if is_do {
            let test = self.expression(test?, context)?;
            self.push(Operation::BranchTrue(test, start));
        } else {
            if let Some(update) = update {
                let update = self.expression(update, context)?;
                self.push(Operation::Expr(update));
            }
            if !self.terminated() {
                self.push(Operation::Jump(start));
            }
        }
        let exit = self.label();
        self.jump_targets[exit_slot] = Some(exit);
        if let Some((block, index)) = branch {
            self.blocks[block][index] = match &self.blocks[block][index] {
                Operation::Branch(test, _) => Operation::Branch(test.clone(), exit),
                _ => unreachable!(),
            };
        }
        self.control_scopes.pop();
        Some(())
    }

    pub(super) fn resolve_jumps(&mut self) -> Option<()> {
        fn resolve(ops: &mut [Operation], targets: &[Option<usize>]) -> Option<()> {
            for op in ops {
                match op {
                    Operation::PendingJump(slot) => {
                        *op = Operation::Jump(*targets.get(*slot)?.as_ref()?)
                    }
                    Operation::If(_, yes, no) => {
                        resolve(&mut yes.operations, targets)?;
                        if let Some(no) = no {
                            resolve(&mut no.operations, targets)?;
                        }
                    }
                    Operation::Loop(loop_) => resolve(&mut loop_.body.operations, targets)?,
                    Operation::Block(block) => resolve(&mut block.body.operations, targets)?,
                    Operation::Switch(switch) => {
                        for (_, body) in &mut switch.cases {
                            resolve(&mut body.operations, targets)?;
                        }
                    }
                    Operation::NativeTry(try_) => {
                        resolve(&mut try_.body.operations, targets)?;
                        if let Some((_, catch)) = &mut try_.catch {
                            resolve(&mut catch.operations, targets)?;
                        }
                        if let Some(finally) = &mut try_.finally {
                            resolve(&mut finally.operations, targets)?;
                        }
                    }
                    _ => {}
                }
            }
            Some(())
        }
        for block in &mut self.blocks {
            resolve(block, &self.jump_targets)?;
        }
        Some(())
    }
}

impl Emitter<'_> {
    pub(super) fn emit_generator_body(&mut self, body: &OperationBody, state: &str) {
        // Erased or expanded statements need braces to remain a single body.
        let braced = body.braced || body.operations.len() != 1;
        if braced {
            self.writeln(" {");
        } else {
            self.newline();
        }
        self.indent += 1;
        for op in &body.operations {
            self.emit_generator_operation(op, state);
        }
        self.indent -= 1;
        if braced {
            self.writeln("}");
        }
    }

    pub(super) fn emit_generator_loop(&mut self, loop_: &NativeLoop, state: &str) {
        for label in &loop_.labels {
            self.write(label);
            self.write(": ");
        }
        match &loop_.kind {
            NativeLoopKind::While(test) => {
                self.write("while (");
                self.emit_expr(test);
                self.write(")");
                self.emit_generator_body(&loop_.body, state);
            }
            NativeLoopKind::DoWhile(test) => {
                self.write("do");
                self.emit_generator_body(&loop_.body, state);
                if loop_.body.braced || loop_.body.operations.len() != 1 {
                    self.output.pop();
                    self.at_line_start = false;
                    self.write(" ");
                }
                self.write("while (");
                self.emit_expr(test);
                self.writeln(");");
            }
            NativeLoopKind::ForIn(left, right) => {
                self.write("for (");
                self.emit_expr(left);
                self.write(" in ");
                self.emit_expr(right);
                self.write(")");
                self.emit_generator_body(&loop_.body, state);
            }
            NativeLoopKind::For(init, test, update) => {
                self.write("for (");
                if let Some(init) = init {
                    self.emit_expr(init);
                }
                self.write("; ");
                if let Some(test) = test {
                    self.emit_expr(test);
                }
                self.write("; ");
                if let Some(update) = update {
                    self.emit_expr(update);
                }
                self.write(")");
                self.emit_generator_body(&loop_.body, state);
            }
        }
    }
}
