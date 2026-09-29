//! Statement and expression analysis helpers for the emitter.

use super::*;

impl<'a> Emitter<'a> {
    /// Check if a statement needs downlevel transformation based on target.
    pub(crate) fn stmt_needs_downlevel(&self, stmt: &Stmt) -> bool {
        if !self.active_lexical_loop_helpers.is_empty() {
            return true;
        }
        if self.lexical_downlevel_plan.control(stmt.span).is_some() {
            return true;
        }
        match &stmt.kind {
            StmtKind::FnDecl(fn_decl) => {
                (fn_decl.is_async
                    && ((!fn_decl.is_generator && self.needs_downlevel("async"))
                        || (fn_decl.is_generator && self.needs_downlevel("async-generator"))))
                    || (!fn_decl.is_async
                        && !fn_decl.is_generator
                        && self.can_downlevel_simple_param_initializers(&fn_decl.params))
            }
            StmtKind::Var(var_stmt) => {
                (matches!(var_stmt.kind, VarKind::Using | VarKind::AwaitUsing)
                    && self.needs_downlevel("using"))
                    || (var_stmt.kind != VarKind::Var && self.needs_lexical_downlevel())
                    || var_stmt.declarations.iter().any(|d| {
                        // Check if the binding pattern has object rest that needs transform
                        // (including nested patterns like `{ f: { a, ...rest } }`)
                        (self.needs_downlevel("object-spread") && pat_has_object_rest(&d.name))
                            || d.init
                                .as_ref()
                                .is_some_and(|e| self.expr_needs_downlevel(e))
                    })
            }
            StmtKind::Expr(expr) => {
                self.expr_needs_downlevel(expr)
                    || self
                        .lexical_downlevel_plan
                        .has_renamed_reference_in(expr.span)
            }
            StmtKind::Return(opt_expr) => opt_expr.as_ref().is_some_and(|e| {
                self.expr_needs_downlevel(e)
                    || self.lexical_downlevel_plan.has_renamed_reference_in(e.span)
            }),
            StmtKind::If(if_stmt) => {
                self.expr_needs_downlevel(&if_stmt.test)
                    || self.stmt_needs_downlevel(&if_stmt.consequent)
                    || if_stmt
                        .alternate
                        .as_ref()
                        .is_some_and(|s| self.stmt_needs_downlevel(s))
            }
            StmtKind::While(wh) => {
                self.expr_needs_downlevel(&wh.test) || self.stmt_needs_downlevel(&wh.body)
            }
            StmtKind::For(f) => {
                f.init.as_ref().is_some_and(|init| match init {
                    ForInit::Expr(e) => self.expr_needs_downlevel(e),
                    ForInit::Var(vs) => {
                        (matches!(vs.kind, VarKind::Using | VarKind::AwaitUsing)
                            && self.needs_downlevel("using"))
                            || (vs.kind != VarKind::Var && self.needs_lexical_downlevel())
                            || vs.declarations.iter().any(|d| {
                                d.init
                                    .as_ref()
                                    .is_some_and(|e| self.expr_needs_downlevel(e))
                            })
                    }
                }) || f
                    .test
                    .as_ref()
                    .is_some_and(|e| self.expr_needs_downlevel(e))
                    || f.update
                        .as_ref()
                        .is_some_and(|e| self.expr_needs_downlevel(e))
                    || self.stmt_needs_downlevel(&f.body)
            }
            StmtKind::ForIn(fi) => {
                (matches!(&fi.left, ForInOfLeft::Var(vs) if vs.kind != VarKind::Var)
                    && self.needs_lexical_downlevel())
                    || self.expr_needs_downlevel(&fi.right)
                    || self.for_in_of_left_needs_oc_downlevel(&fi.left)
                    || self.stmt_needs_downlevel(&fi.body)
            }
            StmtKind::ForOf(fo) => {
                self.needs_downlevel("for-of")
                    || (fo.is_await && self.needs_downlevel("async-generator"))
                    || (matches!(&fo.left, ForInOfLeft::Var(vs) if matches!(vs.kind, VarKind::Using | VarKind::AwaitUsing))
                        && self.needs_downlevel("using"))
                    || (self.needs_downlevel("object-spread") && self.for_of_has_obj_rest(fo))
                    || self.expr_needs_downlevel(&fo.right)
                    || self.for_in_of_left_needs_oc_downlevel(&fo.left)
                    || self.stmt_needs_downlevel(&fo.body)
            }
            StmtKind::Block(stmts) => stmts.iter().any(|s| self.stmt_needs_downlevel(s)),
            StmtKind::Labeled(labeled) => self.stmt_needs_downlevel(&labeled.body),
            StmtKind::Throw(expr) => {
                self.expr_needs_downlevel(expr)
                    || self
                        .lexical_downlevel_plan
                        .has_renamed_reference_in(expr.span)
            }
            _ => false,
        }
    }

    /// Check if a for-in/of left-hand side needs optional chaining downlevel.
    /// This detects `<error>` patterns whose source text contains `?.`.
    fn for_in_of_left_needs_oc_downlevel(&self, left: &ForInOfLeft) -> bool {
        if !self.needs_downlevel("optional-chaining") {
            return false;
        }
        if let ForInOfLeft::Expr(expr) = left {
            return self.expr_needs_downlevel(expr);
        }
        if let ForInOfLeft::Pat(pat) = left {
            if matches!(&pat.kind, PatKind::Ident(name) if name == "<error>") {
                let s = pat.span.start as usize;
                let e = pat.span.end as usize;
                if s < e && e <= self.source.len() {
                    return self.source[s..e].contains("?.");
                }
            }
        }
        false
    }

