//! Definitions for selected call signatures and constructor declarations.
use super::*;
use tsc_rs_types::TypeCheckOutput;

fn at(span: Span, offset: u32) -> bool {
    span.start <= offset && offset <= span.end
}

pub(super) fn resolve(
    qe: &QueryEngine,
    sf: &SourceFile,
    index: &Index<'_>,
    offset: u32,
    output: &TypeCheckOutput,
) -> Option<Vec<Target>> {
    for (class, _) in &index.classes {
        for member in &class.members {
            let ClassMemberKind::Method(method) = &member.kind else {
                continue;
            };
            for decorator in &method.decorators {
                if !at(decorator.span, offset) {
                    continue;
                }
                let ExprKind::Ident(name) = &decorator.kind else {
                    continue;
                };
                let key = match &method.name {
                    PropName::Computed(e, _) => e.clone(),
                    name => Box::new(Expr {
                        kind: ExprKind::StrLit(property_name(name)?.into()),
                        span: name.span(),
                    }),
                };
                let args = vec![
                    Box::new(Expr {
                        kind: ExprKind::This,
                        span: class.span,
                    }),
                    key,
                ];
                let call = Call {
                    args: &args,
                    construct: false,
                    tagged: false,
                    preferred: None,
                    output,
                };
                let signatures: Vec<_> = index
                    .functions
                    .iter()
                    .filter(|f| f.name.as_deref() == Some(name.as_str()) && f.body.is_none())
                    .filter_map(|f| f.name_span.map(|s| (f.params.as_slice(), s)))
                    .collect();
                let selected = if qe.options().experimental_decorators == Some(true) {
                    call.select(&signatures)
                } else {
                    // Standard decorators receive a context object instead of
                    // a legacy property key. No string/symbol overload fits.
                    signatures.first().map(|(_, span)| *span)
                };
                if let Some(span) = selected {
                    return Some(vec![(sf.file_name.clone(), span)]);
                }
            }
        }
    }
    for expr in &index.expressions {
        let jsx = match &expr.kind {
            ExprKind::JsxSelfClosing(e) => Some((e.name.as_ref(), e.attributes.as_slice())),
            ExprKind::JsxElement(e) => Some((e.name.as_ref(), e.attributes.as_slice())),
            _ => None,
        };
        if let Some((name, attributes)) = jsx {
            if !at(name.span, offset) {
                continue;
            }
            if let ExprKind::Ident(n) = &name.kind {
                let functions: Vec<_> = index
                    .functions
                    .iter()
                    .filter(|f| f.name.as_deref() == Some(n.as_str()))
                    .collect();
                if !functions.is_empty() {
                    let function = functions
                        .iter()
                        .find(|f| {
                            f.params
                                .first()
                                .and_then(|p| p.type_ann.as_ref())
                                .is_some_and(|t| jsx_accepts(index, t, attributes, 0))
                        })
                        .unwrap_or(&functions[0]);
                    return function.name_span.map(|s| vec![(sf.file_name.clone(), s)]);
                }
                if let Some(var) = index.variable(n, name.span.start) {
                    if let Some(Expr {
                        kind: ExprKind::Binary(b),
                        ..
                    }) = var.init.as_deref()
                    {
                        if b.op == BinaryOp::LogOr {
                            let call = Call {
                                args: &[],
                                construct: false,
                                tagged: false,
                                preferred: None,
                                output,
                            };
                            let mut targets = vec![(sf.file_name.clone(), var.name.span)];
                            targets.extend(expression(qe, sf, index, &b.left, &call, &mut vec![])?);
                            return Some(targets);
                        }
                    }
                }
            }
            continue;
        }
        let (callee, args, construct, tagged) = match &expr.kind {
            ExprKind::Call(c) => (c.callee.as_ref(), c.args.as_slice(), false, false),
            ExprKind::New(c) => (
                c.callee.as_ref(),
                c.args.as_deref().unwrap_or(&[]),
                true,
                false,
            ),
            ExprKind::TaggedTemplate(t) => (t.tag.as_ref(), t.quasi.exprs.as_slice(), false, true),
            _ => continue,
        };
        if !at(callee.span, offset) {
            continue;
        }
        let preferred = output
            .selected_overload_indices
            .get(&callee.span.start)
            .copied()
            .or_else(|| {
                output
                    .selected_overload_indices
                    .get(&super::super::signature_target_offset(callee))
                    .copied()
            });
        let call = Call {
            args,
            construct,
            tagged,
            preferred,
            output,
        };
        return expression(qe, sf, index, callee, &call, &mut vec![]);
    }
    None
}

