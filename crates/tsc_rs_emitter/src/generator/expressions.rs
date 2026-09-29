use super::*;

pub(super) fn synthetic(kind: ExprKind) -> Expr {
    Expr {
        kind,
        span: Span::new(0, 0),
    }
}
fn anchored(mut value: Expr, end: u32) -> Expr {
    value.span = Span::new(end, end);
    value
}

pub(super) fn ident(name: &str) -> Expr {
    synthetic(ExprKind::Ident(name.into()))
}
pub(super) fn member(object: Expr, property: &str) -> Expr {
    synthetic(ExprKind::Member(Box::new(MemberExpr {
        object: Box::new(object),
        property: property.into(),
        optional: false,
    })))
}
pub(super) fn call(callee: Expr, args: Vec<Expr>) -> Expr {
    synthetic(ExprKind::Call(Box::new(CallExpr {
        callee: Box::new(callee),
        args: args.into_iter().map(Box::new).collect(),
        type_args: None,
        optional: false,
    })))
}
pub(super) fn assignment(left: Expr, right: Expr) -> Expr {
    synthetic(ExprKind::Assign(AssignExpr {
        left: Box::new(left),
        op: AssignOp::Assign,
        right: Box::new(right),
    }))
}
fn paren(value: Expr, end: u32) -> Expr {
    if matches!(value.kind, ExprKind::Paren(_)) {
        value
    } else {
        Expr {
            kind: ExprKind::Paren(Box::new(value)),
            span: Span::new(end, end),
        }
    }
}
pub(super) fn binary(mut left: Expr, op: BinaryOp, right: Expr) -> Expr {
    left.span = Span::new(right.span.start, right.span.start);
    synthetic(ExprKind::Binary(BinaryExpr {
        left: Box::new(left),
        op,
        right: Box::new(right),
    }))
}
fn void_zero() -> Expr {
    synthetic(ExprKind::Void(Box::new(synthetic(ExprKind::NumLit(
        "0".into(),
    )))))
}

impl GeneratorPlan {
    pub(super) fn fresh_name(&mut self, context: &Emitter<'_>) -> String {
        const LETTERS: &[u8] = b"abcdefghjklmopqrstuvwxyz";
        loop {
            let index = self.reserved_temp_count;
            self.reserved_temp_count += 1;
            let name = if index < LETTERS.len() {
                format!("_{}", LETTERS[index] as char)
            } else {
                format!("_{}", index - LETTERS.len())
            };
            if !context.source_has_identifier(&name) {
                return name;
            }
        }
    }

    fn temporary(&mut self, context: &Emitter<'_>) -> Expr {
        let name = self.fresh_name(context);
        self.temps.push(name.clone().into());
        ident(&name)
    }

    pub(super) fn capture(&mut self, value: Expr, context: &Emitter<'_>) -> Expr {
        let temp = self.temporary(context);
        let ExprKind::Ident(name) = &temp.kind else {
            unreachable!()
        };
        self.push(Operation::Assign(name.clone(), value));
        temp
    }

    fn assign_temp(&mut self, temp: &Expr, value: Expr) {
        let ExprKind::Ident(name) = &temp.kind else {
            unreachable!()
        };
        self.push(Operation::Assign(name.clone(), value));
    }

    fn patch_branch(&mut self, block: usize, index: usize, target: usize) {
        let Operation::Branch(_, destination) = &mut self.blocks[block][index] else {
            unreachable!()
        };
        *destination = target;
    }