    /// Check if an expression needs downlevel transformation based on target.
    pub(crate) fn expr_needs_downlevel(&self, expr: &Expr) -> bool {
        let mut stack = vec![expr];
        while let Some(expr) = stack.pop() {
            match &expr.kind {
                ExprKind::Call(call)
                    if (self.should_downlevel_dynamic_import()
                        || (self.is_system() && !self.system_context_fn.is_empty()))
                        && self.is_dynamic_import_call(call) =>
                {
                    return true;
                }
                ExprKind::Member(mem) if mem.optional => {
                    if self.needs_downlevel("optional-chaining") {
                        return true;
                    }
                    stack.push(&mem.object);
                }
                ExprKind::ElemAccess(ea) if ea.optional => {
                    if self.needs_downlevel("optional-chaining") {
                        return true;
                    }
                    stack.push(&ea.index);
                    stack.push(&ea.object);
                }
                ExprKind::Call(call) if call.optional => {
                    if self.needs_downlevel("optional-chaining") {
                        return true;
                    }
                    for arg in call.args.iter().rev() {
                        stack.push(arg);
                    }
                    stack.push(&call.callee);
                }
                ExprKind::Binary(bin) if bin.op == BinaryOp::NullCoal => {
                    if self.needs_downlevel("nullish-coalescing") {
                        return true;
                    }
                }
                ExprKind::Binary(bin) if bin.op == BinaryOp::Exp => {
                    if self.needs_downlevel("exponentiation") {
                        return true;
                    }
                    stack.push(&bin.right);
                    stack.push(&bin.left);
                }
                ExprKind::Binary(bin)
                    if bin.op == BinaryOp::In
                        && matches!(&bin.left.kind, ExprKind::Ident(name) if name.starts_with('#')) =>
                {
                    if self.needs_downlevel("private-fields") {
                        return true;
                    }
                }
                ExprKind::Assign(assign) => match assign.op {
                    AssignOp::LogAndAssign | AssignOp::LogOrAssign | AssignOp::NullCoalAssign => {
                        if self.needs_downlevel("logical-assignment")
                            || (self.needs_downlevel("private-fields")
                                && self.expr_has_private_field_access(&assign.left))
                        {
                            return true;
                        }
                        stack.push(&assign.right);
                        stack.push(&assign.left);
                    }
                    AssignOp::ExpAssign => {
                        if self.needs_downlevel("exponentiation") {
                            return true;
                        }
                        stack.push(&assign.right);
                        stack.push(&assign.left);
                    }
                    _ => {
                        if (self.needs_downlevel("object-spread")
                            && assign.op == AssignOp::Assign
                            && assign_target_has_object_rest(&assign.left))
                            || (self.needs_downlevel("private-fields")
                                && assign.op == AssignOp::Assign
                                && self.expr_has_private_destructure_target_in_assign_lhs(
                                    &assign.left,
                                ))
                        {
                            return true;
                        }
                        stack.push(&assign.right);
                        stack.push(&assign.left);
                    }
                },
                ExprKind::ObjectLit(props) => {
                    if self.computed_object_literal_can_downlevel(props, expr.span)
                        || (self.needs_downlevel("object-spread")
                            && props.iter().any(|p| matches!(p, ObjLitProp::Spread(_, _))))
                        || props.iter().any(|p| {
                            matches!(
                                p,
                                ObjLitProp::Method(m)
                                    if m.is_async
                                        && ((!m.is_generator && self.needs_downlevel("async"))
                                            || (m.is_generator
                                                && self.needs_downlevel("async-generator")))
                            )
                        })
                    {
                        return true;
                    }
                    for prop in props.iter().rev() {
                        match prop {
                            ObjLitProp::Property(prop) => {
                                // Only recurse into property values that don't
                                // themselves contain assignment targets (destructuring
                                // patterns). An assignment with an ObjectLit LHS
                                // containing Spread would falsely trigger object-spread
                                // downlevel for destructuring rest patterns.
                                if !matches!(prop.value.kind, ExprKind::Assign(_)) {
                                    stack.push(&prop.value);
                                }
                            }
                            // A shorthand default is evaluated as a value when
                            // its destructuring lookup produces `undefined`.
                            ObjLitProp::ShorthandDefault(_, default, _) => stack.push(default),
                            _ => {}
                        }
                    }
                }
                ExprKind::Member(mem) => {
                    if mem.property.starts_with('#') && self.needs_downlevel("private-fields") {
                        return true;
                    }
                    stack.push(&mem.object);
                }
                ExprKind::ElemAccess(ea) => {
                    stack.push(&ea.index);
                    stack.push(&ea.object);
                }
                ExprKind::Call(call) => {
                    if self
                        .lexical_downlevel_plan
                        .invocation_spreads
                        .contains_key(&expr.span.into())
                    {
                        return true;
                    }
                    for arg in call.args.iter().rev() {
                        stack.push(arg);
                    }
                    stack.push(&call.callee);
                }
                ExprKind::New(new_expr) => {
                    if self
                        .lexical_downlevel_plan
                        .invocation_spreads
                        .contains_key(&expr.span.into())
                    {
                        return true;
                    }
                    if let Some(args) = &new_expr.args {
                        for arg in args.iter().rev() {
                            stack.push(arg);
                        }
                    }
                    stack.push(&new_expr.callee);
                }
                ExprKind::Binary(bin) => {
                    stack.push(&bin.right);
                    stack.push(&bin.left);
                }
                ExprKind::Update(up) => {
                    // Private field update: ++obj.#field / obj.#field++
                    if self.needs_downlevel("private-fields")
                        && self.expr_has_private_field_access(&up.argument)
                    {
                        return true;
                    }
                    stack.push(&up.argument);
                }
                ExprKind::Unary(un) => stack.push(&un.argument),
                ExprKind::Typeof(inner) | ExprKind::Delete(inner) | ExprKind::Void(inner) => {
                    stack.push(inner);
                }
                ExprKind::Cond(cond) => {
                    stack.push(&cond.alternate);
                    stack.push(&cond.consequent);
                    stack.push(&cond.test);
                }
                ExprKind::Paren(inner) => stack.push(inner),
                ExprKind::Spread(inner) => stack.push(inner),
                ExprKind::Await(inner) => {
                    if self.needs_downlevel("async") {
                        return true;
                    }
                    stack.push(inner);
                }
                ExprKind::FnExpr(fn_decl) => {
                    if (fn_decl.is_async
                        && ((!fn_decl.is_generator && self.needs_downlevel("async"))
                            || (fn_decl.is_generator && self.needs_downlevel("async-generator"))))
                        || self.params_need_rest_transform(&fn_decl.params)
                        || (!fn_decl.is_async
                            && !fn_decl.is_generator
                            && self.can_downlevel_simple_param_initializers(&fn_decl.params))
                        || fn_decl.body.as_ref().is_some_and(|stmts| {
                            stmts.iter().any(|stmt| self.stmt_needs_downlevel(stmt))
                        })
                        || (self.should_downlevel_dynamic_import()
                            && fn_decl.body.as_ref().is_some_and(|stmts| {
                                stmts.iter().any(|s| self.stmt_has_dynamic_import_call(s))
                            }))
                    {
                        return true;
                    }
                }
                ExprKind::StrLit(_) if self.effective_target() < ScriptTarget::ES2015 => {
                    let start = expr.span.start as usize;
                    let end = expr.span.end as usize;
                    if start < end && end <= self.source.len() {
                        let raw = &self.source[start..end];
                        if raw.contains("\\u{") {
                            let quote = raw
                                .as_bytes()
                                .first()
                                .copied()
                                .filter(|q| *q == b'"' || *q == b'\'')
                                .map(char::from)
                                .unwrap_or('"');
                            if crate::emit_expr::downlevel_braced_unicode_escapes_in_string(
                                raw, quote,
                            )
                            .is_some()
                            {
                                return true;
                            }
                        }
                    }
                }
                ExprKind::NoSubstTemplate(_) if self.effective_target() < ScriptTarget::ES2015 => {
                    return true;
                }
                ExprKind::Template(tpl) => {
                    if self.effective_target() < ScriptTarget::ES2015 {
                        return true;
                    }
                    for inner in tpl.exprs.iter().rev() {
                        stack.push(inner);
                    }
                }
                ExprKind::TaggedTemplate(tagged) => {
                    // Optional chain on tagged template (`a?.\`b\``) is not valid JS;
                    // always force expression path so the `?.` is stripped.
                    if let ExprKind::Member(mem) = &tagged.tag.kind {
                        if mem.optional && mem.property == "<error>" {
                            return true;
                        }
                    }
                    // ES5 and earlier lower every tagged template. ES2015-ES2017
                    // only need the helper when a quasi has an invalid escape.
                    if self.effective_target() < ScriptTarget::ES2015
                        || (self.needs_downlevel("tagged-template")
                            && crate::emit_stmt_helpers::tagged_template_needs_lowering(
                                &tagged.quasi,
                            ))
                    {
                        return true;
                    }
                    for inner in tagged.quasi.exprs.iter().rev() {
                        stack.push(inner);
                    }
                    stack.push(&tagged.tag);
                }
                ExprKind::Arrow(arrow) => {
                    if arrow.is_async && self.needs_downlevel("async") {
                        return true;
                    }
                    if self.can_downlevel_hazard_free_es5_arrow(expr.span, arrow) {
                        return true;
                    }
                    if self.can_downlevel_simple_arrow_params(expr.span, arrow) {
                        return true;
                    }
                    if self.params_need_rest_transform(&arrow.params) {
                        return true;
                    }
                    if self.should_downlevel_dynamic_import() {
                        if let ArrowBody::Block(stmts) = &arrow.body {
                            if stmts.iter().any(|s| self.stmt_has_dynamic_import_call(s)) {
                                return true;
                            }
                        }
                    }
                    match &arrow.body {
                        ArrowBody::Expr(e) => stack.push(e),
                        ArrowBody::Block(stmts)
                            if stmts.iter().any(|stmt| self.stmt_needs_downlevel(stmt)) =>
                        {
                            return true;
                        }
                        ArrowBody::Block(_) => {}
                    }
                }
                ExprKind::ArrayLit(elements) => {
                    if self
                        .lexical_downlevel_plan
                        .array_spreads
                        .contains_key(&expr.span.into())
                    {
                        return true;
                    }
                    for inner in elements.iter().rev().flatten() {
                        stack.push(inner);
                    }
                }
                ExprKind::Comma(exprs) => {
                    for inner in exprs.iter().rev() {
                        stack.push(inner);
                    }
                }
                ExprKind::Yield(_, Some(inner)) => stack.push(inner),
                ExprKind::Yield(_, None) => {}
                ExprKind::NonNull(inner) => stack.push(inner),
                ExprKind::As(a) => stack.push(&a.expr),
                ExprKind::Satisfies(s) => stack.push(&s.expr),
                // Class name → static alias substitution: force structured emit
                // when an identifier references the current class name inside a class
                // body that has a static alias.
                ExprKind::Ident(name) => {
                    if self.current_class_static_alias.is_some()
                        && self.current_class_name.as_deref() == Some(name.as_str())
                    {
                        return true;
                    }
                }
                _ => {}
            }
        }
        false
    }

