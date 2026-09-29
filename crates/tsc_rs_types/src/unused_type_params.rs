//! TS6133 / TS6205 for unused type parameters (tsc's
//! `checkUnusedTypeParameters`), reported under `noUnusedParameters`.
//!
//! A type parameter is used when any type reference inside the declaring
//! construct names it: its sibling constraints/defaults, parameter and
//! return annotations, the body (annotations, assertions, explicit type
//! arguments, nested signatures), class/interface members, or an alias's
//! right-hand side. Nested declarations that re-declare the same name are
//! not modelled as shadowing (a reference inside still counts, which can
//! only suppress a report). Only the last declaration of a merged symbol
//! (function overload sets, interface/class merges) is checked.

use crate::diagnostics::error_unused_local;
use crate::TypeChecker;
use tsc_rs_ast::*;

fn error_all_type_parameters_unused(span: Span) -> Diagnostic {
    Diagnostic {
        code: 6205,
        message: "All type parameters are unused.".to_string(),
        category: DiagnosticCategory::Error,
        file_name: None,
        span: Some(span),
        related: None,
    }
}

// ---------------------------------------------------------------------------
// Reference search
// ---------------------------------------------------------------------------

fn name_expr_refs(name: &str, expr: &Expr) -> bool {
    match &expr.kind {
        ExprKind::Ident(n) => n == name,
        ExprKind::Member(m) => name_expr_refs(name, &m.object),
        _ => false,
    }
}

fn tn_refs(name: &str, node: &TypeNode) -> bool {
    match &node.kind {
        TypeNodeKind::Reference(r) => {
            name_expr_refs(name, &r.name)
                || r.type_args
                    .as_deref()
                    .is_some_and(|args| args.iter().any(|a| tn_refs(name, a)))
        }
        TypeNodeKind::Array(inner)
        | TypeNodeKind::Keyof(inner)
        | TypeNodeKind::Unique(inner)
        | TypeNodeKind::Readonly(inner)
        | TypeNodeKind::Paren(inner)
        | TypeNodeKind::Rest(inner)
        | TypeNodeKind::Optional(inner)
        | TypeNodeKind::TypeOperator(_, inner)
        | TypeNodeKind::JSDocNullable(Some(inner)) => tn_refs(name, inner),
        TypeNodeKind::Tuple(elements) => elements.iter().any(|e| tn_refs(name, &e.type_node)),
        TypeNodeKind::NamedTupleMember(member) => tn_refs(name, &member.type_node),
        TypeNodeKind::Union(nodes) | TypeNodeKind::Intersection(nodes) => {
            nodes.iter().any(|n| tn_refs(name, n))
        }
        TypeNodeKind::Function(f) | TypeNodeKind::Constructor(f) => {
            tps_refs(name, f.type_params.as_deref())
                || params_refs(name, &f.params)
                || tn_refs(name, &f.return_type)
        }
        TypeNodeKind::TypeLit(members) => members.iter().any(|m| type_member_refs(name, m)),
        TypeNodeKind::Conditional(c) => {
            tn_refs(name, &c.check)
                || tn_refs(name, &c.extends)
                || tn_refs(name, &c.true_type)
                || tn_refs(name, &c.false_type)
        }
        TypeNodeKind::Mapped(m) => {
            m.type_param
                .constraint
                .as_deref()
                .is_some_and(|c| tn_refs(name, c))
                || m.name_type.as_deref().is_some_and(|n| tn_refs(name, n))
                || m.type_ann.as_deref().is_some_and(|t| tn_refs(name, t))
        }
        TypeNodeKind::IndexedAccess(object, index) => tn_refs(name, object) || tn_refs(name, index),
        TypeNodeKind::TypeQuery(expr) => expr_refs(name, expr),
        TypeNodeKind::Infer(_, constraint) => {
            constraint.as_deref().is_some_and(|c| tn_refs(name, c))
        }
        TypeNodeKind::TemplateLit(t) => t.types.iter().any(|n| tn_refs(name, n)),
        TypeNodeKind::ImportType(import) => {
            tn_refs(name, &import.argument)
                || import
                    .type_args
                    .as_deref()
                    .is_some_and(|args| args.iter().any(|a| tn_refs(name, a)))
        }
        TypeNodeKind::Predicate(p) => p.type_ann.as_deref().is_some_and(|t| tn_refs(name, t)),
        TypeNodeKind::Keyword(_)
        | TypeNodeKind::This
        | TypeNodeKind::Literal(_)
        | TypeNodeKind::JSDocNullable(None) => false,
    }
}

fn tps_refs(name: &str, tps: Option<&[TypeParam]>) -> bool {
    tps.unwrap_or_default().iter().any(|tp| {
        tp.constraint.as_deref().is_some_and(|c| tn_refs(name, c))
            || tp.default.as_deref().is_some_and(|d| tn_refs(name, d))
    })
}

fn pat_refs(name: &str, pat: &Pat) -> bool {
    match &pat.kind {
        PatKind::Ident(_) => false,
        PatKind::Array(elements) => elements.iter().flatten().any(|e| match e {
            ArrayPatElem::Pat(p) | ArrayPatElem::Rest(p) => pat_refs(name, p),
        }),
        PatKind::Object(props) => props.iter().any(|p| match p {
            ObjPatProp::KeyValue(key, value) => {
                matches!(key, PropName::Computed(e, _) if expr_refs(name, e))
                    || pat_refs(name, value)
            }
            ObjPatProp::Rest(p) => pat_refs(name, p),
            ObjPatProp::Shorthand(_, _) => false,
            ObjPatProp::ShorthandAssign(_, init, _) => expr_refs(name, init),
        }),
        PatKind::Assign(p, init) => pat_refs(name, p) || expr_refs(name, init),
        PatKind::Rest(p) => pat_refs(name, p),
    }
}

fn params_refs(name: &str, params: &[Param]) -> bool {
    params.iter().any(|p| {
        p.type_ann.as_ref().is_some_and(|t| tn_refs(name, t))
            || p.initializer.as_deref().is_some_and(|e| expr_refs(name, e))
            || pat_refs(name, &p.name)
            || p.decorators.iter().any(|d| expr_refs(name, d))
    })
}

fn signature_refs(
    name: &str,
    tps: Option<&[TypeParam]>,
    params: &[Param],
    return_type: Option<&TypeNode>,
    body: Option<&[Stmt]>,
) -> bool {
    tps_refs(name, tps)
        || params_refs(name, params)
        || return_type.is_some_and(|t| tn_refs(name, t))
        || body.is_some_and(|b| stmts_refs(name, b))
}

fn prop_name_refs(name: &str, pn: &PropName) -> bool {
    matches!(pn, PropName::Computed(e, _) if expr_refs(name, e))
}