fn jsx_accepts(
    index: &Index<'_>,
    ty: &TypeNode,
    attributes: &[JsxAttribute],
    depth: usize,
) -> bool {
    if depth > 16 {
        return false;
    }
    let members = match &ty.kind {
        TypeNodeKind::TypeLit(m) => m.as_slice(),
        TypeNodeKind::Reference(r) => {
            let ExprKind::Ident(name) = &r.name.kind else {
                return false;
            };
            if let Some(alias) = index.aliases.iter().find(|a| a.name == name.as_str()) {
                return jsx_accepts(index, &alias.type_ann, attributes, depth + 1);
            }
            let Some(i) = index.interfaces.iter().find(|i| i.name == name.as_str()) else {
                return false;
            };
            if !i
                .extends
                .iter()
                .all(|t| jsx_accepts(index, t, attributes, depth + 1))
            {
                return false;
            }
            i.members.as_slice()
        }
        TypeNodeKind::Union(ts) => {
            return ts
                .iter()
                .any(|t| jsx_accepts(index, t, attributes, depth + 1))
        }
        _ => return false,
    };
    members.iter().all(|m| {
        let (name, optional) = match &m.kind { TypeMemberKind::PropertySig(p) => (&p.name, p.optional), TypeMemberKind::MethodSig(p) => (&p.name, p.optional), _ => return true };
        optional || attributes.iter().any(|a| matches!(a, JsxAttribute::Normal { name: n, .. } if Some(n.as_str()) == property_name(name)))
    })
}

struct Call<'a> {
    args: &'a [Box<Expr>],
    construct: bool,
    tagged: bool,
    preferred: Option<usize>,
    output: &'a TypeCheckOutput,
}

impl Call<'_> {
    fn select(&self, signatures: &[(&[Param], Span)]) -> Option<Span> {
        let compatible = |params: &[Param]| {
            let count = self.args.len() + usize::from(self.tagged);
            let required = params
                .iter()
                .filter(|p| !p.optional && p.initializer.is_none() && !p.dotdotdot)
                .count();
            if count < required || (count > params.len() && !params.iter().any(|p| p.dotdotdot)) {
                return false;
            }
            for (param, arg) in params.iter().skip(usize::from(self.tagged)).zip(self.args) {
                if let Some(ty) = &param.type_ann {
                    if !argument_matches(ty, arg, self.output) {
                        return false;
                    }
                }
            }
            true
        };
        self.preferred
            .and_then(|i| signatures.get(i))
            .filter(|(p, _)| compatible(p))
            .or_else(|| signatures.iter().find(|(p, _)| compatible(p)))
            .or_else(|| signatures.first())
            .map(|(_, s)| *s)
    }
}

fn argument_matches(ty: &TypeNode, arg: &Expr, output: &TypeCheckOutput) -> bool {
    let display = output
        .expression_types
        .get(&arg.span.start)
        .map(String::as_str)
        .unwrap_or("any");
    match &ty.kind {
        TypeNodeKind::Keyword(KeywordTypeKind::Number) => {
            matches!(arg.kind, ExprKind::NumLit(_))
                || display == "number"
                || display == "any" && matches!(arg.kind, ExprKind::Ident(_))
        }
        TypeNodeKind::Keyword(KeywordTypeKind::String) => {
            matches!(arg.kind, ExprKind::StrLit(_) | ExprKind::NoSubstTemplate(_))
                || display == "string"
                || display == "any" && matches!(arg.kind, ExprKind::Ident(_))
        }
        TypeNodeKind::Keyword(KeywordTypeKind::Boolean) => {
            matches!(arg.kind, ExprKind::BoolLit(_))
                || matches!(display, "boolean" | "true" | "false")
        }
        TypeNodeKind::Literal(LiteralTypeKind::String(s)) => {
            matches!(&arg.kind, ExprKind::StrLit(n) if n == s)
        }
        TypeNodeKind::Union(ts) => ts.iter().any(|t| argument_matches(t, arg, output)),
        _ => true,
    }
}