    fn expr_has_private_destructure_target_in_assign_lhs(&self, expr: &Expr) -> bool {
        match &expr.kind {
            ExprKind::Member(m) => m.property.starts_with('#'),
            ExprKind::ObjectLit(props) => props.iter().any(|p| match p {
                ObjLitProp::Property(op) => {
                    self.expr_has_private_destructure_target_in_assign_lhs(&op.value)
                }
                ObjLitProp::Spread(e, _) => {
                    self.expr_has_private_destructure_target_in_assign_lhs(e)
                }
                _ => false,
            }),
            ExprKind::ArrayLit(elements) => elements
                .iter()
                .flatten()
                .any(|e| self.expr_has_private_destructure_target_in_assign_lhs(e)),
            ExprKind::Assign(a) => self.expr_has_private_destructure_target_in_assign_lhs(&a.left),
            ExprKind::Paren(e) | ExprKind::NonNull(e) => {
                self.expr_has_private_destructure_target_in_assign_lhs(e)
            }
            ExprKind::As(a) => self.expr_has_private_destructure_target_in_assign_lhs(&a.expr),
            ExprKind::Satisfies(s) => {
                self.expr_has_private_destructure_target_in_assign_lhs(&s.expr)
            }
            ExprKind::TypeAssertion(ta) => {
                self.expr_has_private_destructure_target_in_assign_lhs(&ta.expr)
            }
            _ => false,
        }
    }

