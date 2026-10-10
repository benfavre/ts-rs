//! ES5 flattening of destructuring variable declarations, as tsc's
//! `flattenDestructuringBinding` does for `var`/`let`/`const`:
//!
//! ```text
//! var [x, y = 1] = f();     →  var _a = f(), x = _a[0], _b = _a[1], y = _b === void 0 ? 1 : _b;
//! var { a: b } = { a: 1 };  →  var b = { a: 1 }.a;
//! ```
//!
//! A pattern of more (or fewer) than one element evaluates its value once
//! through a temp declarator; an identifier value is reused. Object rest and
//! computed keys are left to the native path.

use tsc_rs_ast::{
    ArrayPatElem, BinaryExpr, BinaryOp, CallExpr, CondExpr, ElemAccessExpr, Expr, ExprKind,
    MemberExpr, ObjPatProp, Pat, PatKind, PropName, ScriptTarget, Span, Stmt, VarDeclarator,
    VarKind, VarStmt, MOD_DECLARE, MOD_EXPORT,
};

use crate::Emitter;

const NO_SPAN: Span = Span { start: 0, end: 0 };

impl<'a> Emitter<'a> {
    /// The flattened form of a destructuring variable statement for ES5, or
    /// `None` when the statement has no pattern or uses a shape this
    /// transform does not own.
    pub(crate) fn es5_flattened_var_stmt(
        &mut self,
        stmt: &Stmt,
        var_stmt: &VarStmt,
    ) -> Option<VarStmt> {
        if self.effective_target() != ScriptTarget::ES5
            || self.file_has_recovery_errors
            || self.is_js_file
            || self.system_hoist_var_in_execute
            || var_stmt.modifiers & (MOD_EXPORT | MOD_DECLARE) != 0
            || matches!(var_stmt.kind, VarKind::Using | VarKind::AwaitUsing)
            || !var_stmt
                .declarations
                .iter()
                .any(|decl| !matches!(decl.name.kind, PatKind::Ident(_)))
        {
            return None;
        }
        let supported = var_stmt.declarations.iter().all(|decl| {
            matches!(decl.name.kind, PatKind::Ident(_))
                || (decl.init.is_some() && decl.type_ann.is_none() && pattern_supported(&decl.name))
        });
        // Comments inside the statement have no owner in the flattened form.
        let has_inner_comment = self
            .comments
            .iter()
            .any(|comment| comment.pos >= stmt.span.start && comment.end <= stmt.span.end);
        if !supported || has_inner_comment {
            return None;
        }

        let mut declarations = Vec::new();
        for decl in &var_stmt.declarations {
            let Some(init) = decl
                .init
                .as_ref()
                .filter(|_| !matches!(decl.name.kind, PatKind::Ident(_)))
            else {
                declarations.push(decl.clone());
                continue;
            };
            let mut value = init.as_ref().clone();
            // A value the pattern itself assigns is read once first.
            if let ExprKind::Ident(name) = &value.kind {
                if pattern_binds(&decl.name, name) {
                    value = self.es5_temp(value, false, &mut declarations);
                }
            }
            self.es5_flatten_element(&decl.name, value, &mut declarations);
        }
        Some(VarStmt {
            kind: var_stmt.kind,
            declarations,
            modifiers: var_stmt.modifiers,
        })
    }

    /// Names every destructured parameter of a list with a temp (`_a`,
    /// `_b`, … in parameter order, per function, unique against the
    /// source), and reserves them so the body's temps start after.
    pub(crate) fn es5_name_parameter_temps(&mut self, params: &[tsc_rs_ast::Param]) {
        const LETTERS: &[u8] = b"abcdefghjklmopqrstuvwxyz";
        let mut index = 0usize;
        for param in params {
            if matches!(param.name.kind, PatKind::Ident(_)) || param.dotdotdot {
                continue;
            }
            let name = loop {
                let name = if index < LETTERS.len() {
                    format!("_{}", LETTERS[index] as char)
                } else {
                    format!("_{}", index - LETTERS.len())
                };
                index += 1;
                if !self.source_has_identifier(&name) {
                    break name;
                }
            };
            self.es5_param_temp_names.insert(param.span.start, name);
        }
        self.fn_param_rest_temp_count = self.fn_param_rest_temp_count.max(index);
    }