fn class_constructors(
    qe: &QueryEngine,
    sf: &SourceFile,
    index: &Index<'_>,
    class: &ClassDecl,
    call: &Call<'_>,
    seen: &mut Vec<(String, String)>,
) -> Option<Vec<Target>> {
    let mut ctors: Vec<_> = class
        .members
        .iter()
        .filter_map(|m| match &m.kind {
            ClassMemberKind::Constructor(c) => {
                Some((c, Span::new(c.keyword_span.start, m.span.end)))
            }
            _ => None,
        })
        .collect();
    if ctors.iter().any(|(c, _)| c.body.is_none()) {
        ctors.retain(|(c, _)| c.body.is_none());
    }
    if !ctors.is_empty() {
        let signatures: Vec<_> = ctors
            .iter()
            .map(|(c, s)| (c.params.as_slice(), *s))
            .collect();
        return Some(
            call.select(&signatures)
                .map(|s| (sf.file_name.clone(), s))
                .into_iter()
                .collect(),
        );
    }
    if let Some(parent) = &class.extends {
        let targets = expression(qe, sf, index, parent, call, seen)?;
        // The parent expression's own name is not a constructor signature.
        return Some(targets.into_iter().skip(1).collect());
    }
    Some(vec![])
}

fn expression(
    qe: &QueryEngine,
    sf: &SourceFile,
    index: &Index<'_>,
    expr: &Expr,
    call: &Call<'_>,
    seen: &mut Vec<(String, String)>,
) -> Option<Vec<Target>> {
    if seen.len() > 32 {
        return None;
    }
    match &expr.kind {
        ExprKind::Ident(name) => {
            let key = (sf.file_name.clone(), name.to_string());
            if seen.contains(&key) {
                return None;
            }
            seen.push(key);
            if call.construct {
                let class = index
                    .classes
                    .iter()
                    .filter(|(c, expression)| {
                        c.name.as_deref() == Some(name.as_str())
                            && (!expression || contains(c.span, expr.span.start))
                    })
                    .min_by_key(|(c, _)| c.span.end - c.span.start)
                    .map(|(c, _)| *c);
                if let Some(class) = class {
                    let mut targets = class
                        .name_span
                        .map(|s| (sf.file_name.clone(), s))
                        .into_iter()
                        .collect::<Vec<_>>();
                    targets.extend(class_constructors(qe, sf, index, class, call, seen)?);
                    return Some(targets);
                }
            }
            let functions: Vec<_> = index
                .functions
                .iter()
                .filter(|f| f.name.as_deref() == Some(name.as_str()))
                .collect();
            if !functions.is_empty() {
                let has_overloads = functions.iter().any(|f| f.body.is_none());
                let signatures: Vec<_> = functions
                    .iter()
                    .filter(|f| !has_overloads || f.body.is_none())
                    .filter_map(|f| f.name_span.map(|s| (f.params.as_slice(), s)))
                    .collect();
                return Some(
                    call.select(&signatures)
                        .map(|s| (sf.file_name.clone(), s))
                        .into_iter()
                        .collect(),
                );
            }
            if let Some(var) = index
                .variable(name, expr.span.start)
                .filter(|v| v.init.as_deref().and_then(require_spec).is_none())
            {
                let mut targets = vec![(sf.file_name.clone(), var.name.span)];
                if let Some(init) = &var.init {
                    match &init.kind {
                        ExprKind::Ident(_) => targets.extend(
                            expression(qe, sf, index, init, call, seen)?
                                .into_iter()
                                .rev()
                                .take(1),
                        ),
                        ExprKind::ClassExpr(c) if call.construct => {
                            targets.extend(class_constructors(qe, sf, index, c, call, seen)?)
                        }
                        ExprKind::Arrow(_) | ExprKind::FnExpr(_) => {}
                        _ => return None,
                    }
                } else if call.construct {
                    if let Some(ty) = &var.type_ann {
                        targets.extend(
                            construct_signatures(index, ty, call, 0)?
                                .into_iter()
                                .map(|s| (sf.file_name.clone(), s)),
                        );
                    }
                } else {
                    return None;
                }
                return Some(targets);
            }
            if let Some((file, span)) = imported_binding(qe, sf, index, name, &mut vec![]) {
                if file == sf.file_name {
                    return None;
                }
                let source = qe.get_source_file(&file)?;
                let target = tsc_rs_parser::parse(&file, source);
                let target_index = Index::new(&target);
                // Bind to the actual declaration's name, not the import alias.
                let actual_name = target_index
                    .classes
                    .iter()
                    .find(|(c, _)| c.name_span.is_some_and(|s| s.start == span.start))
                    .and_then(|(c, _)| c.name.clone())
                    .or_else(|| {
                        target_index
                            .functions
                            .iter()
                            .find(|f| {
                                f.span.start == span.start
                                    || f.name_span.is_some_and(|s| s.start == span.start)
                            })
                            .and_then(|f| f.name.clone())
                    })
                    .unwrap_or_else(|| super::super::extract_token_at(source, span.start));
                let target_expr = Expr {
                    kind: ExprKind::Ident(actual_name.into()),
                    span,
                };
                return expression(qe, &target, &target_index, &target_expr, call, seen)
                    .or(Some(vec![(file, span)]));
            }
            None
        }
        ExprKind::Member(member) => {
            let (class, is_static) = match &member.object.kind {
                ExprKind::Ident(name) => {
                    if let Some(c) = index.named_class(name) {
                        (Some(c), true)
                    } else if let Some(var) = index.variable(name, member.object.span.start) {
                        let class = var.init.as_ref().and_then(|e| match &e.kind {
                            ExprKind::New(n) => match &n.callee.kind {
                                ExprKind::Ident(n) => index.named_class(n),
                                _ => None,
                            },
                            _ => None,
                        });
                        (class, false)
                    } else {
                        (None, false)
                    }
                }
                _ => (None, false),
            };
            if let Some(class) = class {
                let mut methods: Vec<_> = class
                    .members
                    .iter()
                    .filter_map(|m| match &m.kind {
                        ClassMemberKind::Method(m)
                            if property_name(&m.name) == Some(member.property.as_str())
                                && (m.modifiers & MOD_STATIC != 0) == is_static =>
                        {
                            Some(m)
                        }
                        _ => None,
                    })
                    .collect();
                if methods.iter().any(|m| m.body.is_none()) {
                    methods.retain(|m| m.body.is_none());
                }
                let signatures: Vec<_> = methods
                    .iter()
                    .map(|m| (m.params.as_slice(), m.name.span()))
                    .collect();
                if let Some(span) = call.select(&signatures) {
                    return Some(vec![(sf.file_name.clone(), span)]);
                }
            }
            // Namespace-qualified functions retain their declaration signatures.
            if matches!(member.object.kind, ExprKind::Member(_)) {
                let name = Expr {
                    kind: ExprKind::Ident(member.property.clone()),
                    span: expr.span,
                };
                return expression(qe, sf, index, &name, call, seen);
            }
            if call.construct {
                for candidate in &index.expressions {
                    let ExprKind::Assign(a) = &candidate.kind else {
                        continue;
                    };
                    if sf
                        .text
                        .get(a.left.span.start as usize..a.left.span.end as usize)
                        == sf
                            .text
                            .get(expr.span.start as usize..expr.span.end as usize)
                    {
                        if let ExprKind::ClassExpr(c) = &a.right.kind {
                            let mut targets = vec![(
                                sf.file_name.clone(),
                                Span::new(
                                    a.left.span.end - member.property.len() as u32,
                                    a.left.span.end,
                                ),
                            )];
                            targets.extend(class_constructors(qe, sf, index, c, call, seen)?);
                            return Some(targets);
                        }
                    }
                }
            }
            None
        }
        ExprKind::Paren(e) => expression(qe, sf, index, e, call, seen),
        _ => None,
    }
}