    /// Check if an expression (possibly wrapped in parens/casts) is a private field member access.
    fn expr_has_private_field_access(&self, expr: &Expr) -> bool {
        match &expr.kind {
            ExprKind::Member(mem) => mem.property.starts_with('#'),
            ExprKind::Paren(inner) | ExprKind::NonNull(inner) => {
                self.expr_has_private_field_access(inner)
            }
            ExprKind::As(a) => self.expr_has_private_field_access(&a.expr),
            ExprKind::Satisfies(s) => self.expr_has_private_field_access(&s.expr),
            ExprKind::TypeAssertion(ta) => self.expr_has_private_field_access(&ta.expr),
            _ => false,
        }
    }

    /// Check if a statement contains any function/arrow/method body whose
    /// inner statements are missing explicit semicolons in the source text.
    /// When true, the statement must go through structured emit to get
    /// proper semicolons (ASI recovery).
    pub(crate) fn has_inner_missing_semicolons(&self, stmt: &Stmt) -> bool {
        // Fast emit doesn't reproduce tsc's ASI/explicit-semicolon formatting —
        // verbatim source copy preserves the original layout and ASI handles it.
        // Returning false here skips the subtree walk AND lets the statement take
        // the cheap source-copy path. Only valid on clean parses: this predicate
        // also detects parser-recovery shapes (e.g. `{ a; b }` object literals
        // with `;` separators) whose structured-emit repair is a validity fix,
        // so on files with parse diagnostics the real walk must still run.
        if self.fast_emit && !self.file_has_recovery_errors {
            return false;
        }
        match &stmt.kind {
            StmtKind::Var(var_stmt) => var_stmt.declarations.iter().any(|d| {
                d.init
                    .as_ref()
                    .is_some_and(|e| self.expr_has_missing_semis(e))
            }),
            StmtKind::Expr(expr) => self.expr_has_missing_semis(expr),
            StmtKind::Return(Some(expr)) | StmtKind::Throw(expr) => {
                self.expr_has_missing_semis(expr)
            }
            // Compound statements whose bodies can go through emit_source_line:
            // check their children for missing semicolons so we route them
            // through the structured emit path instead.
            StmtKind::Block(stmts) => {
                self.body_has_missing_semis(stmts)
                    || stmts.iter().any(|s| self.has_inner_missing_semicolons(s))
            }
            StmtKind::For(f) => self.has_inner_missing_semicolons(&f.body),
            StmtKind::ForIn(fi) => self.has_inner_missing_semicolons(&fi.body),
            StmtKind::ForOf(fo) => self.has_inner_missing_semicolons(&fo.body),
            StmtKind::With(w) => self.has_inner_missing_semicolons(&w.body),
            StmtKind::If(if_stmt) => {
                self.has_inner_missing_semicolons(&if_stmt.consequent)
                    || if_stmt
                        .alternate
                        .as_ref()
                        .is_some_and(|alt| self.has_inner_missing_semicolons(alt))
            }
            StmtKind::While(w) => self.has_inner_missing_semicolons(&w.body),
            StmtKind::DoWhile(dw) => self.has_inner_missing_semicolons(&dw.body),
            StmtKind::Switch(sw) => sw.cases.iter().any(|c| {
                self.body_has_missing_semis(&c.consequent)
                    || c.consequent
                        .iter()
                        .any(|s| self.has_inner_missing_semicolons(s))
            }),
            StmtKind::Try(t) => {
                self.body_has_missing_semis(&t.block)
                    || t.block.iter().any(|s| self.has_inner_missing_semicolons(s))
                    || t.handler.as_ref().is_some_and(|h| {
                        self.body_has_missing_semis(&h.body)
                            || h.body.iter().any(|s| self.has_inner_missing_semicolons(s))
                    })
                    || t.finalizer.as_ref().is_some_and(|f| {
                        self.body_has_missing_semis(f)
                            || f.iter().any(|s| self.has_inner_missing_semicolons(s))
                    })
            }
            StmtKind::Labeled(l) => self.has_inner_missing_semicolons(&l.body),
            StmtKind::FnDecl(fn_decl) => fn_decl.body.as_ref().is_some_and(|stmts| {
                self.body_has_missing_semis(stmts)
                    || stmts.iter().any(|s| self.has_inner_missing_semicolons(s))
            }),
            _ => false,
        }
    }