    /// `var <flattened pattern> ;` for a destructured parameter whose value
    /// is `temp` (with its default, if any).
    pub(crate) fn es5_flattened_parameter(
        &mut self,
        param: &tsc_rs_ast::Param,
        temp: &str,
    ) -> VarStmt {
        let target = match &param.initializer {
            Some(default) => Pat {
                kind: PatKind::Assign(Box::new(param.name.clone()), default.clone()),
                span: param.span,
            },
            None => param.name.clone(),
        };
        let mut declarations = Vec::new();
        let value = Expr {
            kind: ExprKind::Ident(temp.into()),
            span: NO_SPAN,
        };
        self.es5_flatten_element(&target, value, &mut declarations);
        VarStmt {
            kind: VarKind::Var,
            declarations,
            modifiers: 0,
        }
    }

    /// tsc's flattenBindingOrAssignmentElement for a declaration target.
    fn es5_flatten_element(&mut self, target: &Pat, value: Expr, out: &mut Vec<VarDeclarator>) {
        match &target.kind {
            PatKind::Ident(_) => out.push(declarator(target.clone(), value)),
            PatKind::Assign(inner, default) => {
                let checked = self.es5_temp(value, true, out);
                let mut value = Expr {
                    kind: ExprKind::Cond(CondExpr {
                        test: Box::new(Expr {
                            kind: ExprKind::Binary(BinaryExpr {
                                left: Box::new(checked.clone()),
                                op: BinaryOp::StrictEq,
                                right: Box::new(void_zero()),
                            }),
                            span: NO_SPAN,
                        }),
                        consequent: default.clone(),
                        alternate: Box::new(checked),
                    }),
                    span: NO_SPAN,
                };
                // A pattern reads its value more than once: evaluate a
                // default that is not a simple literal before it.
                if !matches!(inner.kind, PatKind::Ident(_)) && !simple_inlineable(default) {
                    value = self.es5_temp(value, true, out);
                }
                self.es5_flatten_element(inner, value, out);
            }
            PatKind::Array(elements) => {
                let mut value = value;
                if elements.len() != 1 || elements.iter().all(Option::is_none) {
                    value = self.es5_temp(value, !elements.is_empty(), out);
                }
                for (index, element) in elements.iter().enumerate() {
                    match element {
                        None => {}
                        Some(ArrayPatElem::Pat(pat)) => {
                            let access = element_access(&value, index);
                            self.es5_flatten_element(pat, access, out);
                        }
                        Some(ArrayPatElem::Rest(pat)) => {
                            let slice = slice_call(&value, index);
                            self.es5_flatten_element(pat, slice, out);
                        }
                    }
                }
            }
            PatKind::Object(props) => {
                let mut value = value;
                if props.len() != 1 {
                    value = self.es5_temp(value, !props.is_empty(), out);
                }
                for prop in props {
                    match prop {
                        ObjPatProp::KeyValue(key, pat) => {
                            let access = property_access(&value, key);
                            self.es5_flatten_element(pat, access, out);
                        }
                        ObjPatProp::Shorthand(name, span) => {
                            let target = Pat {
                                kind: PatKind::Ident(name.clone()),
                                span: *span,
                            };
                            let access = member(&value, name);
                            out.push(declarator(target, access));
                        }
                        ObjPatProp::ShorthandAssign(name, default, span) => {
                            let target = Pat {
                                kind: PatKind::Assign(
                                    Box::new(Pat {
                                        kind: PatKind::Ident(name.clone()),
                                        span: Span::new(span.start, span.start + name.len() as u32),
                                    }),
                                    default.clone(),
                                ),
                                span: *span,
                            };
                            let access = member(&value, name);
                            self.es5_flatten_element(&target, access, out);
                        }
                        ObjPatProp::Rest(_) => unreachable!("gated by pattern_supported"),
                    }
                }
            }
            PatKind::Rest(inner) => self.es5_flatten_element(inner, value, out),
        }
    }

