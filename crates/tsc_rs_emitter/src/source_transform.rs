//! Source-copy decision logic and transformation detection helpers.

use super::*;

// ---------------------------------------------------------------------------
// Transformation detection
// ---------------------------------------------------------------------------

/// Returns true if an expression contains a member access referencing a const enum.
pub(crate) fn expr_has_const_enum_ref(
    expr: &Expr,
    cv: &HashMap<(String, String), ConstEnumValue>,
) -> bool {
    match &expr.kind {
        ExprKind::Member(mem) => {
            if let Some(obj) = expr_member_path_for_const_enum(&mem.object) {
                if cv.contains_key(&(obj, mem.property.to_string())) {
                    return true;
                }
            }
            expr_has_const_enum_ref(&mem.object, cv)
        }
        ExprKind::Binary(bin) => {
            expr_has_const_enum_ref(&bin.left, cv) || expr_has_const_enum_ref(&bin.right, cv)
        }
        ExprKind::Unary(un) => expr_has_const_enum_ref(&un.argument, cv),
        ExprKind::Update(up) => expr_has_const_enum_ref(&up.argument, cv),
        ExprKind::Cond(c) => {
            expr_has_const_enum_ref(&c.test, cv)
                || expr_has_const_enum_ref(&c.consequent, cv)
                || expr_has_const_enum_ref(&c.alternate, cv)
        }
        ExprKind::Call(call) => {
            expr_has_const_enum_ref(&call.callee, cv)
                || call.args.iter().any(|a| expr_has_const_enum_ref(a, cv))
        }
        ExprKind::New(n) => {
            expr_has_const_enum_ref(&n.callee, cv)
                || n.args
                    .as_ref()
                    .is_some_and(|a| a.iter().any(|x| expr_has_const_enum_ref(x, cv)))
        }
        ExprKind::Assign(a) => {
            expr_has_const_enum_ref(&a.left, cv) || expr_has_const_enum_ref(&a.right, cv)
        }
        ExprKind::Paren(i)
        | ExprKind::Spread(i)
        | ExprKind::Await(i)
        | ExprKind::Delete(i)
        | ExprKind::Typeof(i)
        | ExprKind::Void(i)
        | ExprKind::NonNull(i) => expr_has_const_enum_ref(i, cv),
        ExprKind::As(a) => expr_has_const_enum_ref(&a.expr, cv),
        ExprKind::Satisfies(s) => expr_has_const_enum_ref(&s.expr, cv),
        ExprKind::TypeAssertion(ta) => expr_has_const_enum_ref(&ta.expr, cv),
        ExprKind::Instantiation(inst) => expr_has_const_enum_ref(&inst.expr, cv),
        ExprKind::ElemAccess(ea) => {
            if let Some(obj) = expr_member_path_for_const_enum(&ea.object) {
                let member = match &ea.index.kind {
                    ExprKind::StrLit(s) | ExprKind::NoSubstTemplate(s) => Some(s.to_string()),
                    ExprKind::NumLit(n) => Some(n.to_string()),
                    _ => None,
                };
                if let Some(member) = member {
                    if cv.contains_key(&(obj, member)) {
                        return true;
                    }
                }
            }
            expr_has_const_enum_ref(&ea.object, cv) || expr_has_const_enum_ref(&ea.index, cv)
        }
        ExprKind::ArrayLit(elems) => elems
            .iter()
            .any(|e| e.as_ref().is_some_and(|e| expr_has_const_enum_ref(e, cv))),
        ExprKind::ObjectLit(props) => props.iter().any(|p| match p {
            ObjLitProp::Property(prop) => {
                if let PropName::Computed(key_expr, _) = &prop.key {
                    if expr_has_const_enum_ref(key_expr, cv) {
                        return true;
                    }
                }
                expr_has_const_enum_ref(&prop.value, cv)
            }
            ObjLitProp::Spread(e, _) => expr_has_const_enum_ref(e, cv),
            _ => false,
        }),
        ExprKind::Template(tpl) => tpl.exprs.iter().any(|e| expr_has_const_enum_ref(e, cv)),
        ExprKind::Comma(es) => es.iter().any(|e| expr_has_const_enum_ref(e, cv)),
        ExprKind::Yield(_, a) => a.as_ref().is_some_and(|e| expr_has_const_enum_ref(e, cv)),
        ExprKind::Arrow(ar) => match &ar.body {
            ArrowBody::Expr(e) => expr_has_const_enum_ref(e, cv),
            ArrowBody::Block(ss) => ss.iter().any(|s| stmt_has_const_enum_ref(s, cv)),
        },
        ExprKind::FnExpr(f) => f
            .body
            .as_ref()
            .is_some_and(|b| b.iter().any(|s| stmt_has_const_enum_ref(s, cv))),
        _ => false,
    }
}

pub(crate) fn expr_member_path_for_const_enum(expr: &Expr) -> Option<String> {
    match &expr.kind {
        ExprKind::Ident(name) => Some(name.to_string()),
        ExprKind::Member(mem) => {
            let mut base = expr_member_path_for_const_enum(&mem.object)?;
            base.push('.');
            base.push_str(&mem.property);
            Some(base)
        }
        ExprKind::Paren(inner) | ExprKind::NonNull(inner) => expr_member_path_for_const_enum(inner),
        ExprKind::TypeAssertion(ta) => expr_member_path_for_const_enum(&ta.expr),
        ExprKind::As(a) => expr_member_path_for_const_enum(&a.expr),
        ExprKind::Satisfies(s) => expr_member_path_for_const_enum(&s.expr),
        ExprKind::Instantiation(inst) => expr_member_path_for_const_enum(&inst.expr),
        _ => None,
    }
}

/// Returns true if a statement contains a const enum member reference.
pub(crate) fn stmt_has_const_enum_ref(
    stmt: &Stmt,
    cv: &HashMap<(String, String), ConstEnumValue>,
) -> bool {
    match &stmt.kind {
        StmtKind::Expr(e) | StmtKind::Throw(e) | StmtKind::ExportAssign(e) => {
            expr_has_const_enum_ref(e, cv)
        }
        StmtKind::Var(v) => v.declarations.iter().any(|d| {
            d.init
                .as_ref()
                .is_some_and(|e| expr_has_const_enum_ref(e, cv))
        }),
        StmtKind::Return(Some(e)) => expr_has_const_enum_ref(e, cv),
        StmtKind::If(i) => {
            expr_has_const_enum_ref(&i.test, cv)
                || stmt_has_const_enum_ref(&i.consequent, cv)
                || i.alternate
                    .as_ref()
                    .is_some_and(|s| stmt_has_const_enum_ref(s, cv))
        }
        StmtKind::While(w) => {
            expr_has_const_enum_ref(&w.test, cv) || stmt_has_const_enum_ref(&w.body, cv)
        }
        StmtKind::DoWhile(dw) => {
            expr_has_const_enum_ref(&dw.test, cv) || stmt_has_const_enum_ref(&dw.body, cv)
        }
        StmtKind::For(f) => {
            f.init.as_ref().is_some_and(|init| match init {
                ForInit::Expr(e) => expr_has_const_enum_ref(e, cv),
                ForInit::Var(v) => v.declarations.iter().any(|d| {
                    d.init
                        .as_ref()
                        .is_some_and(|e| expr_has_const_enum_ref(e, cv))
                }),
            }) || f
                .test
                .as_ref()
                .is_some_and(|e| expr_has_const_enum_ref(e, cv))
                || f.update
                    .as_ref()
                    .is_some_and(|e| expr_has_const_enum_ref(e, cv))
                || stmt_has_const_enum_ref(&f.body, cv)
        }
        StmtKind::ForIn(fi) => {
            expr_has_const_enum_ref(&fi.right, cv) || stmt_has_const_enum_ref(&fi.body, cv)
        }
        StmtKind::ForOf(fo) => {
            expr_has_const_enum_ref(&fo.right, cv) || stmt_has_const_enum_ref(&fo.body, cv)
        }
        StmtKind::Switch(sw) => {
            expr_has_const_enum_ref(&sw.discriminant, cv)
                || sw.cases.iter().any(|c| {
                    c.test
                        .as_ref()
                        .is_some_and(|e| expr_has_const_enum_ref(e, cv))
                        || c.consequent.iter().any(|s| stmt_has_const_enum_ref(s, cv))
                })
        }
        StmtKind::Block(ss) => ss.iter().any(|s| stmt_has_const_enum_ref(s, cv)),
        StmtKind::Labeled(l) => stmt_has_const_enum_ref(&l.body, cv),
        StmtKind::Try(t) => {
            t.block.iter().any(|s| stmt_has_const_enum_ref(s, cv))
                || t.handler
                    .as_ref()
                    .is_some_and(|h| h.body.iter().any(|s| stmt_has_const_enum_ref(s, cv)))
                || t.finalizer
                    .as_ref()
                    .is_some_and(|f| f.iter().any(|s| stmt_has_const_enum_ref(s, cv)))
        }
        StmtKind::With(w) => {
            expr_has_const_enum_ref(&w.object, cv) || stmt_has_const_enum_ref(&w.body, cv)
        }
        StmtKind::Export(ed) => {
            if let ExportDeclKind::Decl(ref inner) = ed.kind {
                stmt_has_const_enum_ref(inner, cv)
            } else {
                false
            }
        }
        _ => false,
    }
}

pub(crate) fn expr_has_live_export_write(expr: &Expr, names: &HashSet<AstString>) -> bool {
    match &expr.kind {
        ExprKind::Assign(assign) => {
            matches!(&assign.left.kind, ExprKind::Ident(name) if names.contains(name.as_str()))
                || expr_has_live_export_write(&assign.left, names)
                || expr_has_live_export_write(&assign.right, names)
        }
        ExprKind::Update(update) => {
            matches!(&update.argument.kind, ExprKind::Ident(name) if names.contains(name.as_str()))
                || expr_has_live_export_write(&update.argument, names)
        }
        ExprKind::Binary(bin) => {
            expr_has_live_export_write(&bin.left, names)
                || expr_has_live_export_write(&bin.right, names)
        }
        ExprKind::Unary(un) => expr_has_live_export_write(&un.argument, names),
        ExprKind::Cond(c) => {
            expr_has_live_export_write(&c.test, names)
                || expr_has_live_export_write(&c.consequent, names)
                || expr_has_live_export_write(&c.alternate, names)
        }
        ExprKind::Call(call) => {
            expr_has_live_export_write(&call.callee, names)
                || call
                    .args
                    .iter()
                    .any(|arg| expr_has_live_export_write(arg, names))
        }
        ExprKind::New(new_expr) => {
            expr_has_live_export_write(&new_expr.callee, names)
                || new_expr.args.as_ref().is_some_and(|args| {
                    args.iter()
                        .any(|arg| expr_has_live_export_write(arg, names))
                })
        }
        ExprKind::Member(mem) => expr_has_live_export_write(&mem.object, names),
        ExprKind::ElemAccess(ea) => {
            expr_has_live_export_write(&ea.object, names)
                || expr_has_live_export_write(&ea.index, names)
        }
        ExprKind::ArrayLit(elements) => elements.iter().any(|e| {
            e.as_ref()
                .is_some_and(|expr| expr_has_live_export_write(expr, names))
        }),
        ExprKind::ObjectLit(props) => props.iter().any(|prop| match prop {
            ObjLitProp::Property(p) => expr_has_live_export_write(&p.value, names),
            ObjLitProp::Spread(e, _) => expr_has_live_export_write(e, names),
            ObjLitProp::Method(m) => m
                .body
                .iter()
                .any(|stmt| stmt_has_live_export_write(stmt, names)),
            ObjLitProp::Get(a) | ObjLitProp::Set(a) => a
                .body
                .iter()
                .any(|stmt| stmt_has_live_export_write(stmt, names)),
            _ => false,
        }),
        ExprKind::Template(tpl) => tpl
            .exprs
            .iter()
            .any(|expr| expr_has_live_export_write(expr, names)),
        ExprKind::Comma(exprs) => exprs
            .iter()
            .any(|expr| expr_has_live_export_write(expr, names)),
        ExprKind::Yield(_, arg) => arg
            .as_ref()
            .is_some_and(|expr| expr_has_live_export_write(expr, names)),
        ExprKind::Arrow(ar) => match &ar.body {
            ArrowBody::Expr(e) => expr_has_live_export_write(e, names),
            ArrowBody::Block(stmts) => stmts
                .iter()
                .any(|stmt| stmt_has_live_export_write(stmt, names)),
        },
        ExprKind::FnExpr(f) => f.body.as_ref().is_some_and(|body| {
            body.iter()
                .any(|stmt| stmt_has_live_export_write(stmt, names))
        }),
        ExprKind::Paren(inner)
        | ExprKind::Spread(inner)
        | ExprKind::Await(inner)
        | ExprKind::Delete(inner)
        | ExprKind::Typeof(inner)
        | ExprKind::Void(inner)
        | ExprKind::NonNull(inner) => expr_has_live_export_write(inner, names),
        ExprKind::As(a) => expr_has_live_export_write(&a.expr, names),
        ExprKind::Satisfies(s) => expr_has_live_export_write(&s.expr, names),
        ExprKind::TypeAssertion(ta) => expr_has_live_export_write(&ta.expr, names),
        ExprKind::Instantiation(inst) => expr_has_live_export_write(&inst.expr, names),
        _ => false,
    }
}