    pub(crate) fn expr_has_missing_semis(&self, expr: &Expr) -> bool {
        // See has_inner_missing_semicolons: skip only on clean parses — the
        // ObjectLit arm detects recovery shapes whose repair is a validity fix.
        if self.fast_emit && !self.file_has_recovery_errors {
            return false;
        }
        let mut stack = vec![expr];
        while let Some(expr) = stack.pop() {
            match &expr.kind {
                ExprKind::Arrow(arrow) => match &arrow.body {
                    ArrowBody::Block(stmts) => {
                        if self.body_has_missing_semis(stmts)
                            || stmts.iter().any(|s| self.has_inner_missing_semicolons(s))
                        {
                            return true;
                        }
                    }
                    ArrowBody::Expr(e) => stack.push(e),
                },
                ExprKind::FnExpr(fn_decl) => {
                    if fn_decl.body.as_ref().is_some_and(|stmts| {
                        self.body_has_missing_semis(stmts)
                            || stmts.iter().any(|s| self.has_inner_missing_semicolons(s))
                    }) {
                        return true;
                    }
                }
                ExprKind::Call(call) => {
                    for arg in call.args.iter().rev() {
                        stack.push(arg);
                    }
                    stack.push(&call.callee);
                }
                ExprKind::New(new_expr) => {
                    if let Some(args) = &new_expr.args {
                        for arg in args.iter().rev() {
                            stack.push(arg);
                        }
                    }
                    stack.push(&new_expr.callee);
                }
                ExprKind::ObjectLit(props) => {
                    // Check if any property uses `;` separator instead of `,` in source.
                    // This happens in error-recovery when the parser accepts `;` between
                    // shorthand properties (e.g., `{ a; b; c }`). When detected, the
                    // statement must go through structured emit to convert `;` to `,`.
                    if !props.is_empty() {
                        for i in 0..props.len() - 1 {
                            let prop_end = obj_lit_prop_span(&props[i]).end as usize;
                            let next_start = obj_lit_prop_span(&props[i + 1]).start as usize;
                            if prop_end < next_start && next_start <= self.source.len() {
                                let between = &self.source[prop_end..next_start];
                                if between.contains(';') && !between.contains(',') {
                                    return true;
                                }
                            }
                        }
                        // Also check for `;` after the last property before `}`
                        let last_end = obj_lit_prop_span(props.last().unwrap()).end as usize;
                        let obj_end = expr.span.end as usize;
                        if last_end < obj_end && obj_end <= self.source.len() {
                            let trailing = &self.source[last_end..obj_end];
                            if trailing.contains(';') {
                                return true;
                            }
                        }
                    }
                    for prop in props.iter().rev() {
                        match prop {
                            ObjLitProp::Property(p) => stack.push(&p.value),
                            ObjLitProp::Spread(e, _) => stack.push(e),
                            ObjLitProp::Method(m) => {
                                if self.body_has_missing_semis(&m.body) {
                                    return true;
                                }
                            }
                            ObjLitProp::Get(a) | ObjLitProp::Set(a) => {
                                if self.body_has_missing_semis(&a.body) {
                                    return true;
                                }
                            }
                            _ => {}
                        }
                    }
                }
                ExprKind::ArrayLit(elements) => {
                    for inner in elements.iter().rev().flatten() {
                        stack.push(inner);
                    }
                }
                ExprKind::Binary(bin) => {
                    stack.push(&bin.right);
                    stack.push(&bin.left);
                }
                ExprKind::Cond(cond) => {
                    stack.push(&cond.alternate);
                    stack.push(&cond.consequent);
                    stack.push(&cond.test);
                }
                ExprKind::Assign(a) => {
                    // Parser recovery can fold a missing class-field semicolon into
                    // an assignment like `"A"\n[expr] = "B"`. Source-copy would
                    // preserve the newline, but TypeScript emits this as one line.
                    // Force structured emit when an element-access assignment target
                    // starts on a new line after a string-like literal.
                    if let ExprKind::ElemAccess(ea) = &a.left.kind {
                        if matches!(
                            ea.object.kind,
                            ExprKind::StrLit(_) | ExprKind::NoSubstTemplate(_)
                        ) {
                            let obj_end = ea.object.span.end as usize;
                            let left_end = a.left.span.end as usize;
                            if obj_end < left_end && left_end <= self.source.len() {
                                let left_src = &self.source[obj_end..left_end];
                                if let Some(open_bracket) = left_src.find('[') {
                                    if left_src[..open_bracket].contains('\n') {
                                        return true;
                                    }
                                }
                            }
                        }
                    }
                    stack.push(&a.right);
                }
                ExprKind::Paren(e)
                | ExprKind::Spread(e)
                | ExprKind::Unary(UnaryExpr { argument: e, .. }) => stack.push(e),
                ExprKind::Comma(exprs) => {
                    for inner in exprs.iter().rev() {
                        stack.push(inner);
                    }
                }
                ExprKind::Template(tpl) => {
                    for inner in tpl.exprs.iter().rev() {
                        stack.push(inner);
                    }
                }
                ExprKind::TaggedTemplate(tt) => {
                    for inner in tt.quasi.exprs.iter().rev() {
                        stack.push(inner);
                    }
                    stack.push(&tt.tag);
                }
                ExprKind::JsxElement(el) => {
                    for attr in el.attributes.iter().rev() {
                        match attr {
                            JsxAttribute::Spread(e, _) => stack.push(e),
                            JsxAttribute::Normal { value: Some(v), .. } => stack.push(v),
                            _ => {}
                        }
                    }
                    for child in el.children.iter().rev() {
                        match child {
                            JsxChild::Element(e) => stack.push(e),
                            JsxChild::Expression(Some(e), _) => stack.push(e),
                            _ => {}
                        }
                    }
                }
                ExprKind::JsxSelfClosing(el) => {
                    for attr in el.attributes.iter().rev() {
                        match attr {
                            JsxAttribute::Spread(e, _) => stack.push(e),
                            JsxAttribute::Normal { value: Some(v), .. } => stack.push(v),
                            _ => {}
                        }
                    }
                }
                ExprKind::JsxFragment(frag) => {
                    for child in frag.children.iter().rev() {
                        match child {
                            JsxChild::Element(e) => stack.push(e),
                            JsxChild::Expression(Some(e), _) => stack.push(e),
                            _ => {}
                        }
                    }
                }
                _ => {}
            }
        }
        false
    }

