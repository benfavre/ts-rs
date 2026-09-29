//! ES5 construction uses a bound constructor with its arguments already packed.
use super::*;

fn synthetic(kind: ExprKind) -> Expr {
    Expr {
        kind,
        span: Span::new(0, 0),
    }
}

fn member(object: Expr, property: &str) -> Expr {
    synthetic(ExprKind::Member(Box::new(MemberExpr {
        object: Box::new(object),
        property: property.into(),
        optional: false,
    })))
}

pub(super) fn eligible(new: &NewExpr) -> bool {
    new.args.as_ref().is_some_and(|args| {
        args.iter()
            .any(|arg| matches!(arg.kind, ExprKind::Spread(_)))
    })
}

pub(super) fn needs_constructor_temp(callee: &Expr) -> bool {
    if let Some(inner) = callee.kind.type_layer_inner() {
        return needs_constructor_temp(inner);
    }
    !matches!(callee.kind, ExprKind::Ident(_))
}

impl Emitter<'_> {
    pub(super) fn lower_spread_new(&self, new: &NewExpr, temporary: Option<Expr>) -> Expr {
        let (constructor, receiver) = if let Some(temp) = temporary {
            let assignment = synthetic(ExprKind::Assign(AssignExpr {
                left: Box::new(temp.clone()),
                op: AssignOp::Assign,
                right: new.callee.clone(),
            }));
            (synthetic(ExprKind::Paren(Box::new(assignment))), temp)
        } else {
            (*new.callee.clone(), *new.callee.clone())
        };
        let mut args = vec![Box::new(synthetic(ExprKind::Void(Box::new(synthetic(
            ExprKind::NumLit("0".into()),
        )))))];
        args.extend(new.args.iter().flatten().cloned());
        let bind = synthetic(ExprKind::Call(Box::new(CallExpr {
            callee: Box::new(member(member(constructor, "bind"), "apply")),
            args: vec![
                Box::new(receiver),
                Box::new(self.lower_argument_spread(&args)),
            ],
            type_args: None,
            optional: false,
        })));
        synthetic(ExprKind::New(Box::new(NewExpr {
            callee: Box::new(synthetic(ExprKind::Paren(Box::new(bind)))),
            args: Some(Vec::new()),
            type_args: None,
        })))
    }
}