pub(crate) fn stmt_has_live_export_write(stmt: &Stmt, names: &HashSet<AstString>) -> bool {
    match &stmt.kind {
        StmtKind::Expr(expr) | StmtKind::Throw(expr) | StmtKind::ExportAssign(expr) => {
            expr_has_live_export_write(expr, names)
        }
        StmtKind::Return(Some(expr)) => expr_has_live_export_write(expr, names),
        StmtKind::Var(var_stmt) => var_stmt.declarations.iter().any(|decl| {
            decl.init
                .as_ref()
                .is_some_and(|expr| expr_has_live_export_write(expr, names))
        }),
        StmtKind::If(if_stmt) => {
            expr_has_live_export_write(&if_stmt.test, names)
                || stmt_has_live_export_write(&if_stmt.consequent, names)
                || if_stmt
                    .alternate
                    .as_ref()
                    .is_some_and(|alt| stmt_has_live_export_write(alt, names))
        }
        StmtKind::While(wh) => {
            expr_has_live_export_write(&wh.test, names)
                || stmt_has_live_export_write(&wh.body, names)
        }
        StmtKind::DoWhile(dw) => {
            stmt_has_live_export_write(&dw.body, names)
                || expr_has_live_export_write(&dw.test, names)
        }
        StmtKind::For(for_stmt) => {
            for_stmt.init.as_ref().is_some_and(|init| match init {
                ForInit::Expr(expr) => expr_has_live_export_write(expr, names),
                ForInit::Var(var_stmt) => var_stmt.declarations.iter().any(|decl| {
                    decl.init
                        .as_ref()
                        .is_some_and(|expr| expr_has_live_export_write(expr, names))
                }),
            }) || for_stmt
                .test
                .as_ref()
                .is_some_and(|expr| expr_has_live_export_write(expr, names))
                || for_stmt
                    .update
                    .as_ref()
                    .is_some_and(|expr| expr_has_live_export_write(expr, names))
                || stmt_has_live_export_write(&for_stmt.body, names)
        }
        StmtKind::ForIn(fi) => {
            expr_has_live_export_write(&fi.right, names)
                || stmt_has_live_export_write(&fi.body, names)
        }
        StmtKind::ForOf(fo) => {
            expr_has_live_export_write(&fo.right, names)
                || stmt_has_live_export_write(&fo.body, names)
        }
        StmtKind::Switch(sw) => {
            expr_has_live_export_write(&sw.discriminant, names)
                || sw.cases.iter().any(|case| {
                    case.test
                        .as_ref()
                        .is_some_and(|expr| expr_has_live_export_write(expr, names))
                        || case
                            .consequent
                            .iter()
                            .any(|stmt| stmt_has_live_export_write(stmt, names))
                })
        }
        StmtKind::Block(stmts) => stmts
            .iter()
            .any(|stmt| stmt_has_live_export_write(stmt, names)),
        StmtKind::Labeled(l) => stmt_has_live_export_write(&l.body, names),
        StmtKind::Try(t) => {
            t.block
                .iter()
                .any(|stmt| stmt_has_live_export_write(stmt, names))
                || t.handler.as_ref().is_some_and(|h| {
                    h.body
                        .iter()
                        .any(|stmt| stmt_has_live_export_write(stmt, names))
                })
                || t.finalizer
                    .as_ref()
                    .is_some_and(|f| f.iter().any(|stmt| stmt_has_live_export_write(stmt, names)))
        }
        StmtKind::With(w) => {
            expr_has_live_export_write(&w.object, names)
                || stmt_has_live_export_write(&w.body, names)
        }
        StmtKind::Export(ed) => {
            if let ExportDeclKind::Decl(inner) = &ed.kind {
                stmt_has_live_export_write(inner, names)
            } else {
                false
            }
        }
        _ => false,
    }
}

/// Parser-recovery placeholder expression (`<error>`).
pub(crate) fn expr_is_error_placeholder(expr: &Expr) -> bool {
    match &expr.kind {
        ExprKind::Ident(name) if name == "<error>" => true,
        // TypeAssertion wrapping an error node is also an error placeholder.
        ExprKind::TypeAssertion(ta) => expr_is_error_placeholder(&ta.expr),
        ExprKind::As(a) => expr_is_error_placeholder(&a.expr),
        ExprKind::Satisfies(s) => expr_is_error_placeholder(&s.expr),
        ExprKind::NonNull(inner) | ExprKind::Paren(inner) => expr_is_error_placeholder(inner),
        ExprKind::Instantiation(inst) => expr_is_error_placeholder(&inst.expr),
        _ => false,
    }
}

/// Returns true if an expression contains a reference to a CJS import binding.
pub(crate) fn expr_has_cjs_import_ref(
    expr: &Expr,
    map: &HashMap<AstString, (AstString, AstString)>,
) -> bool {
    match &expr.kind {
        ExprKind::Ident(name) => map.contains_key(name.as_str()),
        ExprKind::Member(mem) => expr_has_cjs_import_ref(&mem.object, map),
        ExprKind::Binary(bin) => {
            expr_has_cjs_import_ref(&bin.left, map) || expr_has_cjs_import_ref(&bin.right, map)
        }
        ExprKind::Unary(un) => expr_has_cjs_import_ref(&un.argument, map),
        ExprKind::Update(up) => expr_has_cjs_import_ref(&up.argument, map),
        ExprKind::Cond(c) => {
            expr_has_cjs_import_ref(&c.test, map)
                || expr_has_cjs_import_ref(&c.consequent, map)
                || expr_has_cjs_import_ref(&c.alternate, map)
        }
        ExprKind::Call(call) => {
            expr_has_cjs_import_ref(&call.callee, map)
                || call.args.iter().any(|a| expr_has_cjs_import_ref(a, map))
        }
        ExprKind::New(n) => {
            expr_has_cjs_import_ref(&n.callee, map)
                || n.args
                    .as_ref()
                    .is_some_and(|a| a.iter().any(|x| expr_has_cjs_import_ref(x, map)))
        }
        ExprKind::Assign(a) => {
            expr_has_cjs_import_ref(&a.left, map) || expr_has_cjs_import_ref(&a.right, map)
        }
        ExprKind::Paren(i)
        | ExprKind::Spread(i)
        | ExprKind::Await(i)
        | ExprKind::Delete(i)
        | ExprKind::Typeof(i)
        | ExprKind::Void(i)
        | ExprKind::NonNull(i) => expr_has_cjs_import_ref(i, map),
        ExprKind::As(a) => expr_has_cjs_import_ref(&a.expr, map),
        ExprKind::Satisfies(s) => expr_has_cjs_import_ref(&s.expr, map),
        ExprKind::TypeAssertion(ta) => expr_has_cjs_import_ref(&ta.expr, map),
        ExprKind::Instantiation(inst) => expr_has_cjs_import_ref(&inst.expr, map),
        ExprKind::ElemAccess(ea) => {
            expr_has_cjs_import_ref(&ea.object, map) || expr_has_cjs_import_ref(&ea.index, map)
        }
        ExprKind::ArrayLit(elems) => elems
            .iter()
            .any(|e| e.as_ref().is_some_and(|e| expr_has_cjs_import_ref(e, map))),
        ExprKind::ObjectLit(props) => props.iter().any(|p| {
            let key = match p {
                ObjLitProp::Property(prop) => Some(&prop.key),
                ObjLitProp::Method(m) => Some(&m.name),
                ObjLitProp::Get(a) => Some(&a.name),
                ObjLitProp::Set(a) => Some(&a.name),
                _ => None,
            };
            let has_computed_ref = key.is_some_and(|k| {
                matches!(k, PropName::Computed(e, _) if expr_has_cjs_import_ref(e, map))
            });
            has_computed_ref
                || matches!(p, ObjLitProp::Property(prop) if expr_has_cjs_import_ref(&prop.value, map))
                || matches!(p, ObjLitProp::Shorthand(name, _) if map.contains_key(name.as_str()))
                || matches!(p, ObjLitProp::Spread(e, _) if expr_has_cjs_import_ref(e, map))
        }),
        ExprKind::Template(tpl) => tpl.exprs.iter().any(|e| expr_has_cjs_import_ref(e, map)),
        ExprKind::TaggedTemplate(tt) => {
            expr_has_cjs_import_ref(&tt.tag, map)
                || tt.quasi.exprs.iter().any(|e| expr_has_cjs_import_ref(e, map))
        }
        ExprKind::Comma(es) => es.iter().any(|e| expr_has_cjs_import_ref(e, map)),
        ExprKind::Yield(_, a) => a.as_ref().is_some_and(|e| expr_has_cjs_import_ref(e, map)),
        ExprKind::Arrow(ar) => match &ar.body {
            ArrowBody::Expr(e) => expr_has_cjs_import_ref(e, map),
            ArrowBody::Block(ss) => ss.iter().any(|s| stmt_has_cjs_import_ref(s, map)),
        },
        ExprKind::FnExpr(f) => f
            .body
            .as_ref()
            .is_some_and(|b| b.iter().any(|s| stmt_has_cjs_import_ref(s, map))),
        ExprKind::JsxElement(jsx) => {
            expr_has_cjs_import_ref(&jsx.name, map)
                || jsx_attrs_have_cjs_ref(&jsx.attributes, map)
                || jsx_children_have_cjs_ref(&jsx.children, map)
        }
        ExprKind::JsxSelfClosing(jsx) => {
            expr_has_cjs_import_ref(&jsx.name, map)
                || jsx_attrs_have_cjs_ref(&jsx.attributes, map)
        }
        ExprKind::JsxFragment(frag) => jsx_children_have_cjs_ref(&frag.children, map),
        ExprKind::ClassExpr(c) => c.members.iter().any(|m| match &m.kind {
            ClassMemberKind::Property(p) => {
                matches!(&p.name, PropName::Computed(e, _) if expr_has_cjs_import_ref(e, map))
                    || p.initializer
                        .as_ref()
                        .is_some_and(|e| expr_has_cjs_import_ref(e, map))
            }
            ClassMemberKind::Method(m) => {
                matches!(&m.name, PropName::Computed(e, _) if expr_has_cjs_import_ref(e, map))
            }
            _ => false,
        }),
        _ => false,
    }
}

pub(crate) fn jsx_attrs_have_cjs_ref(
    attrs: &[JsxAttribute],
    map: &HashMap<AstString, (AstString, AstString)>,
) -> bool {
    attrs.iter().any(|a| match a {
        JsxAttribute::Normal { value: Some(v), .. } => expr_has_cjs_import_ref(v, map),
        JsxAttribute::Spread(e, _) => expr_has_cjs_import_ref(e, map),
        _ => false,
    })
}

pub(crate) fn jsx_children_have_cjs_ref(
    children: &[JsxChild],
    map: &HashMap<AstString, (AstString, AstString)>,
) -> bool {
    children.iter().any(|c| match c {
        JsxChild::Element(e) => expr_has_cjs_import_ref(e, map),
        JsxChild::Expression(Some(e), _) => expr_has_cjs_import_ref(e, map),
        _ => false,
    })
}