    /// tsc's ensureIdentifier: an identifier value is reused when allowed;
    /// anything else is stored in a fresh temp declarator.
    fn es5_temp(
        &mut self,
        value: Expr,
        reuse_identifier: bool,
        out: &mut Vec<VarDeclarator>,
    ) -> Expr {
        if reuse_identifier && matches!(value.kind, ExprKind::Ident(_)) {
            return value;
        }
        let name = self.make_temp_name();
        out.push(declarator(
            Pat {
                kind: PatKind::Ident(name.clone().into()),
                span: NO_SPAN,
            },
            value,
        ));
        Expr {
            kind: ExprKind::Ident(name.into()),
            span: NO_SPAN,
        }
    }
}

fn declarator(name: Pat, init: Expr) -> VarDeclarator {
    VarDeclarator {
        name,
        type_ann: None,
        init: Some(Box::new(init)),
        full_start: 0,
        binding_name_full_starts: Vec::new(),
        span: NO_SPAN,
        definite: false,
    }
}

fn void_zero() -> Expr {
    Expr {
        kind: ExprKind::Void(Box::new(Expr {
            kind: ExprKind::NumLit("0".into()),
            span: NO_SPAN,
        })),
        span: NO_SPAN,
    }
}

/// The receiver of an element/property access: a conditional (a default
/// check) needs parentheses.
fn receiver(value: &Expr) -> Box<Expr> {
    Box::new(if matches!(value.kind, ExprKind::Cond(_)) {
        Expr {
            kind: ExprKind::Paren(Box::new(value.clone())),
            span: NO_SPAN,
        }
    } else {
        value.clone()
    })
}

fn element_access(value: &Expr, index: usize) -> Expr {
    Expr {
        kind: ExprKind::ElemAccess(ElemAccessExpr {
            object: receiver(value),
            index: Box::new(Expr {
                kind: ExprKind::NumLit(index.to_string().into()),
                span: NO_SPAN,
            }),
            optional: false,
        }),
        span: NO_SPAN,
    }
}

fn member(value: &Expr, name: &str) -> Expr {
    Expr {
        kind: ExprKind::Member(Box::new(MemberExpr {
            object: receiver(value),
            property: name.into(),
            optional: false,
        })),
        span: NO_SPAN,
    }
}

fn property_access(value: &Expr, key: &PropName) -> Expr {
    match key {
        PropName::Ident(name, _) => member(value, name),
        PropName::String(text, span) => Expr {
            kind: ExprKind::ElemAccess(ElemAccessExpr {
                object: receiver(value),
                index: Box::new(Expr {
                    kind: ExprKind::StrLit(text.clone()),
                    span: *span,
                }),
                optional: false,
            }),
            span: NO_SPAN,
        },
        PropName::Number(text, span) => Expr {
            kind: ExprKind::ElemAccess(ElemAccessExpr {
                object: receiver(value),
                index: Box::new(Expr {
                    kind: ExprKind::NumLit(text.clone()),
                    span: *span,
                }),
                optional: false,
            }),
            span: NO_SPAN,
        },
        PropName::Computed(..) | PropName::Private(..) => {
            unreachable!("gated by pattern_supported")
        }
    }
}

fn slice_call(value: &Expr, index: usize) -> Expr {
    Expr {
        kind: ExprKind::Call(Box::new(CallExpr {
            callee: Box::new(member(value, "slice")),
            type_args: None,
            args: vec![Box::new(Expr {
                kind: ExprKind::NumLit(index.to_string().into()),
                span: NO_SPAN,
            })],
            optional: false,
        })),
        span: NO_SPAN,
    }
}

