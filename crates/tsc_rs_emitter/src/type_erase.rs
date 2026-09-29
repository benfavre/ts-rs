//! Fast-emit type erasure.
//!
//! For a `var`/`let`/`const` statement whose only transform is TypeScript type
//! syntax, we can emit far faster than the structured per-node re-emitter by
//! copying the statement source VERBATIM minus the type byte-ranges (the way
//! oxc/esbuild/swc transpile). This module collects those byte-ranges.
//!
//! The collector is intentionally CONSERVATIVE: every construct it does not yet
//! know how to erase makes it bail (`false`), and the caller falls back to the
//! structured emitter. So a missing case is a missed speedup, never wrong
//! output. Correctness is gated by AST-equivalence validation over the PRISM
//! corpus (scratchpad/oxc-spike/asi_validate.mjs) — the collected output must
//! parse to the same JS AST the structured emitter produces.
//!
//! Coverage (increment 1): variable-statement type annotations + definite `!`,
//! `as`/`satisfies`/non-null `!`, arrow/function parameter + return-type
//! annotations and their bodies, and full nested recursion through ordinary
//! expressions, patterns and statements. BAILS on: type parameters/arguments,
//! `<T>expr` assertions, instantiation expressions, classes, JSX, object
//! methods/accessors, computed keys, and parameter properties/decorators.

use super::*;

impl<'a> Emitter<'a> {
    /// Collect erasable type byte-ranges for a statement whose only transform is
    /// type syntax. Returns the sorted, non-overlapping ranges, or None to fall
    /// back to the structured emitter (the caller must already have confirmed
    /// `can_verbatim_modulo_types`, i.e. no non-type transform applies).
    pub(super) fn try_collect_stmt_erasures(&self, stmt: &Stmt) -> Option<Vec<(u32, u32)>> {
        let mut out: Vec<(u32, u32)> = Vec::new();
        if !self.collect_erasures_stmt(stmt, &mut out) {
            return None;
        }
        // Nothing to erase → this is a no-transform statement already served by
        // the verbatim source-copy path; don't intercept it here.
        if out.is_empty() {
            return None;
        }
        out.sort_unstable();
        for w in out.windows(2) {
            if w[0].1 > w[1].0 {
                return None; // overlapping ranges should never happen — be safe
            }
        }
        Some(out)
    }

    /// Copy `span` from source to output, skipping the (sorted, non-overlapping)
    /// `erase` byte-ranges. The fast-emit type-erasure write path.
    pub(super) fn write_stmt_erasing(&mut self, span: Span, erase: &[(u32, u32)]) {
        // `self.source` is `&'a str` (borrows the source, not `self`), so we can
        // slice it and call `self.write` (`&mut self`) without a borrow clash.
        let src: &'a str = self.source;
        let end = (span.end as usize).min(src.len());
        let mut pos = (span.start as usize).min(end);
        for &(es, ee) in erase {
            let es = (es as usize).clamp(pos, end);
            let ee = (ee as usize).clamp(pos, end);
            if es > pos {
                self.write(&src[pos..es]);
            }
            if ee > pos {
                pos = ee;
            }
        }
        if pos < end {
            self.write(&src[pos..end]);
        } else {
            // The erasure consumed the statement's tail, so any reliance on ASI
            // at the original line end is broken: `const a = b as T` followed by
            // `(x).f()` on the next line would re-parse as `const a = b(x).f()`.
            // A tail erasure only happens on expression-terminated statements
            // (a trailing `;` or `}` in the source would sit AFTER the erased
            // type range and take the `pos < end` branch), so an explicit `;`
            // is always valid here — emit one to pin the statement boundary.
            self.write(";");
        }
        self.newline();
    }

    /// Find a single-character marker (`!` or `?`) immediately after `from`
    /// (skipping whitespace) and return the range covering it. Used for the
    /// definite-assignment `!` and optional-parameter `?` with no annotation.
    fn erase_marker_after(&self, from: u32, marker: u8) -> Option<(u32, u32)> {
        let bytes = self.source.as_bytes();
        let mut i = from as usize;
        while i < bytes.len() {
            let c = bytes[i];
            if c == marker {
                return Some((from, (i + 1) as u32));
            }
            if c.is_ascii_whitespace() {
                i += 1;
            } else {
                return None;
            }
        }
        None
    }