/// Returns true if an expression contains a reference to a CJS exported name.
pub(crate) fn expr_has_cjs_export_ref(expr: &Expr, names: &HashSet<AstString>) -> bool {
    match &expr.kind {
        ExprKind::Ident(name) => names.contains(name.as_str()),
        ExprKind::Member(mem) => expr_has_cjs_export_ref(&mem.object, names),
        ExprKind::Binary(bin) => {
            expr_has_cjs_export_ref(&bin.left, names) || expr_has_cjs_export_ref(&bin.right, names)
        }
        ExprKind::Unary(un) => expr_has_cjs_export_ref(&un.argument, names),
        ExprKind::Update(up) => expr_has_cjs_export_ref(&up.argument, names),
        ExprKind::Cond(c) => {
            expr_has_cjs_export_ref(&c.test, names)
                || expr_has_cjs_export_ref(&c.consequent, names)
                || expr_has_cjs_export_ref(&c.alternate, names)
        }
        ExprKind::Call(call) => {
            expr_has_cjs_export_ref(&call.callee, names)
                || call.args.iter().any(|a| expr_has_cjs_export_ref(a, names))
        }
        ExprKind::New(n) => {
            expr_has_cjs_export_ref(&n.callee, names)
                || n.args
                    .as_ref()
                    .is_some_and(|a| a.iter().any(|x| expr_has_cjs_export_ref(x, names)))
        }
        ExprKind::Assign(a) => {
            expr_has_cjs_export_ref(&a.left, names) || expr_has_cjs_export_ref(&a.right, names)
        }
        ExprKind::Paren(i)
        | ExprKind::Spread(i)
        | ExprKind::Await(i)
        | ExprKind::Delete(i)
        | ExprKind::Typeof(i)
        | ExprKind::Void(i)
        | ExprKind::NonNull(i) => expr_has_cjs_export_ref(i, names),
        ExprKind::As(a) => expr_has_cjs_export_ref(&a.expr, names),
        ExprKind::Satisfies(s) => expr_has_cjs_export_ref(&s.expr, names),
        ExprKind::TypeAssertion(ta) => expr_has_cjs_export_ref(&ta.expr, names),
        ExprKind::Instantiation(inst) => expr_has_cjs_export_ref(&inst.expr, names),
        ExprKind::ElemAccess(ea) => {
            expr_has_cjs_export_ref(&ea.object, names) || expr_has_cjs_export_ref(&ea.index, names)
        }
        ExprKind::ArrayLit(elems) => elems.iter().any(|e| {
            e.as_ref()
                .is_some_and(|e| expr_has_cjs_export_ref(e, names))
        }),
        ExprKind::ObjectLit(props) => props.iter().any(|p| match p {
            ObjLitProp::Property(prop) => {
                (matches!(&prop.key, PropName::Computed(e, _) if expr_has_cjs_export_ref(e, names)))
                    || expr_has_cjs_export_ref(&prop.value, names)
            }
            ObjLitProp::Shorthand(name, _) => names.contains(name.as_str()),
            ObjLitProp::Spread(e, _) => expr_has_cjs_export_ref(e, names),
            ObjLitProp::Method(m) => {
                matches!(&m.name, PropName::Computed(e, _) if expr_has_cjs_export_ref(e, names))
            }
            ObjLitProp::Get(a) | ObjLitProp::Set(a) => {
                matches!(&a.name, PropName::Computed(e, _) if expr_has_cjs_export_ref(e, names))
            }
            _ => false,
        }),
        ExprKind::Template(tpl) => tpl.exprs.iter().any(|e| expr_has_cjs_export_ref(e, names)),
        ExprKind::Comma(es) => es.iter().any(|e| expr_has_cjs_export_ref(e, names)),
        ExprKind::Yield(_, a) => a
            .as_ref()
            .is_some_and(|e| expr_has_cjs_export_ref(e, names)),
        ExprKind::Arrow(ar) => match &ar.body {
            ArrowBody::Expr(e) => expr_has_cjs_export_ref(e, names),
            _ => false,
        },
        _ => false,
    }
}

/// Returns true if a statement contains a reference to a CJS import binding.
pub(crate) fn stmt_has_cjs_import_ref(
    stmt: &Stmt,
    map: &HashMap<AstString, (AstString, AstString)>,
) -> bool {
    match &stmt.kind {
        StmtKind::Expr(e) | StmtKind::Throw(e) | StmtKind::ExportAssign(e) => {
            expr_has_cjs_import_ref(e, map)
        }
        StmtKind::Var(v) => v.declarations.iter().any(|d| {
            d.init
                .as_ref()
                .is_some_and(|e| expr_has_cjs_import_ref(e, map))
        }),
        StmtKind::Return(Some(e)) => expr_has_cjs_import_ref(e, map),
        StmtKind::If(i) => {
            expr_has_cjs_import_ref(&i.test, map)
                || stmt_has_cjs_import_ref(&i.consequent, map)
                || i.alternate
                    .as_ref()
                    .is_some_and(|s| stmt_has_cjs_import_ref(s, map))
        }
        StmtKind::While(w) => {
            expr_has_cjs_import_ref(&w.test, map) || stmt_has_cjs_import_ref(&w.body, map)
        }
        StmtKind::DoWhile(dw) => {
            expr_has_cjs_import_ref(&dw.test, map) || stmt_has_cjs_import_ref(&dw.body, map)
        }
        StmtKind::For(f) => {
            f.init.as_ref().is_some_and(|init| match init {
                ForInit::Expr(e) => expr_has_cjs_import_ref(e, map),
                ForInit::Var(v) => v.declarations.iter().any(|d| {
                    d.init
                        .as_ref()
                        .is_some_and(|e| expr_has_cjs_import_ref(e, map))
                }),
            }) || f
                .test
                .as_ref()
                .is_some_and(|e| expr_has_cjs_import_ref(e, map))
                || f.update
                    .as_ref()
                    .is_some_and(|e| expr_has_cjs_import_ref(e, map))
                || stmt_has_cjs_import_ref(&f.body, map)
        }
        StmtKind::ForIn(fi) => {
            expr_has_cjs_import_ref(&fi.right, map) || stmt_has_cjs_import_ref(&fi.body, map)
        }
        StmtKind::ForOf(fo) => {
            expr_has_cjs_import_ref(&fo.right, map) || stmt_has_cjs_import_ref(&fo.body, map)
        }
        StmtKind::Switch(sw) => {
            expr_has_cjs_import_ref(&sw.discriminant, map)
                || sw.cases.iter().any(|c| {
                    c.test
                        .as_ref()
                        .is_some_and(|e| expr_has_cjs_import_ref(e, map))
                        || c.consequent.iter().any(|s| stmt_has_cjs_import_ref(s, map))
                })
        }
        StmtKind::Block(ss) => ss.iter().any(|s| stmt_has_cjs_import_ref(s, map)),
        StmtKind::Labeled(l) => stmt_has_cjs_import_ref(&l.body, map),
        StmtKind::Try(t) => {
            t.block.iter().any(|s| stmt_has_cjs_import_ref(s, map))
                || t.handler
                    .as_ref()
                    .is_some_and(|h| h.body.iter().any(|s| stmt_has_cjs_import_ref(s, map)))
                || t.finalizer
                    .as_ref()
                    .is_some_and(|f| f.iter().any(|s| stmt_has_cjs_import_ref(s, map)))
        }
        StmtKind::With(w) => {
            expr_has_cjs_import_ref(&w.object, map) || stmt_has_cjs_import_ref(&w.body, map)
        }
        StmtKind::Export(ed) => {
            if let ExportDeclKind::Decl(ref inner) = ed.kind {
                stmt_has_cjs_import_ref(inner, map)
            } else {
                false
            }
        }
        _ => false,
    }
}

/// Returns true if an expression contains `import.meta.dirname`,
/// `import.meta.filename`, or `import.meta.url` — accessors that have no
/// runtime form in CommonJS and must be rewritten by the structured emitter.
pub(crate) fn expr_has_cjs_import_meta_rewrite(expr: &Expr) -> bool {
    if let ExprKind::Member(mem) = &expr.kind {
        if !mem.optional {
            if let ExprKind::MetaProp(mp) = &mem.object.kind {
                if mp.meta.as_str() == "import" && mp.property.as_str() == "meta" {
                    if matches!(mem.property.as_str(), "dirname" | "filename" | "url") {
                        return true;
                    }
                }
            }
        }
    }
    match &expr.kind {
        ExprKind::Member(mem) => expr_has_cjs_import_meta_rewrite(&mem.object),
        ExprKind::ElemAccess(ea) => {
            expr_has_cjs_import_meta_rewrite(&ea.object)
                || expr_has_cjs_import_meta_rewrite(&ea.index)
        }
        ExprKind::Binary(b) => {
            expr_has_cjs_import_meta_rewrite(&b.left) || expr_has_cjs_import_meta_rewrite(&b.right)
        }
        ExprKind::Unary(u) => expr_has_cjs_import_meta_rewrite(&u.argument),
        ExprKind::Update(u) => expr_has_cjs_import_meta_rewrite(&u.argument),
        ExprKind::Cond(c) => {
            expr_has_cjs_import_meta_rewrite(&c.test)
                || expr_has_cjs_import_meta_rewrite(&c.consequent)
                || expr_has_cjs_import_meta_rewrite(&c.alternate)
        }
        ExprKind::Call(c) => {
            expr_has_cjs_import_meta_rewrite(&c.callee)
                || c.args.iter().any(|a| expr_has_cjs_import_meta_rewrite(a))
        }
        ExprKind::New(n) => {
            expr_has_cjs_import_meta_rewrite(&n.callee)
                || n.args
                    .as_ref()
                    .is_some_and(|a| a.iter().any(|x| expr_has_cjs_import_meta_rewrite(x)))
        }
        ExprKind::Paren(p) => expr_has_cjs_import_meta_rewrite(p),
        ExprKind::As(a) => expr_has_cjs_import_meta_rewrite(&a.expr),
        ExprKind::Satisfies(s) => expr_has_cjs_import_meta_rewrite(&s.expr),
        ExprKind::TypeAssertion(t) => expr_has_cjs_import_meta_rewrite(&t.expr),
        ExprKind::NonNull(e) => expr_has_cjs_import_meta_rewrite(e),
        ExprKind::Instantiation(i) => expr_has_cjs_import_meta_rewrite(&i.expr),
        ExprKind::Spread(e) => expr_has_cjs_import_meta_rewrite(e),
        ExprKind::Await(e) => expr_has_cjs_import_meta_rewrite(e),
        ExprKind::Delete(e) => expr_has_cjs_import_meta_rewrite(e),
        ExprKind::Typeof(e) => expr_has_cjs_import_meta_rewrite(e),
        ExprKind::Void(e) => expr_has_cjs_import_meta_rewrite(e),
        ExprKind::Yield(_, Some(e)) => expr_has_cjs_import_meta_rewrite(e),
        ExprKind::Assign(a) => {
            expr_has_cjs_import_meta_rewrite(&a.left) || expr_has_cjs_import_meta_rewrite(&a.right)
        }
        ExprKind::Comma(exprs) => exprs.iter().any(|e| expr_has_cjs_import_meta_rewrite(e)),
        ExprKind::ArrayLit(items) => items
            .iter()
            .flatten()
            .any(|i| expr_has_cjs_import_meta_rewrite(i)),
        _ => false,
    }
}

/// Returns true if a statement contains a top-level `import.meta.{dirname,filename,url}`
/// accessor that must be rewritten under CommonJS emit.
pub(crate) fn stmt_has_cjs_import_meta_rewrite(stmt: &Stmt) -> bool {
    match &stmt.kind {
        StmtKind::Expr(e) | StmtKind::Throw(e) | StmtKind::ExportAssign(e) => {
            expr_has_cjs_import_meta_rewrite(e)
        }
        StmtKind::Var(v) => v.declarations.iter().any(|d| {
            d.init
                .as_ref()
                .is_some_and(|e| expr_has_cjs_import_meta_rewrite(e))
        }),
        StmtKind::Return(Some(e)) => expr_has_cjs_import_meta_rewrite(e),
        StmtKind::If(i) => {
            expr_has_cjs_import_meta_rewrite(&i.test)
                || stmt_has_cjs_import_meta_rewrite(&i.consequent)
                || i.alternate
                    .as_ref()
                    .is_some_and(|s| stmt_has_cjs_import_meta_rewrite(s))
        }
        StmtKind::While(w) => {
            expr_has_cjs_import_meta_rewrite(&w.test) || stmt_has_cjs_import_meta_rewrite(&w.body)
        }
        StmtKind::DoWhile(dw) => {
            expr_has_cjs_import_meta_rewrite(&dw.test) || stmt_has_cjs_import_meta_rewrite(&dw.body)
        }
        StmtKind::For(f) => {
            f.test
                .as_ref()
                .is_some_and(|e| expr_has_cjs_import_meta_rewrite(e))
                || f.update
                    .as_ref()
                    .is_some_and(|e| expr_has_cjs_import_meta_rewrite(e))
                || stmt_has_cjs_import_meta_rewrite(&f.body)
        }
        StmtKind::ForIn(fi) => {
            expr_has_cjs_import_meta_rewrite(&fi.right)
                || stmt_has_cjs_import_meta_rewrite(&fi.body)
        }
        StmtKind::ForOf(fo) => {
            expr_has_cjs_import_meta_rewrite(&fo.right)
                || stmt_has_cjs_import_meta_rewrite(&fo.body)
        }
        StmtKind::Switch(sw) => {
            expr_has_cjs_import_meta_rewrite(&sw.discriminant)
                || sw.cases.iter().any(|c| {
                    c.test
                        .as_ref()
                        .is_some_and(|e| expr_has_cjs_import_meta_rewrite(e))
                        || c.consequent
                            .iter()
                            .any(|s| stmt_has_cjs_import_meta_rewrite(s))
                })
        }
        StmtKind::Block(ss) => ss.iter().any(|s| stmt_has_cjs_import_meta_rewrite(s)),
        StmtKind::Labeled(l) => stmt_has_cjs_import_meta_rewrite(&l.body),
        StmtKind::Try(t) => {
            t.block.iter().any(|s| stmt_has_cjs_import_meta_rewrite(s))
                || t.handler
                    .as_ref()
                    .is_some_and(|h| h.body.iter().any(|s| stmt_has_cjs_import_meta_rewrite(s)))
                || t.finalizer
                    .as_ref()
                    .is_some_and(|f| f.iter().any(|s| stmt_has_cjs_import_meta_rewrite(s)))
        }
        StmtKind::Export(ed) => {
            if let ExportDeclKind::Decl(ref inner) = ed.kind {
                stmt_has_cjs_import_meta_rewrite(inner)
            } else if let ExportDeclKind::Default(e) = &ed.kind {
                expr_has_cjs_import_meta_rewrite(e)
            } else {
                false
            }
        }
        _ => false,
    }
}

