//! Exception regions share the generator helper's pending completion stack.
use super::*;

fn catch_placeholder(id: lexical_downlevel::BindingId) -> AstString {
    format!("\0generator_catch:{id}\0").into()
}

impl GeneratorPlan {
    pub(super) fn catch_assignment_name(&self, name: &AstString) -> AstString {
        match self
            .active_catch_bindings
            .iter()
            .rev()
            .find(|(source, _)| source == name)
        {
            Some((_, Some(id))) => catch_placeholder(*id),
            _ => name.clone(),
        }
    }

    pub(super) fn try_statement(
        &mut self,
        span: Span,
        try_: &TryStmt,
        context: &Emitter<'_>,
    ) -> Option<()> {
        self.has_catch |= try_.handler.is_some();
        if try_.handler.is_none() && try_.finalizer.is_none() {
            return None;
        }
        if !context
            .source_between(span.start, span.end)
            .contains("await")
        {
            let body = self.native_statements(&try_.block, context)?;
            let catch = if let Some(handler) = &try_.handler {
                let name = if let Some(param) = &handler.param {
                    let PatKind::Ident(name) = &param.kind else {
                        return None;
                    };
                    name.clone()
                } else {
                    self.fresh_name(context).into()
                };
                self.active_catch_bindings.push((name.clone(), None));
                let body = self.native_statements(&handler.body, context)?;
                self.active_catch_bindings.pop();
                Some((Some(name), body))
            } else {
                None
            };
            let finally = match &try_.finalizer {
                Some(stmts) => Some(self.native_statements(stmts, context)?),
                None => None,
            };
            self.push(Operation::NativeTry(Box::new(NativeTry {
                body,
                catch,
                finally,
            })));
            return Some(());
        }

        // The region must start at a distinct label: a jump back to preceding
        // code must leave this region and run its finalizer first.
        let start = if self.blocks.last()?.is_empty() {
            self.blocks.len() - 1
        } else {
            self.label()
        };
        self.push(Operation::TryRegion(start, None, None, 0));
        let exit_slot = self.jump_targets.len();
        self.jump_targets.push(None);
        self.statements(&try_.block, context)?;
        if !self.terminated() {
            self.push(Operation::PendingJump(exit_slot));
        }

        let catch = if let Some(handler) = &try_.handler {
            let label = self.label();
            let binding = if let Some(param) = &handler.param {
                let PatKind::Ident(name) = &param.kind else {
                    return None;
                };
                let binding = context
                    .lexical_downlevel_plan
                    .binding_for_declaration(param.span)?;
                if binding.kind != lexical_downlevel::BindingKind::Catch
                    || (binding.captured && self.control_scopes.iter().any(|scope| scope.is_loop))
                    || context
                        .source_between(handler.span.start, handler.span.end)
                        .contains("eval")
                {
                    // Captured catches in a loop need a fresh environment on
                    // every entry; a single hoisted variable cannot represent it.
                    // Direct eval can observe the original catch name, including
                    // from nested functions, so conservatively retain that scope.
                    return None;
                }
                let placeholder = catch_placeholder(binding.id);
                self.locals.push(placeholder.clone());
                self.catch_bindings.push(binding.id);
                self.push(Operation::Catch(placeholder));
                self.active_catch_bindings
                    .push((name.clone(), Some(binding.id)));
                true
            } else {
                let value = expressions::synthetic(ExprKind::Call(Box::new(CallExpr {
                    callee: Box::new(expressions::synthetic(ExprKind::Member(Box::new(
                        MemberExpr {
                            object: Box::new(expressions::ident(STATE_BINDING)),
                            property: "sent".into(),
                            optional: false,
                        },
                    )))),
                    args: Vec::new(),
                    type_args: None,
                    optional: false,
                })));
                self.push(Operation::Expr(value));
                false
            };
            self.statements(&handler.body, context)?;
            if binding {
                self.active_catch_bindings.pop();
            }
            if !self.terminated() {
                self.push(Operation::PendingJump(exit_slot));
            }
            Some(label)
        } else {
            None
        };
        let finally = if let Some(stmts) = &try_.finalizer {
            let label = self.label();
            self.statements(stmts, context)?;
            if !self.terminated() {
                self.push(Operation::EndFinally);
            }
            Some(label)
        } else {
            None
        };
        let end = self.label();
        self.jump_targets[exit_slot] = Some(end);
        self.blocks[start][0] = Operation::TryRegion(start, catch, finally, end);
        Some(())
    }
}

impl Emitter<'_> {
    pub(super) fn prepare_generator_catches(&mut self, plan: &mut GeneratorPlan) {
        let mut names = HashMap::new();
        for &id in &plan.catch_bindings {
            let name = if let Some(name) = self.generator_catch_names.get(&id) {
                name.clone()
            } else {
                let source_name = self.lexical_downlevel_plan.bindings[id].source_name.clone();
                let mut suffix = 1;
                let name: AstString = loop {
                    let candidate = format!("{source_name}_{suffix}");
                    if !self.source_has_identifier(&candidate)
                        && self.current_arguments_alias.as_deref() != Some(candidate.as_str())
                        && !self.emitted_var_names.contains(candidate.as_str())
                        && !self
                            .generator_catch_names
                            .values()
                            .any(|name| name == candidate.as_str())
                        && !self
                            .lexical_downlevel_plan
                            .bindings
                            .iter()
                            .any(|binding| binding.emitted_name == candidate.as_str())
                    {
                        break candidate.into();
                    }
                    suffix += 1;
                };
                self.generator_catch_names.insert(id, name.clone());
                name
            };
            self.lexical_downlevel_plan
                .rename_generator_catch(id, name.clone());
            names.insert(catch_placeholder(id), name);
        }
        plan.resolve_synthetic_bindings(&|name| names.get(name).cloned());
    }

    pub(super) fn emit_generator_native_try(&mut self, try_: &NativeTry, state: &str) {
        self.write("try");
        self.emit_generator_body(&try_.body, state);
        if let Some((name, body)) = &try_.catch {
            self.write("catch");
            if let Some(name) = name {
                self.write(" (");
                self.write(name);
                self.write(")");
            }
            self.emit_generator_body(body, state);
        }
        if let Some(body) = &try_.finally {
            self.write("finally");
            self.emit_generator_body(body, state);
        }
    }
}