    // Each saved array prefix is evaluated before the next suspension. This
    // freezes argument values while later arguments remain unevaluated.
    fn array_parts<'e>(
        &mut self,
        args: impl IntoIterator<Item = Option<&'e Expr>>,
        context: &Emitter<'_>,
    ) -> Option<Expr> {
        self.array_parts_with_bind_receiver(args, false, context)
    }

    fn array_parts_with_bind_receiver<'e>(
        &mut self,
        args: impl IntoIterator<Item = Option<&'e Expr>>,
        bind_receiver: bool,
        context: &Emitter<'_>,
    ) -> Option<Expr> {
        let mut prefix: Option<Expr> = None;
        let mut segment = if bind_receiver {
            vec![Some(Box::new(void_zero()))]
        } else {
            Vec::new()
        };
        // The synthetic bind receiver has no evaluation to preserve. Delay its
        // array allocation when the first real argument immediately suspends.
        let mut only_bind_receiver = bind_receiver;
        for arg in args {
            let Some(arg) = arg else {
                segment.push(None);
                only_bind_receiver = false;
                continue;
            };
            if matches!(arg.kind, ExprKind::Spread(_)) {
                return None;
            }
            if Self::has_await(arg, context.source) && !segment.is_empty() && !only_bind_receiver {
                let array = synthetic(ExprKind::ArrayLit(std::mem::take(&mut segment)));
                let array = if let Some(prefix) = prefix.take() {
                    call(member(prefix, "concat"), vec![array])
                } else {
                    array
                };
                prefix = Some(self.capture(array, context));
            }
            segment.push(Some(Box::new(self.expression(arg, context)?)));
            only_bind_receiver = false;
        }
        let array = synthetic(ExprKind::ArrayLit(segment));
        Some(if let Some(prefix) = prefix {
            call(member(prefix, "concat"), vec![array])
        } else {
            array
        })
    }

    pub(super) fn expression(&mut self, expr: &Expr, context: &Emitter<'_>) -> Option<Expr> {
        let source = context.source;
        if let ExprKind::New(new) = &expr.kind {
            if context
                .lexical_downlevel_plan
                .invocation_spreads
                .contains_key(&expr.span.into())
            {
                let temporary = crate::new_spread::needs_constructor_temp(&new.callee)
                    .then(|| self.temporary(context));
                return self.expression(&context.lower_spread_new(new, temporary), context);
            }
        }
        if let ExprKind::Call(invocation) = &expr.kind {
            if context
                .lexical_downlevel_plan
                .invocation_spreads
                .contains_key(&expr.span.into())
            {
                let temporary = crate::call_spread::needs_receiver_temp(&invocation.callee, source)
                    .then(|| self.temporary(context));
                return self.expression(&context.lower_spread_call(invocation, temporary), context);
            }
        }
        if matches!(expr.kind, ExprKind::FnExpr(_) | ExprKind::Arrow(_))
            || !Self::has_await(expr, source)
        {
            return Some(expr.clone());
        }
        let mut result = expr.clone();
        match &mut result.kind {
            ExprKind::Await(value) => {
                let value = self.expression(value, context)?;
                self.push(Operation::Yield(value));
                self.label();
                let mut sent = call(member(ident(STATE_BINDING), "sent"), Vec::new());
                sent.span = Span::new(expr.span.end, expr.span.end);
                return Some(sent);
            }
            ExprKind::Paren(inner)
            | ExprKind::NonNull(inner)
            | ExprKind::Typeof(inner)
            | ExprKind::Void(inner)
            | ExprKind::Delete(inner) => **inner = self.expression(inner, context)?,
            ExprKind::As(inner) => *inner.expr = self.expression(&inner.expr, context)?,
            ExprKind::TypeAssertion(inner) => {
                *inner.expr = self.expression(&inner.expr, context)?
            }
            ExprKind::Satisfies(inner) => *inner.expr = self.expression(&inner.expr, context)?,
            ExprKind::Instantiation(inner) => {
                *inner.expr = self.expression(&inner.expr, context)?
            }
            ExprKind::Unary(inner) => {
                *inner.argument = self.expression(&inner.argument, context)?
            }
            ExprKind::Update(inner) => {
                *inner.argument = self.expression(&inner.argument, context)?
            }
            ExprKind::Binary(bin) => {
                if bin.op == BinaryOp::Exp {
                    // Exponentiation becomes a Math.pow call before generator
                    // lowering, so its callee also precedes argument awaits.
                    let mut invocation = call(
                        member(ident("Math"), "pow"),
                        vec![*bin.left.clone(), *bin.right.clone()],
                    );
                    invocation.span = expr.span;
                    return self.expression(&invocation, context);
                }
                let left = self.expression(&bin.left, context)?;
                if Self::has_await(&bin.right, source) {
                    let temp = self.capture(left, context);
                    if matches!(
                        bin.op,
                        BinaryOp::LogAnd | BinaryOp::LogOr | BinaryOp::NullCoal
                    ) {
                        let condition = short_circuit_test(temp.clone(), bin.op);
                        let origin = self.blocks.len() - 1;
                        let index = self.blocks[origin].len();
                        self.push(Operation::Branch(condition, 0));
                        let right = self.expression(&bin.right, context)?;
                        self.assign_temp(&temp, paren(right, bin.right.span.end));
                        let end = self.label();
                        self.patch_branch(origin, index, end);
                        return Some(temp);
                    }
                    let right = self.expression(&bin.right, context)?;
                    let mut temp = temp;
                    temp.span = Span::new(bin.left.span.end, bin.left.span.end);
                    *bin.left = temp;
                    *bin.right = paren(right, bin.right.span.end);
                } else {
                    *bin.left = paren(left, bin.left.span.end);
                }
            }
            ExprKind::Comma(values) => {
                let mut pending = Vec::new();
                for value in values.iter() {
                    if Self::has_await(value, source) && !pending.is_empty() {
                        self.push(Operation::Expr(synthetic(ExprKind::Comma(std::mem::take(
                            &mut pending,
                        )))));
                    }
                    pending.push(Box::new(self.expression(value, context)?));
                }
                if pending.len() == 1 {
                    return pending.pop().map(|value| *value);
                }
                *values = pending;
            }
            ExprKind::Member(access) => {
                if access.optional {
                    return None;
                }
                *access.object = self.expression(&access.object, context)?;
            }
            ExprKind::ElemAccess(access) => {
                if access.optional {
                    return None;
                }
                let object = self.expression(&access.object, context)?;
                *access.object = if Self::has_await(&access.index, source) {
                    self.capture(object, context)
                } else {
                    object
                };
                *access.index = self.expression(&access.index, context)?;
            }
            ExprKind::ObjectLit(props) => return self.object_literal(expr, props, context),
            ExprKind::ArrayLit(elements) => {
                if elements
                    .iter()
                    .flatten()
                    .any(|e| matches!(e.kind, ExprKind::Spread(_)))
                {
                    // The file prepass owns helper discovery and excludes
                    // recovery/erased shapes. Keep the atomic fallback if it
                    // cannot establish the required helper declarations.
                    if !context
                        .lexical_downlevel_plan
                        .array_spreads
                        .contains_key(&expr.span.into())
                    {
                        return None;
                    }
                    return self.expression(&context.lower_array_spread(elements), context);
                }
                return self
                    .array_parts(elements.iter().map(|element| element.as_deref()), context);
            }
            ExprKind::Assign(assign) => {
                let later_await = Self::has_await(&assign.right, source);
                let left = self.reference(&assign.left, later_await, context)?;
                if later_await && assign.op != AssignOp::Assign {
                    let old_value = self.capture(left.clone(), context);
                    if let Some(operator) = logical_assignment(assign.op) {
                        let origin = self.blocks.len() - 1;
                        let index = self.blocks[origin].len();
                        self.push(Operation::Branch(
                            short_circuit_test(old_value.clone(), operator),
                            0,
                        ));
                        let right = self.expression(&assign.right, context)?;
                        self.assign_temp(&old_value, right);
                        self.push(Operation::Expr(assignment(left, old_value.clone())));
                        let end = self.label();
                        self.patch_branch(origin, index, end);
                        return Some(old_value);
                    }
                    let operator = arithmetic_assignment(assign.op)?;
                    let mut right = self.expression(&assign.right, context)?;
                    if matches!(
                        Self::untyped(&right).kind,
                        ExprKind::Binary(_)
                            | ExprKind::Cond(_)
                            | ExprKind::Assign(_)
                            | ExprKind::Comma(_)
                    ) {
                        right = paren(right, assign.right.span.end);
                    }
                    *assign.left = left;
                    *assign.right = binary(old_value, operator, right);
                    assign.op = AssignOp::Assign;
                } else {
                    *assign.left = left;
                    *assign.right = self.expression(&assign.right, context)?;
                }
            }
            ExprKind::Call(invocation) => {
                if invocation.optional
                    || optional_chain(&invocation.callee)
                    || invocation
                        .args
                        .iter()
                        .any(|arg| matches!(arg.kind, ExprKind::Spread(_)))
                {
                    return None;
                }
                let suspending_args = invocation
                    .args
                    .iter()
                    .any(|arg| Self::has_await(arg, source));
                if !suspending_args {
                    *invocation.callee = self.expression(&invocation.callee, context)?;
                } else if crate::array_spread::is_synthetic_helper(&invocation.callee) {
                    // TypeScript helper identities are stable across suspension;
                    // only their argument values need staging.
                    let args = self.array_parts(
                        invocation.args.iter().map(|arg| Some(arg.as_ref())),
                        context,
                    )?;
                    return Some(call(
                        member(*invocation.callee.clone(), "apply"),
                        vec![void_zero(), args],
                    ));
                } else {
                    let callee = self.expression(&invocation.callee, context)?;
                    let reference = reference_value(&callee);
                    // Applying an extracted eval would change direct-eval scope.
                    if matches!(&reference.kind, ExprKind::Ident(name) if name == "eval") {
                        return None;
                    }
                    let (callee, receiver) = match &reference.kind {
                        ExprKind::Member(access) if !access.optional => {
                            let receiver = self.temporary(context);
                            (
                                member(
                                    paren(assignment(receiver.clone(), *access.object.clone()), 0),
                                    &access.property,
                                ),
                                receiver,
                            )
                        }
                        ExprKind::ElemAccess(access) if !access.optional => {
                            let receiver = self.temporary(context);
                            let callee = synthetic(ExprKind::ElemAccess(ElemAccessExpr {
                                object: Box::new(paren(
                                    assignment(receiver.clone(), *access.object.clone()),
                                    0,
                                )),
                                index: access.index.clone(),
                                optional: false,
                            }));
                            (callee, receiver)
                        }
                        _ => (callee, void_zero()),
                    };
                    let function = self.capture(callee, context);
                    let args = self.array_parts(
                        invocation.args.iter().map(|arg| Some(arg.as_ref())),
                        context,
                    )?;
                    return Some(call(member(function, "apply"), vec![receiver, args]));
                }
            }
            ExprKind::New(new) => {
                if new.args.as_ref().is_some_and(|args| {
                    args.iter()
                        .any(|arg| matches!(arg.kind, ExprKind::Spread(_)))
                }) {
                    return None;
                }
                if new
                    .args
                    .as_ref()
                    .is_some_and(|args| args.iter().any(|arg| Self::has_await(arg, source)))
                {
                    // Capture even identifier constructors: their bindings can
                    // change while an argument awaits. The bind receiver itself
                    // is synthetic, so it need not allocate a saved array alone.
                    let constructor = self.expression(&new.callee, context)?;
                    let receiver = self.temporary(context);
                    let bind = member(paren(assignment(receiver.clone(), constructor), 0), "bind");
                    let function = self.capture(bind, context);
                    let args = self.array_parts_with_bind_receiver(
                        new.args.iter().flatten().map(|arg| Some(arg.as_ref())),
                        true,
                        context,
                    )?;
                    let bound = call(member(function, "apply"), vec![receiver, args]);
                    new.callee = Box::new(paren(bound, 0));
                    new.args = Some(Vec::new());
                } else {
                    *new.callee = self.expression(&new.callee, context)?;
                }
            }
            ExprKind::Cond(conditional) => {
                let test = self.expression(&conditional.test, context)?;
                if !Self::has_await(&conditional.consequent, source)
                    && !Self::has_await(&conditional.alternate, source)
                {
                    *conditional.test = paren(test, conditional.test.span.end);
                } else {
                    let result = self.temporary(context);
                    let origin = self.blocks.len() - 1;
                    let branch_index = self.blocks[origin].len();
                    self.push(Operation::Branch(test, 0));
                    let consequent = self.expression(&conditional.consequent, context)?;
                    self.assign_temp(&result, consequent);
                    let then_end = self.blocks.len() - 1;
                    let jump_index = self.blocks[then_end].len();
                    self.push(Operation::Jump(0));
                    let alternate_label = self.label();
                    self.patch_branch(origin, branch_index, alternate_label);
                    let alternate = self.expression(&conditional.alternate, context)?;
                    self.assign_temp(&result, alternate);
                    let end = self.label();
                    self.blocks[then_end][jump_index] = Operation::Jump(end);
                    return Some(result);
                }
            }
            _ => return None,
        }
        // A zero-width source anchor retains operator layout without allowing
        // source-preserving paths to copy the erased await back into output.
        result.span = Span::new(expr.span.end, expr.span.end);
        Some(result)
    }

    pub(super) fn reference(
        &mut self,
        expr: &Expr,
        later_await: bool,
        context: &Emitter<'_>,
    ) -> Option<Expr> {
        let mut result = Self::untyped(expr).clone();
        match &mut result.kind {
            ExprKind::Paren(inner) => **inner = self.reference(inner, later_await, context)?,
            ExprKind::Ident(_) => {}
            ExprKind::Member(access) if !access.optional => {
                let object = self.expression(&access.object, context)?;
                *access.object = if later_await {
                    anchored(self.capture(object, context), access.object.span.end)
                } else {
                    object
                };
            }
            ExprKind::ElemAccess(access) if !access.optional => {
                let object = self.expression(&access.object, context)?;
                *access.object = if later_await || Self::has_await(&access.index, context.source) {
                    anchored(self.capture(object, context), access.object.span.end)
                } else {
                    object
                };
                let index = self.expression(&access.index, context)?;
                *access.index = if later_await {
                    anchored(self.capture(index, context), access.index.span.end)
                } else {
                    index
                };
            }
            _ => return None,
        }
        // Identifier spans carry lexical binding identity (notably catches).
        // Only reconstructed compound targets need a zero-width source anchor.
        if !matches!(result.kind, ExprKind::Ident(_)) {
            result.span = Span::new(expr.span.end, expr.span.end);
        }
        Some(result)
    }

    pub(super) fn resolve_state_bindings(&mut self) {
        let state: AstString = self.state.clone().into();
        self.resolve_synthetic_bindings(&|name| (name == STATE_BINDING).then(|| state.clone()));
    }

    pub(super) fn resolve_synthetic_bindings<F: Fn(&AstString) -> Option<AstString>>(
        &mut self,
        names: &F,
    ) {
        fn expression<F: Fn(&AstString) -> Option<AstString>>(expr: &mut Expr, names: &F) {
            match &mut expr.kind {
                ExprKind::Ident(name) if expr.span == Span::new(0, 0) => {
                    if let Some(replacement) = names(name) {
                        *name = replacement;
                    }
                }
                ExprKind::Call(call) => {
                    expression(&mut call.callee, names);
                    for arg in &mut call.args {
                        expression(arg, names);
                    }
                }
                ExprKind::New(new) => {
                    expression(&mut new.callee, names);
                    for arg in new.args.iter_mut().flatten() {
                        expression(arg, names);
                    }
                }
                ExprKind::ObjectLit(props) => {
                    for prop in props {
                        let key = match prop {
                            ObjLitProp::Property(p) => {
                                expression(&mut p.value, names);
                                Some(&mut p.key)
                            }
                            ObjLitProp::Method(m) => Some(&mut m.name),
                            ObjLitProp::Get(a) | ObjLitProp::Set(a) => Some(&mut a.name),
                            ObjLitProp::Spread(e, _) | ObjLitProp::ShorthandDefault(_, e, _) => {
                                expression(e, names);
                                None
                            }
                            _ => None,
                        };
                        if let Some(PropName::Computed(e, _)) = key {
                            expression(e, names);
                        }
                    }
                }
                ExprKind::Member(access) => expression(&mut access.object, names),
                ExprKind::ElemAccess(access) => {
                    expression(&mut access.object, names);
                    expression(&mut access.index, names);
                }
                ExprKind::Binary(binary) => {
                    expression(&mut binary.left, names);
                    expression(&mut binary.right, names);
                }
                ExprKind::Assign(assign) => {
                    expression(&mut assign.left, names);
                    expression(&mut assign.right, names);
                }
                ExprKind::Cond(cond) => {
                    expression(&mut cond.test, names);
                    expression(&mut cond.consequent, names);
                    expression(&mut cond.alternate, names);
                }
                ExprKind::Paren(inner)
                | ExprKind::NonNull(inner)
                | ExprKind::Typeof(inner)
                | ExprKind::Void(inner)
                | ExprKind::Delete(inner) => expression(inner, names),
                ExprKind::As(inner) => expression(&mut inner.expr, names),
                ExprKind::TypeAssertion(inner) => expression(&mut inner.expr, names),
                ExprKind::Satisfies(inner) => expression(&mut inner.expr, names),
                ExprKind::Instantiation(inner) => expression(&mut inner.expr, names),
                ExprKind::Unary(inner) => expression(&mut inner.argument, names),
                ExprKind::Update(inner) => expression(&mut inner.argument, names),
                ExprKind::Comma(values) => {
                    for value in values {
                        expression(value, names);
                    }
                }
                ExprKind::ArrayLit(values) => {
                    for value in values.iter_mut().flatten() {
                        expression(value, names);
                    }
                }
                _ => {}
            }
        }
        fn operations<F: Fn(&AstString) -> Option<AstString>>(ops: &mut [Operation], names: &F) {
            for op in ops {
                if let Operation::Assign(name, _) | Operation::Catch(name) = op {
                    if let Some(replacement) = names(name) {
                        *name = replacement;
                    }
                }
                match op {
                    Operation::Expr(value)
                    | Operation::Assign(_, value)
                    | Operation::Throw(value)
                    | Operation::Yield(value)
                    | Operation::Branch(value, _)
                    | Operation::BranchTrue(value, _) => expression(value, names),
                    Operation::Return(Some(value)) => expression(value, names),
                    Operation::Loop(loop_) => {
                        match &mut loop_.kind {
                            NativeLoopKind::While(test) | NativeLoopKind::DoWhile(test) => {
                                expression(test, names)
                            }
                            NativeLoopKind::ForIn(left, right) => {
                                expression(left, names);
                                expression(right, names);
                            }
                            NativeLoopKind::For(init, test, update) => {
                                for expr in [init, test, update].into_iter().flatten() {
                                    expression(expr, names);
                                }
                            }
                        }
                        operations(&mut loop_.body.operations, names);
                    }
                    Operation::Block(block) => operations(&mut block.body.operations, names),
                    Operation::Switch(switch) => {
                        expression(&mut switch.value, names);
                        for (test, body) in &mut switch.cases {
                            if let Some(test) = test {
                                expression(test, names);
                            }
                            operations(&mut body.operations, names);
                        }
                    }
                    Operation::NativeTry(try_) => {
                        operations(&mut try_.body.operations, names);
                        if let Some((_, catch)) = &mut try_.catch {
                            operations(&mut catch.operations, names);
                        }
                        if let Some(finally) = &mut try_.finally {
                            operations(&mut finally.operations, names);
                        }
                    }
                    Operation::If(test, yes, no) => {
                        expression(test, names);
                        operations(&mut yes.operations, names);
                        if let Some(no) = no {
                            operations(&mut no.operations, names);
                        }
                    }
                    _ => {}
                }
            }
        }
        for local in &mut self.locals {
            if let Some(replacement) = names(local) {
                *local = replacement;
            }
        }
        for block in &mut self.blocks {
            operations(block, names);
        }
    }
}