/// Returns true if any identifier in the statement matches a namespace export name.
/// Used to force structured emit so identifiers can be qualified (e.g. `b` → `m1.b`).
pub(crate) fn stmt_has_ns_export_ref(stmt: &Stmt, exports: &HashSet<AstString>) -> bool {
    /// Collect parameter names that shadow export names.
    fn param_names_set(params: &[Param], exports: &HashSet<AstString>) -> HashSet<AstString> {
        let mut result = HashSet::new();
        for p in params {
            collect_pat_idents(&p.name, exports, &mut result);
        }
        result
    }
    fn collect_pat_idents(pat: &Pat, exports: &HashSet<AstString>, out: &mut HashSet<AstString>) {
        match &pat.kind {
            PatKind::Ident(name) => {
                if exports.contains(name.as_str()) {
                    out.insert(name.clone().into());
                }
            }
            PatKind::Array(elems) => {
                for elem in elems.iter().flatten() {
                    match elem {
                        ArrayPatElem::Pat(p) | ArrayPatElem::Rest(p) => {
                            collect_pat_idents(p, exports, out);
                        }
                    }
                }
            }
            PatKind::Object(props) => {
                for prop in props {
                    match prop {
                        ObjPatProp::KeyValue(_, val) => collect_pat_idents(val, exports, out),
                        ObjPatProp::Shorthand(name, _)
                        | ObjPatProp::ShorthandAssign(name, _, _) => {
                            if exports.contains(name.as_str()) {
                                out.insert(name.clone().into());
                            }
                        }
                        ObjPatProp::Rest(p) => collect_pat_idents(p, exports, out),
                    }
                }
            }
            PatKind::Assign(p, _) | PatKind::Rest(p) => collect_pat_idents(p, exports, out),
        }
    }
    fn expr_check(expr: &Expr, exports: &HashSet<AstString>) -> bool {
        match &expr.kind {
            ExprKind::Ident(name) => exports.contains(name.as_str()),
            ExprKind::Member(mem) => expr_check(&mem.object, exports),
            ExprKind::Binary(bin) => {
                expr_check(&bin.left, exports) || expr_check(&bin.right, exports)
            }
            ExprKind::Unary(un) => expr_check(&un.argument, exports),
            ExprKind::Update(up) => expr_check(&up.argument, exports),
            ExprKind::Cond(c) => {
                expr_check(&c.test, exports)
                    || expr_check(&c.consequent, exports)
                    || expr_check(&c.alternate, exports)
            }
            ExprKind::Call(call) => {
                expr_check(&call.callee, exports)
                    || call.args.iter().any(|a| expr_check(a, exports))
            }
            ExprKind::New(n) => {
                expr_check(&n.callee, exports)
                    || n.args
                        .as_ref()
                        .is_some_and(|a| a.iter().any(|x| expr_check(x, exports)))
            }
            ExprKind::Assign(a) => expr_check(&a.left, exports) || expr_check(&a.right, exports),
            ExprKind::Paren(i)
            | ExprKind::Spread(i)
            | ExprKind::Await(i)
            | ExprKind::Delete(i)
            | ExprKind::Typeof(i)
            | ExprKind::Void(i)
            | ExprKind::NonNull(i) => expr_check(i, exports),
            ExprKind::As(a) => expr_check(&a.expr, exports),
            ExprKind::Satisfies(s) => expr_check(&s.expr, exports),
            ExprKind::TypeAssertion(ta) => expr_check(&ta.expr, exports),
            ExprKind::Instantiation(inst) => expr_check(&inst.expr, exports),
            ExprKind::ElemAccess(ea) => {
                expr_check(&ea.object, exports) || expr_check(&ea.index, exports)
            }
            ExprKind::ArrayLit(elems) => elems
                .iter()
                .any(|e| e.as_ref().is_some_and(|e| expr_check(e, exports))),
            ExprKind::ObjectLit(props) => props.iter().any(|p| match p {
                ObjLitProp::Property(prop) => {
                    (matches!(&prop.key, PropName::Computed(e, _) if expr_check(e, exports)))
                        || expr_check(&prop.value, exports)
                }
                ObjLitProp::Spread(e, _) => expr_check(e, exports),
                ObjLitProp::Shorthand(name, _) => exports.contains(name.as_str()),
                ObjLitProp::ShorthandDefault(name, init, _) => {
                    exports.contains(name.as_str()) || expr_check(init, exports)
                }
                ObjLitProp::Method(m) => {
                    matches!(&m.name, PropName::Computed(e, _) if expr_check(e, exports))
                }
                ObjLitProp::Get(a) | ObjLitProp::Set(a) => {
                    matches!(&a.name, PropName::Computed(e, _) if expr_check(e, exports))
                }
            }),
            ExprKind::Template(tpl) => tpl.exprs.iter().any(|e| expr_check(e, exports)),
            ExprKind::Comma(es) => es.iter().any(|e| expr_check(e, exports)),
            ExprKind::Yield(_, a) => a.as_ref().is_some_and(|e| expr_check(e, exports)),
            ExprKind::Arrow(arr) => {
                // Exclude parameter names that shadow exports
                let shadowed = param_names_set(&arr.params, exports);
                let inner = if shadowed.is_empty() {
                    exports.clone()
                } else {
                    exports.difference(&shadowed).cloned().collect()
                };
                let inner_ref = if shadowed.is_empty() { exports } else { &inner };
                match &arr.body {
                    ArrowBody::Expr(e) => expr_check(e, inner_ref),
                    ArrowBody::Block(ss) => ss.iter().any(|s| stmt_check(s, inner_ref)),
                }
            }
            ExprKind::FnExpr(f) => {
                let shadowed = param_names_set(&f.params, exports);
                let inner = if shadowed.is_empty() {
                    exports.clone()
                } else {
                    exports.difference(&shadowed).cloned().collect()
                };
                let inner_ref = if shadowed.is_empty() { exports } else { &inner };
                f.body
                    .as_ref()
                    .is_some_and(|b| b.iter().any(|s| stmt_check(s, inner_ref)))
            }
            _ => false,
        }
    }
    fn stmt_check(stmt: &Stmt, exports: &HashSet<AstString>) -> bool {
        match &stmt.kind {
            StmtKind::Expr(e) | StmtKind::Throw(e) => expr_check(e, exports),
            StmtKind::Var(v) => v
                .declarations
                .iter()
                .any(|d| d.init.as_ref().is_some_and(|e| expr_check(e, exports))),
            StmtKind::Return(Some(e)) => expr_check(e, exports),
            StmtKind::If(i) => {
                expr_check(&i.test, exports)
                    || stmt_check(&i.consequent, exports)
                    || i.alternate.as_ref().is_some_and(|s| stmt_check(s, exports))
            }
            StmtKind::Block(ss) => ss.iter().any(|s| stmt_check(s, exports)),
            StmtKind::For(f) => {
                f.test.as_ref().is_some_and(|e| expr_check(e, exports))
                    || f.update.as_ref().is_some_and(|e| expr_check(e, exports))
                    || stmt_check(&f.body, exports)
            }
            StmtKind::While(w) => expr_check(&w.test, exports) || stmt_check(&w.body, exports),
            StmtKind::DoWhile(dw) => expr_check(&dw.test, exports) || stmt_check(&dw.body, exports),
            StmtKind::Switch(sw) => {
                expr_check(&sw.discriminant, exports)
                    || sw.cases.iter().any(|c| {
                        c.test.as_ref().is_some_and(|e| expr_check(e, exports))
                            || c.consequent.iter().any(|s| stmt_check(s, exports))
                    })
            }
            StmtKind::FnDecl(f) => {
                f.params.iter().any(|p| {
                    p.initializer
                        .as_ref()
                        .is_some_and(|e| expr_check(e, exports))
                }) || {
                    let shadowed = param_names_set(&f.params, exports);
                    let inner = if shadowed.is_empty() {
                        exports.clone()
                    } else {
                        exports.difference(&shadowed).cloned().collect()
                    };
                    let inner_ref = if shadowed.is_empty() { exports } else { &inner };
                    f.body
                        .as_ref()
                        .is_some_and(|b| b.iter().any(|s| stmt_check(s, inner_ref)))
                }
            }
            StmtKind::ClassDecl(c) => c.members.iter().any(|m| match &m.kind {
                ClassMemberKind::Constructor(ctor) => {
                    ctor.params.iter().any(|p| {
                        p.initializer
                            .as_ref()
                            .is_some_and(|e| expr_check(e, exports))
                    }) || ctor
                        .body
                        .as_ref()
                        .is_some_and(|b| b.iter().any(|s| stmt_check(s, exports)))
                }
                ClassMemberKind::Method(meth) => {
                    meth.params.iter().any(|p| {
                        p.initializer
                            .as_ref()
                            .is_some_and(|e| expr_check(e, exports))
                    }) || meth
                        .body
                        .as_ref()
                        .is_some_and(|b| b.iter().any(|s| stmt_check(s, exports)))
                }
                ClassMemberKind::GetAccessor(a) => a
                    .body
                    .as_ref()
                    .is_some_and(|b| b.iter().any(|s| stmt_check(s, exports))),
                ClassMemberKind::SetAccessor(a) => {
                    a.params.iter().any(|p| {
                        p.initializer
                            .as_ref()
                            .is_some_and(|e| expr_check(e, exports))
                    }) || a
                        .body
                        .as_ref()
                        .is_some_and(|b| b.iter().any(|s| stmt_check(s, exports)))
                }
                _ => false,
            }),
            _ => false,
        }
    }
    stmt_check(stmt, exports)
}

/// Returns true if a statement contains TypeScript-specific syntax that
/// requires transformation during JavaScript emission.
pub(crate) fn stmt_needs_transform(stmt: &Stmt) -> bool {
    match &stmt.kind {
        // These are always erased/transformed:
        StmtKind::InterfaceDecl(_)
        | StmtKind::TypeAlias(_)
        | StmtKind::EnumDecl(_)
        | StmtKind::ModuleDecl(_)
        | StmtKind::Import(_)
        | StmtKind::ImportEquals(..)
        | StmtKind::Export(_)
        | StmtKind::ExportAssign(_) => true,

        // Class declarations always need transformation (type annotations, field inits, etc.)
        StmtKind::ClassDecl(_) => true,

        // Function declarations: need transform if they have type annotations or no body (overload)
        StmtKind::FnDecl(fn_decl) => fn_decl.body.is_none() || fn_decl_needs_transform(fn_decl),

        // Variable declarations: need transform if any decl has a type annotation
        // or the destructuring pattern contains default values needing transform
        StmtKind::Var(var_stmt) => {
            var_stmt.modifiers & MOD_DECLARE != 0
                || var_stmt.declarations.iter().all(
                    |d| matches!(&d.name.kind, PatKind::Ident(n) if n.is_empty() || n == "<error>"),
                )
                || var_stmt.declarations.iter().any(|d| {
                    let init_needs_transform =
                        d.init.as_ref().is_some_and(|e| expr_needs_transform(e));
                    d.type_ann.is_some()
                        || d.definite
                        || pat_needs_transform(&d.name)
                        || init_needs_transform
                })
        }

        // Expression statements: need transform if the expression does
        StmtKind::Expr(expr) => expr_needs_transform(expr),

        // Return: need transform if the return value expression does
        StmtKind::Return(opt_expr) => opt_expr.as_ref().is_some_and(|e| expr_needs_transform(e)),

        // If always needs explicit emitter to normalize brace/else placement
        // (TypeScript puts `else` on new line, normalizes Allman to K&R)
        StmtKind::If(_) => true,
        // While always needs explicit emitter to normalize brace placement
        // (TypeScript converts Allman `while (cond)\n{` to K&R `while (cond) {`)
        StmtKind::While(_) => true,
        // DoWhile always needs explicit emitter to normalize `}\nwhile` to `} while`
        StmtKind::DoWhile(_) => true,
        StmtKind::For(f) => {
            f.init.as_ref().is_some_and(|init| match init {
                ForInit::Var(vs) => vs.declarations.iter().any(|d| {
                    d.type_ann.is_some()
                        || d.definite
                        || pat_needs_transform(&d.name)
                        || d.init.as_ref().is_some_and(|e| expr_needs_transform(e))
                }),
                ForInit::Expr(e) => expr_needs_transform(e),
            }) || f.test.as_ref().is_some_and(|e| expr_needs_transform(e))
                || f.update.as_ref().is_some_and(|e| expr_needs_transform(e))
                || stmt_needs_transform(&f.body)
        }
        StmtKind::ForIn(fi) => {
            for_in_of_left_needs_transform(&fi.left)
                || expr_needs_transform(&fi.right)
                || stmt_needs_transform(&fi.body)
        }
        StmtKind::ForOf(fo) => {
            for_in_of_left_needs_transform(&fo.left)
                || expr_needs_transform(&fo.right)
                || stmt_needs_transform(&fo.body)
        }
        StmtKind::Switch(sw) => {
            expr_needs_transform(&sw.discriminant)
                || sw.cases.iter().any(|c| {
                    c.test.as_ref().is_some_and(|e| expr_needs_transform(e))
                        || c.consequent.iter().any(stmt_needs_transform)
                })
        }
        // Try always needs explicit emitter to normalize `} catch` / `} finally`
        // formatting (TypeScript puts catch/finally on new lines after `}`)
        StmtKind::Try(_) => true,
        // Throw always needs explicit emitter to handle `throw` with missing expression
        // (TypeScript emits `throw ;` but source copy would produce `throw;`)
        StmtKind::Throw(_) => true,
        StmtKind::Block(stmts) => stmts.iter().any(stmt_needs_transform),
        // Labeled statements always need the explicit emitter to ensure label
        // and body are on the same line (TypeScript normalizes `label:\nbody` to `label: body`).
        StmtKind::Labeled(_) => true,
        // With always needs explicit emitter to ensure closing `)` is present
        // (source may be missing it) and to normalize brace placement.
        StmtKind::With(_) => true,

        // These never need transformation:
        StmtKind::Empty | StmtKind::Break(_) | StmtKind::Continue(_) | StmtKind::Debugger => false,
    }
}