fn type_member_refs(name: &str, member: &TypeMember) -> bool {
    match &member.kind {
        TypeMemberKind::PropertySig(p) => {
            prop_name_refs(name, &p.name) || p.type_ann.as_ref().is_some_and(|t| tn_refs(name, t))
        }
        TypeMemberKind::MethodSig(m) => {
            prop_name_refs(name, &m.name)
                || signature_refs(
                    name,
                    m.type_params.as_deref(),
                    &m.params,
                    m.return_type.as_ref(),
                    None,
                )
        }
        TypeMemberKind::CallSig(s) => signature_refs(
            name,
            s.type_params.as_deref(),
            &s.params,
            s.return_type.as_ref(),
            None,
        ),
        TypeMemberKind::ConstructSig(s) => signature_refs(
            name,
            s.type_params.as_deref(),
            &s.params,
            s.return_type.as_ref(),
            None,
        ),
        TypeMemberKind::IndexSig(s) => {
            params_refs(name, &s.params) || s.type_ann.as_ref().is_some_and(|t| tn_refs(name, t))
        }
        TypeMemberKind::GetAccessorSig(a) | TypeMemberKind::SetAccessorSig(a) => {
            prop_name_refs(name, &a.name)
                || params_refs(name, &a.params)
                || a.return_type.as_ref().is_some_and(|t| tn_refs(name, t))
        }
    }
}

fn class_member_refs(name: &str, member: &ClassMember) -> bool {
    match &member.kind {
        ClassMemberKind::Property(p) => {
            prop_name_refs(name, &p.name)
                || p.type_ann.as_ref().is_some_and(|t| tn_refs(name, t))
                || p.initializer.as_deref().is_some_and(|e| expr_refs(name, e))
                || p.decorators.iter().any(|d| expr_refs(name, d))
        }
        ClassMemberKind::Method(m) => {
            prop_name_refs(name, &m.name)
                || signature_refs(
                    name,
                    m.type_params.as_deref(),
                    &m.params,
                    m.return_type.as_ref(),
                    m.body.as_deref(),
                )
                || m.decorators.iter().any(|d| expr_refs(name, d))
        }
        ClassMemberKind::Constructor(c) => {
            signature_refs(name, None, &c.params, None, c.body.as_deref())
        }
        ClassMemberKind::GetAccessor(a) | ClassMemberKind::SetAccessor(a) => {
            prop_name_refs(name, &a.name)
                || signature_refs(
                    name,
                    a.type_params.as_deref(),
                    &a.params,
                    a.return_type.as_ref(),
                    a.body.as_deref(),
                )
        }
        ClassMemberKind::IndexSignature(s) => {
            params_refs(name, &s.params) || s.type_ann.as_ref().is_some_and(|t| tn_refs(name, t))
        }
        ClassMemberKind::StaticBlock(stmts) => stmts_refs(name, stmts),
        ClassMemberKind::SemicolonClassElement => false,
    }
}

fn class_refs(name: &str, c: &ClassDecl) -> bool {
    tps_refs(name, c.type_params.as_deref())
        || c.extends.as_deref().is_some_and(|e| expr_refs(name, e))
        || c.extends_type_args
            .as_deref()
            .is_some_and(|args| args.iter().any(|a| tn_refs(name, a)))
        || c.implements.iter().any(|t| tn_refs(name, t))
        || c.members.iter().any(|m| class_member_refs(name, m))
        || c.decorators.iter().any(|d| expr_refs(name, d))
}

fn obj_prop_refs(name: &str, prop: &ObjLitProp) -> bool {
    match prop {
        ObjLitProp::Property(p) => prop_name_refs(name, &p.key) || expr_refs(name, &p.value),
        ObjLitProp::Shorthand(_, _) => false,
        ObjLitProp::ShorthandDefault(_, init, _) => expr_refs(name, init),
        ObjLitProp::Spread(e, _) => expr_refs(name, e),
        ObjLitProp::Method(m) => {
            prop_name_refs(name, &m.name)
                || signature_refs(
                    name,
                    m.type_params.as_deref(),
                    &m.params,
                    m.return_type.as_ref(),
                    Some(&m.body),
                )
        }
        ObjLitProp::Get(a) | ObjLitProp::Set(a) => {
            prop_name_refs(name, &a.name)
                || signature_refs(name, None, &a.params, a.return_type.as_ref(), Some(&a.body))
        }
    }
}

fn expr_refs(name: &str, expr: &Expr) -> bool {
    match &expr.kind {
        ExprKind::Ident(_)
        | ExprKind::NumLit(_)
        | ExprKind::BigIntLit(_)
        | ExprKind::StrLit(_)
        | ExprKind::BoolLit(_)
        | ExprKind::NullLit
        | ExprKind::RegexpLit(_)
        | ExprKind::NoSubstTemplate(_)
        | ExprKind::This
        | ExprKind::Super
        | ExprKind::MetaProp(_)
        | ExprKind::Omitted => false,
        ExprKind::Template(t) => t.exprs.iter().any(|e| expr_refs(name, e)),
        ExprKind::TaggedTemplate(t) => {
            expr_refs(name, &t.tag)
                || t.quasi.exprs.iter().any(|e| expr_refs(name, e))
                || t.type_args
                    .as_deref()
                    .is_some_and(|args| args.iter().any(|a| tn_refs(name, a)))
        }
        ExprKind::ArrayLit(elems) => elems.iter().flatten().any(|e| expr_refs(name, e)),
        ExprKind::ObjectLit(props) => props.iter().any(|p| obj_prop_refs(name, p)),
        ExprKind::FnExpr(f) => signature_refs(
            name,
            f.type_params.as_deref(),
            &f.params,
            f.return_type.as_ref(),
            f.body.as_deref(),
        ),
        ExprKind::Arrow(a) => {
            tps_refs(name, a.type_params.as_deref())
                || params_refs(name, &a.params)
                || a.return_type.as_ref().is_some_and(|t| tn_refs(name, t))
                || match &a.body {
                    ArrowBody::Expr(e) => expr_refs(name, e),
                    ArrowBody::Block(stmts) => stmts_refs(name, stmts),
                }
        }
        ExprKind::ClassExpr(c) => class_refs(name, c),
        ExprKind::Call(c) => {
            expr_refs(name, &c.callee)
                || c.type_args
                    .as_deref()
                    .is_some_and(|args| args.iter().any(|a| tn_refs(name, a)))
                || c.args.iter().any(|a| expr_refs(name, a))
        }
        ExprKind::New(n) => {
            expr_refs(name, &n.callee)
                || n.type_args
                    .as_deref()
                    .is_some_and(|args| args.iter().any(|a| tn_refs(name, a)))
                || n.args
                    .as_deref()
                    .is_some_and(|args| args.iter().any(|a| expr_refs(name, a)))
        }
        ExprKind::Member(m) => expr_refs(name, &m.object),
        ExprKind::ElemAccess(e) => expr_refs(name, &e.object) || expr_refs(name, &e.index),
        ExprKind::Cond(c) => {
            expr_refs(name, &c.test)
                || expr_refs(name, &c.consequent)
                || expr_refs(name, &c.alternate)
        }
        ExprKind::Binary(b) => expr_refs(name, &b.left) || expr_refs(name, &b.right),
        ExprKind::Unary(u) => expr_refs(name, &u.argument),
        ExprKind::Update(u) => expr_refs(name, &u.argument),
        ExprKind::Paren(inner)
        | ExprKind::NonNull(inner)
        | ExprKind::Spread(inner)
        | ExprKind::Await(inner)
        | ExprKind::Delete(inner)
        | ExprKind::Typeof(inner)
        | ExprKind::Void(inner) => expr_refs(name, inner),
        ExprKind::TypeAssertion(t) => tn_refs(name, &t.type_node) || expr_refs(name, &t.expr),
        ExprKind::As(a) => tn_refs(name, &a.type_node) || expr_refs(name, &a.expr),
        ExprKind::Satisfies(s) => tn_refs(name, &s.type_node) || expr_refs(name, &s.expr),
        ExprKind::Instantiation(i) => {
            expr_refs(name, &i.expr) || i.type_args.iter().any(|a| tn_refs(name, a))
        }
        ExprKind::Yield(_, e) => e.as_deref().is_some_and(|e| expr_refs(name, e)),
        ExprKind::Assign(a) => expr_refs(name, &a.left) || expr_refs(name, &a.right),
        ExprKind::Comma(exprs) => exprs.iter().any(|e| expr_refs(name, e)),
        // JSX bodies cannot mention type parameters in ways this pass
        // models; treat them as opaque.
        ExprKind::JsxElement(_) | ExprKind::JsxSelfClosing(_) | ExprKind::JsxFragment(_) => false,
    }
}

