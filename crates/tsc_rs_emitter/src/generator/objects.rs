//! Construct object literals in source order across suspension points.
use super::expressions::{assignment, call, ident, member, synthetic};
use super::*;

fn property_span(prop: &ObjLitProp) -> Span {
    match prop {
        ObjLitProp::Property(p) => p.span,
        ObjLitProp::Method(p) => p.span,
        ObjLitProp::Get(p) | ObjLitProp::Set(p) => p.span,
        ObjLitProp::Shorthand(_, span)
        | ObjLitProp::ShorthandDefault(_, _, span)
        | ObjLitProp::Spread(_, span) => *span,
    }
}

fn property_name(prop: &ObjLitProp) -> Option<&PropName> {
    match prop {
        ObjLitProp::Property(p) => Some(&p.key),
        ObjLitProp::Method(p) => Some(&p.name),
        ObjLitProp::Get(p) | ObjLitProp::Set(p) => Some(&p.name),
        _ => None,
    }
}

fn property(name: &str, value: Expr) -> ObjLitProp {
    ObjLitProp::Property(ObjProp {
        key: PropName::Ident(name.into(), Span::new(0, 0)),
        value: Box::new(value),
        computed: false,
        question_token: false,
        span: Span::new(0, 0),
    })
}

fn function(
    params: &[Param],
    body: &[Stmt],
    span: Span,
    is_async: bool,
    is_generator: bool,
) -> Expr {
    synthetic(ExprKind::FnExpr(Box::new(FnDecl {
        name: None,
        name_span: None,
        type_params: None,
        params: params.to_vec(),
        return_type: None,
        body: Some(body.to_vec()),
        modifiers: MOD_NONE,
        is_async,
        is_generator,
        decorators: Vec::new(),
        span,
    })))
}

impl GeneratorPlan {
    fn property_suspends(prop: &ObjLitProp, source: &str) -> bool {
        property_name(prop).is_some_and(
            |name| matches!(name, PropName::Computed(key, _) if Self::has_await(key, source)),
        ) || match prop {
            ObjLitProp::Property(p) => Self::has_await(&p.value, source),
            ObjLitProp::Spread(value, _) | ObjLitProp::ShorthandDefault(_, value, _) => {
                Self::has_await(value, source)
            }
            _ => false,
        }
    }

    fn object_key(
        &mut self,
        name: &PropName,
        cached: Option<&Expr>,
        context: &Emitter<'_>,
    ) -> Option<Expr> {
        Some(match name {
            PropName::Ident(name, _) => synthetic(ExprKind::StrLit(name.clone())),
            PropName::String(name, span) => Expr {
                kind: ExprKind::StrLit(name.clone()),
                span: *span,
            },
            PropName::Number(value, _) => synthetic(ExprKind::NumLit(value.clone())),
            PropName::Computed(value, _) => {
                let value = self.expression(value, context)?;
                if let Some(cached) = cached {
                    let ExprKind::Ident(name) = &cached.kind else {
                        unreachable!()
                    };
                    self.push(Operation::Assign(name.clone(), value));
                    cached.clone()
                } else {
                    value
                }
            }
            PropName::Private(_, _) => return None,
        })
    }

