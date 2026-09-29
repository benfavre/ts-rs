//! Resumable for-in uses TypeScript's key snapshot and existence-check strategy.
use super::expressions::{assignment, binary, call, ident, member, synthetic};
use super::*;

impl GeneratorPlan {
    fn enumeration_local(&mut self, context: &Emitter<'_>) -> AstString {
        let name: AstString = self.fresh_name(context).into();
        // Enumeration bindings are hoisted at their statement's position,
        // before any variable introduced by its iteration target.
        self.locals.push(name.clone());
        name
    }

    fn enumeration_index(&mut self, context: &Emitter<'_>) -> AstString {
        let first_index = !self.used_loop_index;
        self.used_loop_index = true;
        if first_index && !context.source_has_identifier("_i") {
            self.locals.push("_i".into());
            "_i".into()
        } else {
            self.enumeration_local(context)
        }
    }

    fn enumeration_target(&mut self, left: &ForInOfLeft, context: &Emitter<'_>) -> Option<Expr> {
        match left {
            ForInOfLeft::Expr(expr) => self.reference(expr, false, context),
            ForInOfLeft::Pat(Pat {
                kind: PatKind::Ident(name),
                span,
            }) => Some(Expr {
                kind: ExprKind::Ident(name.clone()),
                span: *span,
            }),
            ForInOfLeft::Var(vars) if vars.kind == VarKind::Var && vars.declarations.len() == 1 => {
                let decl = &vars.declarations[0];
                let PatKind::Ident(name) = &decl.name.kind else {
                    return None;
                };
                if decl.init.is_some() {
                    return None;
                }
                if !self.locals.contains(name) {
                    self.locals.push(name.clone());
                }
                Some(ident(&self.catch_assignment_name(name)))
            }
            _ => None,
        }
    }

    pub(super) fn for_in_statement(
        &mut self,
        span: Span,
        loop_: &ForInStmt,
        labels: Vec<String>,
        context: &Emitter<'_>,
    ) -> Option<()> {
        if !context
            .source_between(span.start, span.end)
            .contains("await")
        {
            let left = self.enumeration_target(&loop_.left, context)?;
            self.control_scopes.push(ControlScope {
                is_loop: true,
                label_only: false,
                labels: labels.clone(),
                targets: None,
            });
            let body = self.native_body(&loop_.body, context)?;
            self.control_scopes.pop();
            self.push(Operation::Loop(Box::new(NativeLoop {
                kind: NativeLoopKind::ForIn(left, *loop_.right.clone()),
                labels,
                body,
            })));
            return Some(());
        }

        let object = self.enumeration_local(context);
        let keys = self.enumeration_local(context);
        let key = self.enumeration_local(context);
        let index = self.enumeration_index(context);
        let value = self.expression(&loop_.right, context)?;
        self.push(Operation::Assign(object.clone(), value));
        self.push(Operation::Assign(
            keys.clone(),
            synthetic(ExprKind::ArrayLit(Vec::new())),
        ));
        self.push(Operation::Loop(Box::new(NativeLoop {
            kind: NativeLoopKind::ForIn(ident(&key), ident(&object)),
            labels: Vec::new(),
            body: OperationBody {
                operations: vec![Operation::Expr(call(
                    member(ident(&keys), "push"),
                    vec![ident(&key)],
                ))],
                braced: false,
            },
        })));
        self.push(Operation::Assign(
            index.clone(),
            synthetic(ExprKind::NumLit("0".into())),
        ));
        let start = self.label();
        let exit_branch = (start, self.blocks[start].len());
        self.push(Operation::Branch(
            binary(ident(&index), BinaryOp::Lt, member(ident(&keys), "length")),
            0,
        ));
        self.push(Operation::Assign(
            key.clone(),
            synthetic(ExprKind::ElemAccess(ElemAccessExpr {
                object: Box::new(ident(&keys)),
                index: Box::new(ident(&index)),
                optional: false,
            })),
        ));
        let missing_branch = (start, self.blocks[start].len());
        self.push(Operation::Branch(
            binary(ident(&key), BinaryOp::In, ident(&object)),
            0,
        ));

        let exit_slot = self.jump_targets.len();
        let next_slot = exit_slot + 1;
        self.jump_targets.extend([None, None]);
        self.control_scopes.push(ControlScope {
            is_loop: true,
            label_only: false,
            labels,
            targets: Some((exit_slot, next_slot)),
        });
        // Build the target only after checking membership. Its receiver and
        // computed key may suspend and must be evaluated anew each iteration.
        let left = self.enumeration_target(&loop_.left, context)?;
        self.push(Operation::Expr(assignment(left, ident(&key))));
        self.statement_body(&loop_.body, context)?;
        let next = self.label();
        self.push(Operation::Expr(synthetic(ExprKind::Update(UpdateExpr {
            op: UpdateOp::PostInc,
            argument: Box::new(ident(&index)),
        }))));
        self.push(Operation::Jump(start));
        let exit = self.label();
        self.jump_targets[exit_slot] = Some(exit);
        self.jump_targets[next_slot] = Some(next);
        for ((block, operation), label) in [(exit_branch, exit), (missing_branch, next)] {
            let Operation::Branch(_, target) = &mut self.blocks[block][operation] else {
                unreachable!()
            };
            *target = label;
        }
        self.control_scopes.pop();
        Some(())
    }
}