    /// Some parser-recovery tests include inline `// => Error` annotations
    /// inside expressions that TypeScript does not preserve in JS output.
    pub(crate) fn expr_has_error_marker_comment(&self, expr: &Expr) -> bool {
        let start = expr.span.start as usize;
        let end = expr.span.end as usize;
        if start >= end || end > self.source.len() {
            return false;
        }
        self.source[start..end].contains("// => Error")
    }

    /// Detect array/object literals whose source has structural issues:
    /// missing closing delimiters, double commas, or skipped modifiers.
    /// These must go through structured emit to produce correct output.
    pub(crate) fn expr_has_unclosed_delimiter(&self, expr: &Expr) -> bool {
        let mut stack = vec![expr];
        while let Some(expr) = stack.pop() {
            match &expr.kind {
                ExprKind::ArrayLit(elements) => {
                    let s = expr.span.start as usize;
                    let end = expr.span.end as usize;
                    if end > 0 && end <= self.source.len() {
                        if self.source.as_bytes()[end - 1] != b']' {
                            return true;
                        }
                        // Detect double commas in source (e.g. `[a,,b]`)
                        if !elements.is_empty() && s < end && self.source[s..end].contains(",,") {
                            return true;
                        }
                        // Detect unbalanced brackets in arrays containing nested
                        // arrays (e.g. `[1, [["world"]]` has 3 [ and 2 ]).
                        // Only check when nested arrays are present to avoid
                        // false positives from brackets in strings/comments.
                        let has_nested_array = elements
                            .iter()
                            .flatten()
                            .any(|e| matches!(&e.kind, ExprKind::ArrayLit(_)));
                        if has_nested_array && s < end {
                            let bytes = self.source.as_bytes();
                            let mut depth: i32 = 0;
                            let mut i = s;
                            while i < end {
                                match bytes[i] {
                                    b'[' => depth += 1,
                                    b']' => depth -= 1,
                                    b'"' | b'\'' => {
                                        let q = bytes[i];
                                        i += 1;
                                        while i < end && bytes[i] != q {
                                            if bytes[i] == b'\\' {
                                                i += 1;
                                            }
                                            i += 1;
                                        }
                                    }
                                    _ => {}
                                }
                                i += 1;
                            }
                            if depth != 0 {
                                return true;
                            }
                        }
                    }
                    for inner in elements.iter().rev().flatten() {
                        stack.push(inner);
                    }
                }
                ExprKind::ObjectLit(props) => {
                    if !props.is_empty() {
                        let s = expr.span.start as usize;
                        let end = expr.span.end as usize;
                        if end > 0 && end <= self.source.len() {
                            if self.source.as_bytes()[end - 1] != b'}' {
                                return true;
                            }
                            // Detect double commas in source (e.g. `{x:0,,}`)
                            if s < end && self.source[s..end].contains(",,") {
                                return true;
                            }
                        }
                        // Detect modifier-preceded properties: when the parser
                        // skipped illegal modifiers, the property spans start
                        // AFTER the modifier.  Check for non-whitespace gaps
                        // between the `{` / previous comma and each property.
                        let obj_start = expr.span.start as usize + 1; // skip `{`
                        let mut prev_end = obj_start;
                        for p in props.iter() {
                            let ps = obj_lit_prop_span(p).start as usize;
                            if prev_end < ps && ps <= self.source.len() {
                                let gap = strip_comments_from_gap(&self.source[prev_end..ps])
                                    .trim()
                                    .to_string();
                                // After trimming whitespace and comments, the gap
                                // should be empty or just a comma.  Any other
                                // content indicates a skipped modifier keyword.
                                if !gap.is_empty() && gap != "," {
                                    return true;
                                }
                            }
                            prev_end = obj_lit_prop_span(p).end as usize;
                        }
                    }
                    for prop in props.iter().rev() {
                        match prop {
                            ObjLitProp::Property(p) => stack.push(&p.value),
                            ObjLitProp::Spread(e, _) => stack.push(e),
                            _ => {}
                        }
                    }
                }
                ExprKind::Assign(a) => {
                    stack.push(&a.right);
                    stack.push(&a.left);
                }
                ExprKind::Binary(bin) => {
                    stack.push(&bin.right);
                    stack.push(&bin.left);
                }
                ExprKind::Paren(e) => {
                    let end = expr.span.end as usize;
                    if end > 0
                        && end <= self.source.len()
                        && self.source.as_bytes()[end - 1] != b')'
                    {
                        return true;
                    }
                    stack.push(e);
                }
                ExprKind::Spread(e) => stack.push(e),
                ExprKind::Cond(cond) => {
                    stack.push(&cond.alternate);
                    stack.push(&cond.consequent);
                    stack.push(&cond.test);
                }
                ExprKind::Call(call) => {
                    let end = expr.span.end as usize;
                    if end > 0
                        && end <= self.source.len()
                        && self.source.as_bytes()[end - 1] != b')'
                    {
                        return true;
                    }
                    for inner in call.args.iter().rev() {
                        stack.push(inner);
                    }
                    stack.push(&call.callee);
                }
                ExprKind::New(new_expr) => {
                    if let Some(ref args) = new_expr.args {
                        for inner in args.iter().rev() {
                            stack.push(inner);
                        }
                    }
                    stack.push(&new_expr.callee);
                }
                _ => {}
            }
        }
        false
    }