fn var_refs(name: &str, var: &VarStmt) -> bool {
    var.declarations.iter().any(|d| {
        pat_refs(name, &d.name)
            || d.type_ann.as_ref().is_some_and(|t| tn_refs(name, t))
            || d.init.as_deref().is_some_and(|e| expr_refs(name, e))
    })
}

fn stmts_refs(name: &str, stmts: &[Stmt]) -> bool {
    stmts.iter().any(|s| stmt_refs(name, s))
}

fn stmt_refs(name: &str, stmt: &Stmt) -> bool {
    match &stmt.kind {
        StmtKind::Var(v) => var_refs(name, v),
        StmtKind::Expr(e) | StmtKind::Throw(e) => expr_refs(name, e),
        StmtKind::Return(e) => e.as_deref().is_some_and(|e| expr_refs(name, e)),
        StmtKind::If(i) => {
            expr_refs(name, &i.test)
                || stmt_refs(name, &i.consequent)
                || i.alternate.as_deref().is_some_and(|a| stmt_refs(name, a))
        }
        StmtKind::While(w) => expr_refs(name, &w.test) || stmt_refs(name, &w.body),
        StmtKind::DoWhile(d) => stmt_refs(name, &d.body) || expr_refs(name, &d.test),
        StmtKind::For(f) => {
            f.init.as_ref().is_some_and(|init| match init {
                ForInit::Var(v) => var_refs(name, v),
                ForInit::Expr(e) => expr_refs(name, e),
            }) || f.test.as_deref().is_some_and(|e| expr_refs(name, e))
                || f.update.as_deref().is_some_and(|e| expr_refs(name, e))
                || stmt_refs(name, &f.body)
        }
        StmtKind::ForIn(f) => {
            for_left_refs(name, &f.left) || expr_refs(name, &f.right) || stmt_refs(name, &f.body)
        }
        StmtKind::ForOf(f) => {
            for_left_refs(name, &f.left) || expr_refs(name, &f.right) || stmt_refs(name, &f.body)
        }
        StmtKind::Switch(s) => {
            expr_refs(name, &s.discriminant)
                || s.cases.iter().any(|c| {
                    c.test.as_deref().is_some_and(|t| expr_refs(name, t))
                        || stmts_refs(name, &c.consequent)
                })
        }
        StmtKind::Try(t) => {
            stmts_refs(name, &t.block)
                || t.handler.as_ref().is_some_and(|h| {
                    h.param_type.as_ref().is_some_and(|t| tn_refs(name, t))
                        || stmts_refs(name, &h.body)
                })
                || t.finalizer.as_deref().is_some_and(|f| stmts_refs(name, f))
        }
        StmtKind::Block(stmts) => stmts_refs(name, stmts),
        StmtKind::FnDecl(f) => signature_refs(
            name,
            f.type_params.as_deref(),
            &f.params,
            f.return_type.as_ref(),
            f.body.as_deref(),
        ),
        StmtKind::ClassDecl(c) => class_refs(name, c),
        StmtKind::InterfaceDecl(i) => {
            tps_refs(name, i.type_params.as_deref())
                || i.extends.iter().any(|t| tn_refs(name, t))
                || i.members.iter().any(|m| type_member_refs(name, m))
        }
        StmtKind::TypeAlias(t) => {
            tps_refs(name, t.type_params.as_deref()) || tn_refs(name, &t.type_ann)
        }
        StmtKind::EnumDecl(e) => e
            .members
            .iter()
            .any(|m| m.initializer.as_deref().is_some_and(|i| expr_refs(name, i))),
        StmtKind::ModuleDecl(m) => match &m.body {
            Some(ModuleBody::Block(stmts)) => stmts_refs(name, stmts),
            Some(ModuleBody::Module(inner)) => match &inner.body {
                Some(ModuleBody::Block(stmts)) => stmts_refs(name, stmts),
                _ => false,
            },
            None => false,
        },
        StmtKind::Export(e) => match &e.kind {
            ExportDeclKind::Decl(s) | ExportDeclKind::DefaultDecl(s) => stmt_refs(name, s),
            ExportDeclKind::Default(expr) => expr_refs(name, expr),
            _ => false,
        },
        StmtKind::ExportAssign(e) => expr_refs(name, e),
        StmtKind::Labeled(l) => stmt_refs(name, &l.body),
        StmtKind::With(w) => expr_refs(name, &w.object) || stmt_refs(name, &w.body),
        StmtKind::ImportEquals(i) => expr_refs(name, &i.module_ref),
        StmtKind::Break(_)
        | StmtKind::Continue(_)
        | StmtKind::Empty
        | StmtKind::Import(_)
        | StmtKind::Debugger => false,
    }
}

fn for_left_refs(name: &str, left: &ForInOfLeft) -> bool {
    match left {
        ForInOfLeft::Var(v) => var_refs(name, v),
        ForInOfLeft::Pat(p) => pat_refs(name, p),
        ForInOfLeft::Expr(e) => expr_refs(name, e),
    }
}