fn reference_value(expr: &Expr) -> &Expr {
    let expr = GeneratorPlan::untyped(expr);
    if let ExprKind::Paren(inner) = &expr.kind {
        reference_value(inner)
    } else {
        expr
    }
}

fn optional_chain(expr: &Expr) -> bool {
    match &reference_value(expr).kind {
        ExprKind::Member(member) => member.optional || optional_chain(&member.object),
        ExprKind::ElemAccess(access) => access.optional || optional_chain(&access.object),
        ExprKind::Call(call) => call.optional || optional_chain(&call.callee),
        _ => false,
    }
}

fn logical_assignment(op: AssignOp) -> Option<BinaryOp> {
    match op {
        AssignOp::LogAndAssign => Some(BinaryOp::LogAnd),
        AssignOp::LogOrAssign => Some(BinaryOp::LogOr),
        AssignOp::NullCoalAssign => Some(BinaryOp::NullCoal),
        _ => None,
    }
}
fn arithmetic_assignment(op: AssignOp) -> Option<BinaryOp> {
    Some(match op {
        AssignOp::AddAssign => BinaryOp::Add,
        AssignOp::SubAssign => BinaryOp::Sub,
        AssignOp::MulAssign => BinaryOp::Mul,
        AssignOp::DivAssign => BinaryOp::Div,
        AssignOp::ModAssign => BinaryOp::Mod,
        AssignOp::ExpAssign => BinaryOp::Exp,
        AssignOp::BitAndAssign => BinaryOp::BitAnd,
        AssignOp::BitOrAssign => BinaryOp::BitOr,
        AssignOp::BitXorAssign => BinaryOp::BitXor,
        AssignOp::ShlAssign => BinaryOp::Shl,
        AssignOp::ShrAssign => BinaryOp::Shr,
        AssignOp::UShrAssign => BinaryOp::UShr,
        _ => return None,
    })
}
fn short_circuit_test(value: Expr, op: BinaryOp) -> Expr {
    match op {
        BinaryOp::LogAnd => value,
        BinaryOp::LogOr => synthetic(ExprKind::Unary(UnaryExpr {
            op: UnaryOp::LogNot,
            argument: Box::new(value),
        })),
        _ => binary(
            binary(
                value.clone(),
                BinaryOp::StrictEq,
                synthetic(ExprKind::NullLit),
            ),
            BinaryOp::LogOr,
            binary(value, BinaryOp::StrictEq, void_zero()),
        ),
    }
}
