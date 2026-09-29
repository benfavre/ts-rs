//! Switch dispatch evaluates case tests only until the first strict match.
use super::*;

impl GeneratorPlan {
    pub(super) fn switch_statement(
        &mut self,
        switch: &SwitchStmt,
        labels: Vec<String>,
        context: &Emitter<'_>,
    ) -> Option<()> {
        // Awaiting only the discriminant leaves the switch itself native.
        let value = self.expression(&switch.discriminant, context)?;
        let suspends = switch.cases.iter().any(|case| {
            case.test
                .as_ref()
                .is_some_and(|test| Self::has_await(test, context.source))
                || case.consequent.iter().any(|stmt| {
                    context
                        .source_between(stmt.span.start, stmt.span.end)
                        .contains("await")
                })
        });
        if !suspends {
            self.control_scopes.push(ControlScope {
                is_loop: false,
                label_only: false,
                labels: labels.clone(),
                targets: None,
            });
            let mut cases = Vec::new();
            for case in &switch.cases {
                let body = self.native_statements(&case.consequent, context)?;
                cases.push((case.test.as_deref().cloned(), body));
            }
            self.control_scopes.pop();
            self.push(Operation::Switch(Box::new(GeneratorSwitch {
                value,
                cases,
                labels,
                dispatch: false,
            })));
            return Some(());
        }

        // Save the discriminant before evaluating any case expressions, since
        // those expressions may mutate its source or suspend several times.
        let value = self.capture(value, context);
        let exit_slot = self.jump_targets.len();
        let first_case_slot = exit_slot + 1;
        self.jump_targets
            .resize(first_case_slot + switch.cases.len(), None);
        self.control_scopes.push(ControlScope {
            is_loop: false,
            label_only: false,
            labels,
            targets: Some((exit_slot, exit_slot)),
        });

        let mut cases = Vec::new();
        let mut default_slot = None;
        for (index, case) in switch.cases.iter().enumerate() {
            let slot = first_case_slot + index;
            let Some(test) = &case.test else {
                if default_slot.replace(slot).is_some() {
                    return None;
                }
                continue;
            };
            // Flush earlier tests before suspension: an earlier match must
            // skip every later case test, including its side effects/rejection.
            if Self::has_await(test, context.source) && !cases.is_empty() {
                self.push_switch_dispatch(&value, std::mem::take(&mut cases));
            }
            let test = self.expression(test, context)?;
            cases.push((
                Some(test),
                OperationBody {
                    operations: vec![Operation::PendingJump(slot)],
                    braced: false,
                },
            ));
        }
        if !cases.is_empty() {
            self.push_switch_dispatch(&value, cases);
        }
        self.push(Operation::PendingJump(default_slot.unwrap_or(exit_slot)));

        // Bodies remain in source order, independently of the dispatch order.
        // Explicit label assignments preserve fallthrough across empty arms.
        for (index, case) in switch.cases.iter().enumerate() {
            let label = self.label();
            self.jump_targets[first_case_slot + index] = Some(label);
            self.statements(&case.consequent, context)?;
        }
        let exit = self.label();
        self.jump_targets[exit_slot] = Some(exit);
        self.control_scopes.pop();
        Some(())
    }

    fn push_switch_dispatch(&mut self, value: &Expr, cases: Vec<(Option<Expr>, OperationBody)>) {
        self.push(Operation::Switch(Box::new(GeneratorSwitch {
            value: value.clone(),
            cases,
            labels: Vec::new(),
            dispatch: true,
        })));
    }
}

impl Emitter<'_> {
    pub(super) fn emit_generator_switch(&mut self, switch: &GeneratorSwitch, state: &str) {
        for label in &switch.labels {
            self.write(label);
            self.write(": ");
        }
        self.write("switch (");
        self.emit_expr(&switch.value);
        self.writeln(") {");
        self.indent += 1;
        for (test, body) in &switch.cases {
            if let Some(test) = test {
                self.write("case ");
                self.emit_expr(test);
                self.write(":");
            } else {
                self.write("default:");
            }
            if switch.dispatch {
                self.write(" ");
            } else {
                self.newline();
                self.indent += 1;
            }
            for op in &body.operations {
                self.emit_generator_operation(op, state);
            }
            if !switch.dispatch {
                self.indent -= 1;
            }
        }
        self.indent -= 1;
        self.writeln("}");
    }
}
