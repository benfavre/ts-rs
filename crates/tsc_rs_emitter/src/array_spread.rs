//! Array spread lowering shared by ordinary emit and resumable async expressions.
use super::*;

#[derive(Debug, Clone, Copy)]
pub(super) struct SpreadHelpers {
    pub spread: bool,
    pub read: bool,
}

fn synthetic(kind: ExprKind) -> Expr {
    Expr {
        kind,
        span: Span::new(0, 0),
    }
}

fn array(elements: Vec<Option<Box<Expr>>>) -> Expr {
    synthetic(ExprKind::ArrayLit(elements))
}

fn dense_literal(expr: &Expr) -> Option<&[Option<Box<Expr>>]> {
    if let ExprKind::ArrayLit(elements) = &expr.kind {
        if elements.iter().all(|e| {
            e.as_ref()
                .is_some_and(|e| !matches!(e.kind, ExprKind::Omitted | ExprKind::Spread(_)))
        }) {
            return Some(elements);
        }
    }
    None
}

pub(super) fn helper_requirements(
    elements: &[Option<Box<Expr>>],
    iteration: bool,
) -> SpreadHelpers {
    spread_requirements(elements.iter().flatten().map(Box::as_ref), iteration)
}

fn spread_requirements<'e>(
    elements: impl Iterator<Item = &'e Expr>,
    iteration: bool,
) -> SpreadHelpers {
    let mut spread = false;
    let mut read = false;
    for element in elements {
        if let ExprKind::Spread(value) = &element.kind {
            if dense_literal(value).is_none() {
                spread = true;
                read |= iteration && !matches!(value.kind, ExprKind::ArrayLit(_));
            }
        }
    }
    SpreadHelpers { spread, read }
}

pub(super) fn argument_helper_requirements(args: &[Box<Expr>], iteration: bool) -> SpreadHelpers {
    if !iteration && matches!(args, [arg] if matches!(arg.kind, ExprKind::Spread(_))) {
        SpreadHelpers {
            spread: false,
            read: false,
        }
    } else {
        spread_requirements(args.iter().map(Box::as_ref), iteration)
    }
}

pub(super) fn constructor_helper_requirements(
    args: &[Box<Expr>],
    iteration: bool,
) -> SpreadHelpers {
    // Construction always prepends the bind receiver, even for a single spread.
    spread_requirements(args.iter().map(Box::as_ref), iteration)
}

pub(super) fn is_synthetic_helper(expr: &Expr) -> bool {
    expr.span == Span::new(0, 0)
        && matches!(&expr.kind, ExprKind::Ident(name) if matches!(name.as_str(), "__spreadArray" | "__read"))
}

impl Emitter<'_> {
    fn array_helper_call(&self, name: &str, args: Vec<Expr>) -> Expr {
        let helper = if self.helper_prefix().is_empty() {
            synthetic(ExprKind::Ident(name.into()))
        } else {
            synthetic(ExprKind::Member(Box::new(MemberExpr {
                object: Box::new(synthetic(ExprKind::Ident("tslib_1".into()))),
                property: name.into(),
                optional: false,
            })))
        };
        synthetic(ExprKind::Call(Box::new(CallExpr {
            callee: Box::new(helper),
            args: args.into_iter().map(Box::new).collect(),
            type_args: None,
            optional: false,
        })))
    }

    pub(super) fn lower_array_spread(&self, elements: &[Option<Box<Expr>>]) -> Expr {
        self.lower_spread_sequence(elements, true)
    }

    pub(super) fn lower_argument_spread(&self, args: &[Box<Expr>]) -> Expr {
        if self.options.down_level_iteration != Some(true) {
            if let [arg] = args {
                if let ExprKind::Spread(value) = &arg.kind {
                    return *value.clone();
                }
            }
        }
        let elements = args.iter().cloned().map(Some).collect::<Vec<_>>();
        self.lower_spread_sequence(&elements, false)
    }

    fn lower_spread_sequence(&self, elements: &[Option<Box<Expr>>], pack: bool) -> Expr {
        let mut result: Option<Expr> = None;
        let mut segment = Vec::new();
        for element in elements {
            let Some(Expr {
                kind: ExprKind::Spread(value),
                ..
            }) = element.as_deref()
            else {
                segment.push(element.clone());
                continue;
            };
            if let Some(literal) = dense_literal(value) {
                segment.extend(literal.iter().cloned());
                continue;
            }
            if !segment.is_empty() || result.is_none() {
                let next = array(std::mem::take(&mut segment));
                result = Some(match result.take() {
                    Some(previous) => self.array_helper_call(
                        "__spreadArray",
                        vec![previous, next, synthetic(ExprKind::BoolLit(false))],
                    ),
                    None => next,
                });
            }
            let read = self.options.down_level_iteration == Some(true)
                && !matches!(value.kind, ExprKind::ArrayLit(_));
            let value = if read {
                self.array_helper_call("__read", vec![*value.clone()])
            } else {
                *value.clone()
            };
            result = Some(self.array_helper_call(
                "__spreadArray",
                vec![
                    result.take().unwrap(),
                    value,
                    synthetic(ExprKind::BoolLit(pack && !read)),
                ],
            ));
        }
        let tail = array(segment);
        match result {
            Some(prefix) if matches!(&tail.kind, ExprKind::ArrayLit(v) if v.is_empty()) => prefix,
            Some(prefix) => self.array_helper_call(
                "__spreadArray",
                vec![prefix, tail, synthetic(ExprKind::BoolLit(false))],
            ),
            None => tail,
        }
    }

    pub(super) fn emit_spread_array_helper(&mut self) {
        self.writeln(
            "var __spreadArray = (this && this.__spreadArray) || function (to, from, pack) {",
        );
        self.writeln("    if (pack || arguments.length === 2) for (var i = 0, l = from.length, ar; i < l; i++) {");
        self.writeln("        if (ar || !(i in from)) {");
        self.writeln("            if (!ar) ar = Array.prototype.slice.call(from, 0, i);");
        self.writeln("            ar[i] = from[i];");
        self.writeln("        }");
        self.writeln("    }");
        self.writeln("    return to.concat(ar || Array.prototype.slice.call(from));");
        self.writeln("};");
    }
}
