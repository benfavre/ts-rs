//! ES5 spread calls retain the callee's receiver and evaluate it once.
use super::*;

fn synthetic(kind: ExprKind) -> Expr {
    Expr {
        kind,
        span: Span::new(0, 0),
    }
}

fn reference(expr: &Expr) -> &Expr {
    if let Some(inner) = expr.kind.type_layer_inner() {
        reference(inner)
    } else if let ExprKind::Paren(inner) = &expr.kind {
        reference(inner)
    } else {
        expr
    }
}

fn supported_reference(expr: &Expr) -> bool {
    match &reference(expr).kind {
        ExprKind::Super => false,
        ExprKind::Member(member) => {
            !member.optional
                && !member.property.starts_with('#')
                && supported_reference(&member.object)
        }
        ExprKind::ElemAccess(access) => !access.optional && supported_reference(&access.object),
        ExprKind::Call(call) => !call.optional && supported_reference(&call.callee),
        _ => true,
    }
}

pub(super) fn eligible(call: &CallExpr) -> bool {
    !call.optional
        && supported_reference(&call.callee)
        && !matches!(&reference(&call.callee).kind, ExprKind::Ident(name) if name == "import")
        && call
            .args
            .iter()
            .any(|arg| matches!(arg.kind, ExprKind::Spread(_)))
}

pub(super) fn needs_receiver_temp(callee: &Expr, source: &str) -> bool {
    let receiver = match &reference(callee).kind {
        ExprKind::Member(member) => &member.object,
        ExprKind::ElemAccess(access) => &access.object,
        _ => return false,
    };
    !matches!(
        reference(receiver).kind,
        ExprKind::Ident(_) | ExprKind::This
    )
        // A computed name can suspend before the reference is complete. Keep
        // the receiver even if its source variable changes during that await.
        || matches!(&reference(callee).kind, ExprKind::ElemAccess(access)
            if source.get(access.index.span.start as usize..access.index.span.end as usize)
                .is_some_and(|text| text.contains("await")))
}

fn member(object: Expr, property: &str) -> Expr {
    synthetic(ExprKind::Member(Box::new(MemberExpr {
        object: Box::new(object),
        property: property.into(),
        optional: false,
    })))
}

impl Emitter<'_> {
    pub(super) fn lower_spread_call(&self, call: &CallExpr, temporary: Option<Expr>) -> Expr {
        let raw = reference(&call.callee);
        let object = match &raw.kind {
            ExprKind::Member(member) => Some(&member.object),
            ExprKind::ElemAccess(access) => Some(&access.object),
            _ => None,
        };
        let (callee, receiver) = if let Some(object) = object {
            let (object, receiver) = if let Some(temp) = temporary {
                let assignment = synthetic(ExprKind::Assign(AssignExpr {
                    left: Box::new(temp.clone()),
                    op: AssignOp::Assign,
                    right: object.clone(),
                }));
                (synthetic(ExprKind::Paren(Box::new(assignment))), temp)
            } else {
                (*object.clone(), *object.clone())
            };
            let callee = match &raw.kind {
                ExprKind::Member(access) => member(object, &access.property),
                ExprKind::ElemAccess(access) => synthetic(ExprKind::ElemAccess(ElemAccessExpr {
                    object: Box::new(object),
                    index: access.index.clone(),
                    optional: false,
                })),
                _ => unreachable!(),
            };
            (callee, receiver)
        } else {
            (
                *call.callee.clone(),
                synthetic(ExprKind::Void(Box::new(synthetic(ExprKind::NumLit(
                    "0".into(),
                ))))),
            )
        };
        synthetic(ExprKind::Call(Box::new(CallExpr {
            callee: Box::new(member(callee, "apply")),
            args: vec![
                Box::new(receiver),
                Box::new(self.lower_argument_spread(&call.args)),
            ],
            type_args: None,
            optional: false,
        })))
    }
}