    /// Statement-level check: force structured emit if any sub-expression has
    /// an unclosed array/object literal.
    pub(crate) fn stmt_has_unclosed_delimiter(&self, stmt: &Stmt) -> bool {
        match &stmt.kind {
            StmtKind::Var(vs) => vs.declarations.iter().any(|d| {
                d.init
                    .as_ref()
                    .is_some_and(|e| self.expr_has_unclosed_delimiter(e))
            }),
            StmtKind::Expr(e) => self.expr_has_unclosed_delimiter(e),
            StmtKind::Return(Some(e)) => self.expr_has_unclosed_delimiter(e),
            _ => false,
        }
    }

    /// Check if a compound statement contains nested `var` declarations whose
    /// names appear in the CJS live-export chain.  Used to force structured emit
    /// so that inline `exports.x = x;` can be injected after each var.
    pub(crate) fn stmt_has_nested_exported_var(&self, stmt: &Stmt) -> bool {
        match &stmt.kind {
            StmtKind::Block(stmts) => stmts.iter().any(|s| self.stmt_or_var_is_exported(s)),
            StmtKind::If(if_stmt) => {
                self.stmt_or_var_is_exported(&if_stmt.consequent)
                    || if_stmt
                        .alternate
                        .as_ref()
                        .is_some_and(|alt| self.stmt_or_var_is_exported(alt))
            }
            StmtKind::While(wh) => self.stmt_or_var_is_exported(&wh.body),
            StmtKind::DoWhile(dw) => self.stmt_or_var_is_exported(&dw.body),
            StmtKind::For(f) => {
                self.stmt_or_var_is_exported(&f.body)
                    || matches!(&f.init, Some(ForInit::Var(vs)) if self.var_stmt_has_exported_names(vs))
            }
            StmtKind::ForIn(fi) => {
                self.stmt_or_var_is_exported(&fi.body)
                    || matches!(&fi.left, ForInOfLeft::Var(vs) if self.var_stmt_has_exported_names(vs))
            }
            StmtKind::ForOf(fo) => {
                self.stmt_or_var_is_exported(&fo.body)
                    || matches!(&fo.left, ForInOfLeft::Var(vs) if self.var_stmt_has_exported_names(vs))
            }
            StmtKind::Switch(sw) => sw
                .cases
                .iter()
                .any(|c| c.consequent.iter().any(|s| self.stmt_or_var_is_exported(s))),
            StmtKind::Try(tr) => {
                tr.block.iter().any(|s| self.stmt_or_var_is_exported(s))
                    || tr
                        .handler
                        .as_ref()
                        .is_some_and(|h| h.body.iter().any(|s| self.stmt_or_var_is_exported(s)))
                    || tr
                        .finalizer
                        .as_ref()
                        .is_some_and(|f| f.iter().any(|s| self.stmt_or_var_is_exported(s)))
            }
            StmtKind::With(w) => self.stmt_or_var_is_exported(&w.body),
            StmtKind::Labeled(l) => self.stmt_or_var_is_exported(&l.body),
            _ => false,
        }
    }

    fn stmt_or_var_is_exported(&self, stmt: &Stmt) -> bool {
        match &stmt.kind {
            StmtKind::Var(vs) if vs.kind == VarKind::Var && vs.modifiers & MOD_DECLARE == 0 => {
                self.var_stmt_has_exported_names(vs)
            }
            _ => self.stmt_has_nested_exported_var(stmt),
        }
    }