fn construct_signatures(
    index: &Index<'_>,
    ty: &TypeNode,
    call: &Call<'_>,
    depth: usize,
) -> Option<Vec<Span>> {
    if depth > 16 {
        return None;
    }
    match &ty.kind {
        TypeNodeKind::Constructor(c) => Some(
            call.select(&[(c.params.as_slice(), ty.span)])
                .into_iter()
                .collect(),
        ),
        TypeNodeKind::TypeLit(ms) => Some(
            call.select(
                &ms.iter()
                    .filter_map(|m| match &m.kind {
                        TypeMemberKind::ConstructSig(c) => Some((c.params.as_slice(), m.span)),
                        _ => None,
                    })
                    .collect::<Vec<_>>(),
            )
            .into_iter()
            .collect(),
        ),
        TypeNodeKind::Reference(r) => {
            let ExprKind::Ident(name) = &r.name.kind else {
                return None;
            };
            if let Some(a) = index.aliases.iter().find(|a| a.name == name.as_str()) {
                return construct_signatures(index, &a.type_ann, call, depth + 1);
            }
            let i = index.interfaces.iter().find(|i| i.name == name.as_str())?;
            Some(
                call.select(
                    &i.members
                        .iter()
                        .filter_map(|m| match &m.kind {
                            TypeMemberKind::ConstructSig(c) => Some((c.params.as_slice(), m.span)),
                            _ => None,
                        })
                        .collect::<Vec<_>>(),
                )
                .into_iter()
                .collect(),
            )
        }
        _ => None,
    }
}
