//! Preserve property origins through type annotations and inferred generic
//! return types. Mapped properties keep the declaration of their source key.

use super::{property_name, Index};
use std::collections::HashMap;
use tsc_rs_ast::*;

#[derive(Clone)]
enum Origin<'a> {
    Type(&'a TypeNode),
    Expr(&'a Expr),
    Key(String),
}
type Bindings<'a> = HashMap<String, Origin<'a>>;

pub(super) fn resolve(index: &Index<'_>, offset: u32) -> Option<Vec<Span>> {
    let lookup = Lookup { index };
    if let Some(targets) = lookup.contextual(offset) {
        return Some(targets);
    }
    let (object, property) = index
        .expressions
        .iter()
        .filter_map(|e| match &e.kind {
            ExprKind::Member(m)
                if e.span.end.saturating_sub(m.property.len() as u32) <= offset
                    && offset <= e.span.end =>
            {
                Some((m.object.as_ref(), m.property.as_str()))
            }
            ExprKind::ElemAccess(m)
                if m.index.span.start <= offset && offset <= m.index.span.end =>
            {
                match &m.index.kind {
                    ExprKind::StrLit(n) => Some((m.object.as_ref(), n.as_str())),
                    _ => None,
                }
            }
            _ => None,
        })
        .next()?;
    if matches!(object.kind, ExprKind::This) {
        if let Some(class) = index.enclosing_class(offset) {
            let spans: Vec<_> = class
                .members
                .iter()
                .filter_map(|m| match &m.kind {
                    ClassMemberKind::GetAccessor(a) | ClassMemberKind::SetAccessor(a)
                        if property_name(&a.name).is_some_and(|n| {
                            n.trim_start_matches('#') == property.trim_start_matches('#')
                        }) =>
                    {
                        Some(a.name.span())
                    }
                    _ => None,
                })
                .collect();
            if !spans.is_empty() {
                return Some(spans);
            }
        }
    }
    if let ExprKind::Ident(receiver) = &object.kind {
        let mut assigned = vec![];
        for expr in &index.expressions {
            if let ExprKind::Assign(a) = &expr.kind {
                if let ExprKind::Member(m) = &a.left.kind {
                    if m.property == property
                        && matches!(&m.object.kind, ExprKind::Ident(n) if n == receiver)
                        && index.binding(receiver, object.span.start).map(|s| s.id)
                            == index.binding(receiver, m.object.span.start).map(|s| s.id)
                    {
                        assigned.push(Span::new(
                            a.left.span.end - property.len() as u32,
                            a.left.span.end,
                        ));
                    }
                }
                if let ExprKind::ElemAccess(m) = &a.left.kind {
                    if matches!(&m.object.kind, ExprKind::Ident(n) if n == receiver)
                        && matches!(&m.index.kind, ExprKind::StrLit(n) if n == property)
                        && index.binding(receiver, object.span.start).map(|s| s.id)
                            == index.binding(receiver, m.object.span.start).map(|s| s.id)
                    {
                        assigned.push(m.index.span);
                    }
                }
            }
        }
        if !assigned.is_empty() {
            return Some(assigned);
        }
    }
    lookup.expression(object, property, &Bindings::new(), 0)
}

struct Lookup<'a, 'b> {
    index: &'b Index<'a>,
}

impl<'a> Lookup<'a, '_> {
    fn contextual(&self, offset: u32) -> Option<Vec<Span>> {
        for expr in &self.index.expressions {
            let ExprKind::ObjectLit(props) = &expr.kind else {
                continue;
            };
            for p in props {
                let (key, span, shorthand) = match p {
                    ObjLitProp::Property(p) => (property_name(&p.key)?, p.key.span(), false),
                    ObjLitProp::Shorthand(n, s) => (n.as_str(), *s, true),
                    _ => continue,
                };
                if !(span.start <= offset && offset <= span.end) {
                    continue;
                }
                let mut lexical = if shorthand {
                    self.index
                        .binding(key, offset)
                        .map(|s| s.declarations.iter().map(|d| d.span).collect())
                        .unwrap_or_default()
                } else {
                    vec![]
                };
                if let Some(var) = self
                    .index
                    .variables
                    .iter()
                    .find(|v| v.init.as_ref().is_some_and(|e| e.span == expr.span))
                {
                    if let Some(t) = &var.type_ann {
                        lexical.extend(self.context_type(t, props, key, &Bindings::new(), 0)?);
                        return Some(lexical);
                    }
                }
                for outer in &self.index.expressions {
                    let ExprKind::Call(call) = &outer.kind else {
                        continue;
                    };
                    let Some(arg) = call.args.iter().position(|a| a.span == expr.span) else {
                        continue;
                    };
                    let ExprKind::Ident(name) = &call.callee.kind else {
                        continue;
                    };
                    let Some(f) = self
                        .index
                        .functions
                        .iter()
                        .find(|f| f.name.as_deref() == Some(name.as_str()))
                    else {
                        continue;
                    };
                    let env: Bindings<'_> = f
                        .type_params
                        .iter()
                        .flatten()
                        .filter_map(|p| {
                            p.constraint
                                .as_deref()
                                .map(|c| (p.name.clone(), Origin::Type(c)))
                        })
                        .collect();
                    if let Some(t) = f.params.get(arg).and_then(|p| p.type_ann.as_ref()) {
                        lexical.extend(self.context_type(t, props, key, &env, 0)?);
                        return Some(lexical);
                    }
                }
                if !lexical.is_empty() {
                    return Some(lexical);
                }
            }
        }
        // Typed destructuring parameters navigate to the input's property.
        for param in &self.index.parameters {
            let PatKind::Object(props) = &param.name.kind else {
                continue;
            };
            for prop in props {
                let (name, span) = match prop {
                    ObjPatProp::Shorthand(n, s) => (n.as_str(), *s),
                    ObjPatProp::KeyValue(k, p) => (property_name(k)?, p.span),
                    _ => continue,
                };
                if span.start <= offset && offset <= span.end {
                    if let Some(t) = &param.type_ann {
                        return self.ty(t, name, &Bindings::new(), 0);
                    }
                    for outer in &self.index.expressions {
                        let ExprKind::Call(call) = &outer.kind else {
                            continue;
                        };
                        let Some(arg_index) = call
                            .args
                            .iter()
                            .position(|e| e.span.start <= span.start && span.end <= e.span.end)
                        else {
                            continue;
                        };
                        let ExprKind::Ident(callee) = &call.callee.kind else {
                            continue;
                        };
                        let Some(f) = self
                            .index
                            .functions
                            .iter()
                            .find(|f| f.name.as_deref() == Some(callee.as_str()))
                        else {
                            continue;
                        };
                        let Some(TypeNode {
                            kind: TypeNodeKind::Function(callback),
                            ..
                        }) = f.params.get(arg_index).and_then(|p| p.type_ann.as_ref())
                        else {
                            continue;
                        };
                        let env: Bindings<'_> = f
                            .type_params
                            .iter()
                            .flatten()
                            .zip(call.type_args.iter().flatten())
                            .map(|(p, t)| (p.name.clone(), Origin::Type(t)))
                            .collect();
                        if let Some(t) = callback.params.first().and_then(|p| p.type_ann.as_ref()) {
                            return self.ty(t, name, &env, 0);
                        }
                    }
                }
            }
        }
        None
    }

    fn context_type(
        &self,
        ty: &'a TypeNode,
        props: &'a [ObjLitProp],
        key: &str,
        env: &Bindings<'a>,
        depth: usize,
    ) -> Option<Vec<Span>> {
        if depth > 32 {
            return None;
        }
        if let TypeNodeKind::Reference(r) = &ty.kind {
            if let ExprKind::Ident(n) = &r.name.kind {
                if let Some(a) = self.index.aliases.iter().find(|a| a.name == n.as_str()) {
                    return self.context_type(&a.type_ann, props, key, env, depth + 1);
                }
            }
        }
        if let TypeNodeKind::Union(types) = &ty.kind {
            let compatible: Vec<_> = types.iter().filter(|t| {
                props.iter().all(|p| {
                    let ObjLitProp::Property(p) = p else { return true; };
                    let Some(name) = property_name(&p.key) else { return true; };
                    let members = match &t.kind {
                        TypeNodeKind::TypeLit(ms) => Some(ms.as_slice()),
                        TypeNodeKind::Reference(r) => match &r.name.kind { ExprKind::Ident(n) => self.index.interfaces.iter().find(|i| i.name == n.as_str()).map(|i| i.members.as_slice()), _ => None },
                        _ => None,
                    };
                    members.into_iter().flatten().all(|m| match &m.kind {
                        TypeMemberKind::PropertySig(s) if property_name(&s.name) == Some(name) => match s.type_ann.as_ref().map(|t| &t.kind) {
                            Some(TypeNodeKind::Literal(LiteralTypeKind::String(expected))) => matches!(&p.value.kind, ExprKind::StrLit(actual) if actual == expected),
                            _ => true,
                        },
                        _ => true,
                    })
                })
            }).collect();
            let candidates: Vec<_> = if compatible.is_empty() {
                types.iter().collect()
            } else {
                compatible
            };
            let mut out = vec![];
            for t in candidates {
                out.extend(self.ty(t, key, env, depth + 1)?);
            }
            return Some(out);
        }
        self.ty(ty, key, env, depth + 1)
    }
    fn origin(
        &self,
        origin: &Origin<'a>,
        key: &str,
        env: &Bindings<'a>,
        depth: usize,
    ) -> Option<Vec<Span>> {
        match origin {
            Origin::Type(t) => self.ty(t, key, env, depth + 1),
            Origin::Expr(e) => self.expression(e, key, env, depth + 1),
            Origin::Key(_) => None,
        }
    }

    fn expression(
        &self,
        expr: &'a Expr,
        key: &str,
        env: &Bindings<'a>,
        depth: usize,
    ) -> Option<Vec<Span>> {
        if depth > 32 {
            return None;
        }
        if let Some(inner) = expr.kind.type_layer_inner() {
            return self.expression(inner, key, env, depth + 1);
        }
        match &expr.kind {
            ExprKind::Paren(e) => self.expression(e, key, env, depth + 1),
            ExprKind::Ident(name) => {
                let Some(var) = self.index.variable(name, expr.span.start) else {
                    let symbol = self.index.binding(name, expr.span.start)?;
                    let param = self
                        .index
                        .parameters
                        .iter()
                        .find(|p| symbol.declarations.iter().any(|d| d.span == p.name.span))?;
                    return self.ty(param.type_ann.as_ref()?, key, env, depth + 1);
                };
                if let Some(ty) = &var.type_ann {
                    return self.ty(ty, key, env, depth + 1);
                }
                self.expression(var.init.as_deref()?, key, env, depth + 1)
            }
            ExprKind::ObjectLit(properties) => {
                let mut found = vec![];
                for prop in properties {
                    let name = match prop {
                        ObjLitProp::Property(p) => Some(&p.key),
                        ObjLitProp::Method(p) => Some(&p.name),
                        ObjLitProp::Get(p) | ObjLitProp::Set(p) => Some(&p.name),
                        ObjLitProp::Shorthand(n, span)
                        | ObjLitProp::ShorthandDefault(n, _, span)
                            if n == key =>
                        {
                            found.push(*span);
                            None
                        }
                        ObjLitProp::Spread(e, _) => {
                            if let Some(spans) = self.expression(e, key, env, depth + 1) {
                                found.extend(spans);
                            }
                            None
                        }
                        _ => None,
                    };
                    if let Some(name) = name {
                        if property_name(name) == Some(key) {
                            found.push(name.span());
                        }
                    }
                }
                Some(found)
            }
            ExprKind::Call(call) => {
                let ExprKind::Ident(name) = &call.callee.kind else {
                    return None;
                };
                let function = self
                    .index
                    .functions
                    .iter()
                    .find(|f| f.name.as_deref() == Some(name.as_str()))?;
                let mut bindings = env.clone();
                if let Some(params) = &function.type_params {
                    for (i, param) in params.iter().enumerate() {
                        if let Some(arg) = call.type_args.as_ref().and_then(|a| a.get(i)) {
                            bindings.insert(param.name.clone(), Origin::Type(arg));
                        }
                    }
                    for (param, arg) in function.params.iter().zip(&call.args) {
                        if let Some(TypeNode {
                            kind: TypeNodeKind::Reference(r),
                            ..
                        }) = &param.type_ann
                        {
                            if let ExprKind::Ident(n) = &r.name.kind {
                                if params.iter().any(|p| p.name == n.as_str()) {
                                    bindings.entry(n.to_string()).or_insert(Origin::Expr(arg));
                                }
                            }
                        }
                    }
                }
                if let Some(t) = &function.return_type {
                    self.ty(t, key, &bindings, depth + 1)
                } else {
                    function.body.as_ref()?.iter().find_map(|s| match &s.kind {
                        StmtKind::Return(Some(e)) => self.expression(e, key, &bindings, depth + 1),
                        _ => None,
                    })
                }
            }
            ExprKind::Binary(b) if b.op == BinaryOp::LogOr => {
                self.expression(&b.left, key, env, depth + 1)
            }
            ExprKind::Member(m) => {
                let ExprKind::Ident(name) = &m.object.kind else {
                    return None;
                };
                let v = self.index.variable(name, m.object.span.start)?;
                self.nested(v.type_ann.as_ref()?, &m.property, key, env, depth + 1)
            }
            _ => None,
        }
    }

    fn nested(
        &self,
        ty: &'a TypeNode,
        first: &str,
        key: &str,
        env: &Bindings<'a>,
        depth: usize,
    ) -> Option<Vec<Span>> {
        if depth > 32 {
            return None;
        }
        let members = match &ty.kind {
            TypeNodeKind::TypeLit(ms) => ms.as_slice(),
            TypeNodeKind::Reference(r) => {
                let ExprKind::Ident(name) = &r.name.kind else {
                    return None;
                };
                if let Some(a) = self.index.aliases.iter().find(|a| a.name == name.as_str()) {
                    return self.nested(&a.type_ann, first, key, env, depth + 1);
                }
                self.index
                    .interfaces
                    .iter()
                    .find(|i| i.name == name.as_str())?
                    .members
                    .as_slice()
            }
            TypeNodeKind::Union(ts) | TypeNodeKind::Intersection(ts) => {
                let mut result = vec![];
                for t in ts {
                    result.extend(self.nested(t, first, key, env, depth + 1)?);
                }
                return Some(result);
            }
            _ => return None,
        };
        let prop = members.iter().find_map(|m| match &m.kind {
            TypeMemberKind::PropertySig(p) if property_name(&p.name) == Some(first) => {
                p.type_ann.as_ref()
            }
            _ => None,
        })?;
        self.ty(prop, key, env, depth + 1)
    }

    fn ty(
        &self,
        ty: &'a TypeNode,
        key: &str,
        env: &Bindings<'a>,
        depth: usize,
    ) -> Option<Vec<Span>> {
        if depth > 32 {
            return None;
        }
        match &ty.kind {
            TypeNodeKind::TypeQuery(e) => self.expression(e, key, env, depth + 1),
            TypeNodeKind::Paren(t) | TypeNodeKind::Readonly(t) => self.ty(t, key, env, depth + 1),
            TypeNodeKind::Reference(r) => {
                let ExprKind::Ident(name) = &r.name.kind else {
                    return None;
                };
                if let Some(origin) = env.get(name.as_str()) {
                    return self.origin(origin, key, env, depth + 1);
                }
                if let Some(alias) = self.index.aliases.iter().find(|a| a.name == name.as_str()) {
                    let mut bindings = env.clone();
                    if let Some(params) = &alias.type_params {
                        for (param, arg) in params.iter().zip(r.type_args.as_deref().unwrap_or(&[]))
                        {
                            // Resolve the caller's type parameters before rebinding
                            // the callee's parameters (both may be named T).
                            let value = match &arg.kind {
                                TypeNodeKind::Reference(r) => match &r.name.kind {
                                    ExprKind::Ident(n) => env.get(n.as_str()).cloned(),
                                    _ => None,
                                },
                                _ => None,
                            }
                            .unwrap_or(Origin::Type(arg));
                            bindings.insert(param.name.clone(), value);
                        }
                    }
                    return self.ty(&alias.type_ann, key, &bindings, depth + 1);
                }
                let interface = self
                    .index
                    .interfaces
                    .iter()
                    .find(|i| i.name == name.as_str())?;
                let mut result = self.members(&interface.members, key, env, depth + 1)?;
                if result.is_empty() {
                    for parent in &interface.extends {
                        if let Some(spans) = self.ty(parent, key, env, depth + 1) {
                            result.extend(spans);
                        }
                    }
                }
                Some(result)
            }
            TypeNodeKind::TypeLit(members) => self.members(members, key, env, depth + 1),
            TypeNodeKind::Union(types) | TypeNodeKind::Intersection(types) => {
                let mut result = vec![];
                for ty in types {
                    result.extend(self.ty(ty, key, env, depth + 1)?);
                }
                Some(result)
            }
            TypeNodeKind::Mapped(m) => {
                let TypeNodeKind::Keyof(source) = &m.type_param.constraint.as_ref()?.kind else {
                    return None;
                };
                let mut bindings = env.clone();
                bindings.insert(m.type_param.name.clone(), Origin::Key(key.into()));
                if let Some(name_type) = &m.name_type {
                    if !self.accepts(name_type, key, &bindings, depth + 1)? {
                        return Some(vec![]);
                    }
                }
                self.ty(source, key, env, depth + 1)
            }
            _ => None,
        }
    }

    fn members(
        &self,
        members: &'a [TypeMember],
        key: &str,
        env: &Bindings<'a>,
        depth: usize,
    ) -> Option<Vec<Span>> {
        let mut properties = vec![];
        let mut indices = vec![];
        for member in members {
            let name = match &member.kind {
                TypeMemberKind::PropertySig(p) => Some(&p.name),
                TypeMemberKind::MethodSig(p) => Some(&p.name),
                TypeMemberKind::GetAccessorSig(p) | TypeMemberKind::SetAccessorSig(p) => {
                    Some(&p.name)
                }
                TypeMemberKind::IndexSig(i) => {
                    if let Some(t) = i.params.first().and_then(|p| p.type_ann.as_ref()) {
                        if self.accepts(t, key, env, depth + 1)? {
                            indices.push(member.span);
                        }
                    }
                    None
                }
                _ => None,
            };
            if let Some(name) = name {
                if property_name(name) == Some(key) {
                    properties.push(name.span());
                }
            }
        }
        Some(if properties.is_empty() {
            indices
        } else {
            properties
        })
    }

    fn accepts(
        &self,
        ty: &'a TypeNode,
        key: &str,
        env: &Bindings<'a>,
        depth: usize,
    ) -> Option<bool> {
        if depth > 32 {
            return None;
        }
        match &ty.kind {
            TypeNodeKind::Keyword(KeywordTypeKind::String) => Some(true),
            TypeNodeKind::Keyword(KeywordTypeKind::Never) => Some(false),
            TypeNodeKind::Keyword(KeywordTypeKind::Number) => Some(key.parse::<f64>().is_ok()),
            TypeNodeKind::Literal(LiteralTypeKind::String(s)) => Some(s == key),
            TypeNodeKind::Keyof(t) => self
                .ty(t, key, env, depth + 1)
                .map(|spans| !spans.is_empty()),
            TypeNodeKind::Union(types) => {
                for t in types {
                    if self.accepts(t, key, env, depth + 1)? {
                        return Some(true);
                    }
                }
                Some(false)
            }
            TypeNodeKind::TemplateLit(t) => {
                // Consume the fixed pieces in order, testing each substitution
                // against its key type. Backtracking handles repeated suffixes.
                self.template(&t.quasis, &t.types, key, env, depth + 1)
            }
            TypeNodeKind::Reference(r) => {
                let ExprKind::Ident(n) = &r.name.kind else {
                    return None;
                };
                if n == "Exclude" {
                    let args = r.type_args.as_ref()?;
                    return Some(
                        self.accepts(args.first()?, key, env, depth + 1)?
                            && !self.accepts(args.get(1)?, key, env, depth + 1)?,
                    );
                }
                match env.get(n.as_str())? {
                    Origin::Key(k) => Some(k == key),
                    Origin::Type(t) => self.accepts(t, key, env, depth + 1),
                    Origin::Expr(_) => None,
                }
            }
            TypeNodeKind::Conditional(c) => {
                let checked = match &c.check.kind {
                    TypeNodeKind::Reference(r) => match &r.name.kind {
                        ExprKind::Ident(n) => match env.get(n.as_str())? {
                            Origin::Key(k) => k.as_str(),
                            _ => return None,
                        },
                        _ => return None,
                    },
                    _ => return None,
                };
                self.accepts(
                    if self.accepts(&c.extends, checked, env, depth + 1)? {
                        &c.true_type
                    } else {
                        &c.false_type
                    },
                    key,
                    env,
                    depth + 1,
                )
            }
            _ => None,
        }
    }

    fn template(
        &self,
        quasis: &'a [TemplateElement],
        types: &'a [TypeNode],
        key: &str,
        env: &Bindings<'a>,
        depth: usize,
    ) -> Option<bool> {
        if depth > 32 {
            return None;
        }
        let first = quasis.first()?;
        let Some(rest) = key.strip_prefix(first.cooked.as_deref().unwrap_or(&first.raw)) else {
            return Some(false);
        };
        let Some(ty) = types.first() else {
            return Some(rest.is_empty());
        };
        for split in (0..=rest.len()).filter(|i| rest.is_char_boundary(*i)) {
            if self.accepts(ty, &rest[..split], env, depth + 1)?
                && self.template(&quasis[1..], &types[1..], &rest[split..], env, depth + 1)?
            {
                return Some(true);
            }
        }
        Some(false)
    }
}