/// tsc's isSimpleInlineableExpression: a literal or keyword, not an
/// identifier.
fn simple_inlineable(expr: &Expr) -> bool {
    matches!(
        expr.kind,
        ExprKind::NumLit(_)
            | ExprKind::StrLit(_)
            | ExprKind::NoSubstTemplate(_)
            | ExprKind::BoolLit(_)
            | ExprKind::NullLit
            | ExprKind::This
    )
}

/// Patterns this transform owns: identifier leaves, nested array/object
/// patterns, defaults, a final array rest; no object rest or computed keys.
pub(crate) fn pattern_supported(pat: &Pat) -> bool {
    match &pat.kind {
        PatKind::Ident(name) => name != "<error>" && !name.is_empty(),
        PatKind::Assign(inner, _) => pattern_supported(inner),
        PatKind::Array(elements) => {
            elements
                .iter()
                .enumerate()
                .all(|(index, element)| match element {
                    None => true,
                    Some(ArrayPatElem::Pat(pat)) => pattern_supported(pat),
                    Some(ArrayPatElem::Rest(pat)) => {
                        index + 1 == elements.len() && pattern_supported(pat)
                    }
                })
        }
        PatKind::Object(props) => props.iter().all(|prop| match prop {
            ObjPatProp::KeyValue(
                PropName::Ident(..) | PropName::String(..) | PropName::Number(..),
                pat,
            ) => pattern_supported(pat),
            ObjPatProp::Shorthand(..) | ObjPatProp::ShorthandAssign(..) => true,
            ObjPatProp::KeyValue(..) | ObjPatProp::Rest(_) => false,
        }),
        PatKind::Rest(_) => false,
    }
}

/// Whether `pat` declares `name`.
fn pattern_binds(pat: &Pat, name: &str) -> bool {
    match &pat.kind {
        PatKind::Ident(bound) => bound == name,
        PatKind::Assign(inner, _) | PatKind::Rest(inner) => pattern_binds(inner, name),
        PatKind::Array(elements) => elements.iter().flatten().any(|element| match element {
            ArrayPatElem::Pat(pat) | ArrayPatElem::Rest(pat) => pattern_binds(pat, name),
        }),
        PatKind::Object(props) => props.iter().any(|prop| match prop {
            ObjPatProp::KeyValue(_, pat) | ObjPatProp::Rest(pat) => pattern_binds(pat, name),
            ObjPatProp::Shorthand(bound, _) | ObjPatProp::ShorthandAssign(bound, _, _) => {
                bound == name
            }
        }),
    }
}

impl<'a> Emitter<'a> {
    /// In a lowered ES5 class member, `super.m(…)` calls the base member
    /// with this receiver (`_super.prototype.m.call(this, …)`) and `super.x`
    /// reads it from the base (`_super.prototype.x`). Returns whether
    /// `expr` was such an access.
    pub(crate) fn emit_es5_super_access(&mut self, expr: &Expr) -> bool {
        let Some(home) = self.es5_super_home.clone() else {
            return false;
        };
        let is_super = |object: &Expr| matches!(object.kind, ExprKind::Super);
        match &expr.kind {
            ExprKind::Call(call) => {
                let callee = &call.callee;
                let receiver = self
                    .lexical_arrow_this_alias
                    .clone()
                    .unwrap_or_else(|| "this".to_string());
                // `super.m?.()`: the base member is read once into a temp.
                if call.optional
                    && matches!(&callee.kind,
                        ExprKind::Member(m) if is_super(&m.object))
                        | matches!(&callee.kind,
                        ExprKind::ElemAccess(a) if is_super(&a.object))
                {
                    let temp = self.next_temp_var();
                    self.write("(");
                    self.write(&temp);
                    self.write(" = ");
                    self.write(&home);
                    match &callee.kind {
                        ExprKind::Member(member) => {
                            self.write(".");
                            self.write(&member.property);
                        }
                        ExprKind::ElemAccess(access) => {
                            self.write("[");
                            self.emit_expr(&access.index);
                            self.write("]");
                        }
                        _ => unreachable!(),
                    }
                    self.write(") === null || ");
                    self.write(&temp);
                    self.write(" === void 0 ? void 0 : ");
                    self.write(&temp);
                    self.write(".call(");
                    self.write(&receiver);
                    for arg in &call.args {
                        self.write(", ");
                        self.emit_expr(arg);
                    }
                    self.write(")");
                    return true;
                }
                let target_written = match &callee.kind {
                    ExprKind::Member(member) if is_super(&member.object) => {
                        self.write(&home);
                        self.write(".");
                        self.write(&member.property);
                        true
                    }
                    ExprKind::ElemAccess(access) if is_super(&access.object) => {
                        self.write(&home);
                        self.write("[");
                        self.emit_expr(&access.index);
                        self.write("]");
                        true
                    }
                    _ => false,
                };
                if !target_written {
                    return false;
                }
                self.write(".call(");
                self.write(&receiver);
                for arg in &call.args {
                    self.write(", ");
                    self.emit_expr(arg);
                }
                self.write(")");
                true
            }
            ExprKind::Member(member) if is_super(&member.object) => {
                self.write(&home);
                self.write(".");
                self.write(&member.property);
                true
            }
            ExprKind::ElemAccess(access) if is_super(&access.object) => {
                self.write(&home);
                self.write("[");
                self.emit_expr(&access.index);
                self.write("]");
                true
            }
            _ => false,
        }
    }
}