fn collect_infer_names(node: &TypeNode, out: &mut Vec<(String, Span)>) {
    match &node.kind {
        TypeNodeKind::Infer(name, constraint) => {
            out.push((name.clone(), node.span));
            if let Some(c) = constraint {
                collect_infer_names(c, out);
            }
        }
        TypeNodeKind::Reference(r) => {
            for a in r.type_args.as_deref().unwrap_or_default() {
                collect_infer_names(a, out);
            }
        }
        TypeNodeKind::Array(inner)
        | TypeNodeKind::Keyof(inner)
        | TypeNodeKind::Unique(inner)
        | TypeNodeKind::Readonly(inner)
        | TypeNodeKind::Paren(inner)
        | TypeNodeKind::Rest(inner)
        | TypeNodeKind::Optional(inner)
        | TypeNodeKind::TypeOperator(_, inner)
        | TypeNodeKind::JSDocNullable(Some(inner)) => collect_infer_names(inner, out),
        TypeNodeKind::Tuple(elements) => {
            for e in elements {
                collect_infer_names(&e.type_node, out);
            }
        }
        TypeNodeKind::NamedTupleMember(m) => collect_infer_names(&m.type_node, out),
        TypeNodeKind::Union(nodes) | TypeNodeKind::Intersection(nodes) => {
            for n in nodes {
                collect_infer_names(n, out);
            }
        }
        TypeNodeKind::Function(f) | TypeNodeKind::Constructor(f) => {
            for p in &f.params {
                if let Some(t) = &p.type_ann {
                    collect_infer_names(t, out);
                }
            }
            collect_infer_names(&f.return_type, out);
        }
        TypeNodeKind::TypeLit(members) => {
            for m in members {
                if let TypeMemberKind::PropertySig(p) = &m.kind {
                    if let Some(t) = &p.type_ann {
                        collect_infer_names(t, out);
                    }
                }
            }
        }
        TypeNodeKind::IndexedAccess(a, b) => {
            collect_infer_names(a, out);
            collect_infer_names(b, out);
        }
        TypeNodeKind::TemplateLit(t) => {
            for n in &t.types {
                collect_infer_names(n, out);
            }
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// Driver
// ---------------------------------------------------------------------------

impl TypeChecker {
    /// Report unused type parameters of every declaration in `stmts`
    /// (recursively, including nested functions, class and object literal
    /// members, function types and `infer` declarations).
    pub(crate) fn check_unused_type_parameters(&mut self, stmts: &[Stmt]) {
        // Declaration files report neither diagnostic.
        if self.current_file_is_js() || self.current_file_is_declaration() {
            return;
        }
        // Unused renamings in bodyless signatures (TS2842) are reported
        // regardless of noUnusedParameters; the walk then reports nothing else.
        self.utp_renamings_only = !self.no_unused_parameters;
        self.utp_stmts(stmts);
        self.utp_renamings_only = false;
    }

    /// TS2842: `{ p: name }` in a parameter of a signature with no body
    /// renames `p` to a binding nothing can use (it reads like a type
    /// annotation). TS2843 points at the end of an unannotated parameter.
    fn report_unused_renamings(&mut self, params: &[Param], return_type: Option<&TypeNode>) {
        fn renamings<'p>(pattern: &'p Pat, out: &mut Vec<(&'p PropName, &'p str, Span)>) {
            match &pattern.kind {
                PatKind::Object(properties) => {
                    for property in properties {
                        match property {
                            ObjPatProp::KeyValue(key, value) => {
                                let target = match &value.kind {
                                    PatKind::Assign(inner, _) => inner.as_ref(),
                                    _ => value,
                                };
                                if let PatKind::Ident(name) = &target.kind {
                                    out.push((key, name.as_str(), target.span));
                                } else {
                                    renamings(target, out);
                                }
                            }
                            ObjPatProp::Rest(inner) => renamings(inner, out),
                            _ => {}
                        }
                    }
                }
                PatKind::Array(elements) => {
                    for element in elements.iter().flatten() {
                        match element {
                            ArrayPatElem::Pat(inner) | ArrayPatElem::Rest(inner) => {
                                renamings(inner, out)
                            }
                        }
                    }
                }
                PatKind::Assign(inner, _) | PatKind::Rest(inner) => renamings(inner, out),
                PatKind::Ident(_) => {}
            }
        }
        let source = self.current_source.clone().unwrap_or_default();
        let text = |span: Span| {
            source
                .get(span.start as usize..span.end as usize)
                .unwrap_or("")
        };
        for param in params {
            let mut found = Vec::new();
            renamings(&param.name, &mut found);
            for (key, name, span) in found {
                // A `typeof name` or `name is T` in the signature uses it.
                let predicate = matches!(return_type.map(|t| &t.kind),
                    Some(TypeNodeKind::Predicate(p)) if p.param_name == name);
                let queried = params
                    .iter()
                    .filter_map(|p| p.type_ann.as_ref())
                    .chain(return_type)
                    .any(|t| {
                        let t = text(t.span);
                        t.match_indices("typeof").any(|(at, _)| {
                            let rest = t[at + 6..].trim_start();
                            rest.strip_prefix(name).is_some_and(|after| {
                                !after.starts_with(|c: char| {
                                    c.is_alphanumeric() || c == '_' || c == '$'
                                })
                            })
                        })
                    });
                if predicate || queried {
                    continue;
                }
                let property = text(key.span());
                let property = match key {
                    PropName::Computed(..) if !property.starts_with('[') => format!("[{property}]"),
                    _ => property.to_string(),
                };
                let related = param.type_ann.is_none().then(|| {
                    vec![tsc_rs_ast::RelatedDiagnostic {
                        code: 2843,
                        message: format!(
                            "We can only write a type for '{property}' by adding a type for the entire parameter here."
                        ),
                        file_name: self.current_file_name.clone(),
                        span: Some(Span::new(param.span.end, param.span.end)),
                    }]
                });
                self.diagnostics.push(Diagnostic {
                    code: 2842,
                    message: format!(
                        "'{name}' is an unused renaming of '{property}'. Did you intend to use it as a type annotation?"
                    ),
                    category: DiagnosticCategory::Error,
                    file_name: None,
                    span: Some(span),
                    related,
                });
            }
        }
    }

    fn utp_stmts(&mut self, stmts: &[Stmt]) {
        // Merged symbols (overload sets, class/interface merges): only the
        // LAST declaration of a name reports, and a reference in ANY of the
        // declarations counts as a use.
        fn declared_name(stmt: &Stmt) -> Option<String> {
            match &TypeChecker::unwrap_export_stmt(stmt).kind {
                StmtKind::FnDecl(f) => f.name.clone(),
                StmtKind::ClassDecl(c) => c.name.clone(),
                StmtKind::InterfaceDecl(i) => Some(i.name.clone()),
                _ => None,
            }
        }
        let mut declarations: rustc_hash::FxHashMap<String, Vec<&Stmt>> =
            rustc_hash::FxHashMap::default();
        for stmt in stmts {
            if let Some(name) = declared_name(stmt) {
                declarations
                    .entry(name)
                    .or_default()
                    .push(TypeChecker::unwrap_export_stmt(stmt));
            }
        }
        let current_file = self
            .current_file_name
            .as_deref()
            .map(TypeChecker::normalized_file_key)
            .unwrap_or_default();
        for stmt in stmts {
            let inner = TypeChecker::unwrap_export_stmt(stmt);
            match declared_name(stmt) {
                Some(name) => {
                    // A global class/interface also declared in another file
                    // merges across files; the program-wide last declaration
                    // is unknown here, so do not report (still recurse).
                    let merges_across_files = self
                        .external_top_level_type_decls
                        .get(&name)
                        .is_some_and(|files| files.iter().any(|f| *f != current_file));
                    let group = &declarations[&name];
                    if !merges_across_files && std::ptr::eq(*group.last().unwrap(), inner) {
                        self.utp_stmt(stmt, Some(group));
                    } else {
                        self.utp_stmt(stmt, None);
                    }
                }
                None => {
                    let single = [inner];
                    self.utp_stmt(stmt, Some(&single));
                }
            }
        }
    }

    /// `merged`: the declarations sharing this statement's symbol when this
    /// statement is the last of them (reports happen here, references are
    /// searched across all of them); `None` for an earlier declaration of a
    /// merged symbol (no report).
    fn utp_stmt(&mut self, stmt: &Stmt, merged: Option<&[&Stmt]>) {
        match &stmt.kind {
            StmtKind::Export(e) => match &e.kind {
                ExportDeclKind::Decl(s) | ExportDeclKind::DefaultDecl(s) => {
                    self.utp_stmt(s, merged)
                }
                ExportDeclKind::Default(expr) => self.utp_expr(expr),
                _ => {}
            },
            StmtKind::FnDecl(f) => {
                if f.modifiers & MOD_DECLARE != 0 {
                    // Ambient: only unused renamings (TS2842) apply.
                    let saved = std::mem::replace(&mut self.utp_renamings_only, true);
                    self.utp_signature(
                        f.type_params.as_deref(),
                        &f.params,
                        f.return_type.as_ref(),
                        true,
                    );
                    self.utp_renamings_only = saved;
                    return;
                }
                if let Some(group) = merged {
                    self.report_unused_type_params(f.type_params.as_deref(), |name| {
                        group.iter().any(|d| stmt_refs(name, d))
                    });
                }
                self.utp_signature(
                    f.type_params.as_deref(),
                    &f.params,
                    f.return_type.as_ref(),
                    f.body.is_none(),
                );
                if let Some(body) = &f.body {
                    self.utp_stmts(body);
                }
            }
            StmtKind::ClassDecl(c) => self.utp_class(c, merged),
            StmtKind::InterfaceDecl(i) => {
                if let Some(group) = merged {
                    self.report_unused_type_params(i.type_params.as_deref(), |name| {
                        group.iter().any(|d| stmt_refs(name, d))
                    });
                }
                self.utp_type_params(i.type_params.as_deref());
                for t in &i.extends {
                    self.utp_type_node(t);
                }
                self.utp_type_members(&i.members);
            }
            StmtKind::TypeAlias(t) => {
                self.report_unused_type_params(t.type_params.as_deref(), |name| {
                    tps_refs(name, t.type_params.as_deref()) || tn_refs(name, &t.type_ann)
                });
                self.utp_type_params(t.type_params.as_deref());
                self.utp_type_node(&t.type_ann);
            }
            StmtKind::Var(v) => self.utp_var(v),
            StmtKind::Expr(e) | StmtKind::Throw(e) => self.utp_expr(e),
            StmtKind::Return(Some(e)) => self.utp_expr(e),
            StmtKind::Return(None) => {}
            StmtKind::If(i) => {
                self.utp_expr(&i.test);
                self.utp_stmt(&i.consequent, Some(&[&*i.consequent]));
                if let Some(a) = &i.alternate {
                    self.utp_stmt(a, Some(&[&**a]));
                }
            }
            StmtKind::While(w) => {
                self.utp_expr(&w.test);
                self.utp_stmt(&w.body, Some(&[&*w.body]));
            }
            StmtKind::DoWhile(d) => {
                self.utp_stmt(&d.body, Some(&[&*d.body]));
                self.utp_expr(&d.test);
            }
            StmtKind::For(f) => {
                match &f.init {
                    Some(ForInit::Var(v)) => self.utp_var(v),
                    Some(ForInit::Expr(e)) => self.utp_expr(e),
                    None => {}
                }
                if let Some(t) = &f.test {
                    self.utp_expr(t);
                }
                if let Some(u) = &f.update {
                    self.utp_expr(u);
                }
                self.utp_stmt(&f.body, Some(&[&*f.body]));
            }
            StmtKind::ForIn(f) => {
                self.utp_for_left(&f.left);
                self.utp_expr(&f.right);
                self.utp_stmt(&f.body, Some(&[&*f.body]));
            }
            StmtKind::ForOf(f) => {
                self.utp_for_left(&f.left);
                self.utp_expr(&f.right);
                self.utp_stmt(&f.body, Some(&[&*f.body]));
            }
            StmtKind::Switch(s) => {
                self.utp_expr(&s.discriminant);
                for c in &s.cases {
                    if let Some(t) = &c.test {
                        self.utp_expr(t);
                    }
                    self.utp_stmts(&c.consequent);
                }
            }
            StmtKind::Try(t) => {
                self.utp_stmts(&t.block);
                if let Some(h) = &t.handler {
                    if let Some(pt) = &h.param_type {
                        self.utp_type_node(pt);
                    }
                    self.utp_stmts(&h.body);
                }
                if let Some(f) = &t.finalizer {
                    self.utp_stmts(f);
                }
            }
            StmtKind::Block(stmts) => self.utp_stmts(stmts),
            StmtKind::Labeled(l) => self.utp_stmt(&l.body, Some(&[&*l.body])),
            StmtKind::With(w) => {
                self.utp_expr(&w.object);
                self.utp_stmt(&w.body, Some(&[&*w.body]));
            }
            StmtKind::EnumDecl(e) => {
                for m in &e.members {
                    if let Some(init) = &m.initializer {
                        self.utp_expr(init);
                    }
                }
            }
            StmtKind::ModuleDecl(m)
                if m.modifiers & MOD_DECLARE != 0 && !self.utp_renamings_only =>
            {
                // Ambient: only unused renamings (TS2842) apply.
                self.utp_renamings_only = true;
                self.utp_stmt(stmt, merged);
                self.utp_renamings_only = false;
            }
            StmtKind::ModuleDecl(m) => {
                let mut current = m;
                loop {
                    match &current.body {
                        Some(ModuleBody::Block(stmts)) => {
                            self.utp_stmts(stmts);
                            break;
                        }
                        Some(ModuleBody::Module(inner)) => current = inner,
                        None => break,
                    }
                }
            }
            StmtKind::ExportAssign(e) => self.utp_expr(e),
            StmtKind::Break(_)
            | StmtKind::Continue(_)
            | StmtKind::Empty
            | StmtKind::Import(_)
            | StmtKind::ImportEquals(_)
            | StmtKind::Debugger => {}
        }
    }

    fn utp_var(&mut self, var: &VarStmt) {
        for d in &var.declarations {
            self.utp_pat(&d.name);
            if let Some(t) = &d.type_ann {
                self.utp_type_node(t);
            }
            if let Some(init) = &d.init {
                self.utp_expr(init);
            }
        }
    }

    fn utp_for_left(&mut self, left: &ForInOfLeft) {
        match left {
            ForInOfLeft::Var(v) => self.utp_var(v),
            ForInOfLeft::Pat(p) => self.utp_pat(p),
            ForInOfLeft::Expr(e) => self.utp_expr(e),
        }
    }

    fn utp_pat(&mut self, pat: &Pat) {
        match &pat.kind {
            PatKind::Ident(_) => {}
            PatKind::Array(elements) => {
                for e in elements.iter().flatten() {
                    match e {
                        ArrayPatElem::Pat(p) | ArrayPatElem::Rest(p) => self.utp_pat(p),
                    }
                }
            }
            PatKind::Object(props) => {
                for p in props {
                    match p {
                        ObjPatProp::KeyValue(_, value) => self.utp_pat(value),
                        ObjPatProp::Rest(p) => self.utp_pat(p),
                        ObjPatProp::Shorthand(_, _) => {}
                        ObjPatProp::ShorthandAssign(_, init, _) => self.utp_expr(init),
                    }
                }
            }
            PatKind::Assign(p, init) => {
                self.utp_pat(p);
                self.utp_expr(init);
            }
            PatKind::Rest(p) => self.utp_pat(p),
        }
    }

    fn utp_type_params(&mut self, tps: Option<&[TypeParam]>) {
        for tp in tps.unwrap_or_default() {
            if let Some(c) = &tp.constraint {
                self.utp_type_node(c);
            }
            if let Some(d) = &tp.default {
                self.utp_type_node(d);
            }
        }
    }

    fn utp_signature(
        &mut self,
        tps: Option<&[TypeParam]>,
        params: &[Param],
        return_type: Option<&TypeNode>,
        bodyless: bool,
    ) {
        if bodyless {
            self.report_unused_renamings(params, return_type);
        }
        if self.utp_renamings_only {
            for p in params {
                if let Some(t) = &p.type_ann {
                    self.utp_type_node(t);
                }
            }
            if let Some(t) = return_type {
                self.utp_type_node(t);
            }
            return;
        }
        self.utp_type_params(tps);
        for p in params {
            self.utp_pat(&p.name);
            if let Some(t) = &p.type_ann {
                self.utp_type_node(t);
            }
            if let Some(init) = &p.initializer {
                self.utp_expr(init);
            }
        }
        if let Some(t) = return_type {
            self.utp_type_node(t);
        }
    }

    fn utp_class(&mut self, c: &ClassDecl, merged: Option<&[&Stmt]>) {
        if c.modifiers & MOD_DECLARE != 0 && !self.utp_renamings_only {
            // Ambient: only unused renamings (TS2842) apply.
            self.utp_renamings_only = true;
            self.utp_class(c, merged);
            self.utp_renamings_only = false;
            return;
        }
        if let Some(group) = merged {
            self.report_unused_type_params(c.type_params.as_deref(), |name| {
                class_refs(name, c) || group.iter().any(|d| stmt_refs(name, d))
            });
        }
        self.utp_type_params(c.type_params.as_deref());
        if let Some(e) = &c.extends {
            self.utp_expr(e);
        }
        for a in c.extends_type_args.as_deref().unwrap_or_default() {
            self.utp_type_node(a);
        }
        for t in &c.implements {
            self.utp_type_node(t);
        }
        // Method overload sets: only the implementation (last declaration)
        // is checked.
        let mut last_method: rustc_hash::FxHashMap<String, usize> =
            rustc_hash::FxHashMap::default();
        for (index, m) in c.members.iter().enumerate() {
            if let ClassMemberKind::Method(method) = &m.kind {
                if let Some(name) = TypeChecker::propname_text_opt(&method.name) {
                    let key = format!("{}{}", method.modifiers & MOD_STATIC, name);
                    last_method.insert(key, index);
                }
            }
        }
        for (index, m) in c.members.iter().enumerate() {
            match &m.kind {
                ClassMemberKind::Property(p) => {
                    if let Some(t) = &p.type_ann {
                        self.utp_type_node(t);
                    }
                    if let Some(init) = &p.initializer {
                        self.utp_expr(init);
                    }
                }
                ClassMemberKind::Method(method) => {
                    let is_last =
                        TypeChecker::propname_text_opt(&method.name).map_or(true, |name| {
                            let key = format!("{}{}", method.modifiers & MOD_STATIC, name);
                            last_method.get(&key) == Some(&index)
                        });
                    if is_last {
                        self.report_unused_type_params(method.type_params.as_deref(), |name| {
                            signature_refs(
                                name,
                                method.type_params.as_deref(),
                                &method.params,
                                method.return_type.as_ref(),
                                method.body.as_deref(),
                            )
                        });
                    }
                    self.utp_signature(
                        method.type_params.as_deref(),
                        &method.params,
                        method.return_type.as_ref(),
                        method.body.is_none(),
                    );
                    if let Some(body) = &method.body {
                        self.utp_stmts(body);
                    }
                }
                ClassMemberKind::Constructor(ctor) => {
                    self.utp_signature(None, &ctor.params, None, ctor.body.is_none());
                    if let Some(body) = &ctor.body {
                        self.utp_stmts(body);
                    }
                }
                ClassMemberKind::GetAccessor(a) | ClassMemberKind::SetAccessor(a) => {
                    self.utp_signature(
                        a.type_params.as_deref(),
                        &a.params,
                        a.return_type.as_ref(),
                        a.body.is_none(),
                    );
                    if let Some(body) = &a.body {
                        self.utp_stmts(body);
                    }
                }
                ClassMemberKind::IndexSignature(s) => {
                    self.utp_signature(None, &s.params, s.type_ann.as_ref(), true);
                }
                ClassMemberKind::StaticBlock(stmts) => self.utp_stmts(stmts),
                ClassMemberKind::SemicolonClassElement => {}
            }
        }
    }

    fn utp_type_members(&mut self, members: &[TypeMember]) {
        // Overloaded method signatures: only the last is checked.
        let mut last_method: rustc_hash::FxHashMap<String, usize> =
            rustc_hash::FxHashMap::default();
        for (index, m) in members.iter().enumerate() {
            if let TypeMemberKind::MethodSig(method) = &m.kind {
                if let Some(name) = TypeChecker::propname_text_opt(&method.name) {
                    last_method.insert(name, index);
                }
            }
        }
        for (index, m) in members.iter().enumerate() {
            match &m.kind {
                TypeMemberKind::PropertySig(p) => {
                    if let Some(t) = &p.type_ann {
                        self.utp_type_node(t);
                    }
                }
                TypeMemberKind::MethodSig(method) => {
                    let is_last = TypeChecker::propname_text_opt(&method.name)
                        .map_or(true, |name| last_method.get(&name) == Some(&index));
                    if is_last {
                        self.report_unused_type_params(method.type_params.as_deref(), |name| {
                            signature_refs(
                                name,
                                method.type_params.as_deref(),
                                &method.params,
                                method.return_type.as_ref(),
                                None,
                            )
                        });
                    }
                    self.utp_signature(
                        method.type_params.as_deref(),
                        &method.params,
                        method.return_type.as_ref(),
                        true,
                    );
                }
                TypeMemberKind::CallSig(s) => {
                    self.report_unused_type_params(s.type_params.as_deref(), |name| {
                        signature_refs(
                            name,
                            s.type_params.as_deref(),
                            &s.params,
                            s.return_type.as_ref(),
                            None,
                        )
                    });
                    self.utp_signature(
                        s.type_params.as_deref(),
                        &s.params,
                        s.return_type.as_ref(),
                        true,
                    );
                }
                TypeMemberKind::ConstructSig(s) => {
                    self.report_unused_type_params(s.type_params.as_deref(), |name| {
                        signature_refs(
                            name,
                            s.type_params.as_deref(),
                            &s.params,
                            s.return_type.as_ref(),
                            None,
                        )
                    });
                    self.utp_signature(
                        s.type_params.as_deref(),
                        &s.params,
                        s.return_type.as_ref(),
                        true,
                    );
                }
                TypeMemberKind::IndexSig(s) => {
                    self.utp_signature(None, &s.params, s.type_ann.as_ref(), true);
                }
                TypeMemberKind::GetAccessorSig(a) | TypeMemberKind::SetAccessorSig(a) => {
                    self.utp_signature(None, &a.params, a.return_type.as_ref(), true);
                }
            }
        }
    }

    fn utp_type_node(&mut self, node: &TypeNode) {
        match &node.kind {
            TypeNodeKind::Function(f) | TypeNodeKind::Constructor(f) => {
                self.report_unused_type_params(f.type_params.as_deref(), |name| {
                    signature_refs(
                        name,
                        f.type_params.as_deref(),
                        &f.params,
                        Some(&f.return_type),
                        None,
                    )
                });
                self.utp_signature(
                    f.type_params.as_deref(),
                    &f.params,
                    Some(&f.return_type),
                    true,
                );
            }
            TypeNodeKind::TypeLit(members) => self.utp_type_members(members),
            TypeNodeKind::Conditional(c) => {
                let mut infers = Vec::new();
                collect_infer_names(&c.extends, &mut infers);
                for (name, span) in infers {
                    if self.utp_renamings_only || name.starts_with('_') {
                        continue;
                    }
                    let used = tn_refs(&name, &c.extends) || tn_refs(&name, &c.true_type);
                    if !used {
                        // tsc underlines the whole `infer U`.
                        self.diagnostics.push(error_unused_local(&name, span));
                    }
                }
                self.utp_type_node(&c.check);
                self.utp_type_node(&c.extends);
                self.utp_type_node(&c.true_type);
                self.utp_type_node(&c.false_type);
            }
            TypeNodeKind::Reference(r) => {
                for a in r.type_args.as_deref().unwrap_or_default() {
                    self.utp_type_node(a);
                }
            }
            TypeNodeKind::Array(inner)
            | TypeNodeKind::Keyof(inner)
            | TypeNodeKind::Unique(inner)
            | TypeNodeKind::Readonly(inner)
            | TypeNodeKind::Paren(inner)
            | TypeNodeKind::Rest(inner)
            | TypeNodeKind::Optional(inner)
            | TypeNodeKind::TypeOperator(_, inner)
            | TypeNodeKind::JSDocNullable(Some(inner)) => self.utp_type_node(inner),
            TypeNodeKind::Tuple(elements) => {
                for e in elements {
                    self.utp_type_node(&e.type_node);
                }
            }
            TypeNodeKind::NamedTupleMember(m) => self.utp_type_node(&m.type_node),
            TypeNodeKind::Union(nodes) | TypeNodeKind::Intersection(nodes) => {
                for n in nodes {
                    self.utp_type_node(n);
                }
            }
            TypeNodeKind::Mapped(m) => {
                if let Some(c) = &m.type_param.constraint {
                    self.utp_type_node(c);
                }
                if let Some(n) = &m.name_type {
                    self.utp_type_node(n);
                }
                if let Some(t) = &m.type_ann {
                    self.utp_type_node(t);
                }
            }
            TypeNodeKind::IndexedAccess(a, b) => {
                self.utp_type_node(a);
                self.utp_type_node(b);
            }
            TypeNodeKind::TypeQuery(e) => self.utp_expr(e),
            TypeNodeKind::Infer(_, Some(c)) => self.utp_type_node(c),
            TypeNodeKind::TemplateLit(t) => {
                for n in &t.types {
                    self.utp_type_node(n);
                }
            }
            TypeNodeKind::ImportType(i) => {
                for a in i.type_args.as_deref().unwrap_or_default() {
                    self.utp_type_node(a);
                }
            }
            TypeNodeKind::Predicate(p) => {
                if let Some(t) = &p.type_ann {
                    self.utp_type_node(t);
                }
            }
            TypeNodeKind::Keyword(_)
            | TypeNodeKind::This
            | TypeNodeKind::Literal(_)
            | TypeNodeKind::Infer(_, None)
            | TypeNodeKind::JSDocNullable(None) => {}
        }
    }

    fn utp_expr(&mut self, expr: &Expr) {
        match &expr.kind {
            ExprKind::FnExpr(f) => {
                self.report_unused_type_params(f.type_params.as_deref(), |name| {
                    signature_refs(
                        name,
                        f.type_params.as_deref(),
                        &f.params,
                        f.return_type.as_ref(),
                        f.body.as_deref(),
                    )
                });
                self.utp_signature(
                    f.type_params.as_deref(),
                    &f.params,
                    f.return_type.as_ref(),
                    f.body.is_none(),
                );
                if let Some(body) = &f.body {
                    self.utp_stmts(body);
                }
            }
            ExprKind::Arrow(a) => {
                self.report_unused_type_params(a.type_params.as_deref(), |name| {
                    tps_refs(name, a.type_params.as_deref())
                        || params_refs(name, &a.params)
                        || a.return_type.as_ref().is_some_and(|t| tn_refs(name, t))
                        || match &a.body {
                            ArrowBody::Expr(e) => expr_refs(name, e),
                            ArrowBody::Block(stmts) => stmts_refs(name, stmts),
                        }
                });
                self.utp_signature(
                    a.type_params.as_deref(),
                    &a.params,
                    a.return_type.as_ref(),
                    false,
                );
                match &a.body {
                    ArrowBody::Expr(e) => self.utp_expr(e),
                    ArrowBody::Block(stmts) => self.utp_stmts(stmts),
                }
            }
            ExprKind::ClassExpr(c) => self.utp_class(c, Some(&[])),
            ExprKind::Call(c) => {
                self.utp_expr(&c.callee);
                for a in c.type_args.as_deref().unwrap_or_default() {
                    self.utp_type_node(a);
                }
                for a in &c.args {
                    self.utp_expr(a);
                }
            }
            ExprKind::New(n) => {
                self.utp_expr(&n.callee);
                for a in n.type_args.as_deref().unwrap_or_default() {
                    self.utp_type_node(a);
                }
                for a in n.args.as_deref().unwrap_or_default() {
                    self.utp_expr(a);
                }
            }
            ExprKind::Template(t) => {
                for e in &t.exprs {
                    self.utp_expr(e);
                }
            }
            ExprKind::TaggedTemplate(t) => {
                self.utp_expr(&t.tag);
                for e in &t.quasi.exprs {
                    self.utp_expr(e);
                }
                for a in t.type_args.as_deref().unwrap_or_default() {
                    self.utp_type_node(a);
                }
            }
            ExprKind::ArrayLit(elems) => {
                for e in elems.iter().flatten() {
                    self.utp_expr(e);
                }
            }
            ExprKind::ObjectLit(props) => {
                for p in props {
                    match p {
                        ObjLitProp::Property(p) => self.utp_expr(&p.value),
                        ObjLitProp::Shorthand(_, _) => {}
                        ObjLitProp::ShorthandDefault(_, init, _) => self.utp_expr(init),
                        ObjLitProp::Spread(e, _) => self.utp_expr(e),
                        ObjLitProp::Method(m) => {
                            self.report_unused_type_params(m.type_params.as_deref(), |name| {
                                signature_refs(
                                    name,
                                    m.type_params.as_deref(),
                                    &m.params,
                                    m.return_type.as_ref(),
                                    Some(&m.body),
                                )
                            });
                            self.utp_signature(
                                m.type_params.as_deref(),
                                &m.params,
                                m.return_type.as_ref(),
                                false,
                            );
                            self.utp_stmts(&m.body);
                        }
                        ObjLitProp::Get(a) | ObjLitProp::Set(a) => {
                            self.utp_signature(None, &a.params, a.return_type.as_ref(), false);
                            self.utp_stmts(&a.body);
                        }
                    }
                }
            }
            ExprKind::Member(m) => self.utp_expr(&m.object),
            ExprKind::ElemAccess(e) => {
                self.utp_expr(&e.object);
                self.utp_expr(&e.index);
            }
            ExprKind::Cond(c) => {
                self.utp_expr(&c.test);
                self.utp_expr(&c.consequent);
                self.utp_expr(&c.alternate);
            }
            ExprKind::Binary(b) => {
                self.utp_expr(&b.left);
                self.utp_expr(&b.right);
            }
            ExprKind::Unary(u) => self.utp_expr(&u.argument),
            ExprKind::Update(u) => self.utp_expr(&u.argument),
            ExprKind::Paren(inner)
            | ExprKind::NonNull(inner)
            | ExprKind::Spread(inner)
            | ExprKind::Await(inner)
            | ExprKind::Delete(inner)
            | ExprKind::Typeof(inner)
            | ExprKind::Void(inner) => self.utp_expr(inner),
            ExprKind::TypeAssertion(t) => {
                self.utp_type_node(&t.type_node);
                self.utp_expr(&t.expr);
            }
            ExprKind::As(a) => {
                self.utp_type_node(&a.type_node);
                self.utp_expr(&a.expr);
            }
            ExprKind::Satisfies(s) => {
                self.utp_type_node(&s.type_node);
                self.utp_expr(&s.expr);
            }
            ExprKind::Instantiation(i) => {
                self.utp_expr(&i.expr);
                for a in &i.type_args {
                    self.utp_type_node(a);
                }
            }
            ExprKind::Yield(_, Some(e)) => self.utp_expr(e),
            ExprKind::Assign(a) => {
                self.utp_expr(&a.left);
                self.utp_expr(&a.right);
            }
            ExprKind::Comma(exprs) => {
                for e in exprs {
                    self.utp_expr(e);
                }
            }
            _ => {}
        }
    }

    /// Span from `<` through `>` of a type parameter list.
    fn type_param_list_span(&self, tps: &[TypeParam]) -> Span {
        let first = tps[0].span;
        let last = tps[tps.len() - 1].span;
        let start = first.start.saturating_sub(1);
        let end = self
            .current_source
            .as_deref()
            .and_then(|s| {
                let tail = s.get(last.end as usize..)?;
                let skipped = tail.len() - tail.trim_start().len();
                tail[skipped..]
                    .starts_with('>')
                    .then_some(last.end + skipped as u32 + 1)
            })
            .unwrap_or(last.end);
        Span::new(start, end)
    }

    fn report_unused_type_params(
        &mut self,
        tps: Option<&[TypeParam]>,
        used: impl Fn(&str) -> bool,
    ) {
        if self.utp_renamings_only {
            return;
        }
        let Some(tps) = tps else {
            return;
        };
        if tps.is_empty() {
            return;
        }
        let unused: Vec<bool> = tps
            .iter()
            .map(|tp| !tp.name.starts_with('_') && !used(&tp.name))
            .collect();
        if unused.iter().all(|u| *u) {
            let span = self.type_param_list_span(tps);
            if tps.len() == 1 {
                self.diagnostics
                    .push(error_unused_local(&tps[0].name, span));
            } else {
                self.diagnostics
                    .push(error_all_type_parameters_unused(span));
            }
            return;
        }
        for (tp, is_unused) in tps.iter().zip(unused) {
            if is_unused {
                self.diagnostics.push(error_unused_local(&tp.name, tp.span));
            }
        }
    }
}
