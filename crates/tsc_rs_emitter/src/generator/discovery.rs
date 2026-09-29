//! Find supported async bodies through expression and statement containers.
use super::*;

impl Emitter<'_> {
    fn es5_simple_async_params(params: &[Param]) -> bool {
        params.iter().all(|param| {
            param.initializer.is_none()
                && !param.dotdotdot
                && matches!(param.name.kind, PatKind::Ident(_))
        })
    }

    pub(crate) fn plan_es5_async_arrow(&self, arrow: &ArrowFn) -> Option<GeneratorPlan> {
        if !arrow.is_async || !Self::es5_simple_async_params(&arrow.params) {
            return None;
        }
        match &arrow.body {
            ArrowBody::Block(body) => self.plan_es5_async_body(body),
            ArrowBody::Expr(value) => self.plan_es5_async_body(&[Stmt {
                kind: StmtKind::Return(Some(value.clone())),
                span: value.span,
            }]),
        }
    }

    fn function_needs_es5_generator(
        &self,
        is_async: bool,
        is_generator: bool,
        params: &[Param],
        body: Option<&[Stmt]>,
    ) -> bool {
        is_async
            && !is_generator
            && Self::es5_simple_async_params(params)
            && body.is_some_and(|body| self.plan_es5_async_body(body).is_some())
            || params.iter().any(|p| {
                self.pat_needs_es5_generator(&p.name)
                    || p.initializer
                        .as_deref()
                        .is_some_and(|e| self.expr_needs_es5_generator(e))
            })
            || body.is_some_and(|body| self.stmts_need_es5_generator(body))
    }

    fn stmts_need_es5_generator(&self, body: &[Stmt]) -> bool {
        body.iter().any(|stmt| self.stmt_needs_es5_generator(stmt))
    }

    fn vars_need_es5_generator(&self, vars: &VarStmt) -> bool {
        vars.declarations.iter().any(|decl| {
            self.pat_needs_es5_generator(&decl.name)
                || decl
                    .init
                    .as_deref()
                    .is_some_and(|e| self.expr_needs_es5_generator(e))
        })
    }

    fn pat_needs_es5_generator(&self, pat: &Pat) -> bool {
        match &pat.kind {
            PatKind::Assign(pat, init) => {
                self.pat_needs_es5_generator(pat) || self.expr_needs_es5_generator(init)
            }
            PatKind::Rest(pat) => self.pat_needs_es5_generator(pat),
            PatKind::Array(elements) => elements.iter().flatten().any(|elem| match elem {
                ArrayPatElem::Pat(pat) | ArrayPatElem::Rest(pat) => {
                    self.pat_needs_es5_generator(pat)
                }
            }),
            PatKind::Object(props) => props.iter().any(|prop| match prop {
                ObjPatProp::KeyValue(key, pat) => {
                    self.prop_needs_es5_generator(key) || self.pat_needs_es5_generator(pat)
                }
                ObjPatProp::Rest(pat) => self.pat_needs_es5_generator(pat),
                ObjPatProp::ShorthandAssign(_, init, _) => self.expr_needs_es5_generator(init),
                _ => false,
            }),
            _ => false,
        }
    }

    fn prop_needs_es5_generator(&self, key: &PropName) -> bool {
        matches!(key, PropName::Computed(expr, _) if self.expr_needs_es5_generator(expr))
    }

    fn class_needs_es5_generator(&self, class: &ClassDecl) -> bool {
        class
            .extends
            .as_deref()
            .is_some_and(|e| self.expr_needs_es5_generator(e))
            || class
                .decorators
                .iter()
                .any(|e| self.expr_needs_es5_generator(e))
            || class.members.iter().any(|member| match &member.kind {
                ClassMemberKind::Method(method) => {
                    self.prop_needs_es5_generator(&method.name)
                        || self.function_needs_es5_generator(
                            method.is_async,
                            method.is_generator,
                            &method.params,
                            method.body.as_deref(),
                        )
                }
                ClassMemberKind::Constructor(ctor) => self.function_needs_es5_generator(
                    false,
                    false,
                    &ctor.params,
                    ctor.body.as_deref(),
                ),
                ClassMemberKind::GetAccessor(accessor) | ClassMemberKind::SetAccessor(accessor) => {
                    self.prop_needs_es5_generator(&accessor.name)
                        || self.function_needs_es5_generator(
                            false,
                            false,
                            &accessor.params,
                            accessor.body.as_deref(),
                        )
                }
                ClassMemberKind::Property(prop) => {
                    self.prop_needs_es5_generator(&prop.name)
                        || prop
                            .initializer
                            .as_deref()
                            .is_some_and(|e| self.expr_needs_es5_generator(e))
                }
                ClassMemberKind::StaticBlock(body) => self.stmts_need_es5_generator(body),
                _ => false,
            })
    }

    fn module_needs_es5_generator(&self, module: &ModuleDecl) -> bool {
        match &module.body {
            Some(ModuleBody::Block(body)) => self.stmts_need_es5_generator(body),
            Some(ModuleBody::Module(module)) => self.module_needs_es5_generator(module),
            None => false,
        }
    }

    pub(super) fn stmt_needs_es5_generator(&self, stmt: &Stmt) -> bool {
        if stmt_is_erased(stmt, self.preserve_const_enums_effective()) {
            return false;
        }
        match &stmt.kind {
            StmtKind::FnDecl(f) => self.function_needs_es5_generator(
                f.is_async,
                f.is_generator,
                &f.params,
                f.body.as_deref(),
            ),
            StmtKind::ClassDecl(class) => self.class_needs_es5_generator(class),
            StmtKind::Expr(e) | StmtKind::Throw(e) | StmtKind::ExportAssign(e) => {
                self.expr_needs_es5_generator(e)
            }
            StmtKind::Return(e) => e
                .as_deref()
                .is_some_and(|e| self.expr_needs_es5_generator(e)),
            StmtKind::Var(vars) => self.vars_need_es5_generator(vars),
            StmtKind::Export(export) => match &export.kind {
                ExportDeclKind::Decl(inner) | ExportDeclKind::DefaultDecl(inner) => {
                    self.stmt_needs_es5_generator(inner)
                }
                ExportDeclKind::Default(e) => self.expr_needs_es5_generator(e),
                _ => false,
            },
            StmtKind::Block(body) => self.stmts_need_es5_generator(body),
            StmtKind::If(branch) => {
                self.expr_needs_es5_generator(&branch.test)
                    || self.stmt_needs_es5_generator(&branch.consequent)
                    || branch
                        .alternate
                        .as_deref()
                        .is_some_and(|s| self.stmt_needs_es5_generator(s))
            }
            StmtKind::For(loop_) => {
                loop_.init.as_ref().is_some_and(|init| match init {
                    ForInit::Expr(e) => self.expr_needs_es5_generator(e),
                    ForInit::Var(vars) => self.vars_need_es5_generator(vars),
                }) || loop_
                    .test
                    .as_deref()
                    .is_some_and(|e| self.expr_needs_es5_generator(e))
                    || loop_
                        .update
                        .as_deref()
                        .is_some_and(|e| self.expr_needs_es5_generator(e))
                    || self.stmt_needs_es5_generator(&loop_.body)
            }
            StmtKind::While(loop_) => {
                self.expr_needs_es5_generator(&loop_.test)
                    || self.stmt_needs_es5_generator(&loop_.body)
            }
            StmtKind::DoWhile(loop_) => {
                self.expr_needs_es5_generator(&loop_.test)
                    || self.stmt_needs_es5_generator(&loop_.body)
            }
            StmtKind::ForIn(loop_) => {
                self.expr_needs_es5_generator(&loop_.right)
                    || self.stmt_needs_es5_generator(&loop_.body)
            }
            StmtKind::ForOf(loop_) => {
                self.expr_needs_es5_generator(&loop_.right)
                    || self.stmt_needs_es5_generator(&loop_.body)
            }
            StmtKind::Labeled(label) => self.stmt_needs_es5_generator(&label.body),
            StmtKind::With(with) => {
                self.expr_needs_es5_generator(&with.object)
                    || self.stmt_needs_es5_generator(&with.body)
            }
            StmtKind::Try(try_) => {
                self.stmts_need_es5_generator(&try_.block)
                    || try_
                        .handler
                        .as_ref()
                        .is_some_and(|h| self.stmts_need_es5_generator(&h.body))
                    || try_
                        .finalizer
                        .as_deref()
                        .is_some_and(|body| self.stmts_need_es5_generator(body))
            }
            StmtKind::Switch(switch) => {
                self.expr_needs_es5_generator(&switch.discriminant)
                    || switch.cases.iter().any(|case| {
                        case.test
                            .as_deref()
                            .is_some_and(|e| self.expr_needs_es5_generator(e))
                            || self.stmts_need_es5_generator(&case.consequent)
                    })
            }
            StmtKind::ModuleDecl(module) => self.module_needs_es5_generator(module),
            StmtKind::EnumDecl(enum_) => enum_.members.iter().any(|m| {
                m.initializer
                    .as_deref()
                    .is_some_and(|e| self.expr_needs_es5_generator(e))
            }),
            _ => false,
        }
    }

    fn jsx_children_need_es5_generator(&self, children: &[JsxChild]) -> bool {
        children.iter().any(|child| match child {
            JsxChild::Element(e) => self.expr_needs_es5_generator(e),
            JsxChild::Expression(e, _) => e
                .as_deref()
                .is_some_and(|e| self.expr_needs_es5_generator(e)),
            JsxChild::Fragment(fragment) => {
                self.jsx_children_need_es5_generator(&fragment.children)
            }
            _ => false,
        })
    }

    fn jsx_attributes_need_es5_generator(&self, attrs: &[JsxAttribute]) -> bool {
        attrs.iter().any(|attr| match attr {
            JsxAttribute::Normal { value, .. } => value
                .as_deref()
                .is_some_and(|e| self.expr_needs_es5_generator(e)),
            JsxAttribute::Spread(e, _) => self.expr_needs_es5_generator(e),
        })
    }

    fn expr_needs_es5_generator(&self, expr: &Expr) -> bool {
        if let Some(inner) = expr.kind.type_layer_inner() {
            return self.expr_needs_es5_generator(inner);
        }
        match &expr.kind {
            ExprKind::FnExpr(f) => self.function_needs_es5_generator(
                f.is_async,
                f.is_generator,
                &f.params,
                f.body.as_deref(),
            ),
            ExprKind::Arrow(arrow) => {
                self.plan_es5_async_arrow(arrow).is_some()
                    || arrow.params.iter().any(|p| {
                        self.pat_needs_es5_generator(&p.name)
                            || p.initializer
                                .as_deref()
                                .is_some_and(|e| self.expr_needs_es5_generator(e))
                    })
                    || match &arrow.body {
                        ArrowBody::Expr(e) => self.expr_needs_es5_generator(e),
                        ArrowBody::Block(body) => self.stmts_need_es5_generator(body),
                    }
            }
            ExprKind::ClassExpr(class) => self.class_needs_es5_generator(class),
            ExprKind::ArrayLit(values) => values
                .iter()
                .flatten()
                .any(|e| self.expr_needs_es5_generator(e)),
            ExprKind::ObjectLit(props) => props.iter().any(|prop| match prop {
                ObjLitProp::Property(prop) => {
                    self.prop_needs_es5_generator(&prop.key)
                        || self.expr_needs_es5_generator(&prop.value)
                }
                ObjLitProp::ShorthandDefault(_, e, _) | ObjLitProp::Spread(e, _) => {
                    self.expr_needs_es5_generator(e)
                }
                ObjLitProp::Method(m) => {
                    self.prop_needs_es5_generator(&m.name)
                        || self.function_needs_es5_generator(
                            m.is_async,
                            m.is_generator,
                            &m.params,
                            Some(&m.body),
                        )
                }
                ObjLitProp::Get(a) | ObjLitProp::Set(a) => {
                    self.prop_needs_es5_generator(&a.name)
                        || self.function_needs_es5_generator(false, false, &a.params, Some(&a.body))
                }
                _ => false,
            }),
            ExprKind::Call(call) => {
                self.expr_needs_es5_generator(&call.callee)
                    || call.args.iter().any(|e| self.expr_needs_es5_generator(e))
            }
            ExprKind::New(new) => {
                self.expr_needs_es5_generator(&new.callee)
                    || new
                        .args
                        .as_ref()
                        .is_some_and(|args| args.iter().any(|e| self.expr_needs_es5_generator(e)))
            }
            ExprKind::Member(member) => self.expr_needs_es5_generator(&member.object),
            ExprKind::ElemAccess(access) => {
                self.expr_needs_es5_generator(&access.object)
                    || self.expr_needs_es5_generator(&access.index)
            }
            ExprKind::Binary(binary) => {
                self.expr_needs_es5_generator(&binary.left)
                    || self.expr_needs_es5_generator(&binary.right)
            }
            ExprKind::Assign(assign) => {
                self.expr_needs_es5_generator(&assign.left)
                    || self.expr_needs_es5_generator(&assign.right)
            }
            ExprKind::Cond(cond) => {
                self.expr_needs_es5_generator(&cond.test)
                    || self.expr_needs_es5_generator(&cond.consequent)
                    || self.expr_needs_es5_generator(&cond.alternate)
            }
            ExprKind::Unary(unary) => self.expr_needs_es5_generator(&unary.argument),
            ExprKind::Update(update) => self.expr_needs_es5_generator(&update.argument),
            ExprKind::Paren(e)
            | ExprKind::Spread(e)
            | ExprKind::Await(e)
            | ExprKind::Delete(e)
            | ExprKind::Typeof(e)
            | ExprKind::Void(e) => self.expr_needs_es5_generator(e),
            ExprKind::Yield(_, e) => e
                .as_deref()
                .is_some_and(|e| self.expr_needs_es5_generator(e)),
            ExprKind::Comma(values) => values.iter().any(|e| self.expr_needs_es5_generator(e)),
            ExprKind::Template(template) => template
                .exprs
                .iter()
                .any(|e| self.expr_needs_es5_generator(e)),
            ExprKind::TaggedTemplate(template) => {
                self.expr_needs_es5_generator(&template.tag)
                    || template
                        .quasi
                        .exprs
                        .iter()
                        .any(|e| self.expr_needs_es5_generator(e))
            }
            ExprKind::JsxElement(element) => {
                self.expr_needs_es5_generator(&element.name)
                    || self.jsx_attributes_need_es5_generator(&element.attributes)
                    || self.jsx_children_need_es5_generator(&element.children)
            }
            ExprKind::JsxSelfClosing(element) => {
                self.expr_needs_es5_generator(&element.name)
                    || self.jsx_attributes_need_es5_generator(&element.attributes)
            }
            ExprKind::JsxFragment(fragment) => {
                self.jsx_children_need_es5_generator(&fragment.children)
            }
            _ => false,
        }
    }
}