    pub(super) fn object_literal(
        &mut self,
        expr: &Expr,
        props: &[ObjLitProp],
        context: &Emitter<'_>,
    ) -> Option<Expr> {
        let first = props.iter().position(|p| {
            Self::property_suspends(p, context.source)
                || matches!(property_name(p), Some(PropName::Computed(_, _)))
        })?;
        // Computed-key caches belong to the object transform, before the
        // generator's object/argument temporaries in TypeScript's allocation order.
        let keys: Vec<_> = props[first..].iter().map(|prop| {
            if matches!(prop, ObjLitProp::Property(p) if matches!(p.key, PropName::Computed(_, _)) && Self::has_await(&p.value, context.source)) {
                let name = self.fresh_name(context);
                self.object_key_temps.push(name.clone().into());
                Some(ident(&name))
            } else { None }
        }).collect();
        let prefix = Expr {
            kind: ExprKind::ObjectLit(props[..first].to_vec()),
            span: if first == 0 {
                Span::new(0, 0)
            } else {
                Span::new(expr.span.start, property_span(&props[first - 1]).end)
            },
        };
        let object = self.capture(prefix, context);
        let has_accessors = props
            .iter()
            .any(|p| matches!(p, ObjLitProp::Get(_) | ObjLitProp::Set(_)));
        let mut pending = Vec::new();
        for (prop, key_cache) in props[first..].iter().zip(&keys) {
            if Self::property_suspends(prop, context.source) {
                for value in pending.drain(..) {
                    self.push(Operation::Expr(value));
                }
            }
            let (key, value, accessor, plain_name) = match prop {
                ObjLitProp::Property(p) => {
                    let key = self.object_key(&p.key, key_cache.as_ref(), context)?;
                    let value = self.expression(&p.value, context)?;
                    (key, value, None, p.key.ident_name())
                }
                ObjLitProp::Shorthand(name, span) => (
                    synthetic(ExprKind::StrLit(name.clone())),
                    Expr {
                        kind: ExprKind::Ident(name.clone()),
                        span: *span,
                    },
                    None,
                    Some(name.as_str()),
                ),
                ObjLitProp::Method(m) => (
                    self.object_key(&m.name, None, context)?,
                    function(&m.params, &m.body, m.span, m.is_async, m.is_generator),
                    None,
                    m.name.ident_name(),
                ),
                ObjLitProp::Get(a) | ObjLitProp::Set(a) => (
                    self.object_key(&a.name, None, context)?,
                    function(&a.params, &a.body, a.span, false, false),
                    Some(if matches!(prop, ObjLitProp::Get(_)) {
                        "get"
                    } else {
                        "set"
                    }),
                    a.name.ident_name(),
                ),
                ObjLitProp::Spread(value, _) => {
                    let value = self.expression(value, context)?;
                    pending.push(call(
                        member(ident("Object"), "assign"),
                        vec![object.clone(), value],
                    ));
                    continue;
                }
                ObjLitProp::ShorthandDefault(_, _, _) => return None,
            };
            let definition = if has_accessors {
                let mut descriptor = vec![
                    property(accessor.unwrap_or("value"), value),
                    property("enumerable", synthetic(ExprKind::BoolLit(true))),
                    property("configurable", synthetic(ExprKind::BoolLit(true))),
                ];
                if accessor.is_none() {
                    descriptor.push(property("writable", synthetic(ExprKind::BoolLit(true))));
                }
                call(
                    member(ident("Object"), "defineProperty"),
                    vec![
                        object.clone(),
                        key,
                        synthetic(ExprKind::ObjectLit(descriptor)),
                    ],
                )
            } else {
                let target = if let Some(name) = plain_name {
                    member(object.clone(), name)
                } else {
                    synthetic(ExprKind::ElemAccess(ElemAccessExpr {
                        object: Box::new(object.clone()),
                        index: Box::new(key),
                        optional: false,
                    }))
                };
                assignment(target, value)
            };
            pending.push(definition);
        }
        pending.push(object);
        let sequence = Expr {
            kind: ExprKind::Comma(pending.into_iter().map(Box::new).collect()),
            // A zero-width origin retains the object's layout without allowing
            // the source-preserving emitter to copy its original awaits.
            span: Span::new(expr.span.start, expr.span.start),
        };
        Some(synthetic(ExprKind::Paren(Box::new(sequence))))
    }
}

impl Emitter<'_> {
    pub(crate) fn emit_resumable_object_sequence(
        &mut self,
        expr: &Expr,
        values: &[Box<Expr>],
    ) -> bool {
        if expr.span.start != expr.span.end
            || self.source.as_bytes().get(expr.span.start as usize) != Some(&b'{')
            || !values
                .last()
                .is_some_and(|v| v.span == Span::new(0, 0) && matches!(v.kind, ExprKind::Ident(_)))
        {
            return false;
        }
        // An inline state label has not entered the usual statement-body
        // indentation level yet; continuation lines still need that level.
        let continuation = if self
            .output
            .rsplit('\n')
            .next()
            .unwrap_or("")
            .trim_start()
            .starts_with("case ")
        {
            2
        } else {
            1
        };
        for (i, value) in values.iter().enumerate() {
            if i > 0 {
                self.writeln(",");
                self.indent += continuation;
            }
            self.emit_expr(value);
            if i > 0 {
                self.indent -= continuation;
            }
        }
        true
    }
}