/// Returns true if a statement should end with a semicolon when emitted.
/// Used by the source-text copier to normalize ASI (automatic semicolon
/// insertion) to always include explicit semicolons, matching TypeScript's
/// emitter output.
/// Returns true if a statement will be completely erased in the output
/// Check if a statement is or wraps an `export = expr` assignment.
/// Covers both bare `export = x;` (StmtKind::ExportAssign) and
/// `export declare export = x;` (Export(Decl(ExportAssign))).
pub(crate) fn stmt_is_export_assign(stmt: &Stmt) -> bool {
    match &stmt.kind {
        StmtKind::ExportAssign(_) => true,
        StmtKind::Export(ed) => {
            matches!(ed.kind, ExportDeclKind::Decl(ref d) if matches!(d.kind, StmtKind::ExportAssign(_)))
        }
        _ => false,
    }
}

/// Extract the identifier name from an `export = X` statement, if X is a simple identifier.
pub(crate) fn export_assign_name(stmt: &Stmt) -> Option<&str> {
    let expr = match &stmt.kind {
        StmtKind::ExportAssign(e) => e,
        StmtKind::Export(ed) => {
            if let ExportDeclKind::Decl(ref d) = ed.kind {
                if let StmtKind::ExportAssign(ref e) = d.kind {
                    e
                } else {
                    return None;
                }
            } else {
                return None;
            }
        }
        _ => return None,
    };
    if let ExprKind::Ident(ref name) = expr.kind {
        Some(name.as_str())
    } else {
        // Non-identifier expressions (e.g. `export = { ... }`) are always runtime values
        None
    }
}