    /// Erase a `: T` annotation whose binding name is not adjacent (return
    /// types): scan back from the type to its introducing `:`.
    fn erase_return_type(&self, ty: &TypeNode) -> Option<(u32, u32)> {
        let bytes = self.source.as_bytes();
        let mut i = ty.span.start as usize;
        while i > 0 {
            i -= 1;
            match bytes[i] {
                b':' => return Some((i as u32, ty.span.end)),
                c if c.is_ascii_whitespace() => continue,
                _ => return None,
            }
        }
        None
    }

    fn collect_erasures_params(&self, params: &[Param], out: &mut Vec<(u32, u32)>) -> bool {
        for (i, p) in params.iter().enumerate() {
            // Parameter properties (access modifiers) and decorators need the
            // structured emitter (they synthesize class fields).
            if p.modifiers != 0 || !p.decorators.is_empty() {
                return false;
            }
            // TypeScript `this` pseudo-parameter (`function f(this: T, ...)`):
            // `this` is NOT a real JS parameter, so the WHOLE parameter must be
            // erased, not just its `: T`. With a following parameter, erase up
            // to its start (consuming the separating comma); as the only param,
            // erase the parameter's own span (`Param.span` covers name through
            // annotation), leaving `()`.
            if matches!(&p.name.kind, PatKind::Ident(n) if n.as_str() == "this") {
                if i + 1 < params.len() {
                    out.push((p.span.start, params[i + 1].span.start));
                } else {
                    out.push((p.span.start, p.span.end));
                }
                continue;
            }
            if let Some(ty) = &p.type_ann {
                if ty.span.end <= p.name.span.end {
                    return false;
                }
                out.push((p.name.span.end, ty.span.end)); // covers `?: T` and `: T`
            } else if p.optional {
                match self.erase_marker_after(p.name.span.end, b'?') {
                    Some(r) => out.push(r),
                    None => return false,
                }
            }
            if !self.collect_erasures_pat(&p.name, out) {
                return false;
            }
            if let Some(init) = &p.initializer {
                if !self.collect_erasures_expr(init, out) {
                    return false;
                }
            }
        }
        true
    }

    fn collect_fn_like(
        &self,
        type_params: &Option<Vec<TypeParam>>,
        params: &[Param],
        return_type: &Option<TypeNode>,
        out: &mut Vec<(u32, u32)>,
    ) -> bool {
        if type_params.is_some() {
            return false; // `<T>` generic params — bail for now
        }
        if !self.collect_erasures_params(params, out) {
            return false;
        }
        if let Some(rt) = return_type {
            match self.erase_return_type(rt) {
                Some(r) => out.push(r),
                None => return false,
            }
        }
        true
    }

    fn collect_erasures_pat(&self, pat: &Pat, out: &mut Vec<(u32, u32)>) -> bool {
        match &pat.kind {
            PatKind::Ident(_) => true,
            PatKind::Rest(inner) => self.collect_erasures_pat(inner, out),
            PatKind::Assign(inner, default) => {
                self.collect_erasures_pat(inner, out) && self.collect_erasures_expr(default, out)
            }
            PatKind::Array(elems) => elems.iter().flatten().all(|e| match e {
                ArrayPatElem::Pat(p) | ArrayPatElem::Rest(p) => self.collect_erasures_pat(p, out),
            }),
            PatKind::Object(props) => props
                .iter()
                .all(|p| self.collect_erasures_obj_pat_prop(p, out)),
        }
    }

    fn collect_erasures_obj_pat_prop(&self, prop: &ObjPatProp, out: &mut Vec<(u32, u32)>) -> bool {
        // Object-pattern properties carry no type syntax themselves; only their
        // (nested) value patterns, computed keys and default expressions can.
        match prop {
            ObjPatProp::KeyValue(key, pat) => {
                if let PropName::Computed(e, _) = key {
                    if !self.collect_erasures_expr(e, out) {
                        return false;
                    }
                }
                self.collect_erasures_pat(pat, out)
            }
            ObjPatProp::Rest(pat) => self.collect_erasures_pat(pat, out),
            ObjPatProp::ShorthandAssign(_, default, _) => self.collect_erasures_expr(default, out),
            ObjPatProp::Shorthand(_, _) => true,
        }
    }