/// The `this` environment of a function body being emitted (ES5 capture).
pub(crate) struct ThisCaptureScope {
    /// `_this` when arrows in this body read `this` through a capture.
    pub(crate) alias: Option<String>,
    saved_alias: Option<String>,
    saved_lexical: Option<String>,
}

/// `var _this = this;`
pub(crate) fn this_capture_stmt(alias: &str) -> Stmt {
    Stmt {
        kind: tsc_rs_ast::StmtKind::Var(Box::new(VarStmt {
            kind: VarKind::Var,
            declarations: vec![declarator(
                Pat {
                    kind: PatKind::Ident(alias.into()),
                    span: NO_SPAN,
                },
                Expr {
                    kind: ExprKind::This,
                    span: NO_SPAN,
                },
            )],
            modifiers: 0,
        })),
        span: NO_SPAN,
    }
}

impl<'a> Emitter<'a> {
    /// Entering a function body. A lowered arrow's body shares the enclosing
    /// capture (`None`); any other body starts its own `this` environment,
    /// capturing it as `_this` when an ES5 arrow inside reads `this`.
    pub(crate) fn enter_this_capture_scope(&mut self, enclosing: Span) -> Option<ThisCaptureScope> {
        if std::mem::take(&mut self.arrow_body_pending) {
            return None;
        }
        let alias = (self.effective_target() == ScriptTarget::ES5
            && !self.is_js_file
            && !self.file_has_recovery_errors
            && self.body_needs_this_capture(enclosing))
        .then(|| self.lexical_loop_this_alias());
        let saved_alias = std::mem::replace(&mut self.this_capture_alias, alias.clone());
        let saved_lexical = self.lexical_arrow_this_alias.take();
        // A nested function's own `this` is not rewritten.
        crate::source_transform::ES5_THIS_REWRITE.with(|flag| flag.set(false));
        Some(ThisCaptureScope {
            alias,
            saved_alias,
            saved_lexical,
        })
    }

    pub(crate) fn leave_this_capture_scope(&mut self, scope: ThisCaptureScope) {
        self.this_capture_alias = scope.saved_alias;
        self.lexical_arrow_this_alias = scope.saved_lexical;
    }

    /// Whether an arrow that reads `this` sits in the body directly (not in
    /// a nested function or class).
    fn body_needs_this_capture(&self, enclosing: Span) -> bool {
        self.this_arrow_spans.iter().any(|arrow| {
            enclosing.start <= arrow.start
                && arrow.end <= enclosing.end
                && *arrow != enclosing
                && !self.this_boundary_spans.iter().any(|boundary| {
                    boundary.start > enclosing.start
                        && boundary.start <= arrow.start
                        && arrow.end <= boundary.end
                })
        })
    }
}