/// Check if a name is declared as a type-only entity (interface or type alias) in the file.
/// Returns true if the name is ONLY declared as a type (no value declaration).
pub(crate) fn name_is_type_only_in_file(
    name: &str,
    stmts: &[Stmt],
    preserve_const_enums: bool,
) -> bool {
    let mut found_type = false;
    let mut found_value = false;
    for stmt in stmts {
        match &stmt.kind {
            StmtKind::InterfaceDecl(iface) => {
                if iface.name == name {
                    found_type = true;
                }
            }
            StmtKind::TypeAlias(ta) => {
                if ta.name == name {
                    found_type = true;
                }
            }
            StmtKind::ClassDecl(cls) => {
                if cls.name.as_deref() == Some(name) {
                    found_value = true;
                }
            }
            StmtKind::FnDecl(f) => {
                if f.name.as_deref() == Some(name) {
                    found_value = true;
                }
            }
            StmtKind::Var(v) => {
                for d in &v.declarations {
                    if pat_declares_name(&d.name, name) {
                        found_value = true;
                    }
                }
            }
            StmtKind::EnumDecl(e) => {
                if e.name == name {
                    // Const enums are type-only (erased) unless preserveConstEnums is set
                    if e.is_const && !preserve_const_enums {
                        found_type = true;
                    } else {
                        found_value = true;
                    }
                }
            }
            StmtKind::ModuleDecl(m) => {
                if let ModuleName::Ident(ref n) = m.name {
                    if n == name {
                        // `declare namespace M1` is an ambient value declaration —
                        // it describes a runtime value, so treat it as a value for
                        // export= elision purposes. Only non-declare namespaces
                        // use the deeper type-only check.
                        if m.modifiers & MOD_DECLARE != 0 {
                            found_value = true;
                        } else if module_decl_is_type_only(m, preserve_const_enums) {
                            found_type = true;
                        } else {
                            found_value = true;
                        }
                    }
                }
            }
            StmtKind::Export(ed) => {
                if let ExportDeclKind::Decl(ref d) = ed.kind {
                    match &d.kind {
                        StmtKind::InterfaceDecl(iface) if iface.name == name => {
                            found_type = true;
                        }
                        StmtKind::TypeAlias(ta) if ta.name == name => {
                            found_type = true;
                        }
                        StmtKind::ClassDecl(cls) if cls.name.as_deref() == Some(name) => {
                            found_value = true;
                        }
                        StmtKind::FnDecl(f) if f.name.as_deref() == Some(name) => {
                            found_value = true;
                        }
                        StmtKind::Var(v) => {
                            for decl in &v.declarations {
                                if pat_declares_name(&decl.name, name) {
                                    found_value = true;
                                }
                            }
                        }
                        StmtKind::EnumDecl(e) if e.name == name => {
                            if e.is_const && !preserve_const_enums {
                                found_type = true;
                            } else {
                                found_value = true;
                            }
                        }
                        StmtKind::ModuleDecl(m) => {
                            if let ModuleName::Ident(ref n) = m.name {
                                if n == name {
                                    if module_decl_is_type_only(m, preserve_const_enums) {
                                        found_type = true;
                                    } else {
                                        found_value = true;
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            StmtKind::Import(import_decl) => {
                // `import type { A }` → A is type-only
                // `import { type A }` → A is type-only
                // `import { A }` → A is a value import
                if import_decl.type_only {
                    // Whole import is type-only
                    if import_imports_name(import_decl, name) {
                        found_type = true;
                    }
                } else if let ImportClause::Named {
                    named,
                    default,
                    namespace,
                } = &import_decl.specifiers
                {
                    for spec in named {
                        let local = &spec.local;
                        if local == name {
                            if spec.is_type {
                                found_type = true;
                            } else {
                                found_value = true;
                            }
                        }
                    }
                    if let Some(def) = default {
                        if def == name {
                            found_value = true;
                        }
                    }
                    if let Some(ns) = namespace {
                        if ns == name {
                            found_value = true;
                        }
                    }
                }
            }
            _ => {}
        }
    }
    // Type-only if we found a type declaration but no value declaration
    found_type && !found_value
}

/// Check if an import declaration imports a given name (any clause form).
fn import_imports_name(import_decl: &ImportDecl, name: &str) -> bool {
    match &import_decl.specifiers {
        ImportClause::Named {
            default,
            named,
            namespace,
        } => {
            if default.as_deref() == Some(name) {
                return true;
            }
            if namespace.as_deref() == Some(name) {
                return true;
            }
            named.iter().any(|s| s.local == name)
        }
        ImportClause::Require(local) => local == name,
    }
}

/// Check if a pattern declares a given name.
pub(crate) fn pat_declares_name(pat: &Pat, name: &str) -> bool {
    match &pat.kind {
        PatKind::Ident(n) => n == name,
        PatKind::Array(elems) => elems.iter().any(|e| match e {
            Some(ArrayPatElem::Pat(p)) | Some(ArrayPatElem::Rest(p)) => pat_declares_name(p, name),
            None => false,
        }),
        PatKind::Object(props) => props.iter().any(|p| match p {
            ObjPatProp::KeyValue(_, pat) => pat_declares_name(pat, name),
            ObjPatProp::Shorthand(n, _) | ObjPatProp::ShorthandAssign(n, _, _) => n == name,
            ObjPatProp::Rest(pat) => pat_declares_name(pat, name),
        }),
        PatKind::Assign(inner, _) => pat_declares_name(inner, name),
        PatKind::Rest(inner) => pat_declares_name(inner, name),
    }
}

/// Check if an `export = X` in the file targets a runtime value.
/// Returns true if there's an `export =` and the target is a value (not type-only).
/// This determines whether to use `module.exports = X` (value) or
/// `Object.defineProperty(exports, "__esModule", ...)` (type-only / no export=).
///
/// A type-only namespace that is used as a value elsewhere (e.g. via
/// `import X = Ns.Member`) is treated as a value by TypeScript — the
/// `export =` emits `module.exports = X`.  But a type-only namespace
/// that is never used as a value stays type-only and gets erased.
pub(crate) fn has_value_export_assign(stmts: &[Stmt], preserve_const_enums: bool) -> bool {
    for stmt in stmts {
        if stmt_is_export_assign(stmt) {
            // Found an export = statement
            if let Some(name) = export_assign_name(stmt) {
                // If the target is type-only BUT used as a value elsewhere
                // (e.g. `export import a = x.c;` uses `x` as a value),
                // treat it as a value export.
                if name_is_type_only_in_file(name, stmts, preserve_const_enums)
                    && name_used_as_value_in_file(name, stmts)
                {
                    return true;
                }
                // Named target: check if it's type-only
                return !name_is_type_only_in_file(name, stmts, preserve_const_enums);
            } else {
                // Expression target (e.g. `export = { ... }`) is always a value
                return true;
            }
        }
    }
    false
}

/// Check if a name is used as a value (runtime) in the file.
/// This detects `import X = Name.Member` patterns which access the
/// namespace as a runtime object.
fn name_used_as_value_in_file(name: &str, stmts: &[Stmt]) -> bool {
    for stmt in stmts {
        match &stmt.kind {
            StmtKind::ImportEquals(ie) => {
                if let Some(root) = expr_root_ident_static(&ie.module_ref) {
                    if root == name {
                        return true;
                    }
                }
            }
            StmtKind::Export(ed) => {
                if let ExportDeclKind::Decl(ref d) = ed.kind {
                    if let StmtKind::ImportEquals(ie) = &d.kind {
                        if let Some(root) = expr_root_ident_static(&ie.module_ref) {
                            if root == name {
                                return true;
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }
    false
}

/// (type-only declarations). Comments attached to such statements should
/// not be emitted as leading comments for the next visible statement.
pub(crate) fn stmt_is_erased(stmt: &Stmt, preserve_const_enums: bool) -> bool {
    match &stmt.kind {
        StmtKind::InterfaceDecl(_) | StmtKind::TypeAlias(_) => true,
        StmtKind::FnDecl(fn_decl) => fn_decl.body.is_none() || fn_decl.modifiers & MOD_DECLARE != 0,
        StmtKind::Var(var_stmt) => var_stmt.modifiers & MOD_DECLARE != 0,
        StmtKind::ClassDecl(cls) => cls.modifiers & MOD_DECLARE != 0,
        StmtKind::ModuleDecl(m) => {
            if m.modifiers & MOD_DECLARE != 0 {
                // Keep malformed `declare namespace` declarations with `<error>`
                // names so emit-time recovery can synthesize tokenized output.
                !matches!(&m.name, ModuleName::Ident(name) if name == "<error>")
            } else {
                module_decl_is_type_only(m, preserve_const_enums)
            }
        }
        StmtKind::EnumDecl(e) => {
            e.modifiers & MOD_DECLARE != 0 || (e.is_const && !preserve_const_enums)
        }
        StmtKind::Export(ed) => {
            matches!(&ed.kind, ExportDeclKind::Decl(inner) if stmt_is_erased(inner, preserve_const_enums))
        }
        _ => false,
    }
}

pub(crate) fn stmt_needs_trailing_semicolon(stmt: &Stmt) -> bool {
    matches!(
        &stmt.kind,
        StmtKind::Var(_)
            | StmtKind::Expr(_)
            | StmtKind::Return(_)
            | StmtKind::Throw(_)
            | StmtKind::Break(_)
            | StmtKind::Continue(_)
            | StmtKind::DoWhile(_)
            | StmtKind::Debugger
    )
}

/// Check whether an arrow-body expression will emit as an object literal after
/// Recursively strip type-level wrappers (type assertions, `as`, `satisfies`,
/// non-null `!`) to find the underlying runtime expression.
pub(crate) fn strip_type_layers(expr: &Expr) -> &Expr {
    match &expr.kind {
        ExprKind::NonNull(inner) => strip_type_layers(inner),
        ExprKind::TypeAssertion(ta) => strip_type_layers(&ta.expr),
        ExprKind::As(a) => strip_type_layers(&a.expr),
        ExprKind::Satisfies(s) => strip_type_layers(&s.expr),
        ExprKind::Instantiation(inst) => strip_type_layers(&inst.expr),
        _ => expr,
    }
}

/// Collect source-text replacements for type assertions inside JSX attribute
/// values and expression children.  Each entry is `(start, end, replacement)`
/// where `[start, end)` in the source should be replaced by `replacement`.
pub(crate) fn collect_jsx_type_strip_replacements(
    type_args: Option<&Vec<TypeNode>>,
    attrs: &[JsxAttribute],
    children: &[JsxChild],
    source: &str,
    out: &mut Vec<(usize, usize, String)>,
) {
    // Strip type arguments from JSX tag names (e.g. `<SFC<string>` → `<SFC`).
    if let Some(ta) = type_args {
        if let (Some(first), Some(last)) = (ta.first(), ta.last()) {
            // The `<` is one character before the first type arg span.
            let start = first.span.start.saturating_sub(1) as usize;
            // The `>` is at or just after the last type arg span end.
            let mut end = last.span.end as usize;
            // Skip past whitespace and the closing `>`.
            while end < source.len() && source.as_bytes()[end] == b' ' {
                end += 1;
            }
            if end < source.len() && source.as_bytes()[end] == b'>' {
                end += 1;
            }
            out.push((start, end, String::new()));
        } else if ta.is_empty() {
            // Empty type arguments `<>` — find and strip them from source.
            // Look for `<>` after the first attribute or element name.
            // The `<>` starts at the first attr span start - scan backwards.
            if let Some(first_attr) = attrs.first() {
                let attr_start = match first_attr {
                    JsxAttribute::Normal { span, .. } => span.start as usize,
                    JsxAttribute::Spread(_, span) => span.start as usize,
                };
                // Search backwards from the first attribute for `<>`
                if attr_start >= 2 {
                    let search = &source[..attr_start];
                    if let Some(pos) = search.rfind("<>") {
                        out.push((pos, pos + 2, String::new()));
                    }
                }
            }
        }
    }
    for attr in attrs {
        match attr {
            JsxAttribute::Normal {
                value: Some(val),
                span,
                name,
            } => {
                // Strip `= <value>` when the value is an error placeholder or
                // has an empty span (error recovery for `<div foo= >`).
                let is_error_val = expr_is_error_placeholder(val) || {
                    let vs = val.span.start as usize;
                    let ve = val.span.end as usize;
                    vs >= ve || (ve <= source.len() && source[vs..ve].trim().is_empty())
                };
                if is_error_val {
                    let s = span.start as usize;
                    let e = span.end as usize;
                    if s < e && e <= source.len() && source[s..e].contains('=') {
                        out.push((s, e, name.clone()));
                    }
                } else {
                    collect_expr_jsx_type_strips(val, source, out);
                    // Normalize object literal brace spacing in attribute values:
                    // `prop={{a: "x"}}` → `prop={{ a: "x" }}`
                    collect_obj_lit_brace_spacing(val, source, out);
                    // TypeScript normalizes whitespace before `=` in JSX attributes:
                    // `children ="lol"` → `children="lol"`
                    let name_end = span.start as usize + name.len();
                    if name_end < source.len() {
                        let mut eq_pos = name_end;
                        while eq_pos < source.len() && source.as_bytes()[eq_pos] == b' ' {
                            eq_pos += 1;
                        }
                        if eq_pos > name_end
                            && eq_pos < source.len()
                            && source.as_bytes()[eq_pos] == b'='
                        {
                            // Additional guard: the character after `=` (skipping
                            // spaces) must be a JSX attribute value delimiter
                            // (`"`, `'`, or `{`).  Misparsed JSX can create
                            // attributes from non-JSX code where `=` is followed
                            // by other tokens.
                            let mut after_eq = eq_pos + 1;
                            while after_eq < source.len() && source.as_bytes()[after_eq] == b' ' {
                                after_eq += 1;
                            }
                            let is_jsx_value_start = after_eq < source.len()
                                && matches!(source.as_bytes()[after_eq], b'"' | b'\'' | b'{');
                            if is_jsx_value_start {
                                out.push((name_end, eq_pos, String::new()));
                            }
                        }
                    }
                }
            }
            // Strip `= ` from attributes with missing values (error recovery):
            // `<div foo= >` → `<div foo>`.
            JsxAttribute::Normal {
                value: None,
                span,
                name,
            } => {
                let s = span.start as usize;
                let mut e = span.end as usize;
                if s < e && e <= source.len() && source[s..e].contains('=') {
                    // Also consume trailing whitespace after `=` that
                    // preceded the missing value.
                    while e < source.len() && source.as_bytes()[e] == b' ' {
                        e += 1;
                    }
                    out.push((s, e, name.clone()));
                }
            }
            JsxAttribute::Spread(expr, attr_span) => {
                collect_expr_jsx_type_strips(expr, source, out);
                // TypeScript normalizes spread attribute formatting:
                // `{... {x: 0} }` → `{...{ x: 0 }}`
                let attr_start = attr_span.start as usize;
                let expr_start = expr.span.start as usize;
                let expr_end = expr.span.end as usize;
                let attr_end = attr_span.end as usize;
                // 1. Strip spaces between `{` and `...` and between `...` and expression
                if attr_start + 4 <= expr_start && expr_start <= source.len() {
                    let slice = &source[attr_start..expr_start];
                    if let Some(dots_pos) = slice.find("...") {
                        // Strip space between `{` and `...`: `{ ...` → `{...`
                        let brace_end = attr_start + 1; // byte after `{`
                        let dots_start = attr_start + dots_pos;
                        if brace_end < dots_start
                            && source.as_bytes()[brace_end..dots_start]
                                .iter()
                                .all(|&b| b == b' ')
                        {
                            out.push((brace_end, dots_start, String::new()));
                        }
                        // Strip space between `...` and expression
                        let after_dots = attr_start + dots_pos + 3;
                        if after_dots < expr_start
                            && source.as_bytes()[after_dots..expr_start]
                                .iter()
                                .all(|&b| b == b' ')
                        {
                            out.push((after_dots, expr_start, String::new()));
                        }
                    }
                }
                // 2. Strip space between expression end and closing `}`
                if expr_end < attr_end && attr_end <= source.len() {
                    let trailing = &source[expr_end..attr_end];
                    if trailing.trim() == "}" && trailing != "}" {
                        out.push((expr_end, attr_end, "}".to_string()));
                    }
                }
                // 3. Normalize object literal brace spacing:
                // `{x: 0}` → `{ x: 0 }` (add space after `{` and before `}`)
                // But NOT for empty objects: `{}` stays `{}`
                if expr_start < expr_end && expr_end <= source.len() {
                    let expr_src = &source[expr_start..expr_end];
                    let is_empty_obj = expr_src.trim() == "{}"
                        || expr_src
                            .trim_matches(|c: char| c == '{' || c == '}' || c == ' ')
                            .is_empty();
                    if !is_empty_obj {
                        if expr_src.starts_with('{') && !expr_src.starts_with("{ ") {
                            out.push((expr_start, expr_start + 1, "{ ".to_string()));
                        }
                        if expr_src.ends_with('}') && !expr_src.ends_with(" }") {
                            out.push((expr_end - 1, expr_end, " }".to_string()));
                        }
                    }
                }
                // 4. Normalize property colon spacing in object literals:
                // `key:"value"` → `key: "value"` — add space after `:` when missing.
                let inner = match &expr.kind {
                    ExprKind::Paren(inner) => &**inner,
                    _ => expr,
                };
                if let ExprKind::ObjectLit(props) = &inner.kind {
                    for prop in props {
                        if let ObjLitProp::Property(p) = prop {
                            let key_end = p.key.span().end as usize;
                            let val_start = p.value.span.start as usize;
                            if key_end < val_start && val_start <= source.len() {
                                let between = &source[key_end..val_start];
                                // Look for `:value` (colon followed by non-space)
                                if let Some(colon_off) = between.find(':') {
                                    let after_colon = key_end + colon_off + 1;
                                    if after_colon <= val_start
                                        && source.as_bytes().get(after_colon) != Some(&b' ')
                                    {
                                        out.push((after_colon, after_colon, " ".to_string()));
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    // Recursively strip TypeScript syntax from all children: nested JSX
    // elements (type args), expression children (arrows with typed params,
    // type assertions, etc.), and fragments.
    collect_jsx_children_deep_type_strips(children, source, out);
}

/// Recursively walk an expression looking for `ObjectLit` nodes and add
/// brace spacing replacements: `{a: "x"}` → `{ a: "x" }`.
/// Also normalizes colon spacing: `a:"x"` → `a: "x"`.
fn collect_obj_lit_brace_spacing(expr: &Expr, source: &str, out: &mut Vec<(usize, usize, String)>) {
    match &expr.kind {
        ExprKind::ObjectLit(props) => {
            let s = expr.span.start as usize;
            let e = expr.span.end as usize;
            if s < e && e <= source.len() {
                let src = &source[s..e];
                // Only single-line object literals get brace spacing.
                if !src.contains('\n') && src.trim() != "{}" {
                    if src.starts_with('{') && !src.starts_with("{ ") {
                        out.push((s, s + 1, "{ ".to_string()));
                    }
                    if src.ends_with('}') && !src.ends_with(" }") {
                        out.push((e - 1, e, " }".to_string()));
                    }
                }
            }
            // Normalize colon spacing in properties
            for prop in props {
                if let ObjLitProp::Property(p) = prop {
                    let key_end = p.key.span().end as usize;
                    let val_start = p.value.span.start as usize;
                    if key_end < val_start && val_start <= source.len() {
                        let between = &source[key_end..val_start];
                        if let Some(colon_off) = between.find(':') {
                            let after_colon = key_end + colon_off + 1;
                            if after_colon <= val_start
                                && source.as_bytes().get(after_colon) != Some(&b' ')
                            {
                                out.push((after_colon, after_colon, " ".to_string()));
                            }
                        }
                    }
                }
                // Recurse into property values
                match prop {
                    ObjLitProp::Property(p) => {
                        collect_obj_lit_brace_spacing(&p.value, source, out);
                    }
                    ObjLitProp::Spread(e, _) => {
                        collect_obj_lit_brace_spacing(e, source, out);
                    }
                    _ => {}
                }
            }
        }
        ExprKind::Paren(inner) => {
            collect_obj_lit_brace_spacing(inner, source, out);
        }
        ExprKind::Arrow(arrow) => {
            if let ArrowBody::Expr(body) = &arrow.body {
                collect_obj_lit_brace_spacing(body, source, out);
            }
        }
        ExprKind::Cond(c) => {
            collect_obj_lit_brace_spacing(&c.consequent, source, out);
            collect_obj_lit_brace_spacing(&c.alternate, source, out);
        }
        ExprKind::Call(c) => {
            for arg in &c.args {
                collect_obj_lit_brace_spacing(arg, source, out);
            }
        }
        ExprKind::ArrayLit(items) => {
            for item in items.iter().flatten() {
                collect_obj_lit_brace_spacing(item, source, out);
            }
        }
        ExprKind::Spread(inner) => {
            collect_obj_lit_brace_spacing(inner, source, out);
        }
        _ => {}
    }
}

/// Recursively collect type-strip replacements from nested JSX children.
/// This handles children that are JsxElement or JsxSelfClosing — their type
/// arguments need stripping even when they're children of a parent element or
/// fragment that copies its span verbatim.
pub(crate) fn collect_jsx_children_deep_type_strips(
    children: &[JsxChild],
    source: &str,
    out: &mut Vec<(usize, usize, String)>,
) {
    for child in children {
        match child {
            JsxChild::Element(expr) => {
                collect_expr_jsx_type_strips(expr, source, out);
            }
            JsxChild::Expression(Some(expr), child_span) => {
                collect_expr_jsx_type_strips(expr, source, out);
                // Normalize object literal brace spacing in child expressions
                collect_obj_lit_brace_spacing(expr, source, out);
                // Normalize JSX expression child spacing: `{ expr }` → `{expr}`
                // TypeScript strips leading/trailing whitespace inside `{...}`.
                let cs = child_span.start as usize;
                let ce = child_span.end as usize;
                let es = expr.span.start as usize;
                let ee = expr.span.end as usize;
                if cs + 1 < es && es <= source.len() {
                    let between = &source[cs + 1..es];
                    if !between.is_empty() && between.bytes().all(|b| b == b' ') {
                        out.push((cs + 1, es, String::new()));
                    }
                }
                if ee < ce.saturating_sub(1) && ce <= source.len() {
                    let between = &source[ee..ce - 1];
                    if !between.is_empty() && between.bytes().all(|b| b == b' ') {
                        out.push((ee, ce - 1, String::new()));
                    }
                }
            }
            JsxChild::Fragment(frag) => {
                collect_jsx_children_deep_type_strips(&frag.children, source, out);
            }
            _ => {}
        }
    }
}

/// In JS files, JSX tag type-argument syntax is parsed as recovery trivia.
/// Preserve it as a comma expression tail to match TypeScript baselines:
/// `<Foo<number>>` -> `<Foo />, <number>>`.
pub(crate) fn collect_jsx_js_file_type_arg_replacements(
    type_args: Option<&Vec<TypeNode>>,
    source: &str,
    out: &mut Vec<(usize, usize, String)>,
) {
    let Some(ta) = type_args else {
        return;
    };
    let (Some(first), Some(last)) = (ta.first(), ta.last()) else {
        return;
    };
    let start = first.span.start.saturating_sub(1) as usize;
    let mut end = last.span.end as usize;
    while end < source.len() && source.as_bytes()[end] == b' ' {
        end += 1;
    }
    if end < source.len() && source.as_bytes()[end] == b'>' {
        end += 1;
    }
    if start >= end || end > source.len() {
        return;
    }
    let type_args_text = &source[start..end];
    out.push((start, end, format!(" />, {}", type_args_text)));
}

/// Collect type-strip replacements from expressions that might contain JSX.
fn collect_expr_jsx_type_strips(expr: &Expr, source: &str, out: &mut Vec<(usize, usize, String)>) {
    match &expr.kind {
        ExprKind::JsxSelfClosing(el) => {
            collect_jsx_type_strip_replacements(
                el.type_args.as_ref(),
                &el.attributes,
                &[],
                source,
                out,
            );
        }
        ExprKind::JsxElement(el) => {
            collect_jsx_type_strip_replacements(
                el.type_args.as_ref(),
                &el.attributes,
                &el.children,
                source,
                out,
            );
            collect_jsx_children_deep_type_strips(&el.children, source, out);
        }
        ExprKind::JsxFragment(frag) => {
            collect_jsx_children_deep_type_strips(&frag.children, source, out);
        }
        ExprKind::Arrow(arrow) => {
            collect_arrow_type_strips(arrow, source, out);
            // Recurse into the body for nested JSX
            match &arrow.body {
                ArrowBody::Expr(body) => {
                    collect_expr_jsx_type_strips(body, source, out);
                }
                _ => {}
            }
        }
        ExprKind::Paren(inner) => {
            collect_expr_jsx_type_strips(inner, source, out);
        }
        ExprKind::ObjectLit(props) => {
            for prop in props {
                match prop {
                    ObjLitProp::Method(m) => {
                        collect_params_type_strips(&m.params, m.return_type.as_ref(), source, out);
                    }
                    ObjLitProp::Get(g) => {
                        collect_params_type_strips(&g.params, None, source, out);
                    }
                    ObjLitProp::Set(s) => {
                        collect_params_type_strips(&s.params, None, source, out);
                    }
                    _ => {}
                }
            }
        }
        _ => {
            // Also handle type assertions wrapping JSX
            collect_expr_type_strip(expr, source, out);
        }
    }
}

/// Collect replacement spans that strip TypeScript type annotations from
/// arrow function parameters, return types, and type parameters.
fn collect_arrow_type_strips(arrow: &ArrowFn, source: &str, out: &mut Vec<(usize, usize, String)>) {
    // Strip type annotations from parameters: `(x: Type)` → `(x)`
    for param in &arrow.params {
        if let Some(ref type_ann) = param.type_ann {
            let name_end = param.name.span.end as usize;
            let type_end = type_ann.span.end as usize;
            if name_end < type_end && type_end <= source.len() {
                out.push((name_end, type_end, String::new()));
            }
        }
    }
    // Strip return type: `): ReturnType =>` → `) =>`
    if let Some(ref ret_type) = arrow.return_type {
        // Find the `:` between `)` and the return type, strip from there
        // to end of return type span.
        let ret_start = ret_type.span.start as usize;
        let ret_end = ret_type.span.end as usize;
        if ret_start > 0 && ret_end <= source.len() {
            // Walk backwards from ret_start to find the `:` (skipping spaces)
            let mut colon_pos = ret_start;
            while colon_pos > 0 {
                colon_pos -= 1;
                let b = source.as_bytes()[colon_pos];
                if b == b':' {
                    break;
                }
                if b != b' ' && b != b'\t' {
                    colon_pos = ret_start; // no colon found, strip type only
                    break;
                }
            }
            out.push((colon_pos, ret_end, String::new()));
        }
    }
    // Strip type parameters: `<T, U>(...)` → `(...)`
    if let Some(ref type_params) = arrow.type_params {
        if !type_params.is_empty() {
            let first_start = type_params.first().unwrap().span.start as usize;
            let last_end = type_params.last().unwrap().span.end as usize;
            // Find `<` before first type param and `>` after last
            if first_start > 0 && last_end <= source.len() {
                let mut lt_pos = first_start;
                while lt_pos > 0 {
                    lt_pos -= 1;
                    if source.as_bytes()[lt_pos] == b'<' {
                        break;
                    }
                }
                let mut gt_pos = last_end;
                while gt_pos < source.len() {
                    if source.as_bytes()[gt_pos] == b'>' {
                        gt_pos += 1;
                        break;
                    }
                    gt_pos += 1;
                }
                out.push((lt_pos, gt_pos, String::new()));
            }
        }
    }
}

fn collect_params_type_strips(
    params: &[Param],
    return_type: Option<&TypeNode>,
    source: &str,
    out: &mut Vec<(usize, usize, String)>,
) {
    for param in params {
        if let Some(ref type_ann) = param.type_ann {
            let name_end = param.name.span.end as usize;
            let type_end = type_ann.span.end as usize;
            if name_end < type_end && type_end <= source.len() {
                out.push((name_end, type_end, String::new()));
            }
        }
    }
    if let Some(ret_type) = return_type {
        let ret_start = ret_type.span.start as usize;
        let ret_end = ret_type.span.end as usize;
        if ret_start > 0 && ret_end <= source.len() {
            let mut colon_pos = ret_start;
            while colon_pos > 0 {
                colon_pos -= 1;
                let b = source.as_bytes()[colon_pos];
                if b == b':' {
                    break;
                }
                if b != b' ' && b != b'\t' {
                    colon_pos = ret_start;
                    break;
                }
            }
            out.push((colon_pos, ret_end, String::new()));
        }
    }
}

pub(crate) fn collect_expr_type_strip(
    expr: &Expr,
    source: &str,
    out: &mut Vec<(usize, usize, String)>,
) {
    match &expr.kind {
        ExprKind::NonNull(inner) => {
            let inner_stripped = strip_type_layers(inner);
            let s = inner_stripped.span.start as usize;
            let e = inner_stripped.span.end as usize;
            if s < e && e <= source.len() {
                out.push((
                    expr.span.start as usize,
                    expr.span.end as usize,
                    source[s..e].to_string(),
                ));
            }
        }
        ExprKind::TypeAssertion(ta) => {
            let inner_stripped = strip_type_layers(&ta.expr);
            let s = inner_stripped.span.start as usize;
            let e = inner_stripped.span.end as usize;
            if s < e && e <= source.len() {
                out.push((
                    expr.span.start as usize,
                    expr.span.end as usize,
                    source[s..e].to_string(),
                ));
            }
        }
        ExprKind::As(a) => {
            let inner_stripped = strip_type_layers(&a.expr);
            let s = inner_stripped.span.start as usize;
            let e = inner_stripped.span.end as usize;
            if s < e && e <= source.len() {
                out.push((
                    expr.span.start as usize,
                    expr.span.end as usize,
                    source[s..e].to_string(),
                ));
            }
        }
        ExprKind::Satisfies(sat) => {
            let inner_stripped = strip_type_layers(&sat.expr);
            let s = inner_stripped.span.start as usize;
            let e = inner_stripped.span.end as usize;
            if s < e && e <= source.len() {
                out.push((
                    expr.span.start as usize,
                    expr.span.end as usize,
                    source[s..e].to_string(),
                ));
            }
        }
        ExprKind::Instantiation(inst) => {
            let inner_stripped = strip_type_layers(&inst.expr);
            let s = inner_stripped.span.start as usize;
            let e = inner_stripped.span.end as usize;
            if s < e && e <= source.len() {
                out.push((
                    expr.span.start as usize,
                    expr.span.end as usize,
                    source[s..e].to_string(),
                ));
            }
        }
        _ => {}
    }
}

/// type-level wrappers (type assertions, `as`, `satisfies`, non-null `!`) are
/// stripped.  When this returns `true` the emitter must wrap the body in
/// parentheses so that `() => ({})` is not misinterpreted as an empty block.
pub(crate) fn arrow_body_emits_as_object_lit(expr: &Expr) -> bool {
    match &expr.kind {
        ExprKind::ObjectLit(_) => true,
        ExprKind::NonNull(inner) => arrow_body_emits_as_object_lit(inner),
        ExprKind::TypeAssertion(ta) => arrow_body_emits_as_object_lit(&ta.expr),
        ExprKind::As(a) => arrow_body_emits_as_object_lit(&a.expr),
        ExprKind::Satisfies(s) => arrow_body_emits_as_object_lit(&s.expr),
        ExprKind::Instantiation(inst) => arrow_body_emits_as_object_lit(&inst.expr),
        _ => false,
    }
}

/// Returns true if an expression contains TypeScript-specific syntax.
/// Returns `true` if the expression is complex enough to need a local variable
/// when used as a CJS export initializer (function/class expressions, etc.).
pub(crate) fn expr_needs_local_var(expr: &Expr) -> bool {
    match &expr.kind {
        ExprKind::FnExpr(_) | ExprKind::ClassExpr(_) | ExprKind::Arrow(_) => true,
        ExprKind::Paren(inner) => expr_needs_local_var(inner),
        _ => false,
    }
}

/// Like `expr_needs_transform` but for `.js` files where type-like syntax
/// (`<Type>`, `as Type`, `!`, `Foo<T>()`) is NOT real TypeScript and should
/// be preserved in the output.
#[allow(dead_code)] // JS file type assertion detection
pub(crate) fn expr_needs_transform_js(expr: &Expr) -> bool {
    match &expr.kind {
        // In JS files, these are NOT type constructs -- preserve them
        ExprKind::NonNull(_) => false,
        ExprKind::TypeAssertion(_) => false,
        ExprKind::As(_) => false,
        ExprKind::Satisfies(_) => false,
        ExprKind::Instantiation(_) => false,
        // Everything else: delegate to the normal check
        _ => expr_needs_transform(expr),
    }
}

pub(crate) fn expr_needs_transform(expr: &Expr) -> bool {
    let mut stack = vec![expr];
    while let Some(expr) = stack.pop() {
        match &expr.kind {
            // Type assertions/casts are always erased
            ExprKind::NonNull(_) => return true,
            ExprKind::TypeAssertion(_) => return true,
            ExprKind::As(_) => return true,
            ExprKind::Satisfies(_) => return true,
            ExprKind::Instantiation(_) => return true,

            // BigInt literals need transform for hex case, binary/octal→decimal, separators
            ExprKind::BigIntLit(n) => {
                if Emitter::bigint_needs_normalize(n) {
                    return true;
                }
            }

            // Numeric literals need transform for legacy octals (02343) and numeric separators (1_000)
            ExprKind::NumLit(n) => {
                if Emitter::numlit_needs_normalize_str(n) {
                    return true;
                }
            }

            // Literals never need transformation
            ExprKind::Ident(_)
            | ExprKind::StrLit(_)
            | ExprKind::BoolLit(_)
            | ExprKind::NullLit
            | ExprKind::RegexpLit(_)
            | ExprKind::NoSubstTemplate(_)
            | ExprKind::This
            | ExprKind::Super
            | ExprKind::MetaProp(_) => {}

            // Omitted expressions (error recovery) should never use source-copy
            ExprKind::Omitted => return true,

            // Function expressions need transform if they have type annotations
            ExprKind::FnExpr(fn_decl) => {
                if fn_decl_needs_transform(fn_decl) {
                    return true;
                }
            }

            // Arrow functions: check params for type annotations
            ExprKind::Arrow(arrow) => {
                if arrow.type_params.is_some()
                    || arrow.return_type.is_some()
                    || arrow.params.iter().any(param_needs_transform)
                {
                    return true;
                }
                match &arrow.body {
                    ArrowBody::Expr(e) => stack.push(e),
                    ArrowBody::Block(stmts) => {
                        if stmts.iter().any(stmt_needs_transform) {
                            return true;
                        }
                    }
                }
            }

            // Class expressions always need transformation
            ExprKind::ClassExpr(_) => return true,

            // Calls may have type arguments
            ExprKind::Call(call) => {
                if call.type_args.is_some() {
                    return true;
                }
                // Calls with error-placeholder arguments need structured emit
                // so the error token (e.g. `\`) can be dropped from the arg list.
                // Exclude dynamic import() — its callee span covers `import(...)`,
                // and the parser always creates a placeholder arg for the missing operand.
                let is_import_call =
                    matches!(&call.callee.kind, ExprKind::Ident(n) if n == "import");
                if !is_import_call && call.args.iter().any(|a| expr_is_error_placeholder(a)) {
                    return true;
                }
                for arg in call.args.iter().rev() {
                    stack.push(arg);
                }
                stack.push(&call.callee);
            }

            // New may have type arguments
            ExprKind::New(new_expr) => {
                if new_expr.type_args.is_some() {
                    return true;
                }
                if let Some(args) = &new_expr.args {
                    for arg in args.iter().rev() {
                        stack.push(arg);
                    }
                }
                stack.push(&new_expr.callee);
            }

            // Templates: check sub-expressions
            ExprKind::Template(tpl) => {
                for inner in tpl.exprs.iter().rev() {
                    stack.push(inner);
                }
            }
            ExprKind::TaggedTemplate(tt) => {
                if tt.type_args.is_some() {
                    return true;
                }
                for inner in tt.quasi.exprs.iter().rev() {
                    stack.push(inner);
                }
                stack.push(&tt.tag);
            }

            // Compound expressions: check sub-expressions
            ExprKind::ArrayLit(elements) => {
                for inner in elements.iter().rev().flatten() {
                    stack.push(inner);
                }
            }
            ExprKind::ObjectLit(props) => {
                for prop in props.iter().rev() {
                    match prop {
                        ObjLitProp::Property(p) => {
                            if p.question_token {
                                return true;
                            }
                            // Private names in object literals are an error —
                            // TypeScript strips them, so we need structured emit.
                            if matches!(&p.key, PropName::Private(_, _)) {
                                return true;
                            }
                            stack.push(&p.value);
                            if let PropName::Computed(key_expr, _) = &p.key {
                                stack.push(key_expr);
                            }
                        }
                        ObjLitProp::Shorthand(_, _) => {}
                        ObjLitProp::ShorthandDefault(_, e, _) | ObjLitProp::Spread(e, _) => {
                            stack.push(e);
                        }
                        // Methods and accessors always use structured emit so that any
                        // illegal modifiers the parser skipped (e.g. `public`) are not
                        // carried into the output via source-copy.
                        ObjLitProp::Method(_) | ObjLitProp::Get(_) | ObjLitProp::Set(_) => {
                            return true;
                        }
                    }
                }
            }
            ExprKind::Binary(bin) => {
                // `3in[null]` must be structurally emitted as `3 in [null]` —
                // otherwise the source-copy produces ambiguous tokens.
                // Same for `instanceof` after numeric literals.
                if matches!(bin.op, BinaryOp::In | BinaryOp::InstanceOf)
                    && matches!(bin.left.kind, ExprKind::NumLit(_))
                {
                    return true;
                }
                stack.push(&bin.right);
                stack.push(&bin.left);
            }
            ExprKind::Unary(un) => stack.push(&un.argument),
            ExprKind::Update(up) => stack.push(&up.argument),
            ExprKind::Cond(cond) => {
                stack.push(&cond.alternate);
                stack.push(&cond.consequent);
                stack.push(&cond.test);
            }
            ExprKind::Member(mem) => {
                // Integer literals need `..` before property access (e.g., `3..toString()`)
                // so we must use structured emit to apply `member_object_needs_double_dot`.
                if matches!(mem.object.kind, ExprKind::NumLit(_)) {
                    return true;
                }
                stack.push(&mem.object);
            }
            ExprKind::ElemAccess(ea) => {
                stack.push(&ea.index);
                stack.push(&ea.object);
            }
            ExprKind::Assign(assign) => {
                stack.push(&assign.right);
                stack.push(&assign.left);
            }
            ExprKind::Paren(inner) | ExprKind::Spread(inner) | ExprKind::Await(inner) => {
                stack.push(inner)
            }
            ExprKind::Yield(_, Some(inner)) => stack.push(inner),
            ExprKind::Yield(_, None) => {}
            ExprKind::Delete(inner) | ExprKind::Typeof(inner) | ExprKind::Void(inner) => {
                if matches!(&inner.kind, ExprKind::Ident(n) if n == "<error>") {
                    return true;
                }
                stack.push(inner);
            }
            ExprKind::Comma(exprs) => {
                for inner in exprs.iter().rev() {
                    stack.push(inner);
                }
            }
            // JSX always needs transform (converted to React.createElement)
            ExprKind::JsxElement(_) | ExprKind::JsxSelfClosing(_) | ExprKind::JsxFragment(_) => {
                return true;
            }
        }
    }
    false
}

pub(crate) fn fn_decl_needs_transform(fn_decl: &FnDecl) -> bool {
    fn_decl.modifiers & MOD_DECLARE != 0
        || fn_decl.type_params.is_some()
        || fn_decl.return_type.is_some()
        || fn_decl.params.iter().any(param_needs_transform)
        || fn_decl
            .body
            .as_ref()
            .is_some_and(|body| body.iter().any(stmt_needs_transform))
}

pub(crate) fn param_needs_transform(param: &Param) -> bool {
    param.type_ann.is_some()
        || param.modifiers != MOD_NONE
        || param.optional
        || param
            .initializer
            .as_ref()
            .is_some_and(|e| expr_needs_transform(e))
        || pat_needs_transform(&param.name)
        // Error-recovered params or `this` params need structured emit so emit_params can skip them
        || matches!(&param.name.kind, PatKind::Ident(ref name) if name == "<error>" || name == "this")
}

pub(crate) fn pat_needs_transform(pat: &Pat) -> bool {
    match &pat.kind {
        PatKind::Ident(_) => false,
        PatKind::Array(elems) => elems.iter().any(|e| {
            e.as_ref().is_some_and(|elem| match elem {
                ArrayPatElem::Pat(p) => pat_needs_transform(p),
                ArrayPatElem::Rest(p) => pat_needs_transform(p),
            })
        }),
        PatKind::Object(props) => props.iter().any(|p| match p {
            ObjPatProp::KeyValue(_, val) => pat_needs_transform(val),
            ObjPatProp::Shorthand(_, _) => false,
            ObjPatProp::ShorthandAssign(_, init, _) => expr_needs_transform(init),
            ObjPatProp::Rest(p) => pat_needs_transform(p),
        }),
        PatKind::Assign(p, init) => pat_needs_transform(p) || expr_needs_transform(init),
        PatKind::Rest(p) => pat_needs_transform(p),
    }
}

/// Returns true when the body is `Block([])` and the source text between
/// braces contains only whitespace (no comments or other content).
pub(crate) fn for_in_of_left_needs_transform(left: &ForInOfLeft) -> bool {
    match left {
        ForInOfLeft::Var(vs) => vs.declarations.iter().any(|d| d.type_ann.is_some()),
        ForInOfLeft::Pat(pat) => pat_needs_transform(pat),
        ForInOfLeft::Expr(expr) => expr_needs_transform(expr),
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Find the position of a `//` line comment in a string, skipping those inside string literals.
pub(crate) fn find_line_comment(s: &str) -> Option<usize> {
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let ch = bytes[i];
        // Skip string literals
        if ch == b'"' || ch == b'\'' || ch == b'`' {
            i += 1;
            while i < bytes.len() {
                if bytes[i] == ch && (i == 0 || bytes[i - 1] != b'\\') {
                    break;
                }
                i += 1;
            }
            i += 1;
            continue;
        }
        // Check for //
        if ch == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// Find the first trailing comment start (`//` or `/*`) in a line fragment.
pub(crate) fn find_trailing_comment_start(s: &str) -> Option<usize> {
    let line = find_line_comment(s);
    let block = s.find("/*");
    match (line, block) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (Some(a), None) => Some(a),
        (None, Some(b)) => Some(b),
        (None, None) => None,
    }
}

/// Find the position of the first `,` or `)` that is not inside a comment.
/// Returns `s.len()` when no separator is found before end of input.
pub(crate) fn find_separator_outside_comments(s: &str) -> usize {
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let ch = bytes[i];
        if ch == b',' || ch == b')' {
            return i;
        }
        // Skip line comments — everything until newline
        if ch == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
            i += 2;
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        // Skip block comments — everything until */
        if ch == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'*' {
            i += 2;
            while i + 1 < bytes.len() {
                if bytes[i] == b'*' && bytes[i + 1] == b'/' {
                    i += 2;
                    break;
                }
                i += 1;
            }
            continue;
        }
        i += 1;
    }
    s.len()
}

pub(crate) fn is_string_literal(expr: &Expr) -> bool {
    is_string_valued(expr, &std::collections::HashSet::new())
}

/// Check whether an enum member initializer is string-valued, taking into
/// account a set of previously-emitted members known to have string values.
pub(crate) fn is_string_valued(
    expr: &Expr,
    string_members: &std::collections::HashSet<String>,
) -> bool {
    is_string_valued_ext(
        expr,
        string_members,
        &std::collections::HashMap::new(),
        &std::collections::HashMap::new(),
    )
}

/// Extended version of `is_string_valued` that also checks cross-enum
/// string member references via `merged_string_enum_values`.
pub(crate) fn is_string_valued_ext(
    expr: &Expr,
    string_members: &std::collections::HashSet<String>,
    merged_string_enum_values: &std::collections::HashMap<
        String,
        std::collections::HashMap<String, String>,
    >,
    file_string_consts: &std::collections::HashMap<String, String>,
) -> bool {
    match &expr.kind {
        ExprKind::StrLit(_) | ExprKind::NoSubstTemplate(_) | ExprKind::Template(_) => true,
        ExprKind::Paren(inner) => is_string_valued_ext(
            inner,
            string_members,
            merged_string_enum_values,
            file_string_consts,
        ),
        ExprKind::Binary(bin) if bin.op == BinaryOp::Add => {
            is_string_valued_ext(
                &bin.left,
                string_members,
                merged_string_enum_values,
                file_string_consts,
            ) || is_string_valued_ext(
                &bin.right,
                string_members,
                merged_string_enum_values,
                file_string_consts,
            )
        }
        // Reference to another enum member known to be string-valued, or file-level string const
        ExprKind::Ident(id)
            if string_members.contains(id.as_str())
                || file_string_consts.contains_key(id.as_str()) =>
        {
            true
        }
        // Member access: `EnumName.Member` — check current and cross-enum
        ExprKind::Member(member) => {
            if string_members.contains(member.property.as_str()) {
                return true;
            }
            if let ExprKind::Ident(obj_name) = &member.object.kind {
                if let Some(other) = merged_string_enum_values.get(obj_name.as_str()) {
                    return other.contains_key(member.property.as_str());
                }
            }
            false
        }
        // Element access: `EnumName["Member"]`
        ExprKind::ElemAccess(ea) => {
            if let ExprKind::StrLit(key) = &ea.index.kind {
                if string_members.contains(key.as_str()) {
                    return true;
                }
                if let ExprKind::Ident(obj_name) = &ea.object.kind {
                    if let Some(other) = merged_string_enum_values.get(obj_name.as_str()) {
                        return other.contains_key(key.as_str());
                    }
                }
            }
            false
        }
        _ => false,
    }
}

/// Returns `true` if the statement is a `"use strict"` directive.
pub(crate) fn is_use_strict_directive(stmt: &Stmt) -> bool {
    if let StmtKind::Expr(expr) = &stmt.kind {
        if let ExprKind::StrLit(s) = &expr.kind {
            return s == "use strict";
        }
    }
    false
}

pub(crate) fn is_super_call(stmt: &Stmt) -> bool {
    if let StmtKind::Expr(ref expr) = stmt.kind {
        return expr_is_super_call(expr);
    }
    false
}

/// Check if an expression is a super() call, including parenthesized forms like `(super())`.
fn expr_is_super_call(expr: &Expr) -> bool {
    match &expr.kind {
        ExprKind::Call(ref call) if matches!(call.callee.kind, ExprKind::Super) => true,
        ExprKind::Paren(ref inner) => expr_is_super_call(inner),
        _ => false,
    }
}