    /// Erase the type syntax of one `var`/`let`/`const` declarator: the `: T`
    /// annotation (or bare definite-assignment `!`), plus recursion into the
    /// binding pattern and initializer. Shared by the variable-statement and
    /// for-init arms so the two can't drift.
    fn collect_erasures_var_declarator(
        &self,
        d: &VarDeclarator,
        out: &mut Vec<(u32, u32)>,
    ) -> bool {
        if let Some(ty) = &d.type_ann {
            if ty.span.end <= d.name.span.end {
                return false;
            }
            out.push((d.name.span.end, ty.span.end)); // covers `!: T` and `: T`
        } else if d.definite {
            match self.erase_marker_after(d.name.span.end, b'!') {
                Some(r) => out.push(r),
                None => return false,
            }
        }
        if !self.collect_erasures_pat(&d.name, out) {
            return false;
        }
        if let Some(init) = &d.init {
            if !self.collect_erasures_expr(init, out) {
                return false;
            }
        }
        true
    }

    fn collect_erasures_stmt(&self, stmt: &Stmt, out: &mut Vec<(u32, u32)>) -> bool {
        match &stmt.kind {
            StmtKind::Var(vs) => {
                if vs.modifiers != 0 {
                    return false;
                }
                vs.declarations
                    .iter()
                    .all(|d| self.collect_erasures_var_declarator(d, out))
            }
            StmtKind::Expr(e) => self.collect_erasures_expr(e, out),
            StmtKind::Return(opt) => opt
                .as_ref()
                .map(|e| self.collect_erasures_expr(e, out))
                .unwrap_or(true),
            StmtKind::Throw(e) => self.collect_erasures_expr(e, out),
            StmtKind::Block(stmts) => stmts.iter().all(|s| self.collect_erasures_stmt(s, out)),
            StmtKind::If(i) => {
                self.collect_erasures_expr(&i.test, out)
                    && self.collect_erasures_stmt(&i.consequent, out)
                    && i.alternate
                        .as_ref()
                        .map(|a| self.collect_erasures_stmt(a, out))
                        .unwrap_or(true)
            }
            StmtKind::While(w) => {
                self.collect_erasures_expr(&w.test, out) && self.collect_erasures_stmt(&w.body, out)
            }
            StmtKind::DoWhile(d) => {
                self.collect_erasures_stmt(&d.body, out) && self.collect_erasures_expr(&d.test, out)
            }
            StmtKind::For(f) => {
                let init_ok = match &f.init {
                    None => true,
                    Some(ForInit::Expr(e)) => self.collect_erasures_expr(e, out),
                    Some(ForInit::Var(vs)) => {
                        if vs.modifiers != 0 {
                            return false;
                        }
                        vs.declarations
                            .iter()
                            .all(|d| self.collect_erasures_var_declarator(d, out))
                    }
                };
                init_ok
                    && f.test
                        .as_ref()
                        .map(|e| self.collect_erasures_expr(e, out))
                        .unwrap_or(true)
                    && f.update
                        .as_ref()
                        .map(|e| self.collect_erasures_expr(e, out))
                        .unwrap_or(true)
                    && self.collect_erasures_stmt(&f.body, out)
            }
            StmtKind::Switch(sw) => {
                self.collect_erasures_expr(&sw.discriminant, out)
                    && sw.cases.iter().all(|c| {
                        c.test
                            .as_ref()
                            .map(|e| self.collect_erasures_expr(e, out))
                            .unwrap_or(true)
                            && c.consequent
                                .iter()
                                .all(|s| self.collect_erasures_stmt(s, out))
                    })
            }
            StmtKind::Try(t) => {
                if !t.block.iter().all(|s| self.collect_erasures_stmt(s, out)) {
                    return false;
                }
                if let Some(h) = &t.handler {
                    // `catch (e: T)` — the catch parameter type must be erased
                    // (JS catch bindings cannot be typed).
                    if let Some(pt) = &h.param_type {
                        match &h.param {
                            Some(p) if pt.span.end > p.span.end => {
                                out.push((p.span.end, pt.span.end));
                            }
                            _ => match self.erase_return_type(pt) {
                                Some(r) => out.push(r),
                                None => return false,
                            },
                        }
                    }
                    if let Some(p) = &h.param {
                        if !self.collect_erasures_pat(p, out) {
                            return false;
                        }
                    }
                    if !h.body.iter().all(|s| self.collect_erasures_stmt(s, out)) {
                        return false;
                    }
                }
                if let Some(f) = &t.finalizer {
                    if !f.iter().all(|s| self.collect_erasures_stmt(s, out)) {
                        return false;
                    }
                }
                true
            }
            StmtKind::FnDecl(f) => {
                // Plain function declaration: erase its signature + body type
                // syntax. Modifiers (export/declare), decorators and overload
                // signatures (no body) → structured.
                if f.modifiers != 0 || !f.decorators.is_empty() {
                    return false;
                }
                let Some(body) = &f.body else {
                    return false;
                };
                if !self.collect_fn_like(&f.type_params, &f.params, &f.return_type, out) {
                    return false;
                }
                body.iter().all(|s| self.collect_erasures_stmt(s, out))
            }
            StmtKind::Break(_) | StmtKind::Continue(_) | StmtKind::Empty | StmtKind::Debugger => {
                true
            }
            // ForIn/ForOf, other nested declarations (class/interface/type/enum/
            // import/export/module), labeled, with, etc. → bail for now.
            _ => false,
        }
    }