    fn var_stmt_has_exported_names(&self, vs: &VarStmt) -> bool {
        vs.declarations.iter().any(|d| {
            if let PatKind::Ident(ref name) = d.name.kind {
                self.cjs_live_export_chain.contains_key(name.as_str())
            } else {
                false
            }
        })
    }

    /// Check if a statement contains any BigInt literal that needs normalization
    /// (binary/octal → decimal, separators, hex case).
    #[allow(dead_code)] // bigint normalization
    pub(crate) fn stmt_has_bigint_needing_normalize(stmt: &Stmt) -> bool {
        fn expr_has(e: &Expr) -> bool {
            match &e.kind {
                ExprKind::BigIntLit(n) => Emitter::bigint_needs_normalize(n),
                ExprKind::Paren(inner) => expr_has(inner),
                ExprKind::Unary(u) => expr_has(&u.argument),
                ExprKind::Binary(b) => expr_has(&b.left) || expr_has(&b.right),
                ExprKind::Assign(a) => expr_has(&a.left) || expr_has(&a.right),
                ExprKind::Cond(c) => {
                    expr_has(&c.test) || expr_has(&c.consequent) || expr_has(&c.alternate)
                }
                ExprKind::Comma(exprs) => exprs.iter().any(|e| expr_has(e)),
                ExprKind::Call(c) => expr_has(&c.callee) || c.args.iter().any(|a| expr_has(a)),
                ExprKind::ArrayLit(elts) => {
                    elts.iter().any(|e| e.as_ref().is_some_and(|e| expr_has(e)))
                }
                _ => false,
            }
        }
        match &stmt.kind {
            StmtKind::Var(vs) => vs
                .declarations
                .iter()
                .any(|d| d.init.as_ref().is_some_and(|e| expr_has(e))),
            StmtKind::Expr(e) => expr_has(e),
            StmtKind::Return(Some(e)) => expr_has(e),
            _ => false,
        }
    }

    pub(crate) fn span_has_error_marker_comment(&self, span: Span) -> bool {
        // The marker is a rare test artifact; if it appears nowhere in the file
        // (computed once at construction), no statement span can contain it —
        // skip the per-statement substring scan of the whole span text.
        if !self.source_has_error_marker {
            return false;
        }
        let start = span.start as usize;
        let end = span.end as usize;
        if start >= end || end > self.source.len() {
            return false;
        }
        self.source[start..end].contains("// => Error")
    }

    /// Detect accidental-call formatting where the source contains
    /// `callee()\n(args)` and emit should keep the continued call inline.
    pub(crate) fn expr_has_split_call_continuation(&self, expr: &Expr) -> bool {
        // Cosmetic only: the AST already joined `callee()\n(args)` into one
        // call, and a verbatim copy of that source re-parses identically.
        if self.fast_emit {
            return false;
        }
        if !matches!(
            expr.kind,
            ExprKind::Call(_) | ExprKind::Member(_) | ExprKind::ElemAccess(_)
        ) {
            return false;
        }
        let text = self.copy_span_trimmed(expr.span);
        if !text.contains('\n') {
            return false;
        }
        let bytes = text.as_bytes();
        let mut i = 0usize;
        while i < bytes.len() {
            if bytes[i] == b')' {
                let mut j = i + 1;
                while j < bytes.len()
                    && (bytes[j] == b' ' || bytes[j] == b'\t' || bytes[j] == b'\r')
                {
                    j += 1;
                }
                if j < bytes.len() && bytes[j] == b'\n' {
                    j += 1;
                    while j < bytes.len()
                        && (bytes[j] == b' ' || bytes[j] == b'\t' || bytes[j] == b'\r')
                    {
                        j += 1;
                    }
                    if j < bytes.len() && bytes[j] == b'(' {
                        return true;
                    }
                }
            }
            i += 1;
        }
        false
    }

    pub(crate) fn body_has_missing_semis(&self, stmts: &[Stmt]) -> bool {
        stmts.iter().any(|s| {
            if !stmt_needs_trailing_semicolon(s) {
                return false;
            }
            let end = s.span.end as usize;
            if end == 0 || end > self.source.len() {
                return false;
            }
            // Check if the source text before the statement's span end
            // contains a semicolon (the parser span end may include it).
            let ch_before = self.source.as_bytes().get(end.wrapping_sub(1)).copied();
            if ch_before == Some(b';') {
                return false;
            }
            // Also check trimmed content up to span end
            let text_to_end = &self.source[..end];
            let trimmed = text_to_end.trim_end();
            !trimmed.ends_with(';')
        })
    }
}

/// Strip `// …` line comments and `/* … */` block comments from a gap string,
/// leaving only non-comment text (commas, whitespace, modifier keywords, etc.).
fn strip_comments_from_gap(gap: &str) -> String {
    let mut result = String::with_capacity(gap.len());
    let bytes = gap.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if i + 1 < bytes.len() && bytes[i] == b'/' && bytes[i + 1] == b'/' {
            // Line comment: skip to end of line
            i += 2;
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
        } else if i + 1 < bytes.len() && bytes[i] == b'/' && bytes[i + 1] == b'*' {
            // Block comment: skip to */
            i += 2;
            while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                i += 1;
            }
            if i + 1 < bytes.len() {
                i += 2; // skip */
            }
        } else {
            i = crate::push_utf8_aware(&mut result, gap, i);
        }
    }
    result
}