    /// True if an identifier reference is rewritten by the structured emitter
    /// (CJS import binding `x`→`(0, mod_1.x)` or exported local `X`→`exports.X`).
    /// Verbatim copy can't reproduce that, so the collector must bail. This check
    /// runs at EVERY identifier the recursion visits, covering nested function
    /// and compound-statement bodies that the statement-level `can_verbatim_*`
    /// guards do not fully recurse into.
    #[inline]
    fn ident_needs_cjs_rewrite(&self, name: &str) -> bool {
        (!self.cjs_import_map.is_empty() && self.cjs_import_map.contains_key(name))
            || (!self.cjs_var_export_names.is_empty() && self.cjs_var_export_names.contains(name))
    }

    /// The recursive workhorse. Returns false to bail (→ structured fallback).
    fn collect_erasures_expr(&self, expr: &Expr, out: &mut Vec<(u32, u32)>) -> bool {
        match &expr.kind {
            ExprKind::Ident(name) => !self.ident_needs_cjs_rewrite(name.as_str()),
            // Leaves — no type syntax, no rewritten reference.
            ExprKind::NumLit(_)
            | ExprKind::BigIntLit(_)
            | ExprKind::StrLit(_)
            | ExprKind::BoolLit(_)
            | ExprKind::NullLit
            | ExprKind::RegexpLit(_)
            | ExprKind::NoSubstTemplate(_)
            | ExprKind::This
            | ExprKind::Super
            | ExprKind::Omitted
            | ExprKind::MetaProp(_) => true,

            // Type-erasing wrappers.
            ExprKind::As(a) => {
                if a.type_node.span.end <= a.expr.span.end {
                    return false;
                }
                out.push((a.expr.span.end, a.type_node.span.end)); // ` as T`
                self.collect_erasures_expr(&a.expr, out)
            }
            ExprKind::Satisfies(s) => {
                if s.type_node.span.end <= s.expr.span.end {
                    return false;
                }
                out.push((s.expr.span.end, s.type_node.span.end)); // ` satisfies T`
                self.collect_erasures_expr(&s.expr, out)
            }
            ExprKind::NonNull(inner) => {
                if expr.span.end <= inner.span.end {
                    return false;
                }
                out.push((inner.span.end, expr.span.end)); // the `!`
                self.collect_erasures_expr(inner, out)
            }

            // `<T>expr`, `expr<T>` — fiddly angle-bracket spans; bail for now.
            ExprKind::TypeAssertion(_) | ExprKind::Instantiation(_) => false,

            // Transparent single-child wrappers.
            ExprKind::Paren(e)
            | ExprKind::Spread(e)
            | ExprKind::Await(e)
            | ExprKind::Delete(e)
            | ExprKind::Typeof(e)
            | ExprKind::Void(e) => self.collect_erasures_expr(e, out),

            ExprKind::Yield(_, opt) => opt
                .as_ref()
                .map(|e| self.collect_erasures_expr(e, out))
                .unwrap_or(true),

            ExprKind::Member(m) => {
                // Const-enum member access (`E.Member`, incl. qualified paths
                // like `Ns.E.Member`) is inlined to its value by the structured
                // emitter — verbatim copy can't, so bail. Must mirror
                // `expr_has_const_enum_ref`: the statement-level guard has no
                // FnDecl arm, so for function bodies this check is the only one.
                if !self.const_enum_values.is_empty() {
                    if let Some(obj) = expr_member_path_for_const_enum(&m.object) {
                        if self
                            .const_enum_values
                            .contains_key(&(obj, m.property.to_string()))
                        {
                            return false;
                        }
                    }
                }
                self.collect_erasures_expr(&m.object, out)
            }
            ExprKind::ElemAccess(ea) => {
                // `E["Member"]` / `E[0]` is also inlined by the structured
                // emitter (same key space as `E.Member`) — bail like the
                // Member arm, mirroring `expr_has_const_enum_ref`.
                if !self.const_enum_values.is_empty() {
                    if let Some(obj) = expr_member_path_for_const_enum(&ea.object) {
                        let member = match &ea.index.kind {
                            ExprKind::StrLit(s) | ExprKind::NoSubstTemplate(s) => {
                                Some(s.to_string())
                            }
                            ExprKind::NumLit(n) => Some(n.to_string()),
                            _ => None,
                        };
                        if let Some(member) = member {
                            if self.const_enum_values.contains_key(&(obj, member)) {
                                return false;
                            }
                        }
                    }
                }
                self.collect_erasures_expr(&ea.object, out)
                    && self.collect_erasures_expr(&ea.index, out)
            }
            ExprKind::Binary(b) => {
                self.collect_erasures_expr(&b.left, out)
                    && self.collect_erasures_expr(&b.right, out)
            }
            ExprKind::Unary(u) => self.collect_erasures_expr(&u.argument, out),
            ExprKind::Update(u) => self.collect_erasures_expr(&u.argument, out),
            ExprKind::Cond(c) => {
                self.collect_erasures_expr(&c.test, out)
                    && self.collect_erasures_expr(&c.consequent, out)
                    && self.collect_erasures_expr(&c.alternate, out)
            }
            ExprKind::Assign(a) => {
                self.collect_erasures_expr(&a.left, out)
                    && self.collect_erasures_expr(&a.right, out)
            }
            ExprKind::Comma(es) => es.iter().all(|e| self.collect_erasures_expr(e, out)),
            ExprKind::ArrayLit(es) => es
                .iter()
                .flatten()
                .all(|e| self.collect_erasures_expr(e, out)),

            ExprKind::Call(c) => {
                if c.type_args.is_some() {
                    return false;
                }
                self.collect_erasures_expr(&c.callee, out)
                    && c.args.iter().all(|a| self.collect_erasures_expr(a, out))
            }
            ExprKind::New(n) => {
                if n.type_args.is_some() {
                    return false;
                }
                self.collect_erasures_expr(&n.callee, out)
                    && n.args
                        .as_ref()
                        .map(|args| args.iter().all(|a| self.collect_erasures_expr(a, out)))
                        .unwrap_or(true)
            }
            ExprKind::Template(t) => t.exprs.iter().all(|e| self.collect_erasures_expr(e, out)),
            ExprKind::TaggedTemplate(tt) => {
                if tt.type_args.is_some() {
                    return false;
                }
                self.collect_erasures_expr(&tt.tag, out)
                    && tt
                        .quasi
                        .exprs
                        .iter()
                        .all(|e| self.collect_erasures_expr(e, out))
            }

            ExprKind::ObjectLit(props) => props.iter().all(|p| match p {
                ObjLitProp::Property(op) => {
                    if op.computed {
                        return false;
                    }
                    self.collect_erasures_expr(&op.value, out)
                }
                ObjLitProp::ShorthandDefault(_, default, _) => {
                    self.collect_erasures_expr(default, out)
                }
                ObjLitProp::Spread(e, _) => self.collect_erasures_expr(e, out),
                ObjLitProp::Shorthand(_, _) => true,
                // Methods/accessors carry their own signatures — bail for now.
                ObjLitProp::Method(_) | ObjLitProp::Get(_) | ObjLitProp::Set(_) => false,
            }),

            ExprKind::Arrow(a) => {
                if !self.collect_fn_like(&a.type_params, &a.params, &a.return_type, out) {
                    return false;
                }
                match &a.body {
                    ArrowBody::Expr(e) => self.collect_erasures_expr(e, out),
                    ArrowBody::Block(stmts) => {
                        stmts.iter().all(|s| self.collect_erasures_stmt(s, out))
                    }
                }
            }
            ExprKind::FnExpr(f) => {
                if !self.collect_fn_like(&f.type_params, &f.params, &f.return_type, out) {
                    return false;
                }
                match &f.body {
                    Some(stmts) => stmts.iter().all(|s| self.collect_erasures_stmt(s, out)),
                    None => false, // overload signature — bail
                }
            }

            // Classes, JSX, anything else → structured emitter.
            _ => false,
        }
    }
}
