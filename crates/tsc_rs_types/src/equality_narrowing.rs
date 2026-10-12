use super::*;

impl TypeChecker {
    pub(super) fn exclude_equality_literal(&self, ty: &Type, literal: &Type, depth: usize) -> Type {
        if depth >= 16 || ty == literal {
            return if ty == literal {
                Type::Never
            } else {
                ty.clone()
            };
        }
        match ty {
            Type::Union(members) => Type::flatten_union(
                members
                    .iter()
                    .map(|member| self.exclude_equality_literal(member, literal, depth + 1))
                    .collect(),
            ),
            Type::Intersection(members)
                if members.iter().any(|member| {
                    matches!(
                        self.exclude_equality_literal(member, literal, depth + 1),
                        Type::Never
                    )
                }) =>
            {
                Type::Never
            }
            Type::TypeReference(name, _)
                if self.type_aliases.contains_key(name)
                    && self.alias_name_means_registered_alias(name) =>
            {
                if let Some(resolved) = self
                    .resolve_type_for_assignability(ty)
                    .filter(|resolved| resolved != ty)
                {
                    let remaining = self.exclude_equality_literal(&resolved, literal, depth + 1);
                    if remaining != resolved {
                        return remaining;
                    }
                }
                ty.clone()
            }
            _ => ty.clone(),
        }
    }

    /// Equality with unknown (or the empty object type) can establish a
    /// primitive type or prove that a value is an object. Loose equality
    /// permits coercion and only its nullish comparisons narrow unknown.
    pub(super) fn top_type_equality_narrowing(
        &self,
        binary: &tsc_rs_ast::BinaryExpr,
    ) -> Option<NarrowingResult> {
        let equal = matches!(binary.op, BinaryOp::Eq | BinaryOp::StrictEq);
        let strict = matches!(binary.op, BinaryOp::StrictEq | BinaryOp::StrictNe);
        if !equal && !matches!(binary.op, BinaryOp::Ne | BinaryOp::StrictNe) {
            return None;
        }
        for (reference, value) in [(&binary.left, &binary.right), (&binary.right, &binary.left)] {
            let Some(name) = Self::member_path(reference) else {
                continue;
            };
            let Some(source) = self.resolve_member_path_type(&name) else {
                continue;
            };
            if matches!(source, Type::Any) {
                return Some((Vec::new(), Vec::new()));
            }
            let unknown = self.is_unknown_receiver(&source);
            let empty = matches!(&source, Type::ObjectType(info)
                if info.properties.is_empty() && info.call_signatures.is_empty()
                    && info.construct_signatures.is_empty() && info.index_signature.is_none());
            if !unknown && !empty {
                continue;
            }
            let value_type = if matches!(&value.kind, ExprKind::Ident(name)
                if name == "undefined" && self.lookup_var(name).is_none())
            {
                Type::Undefined
            } else {
                self.infer_expr_type(value)
            };
            let (yes, no) = if matches!(value_type, Type::Null | Type::Undefined) {
                if !self.strict_null_checks {
                    (Vec::new(), Vec::new())
                } else if !unknown {
                    (vec![(name.clone(), Type::Never)], Vec::new())
                } else if strict {
                    (
                        vec![(name.clone(), value_type.clone())],
                        self.remove_type_from_var(&name, &value_type),
                    )
                } else {
                    (
                        self.keep_nullish_of_var(&name),
                        self.remove_nullish_from_var(&name),
                    )
                }
            } else if strict {
                let narrowed = self.unknown_equality_candidate_for_expr(value, &value_type);
                (
                    narrowed
                        .map(|ty| vec![(name.clone(), ty)])
                        .unwrap_or_default(),
                    Vec::new(),
                )
            } else {
                (Vec::new(), Vec::new())
            };
            return Some(if equal { (yes, no) } else { (no, yes) });
        }
        None
    }

    pub(super) fn unknown_equality_candidate_for_expr(
        &self,
        expression: &Expr,
        value: &Type,
    ) -> Option<Type> {
        if let ExprKind::Ident(name) = &expression.kind {
            if self.enum_info.contains_key(name.as_str())
                && matches!(value, Type::TypeReference(reference, _) if reference == name.as_str())
            {
                return Some(Type::Object);
            }
        }
        if let ExprKind::Member(member) = &expression.kind {
            if let ExprKind::Ident(name) = &member.object.kind {
                if matches!(self.lookup_var(name), Some(Type::TypeReference(reference, _)) if reference == name.as_str())
                {
                    if let Some((_, value)) =
                        self.enum_info.get(name.as_str()).and_then(|members| {
                            members
                                .iter()
                                .find(|(key, _)| key == member.property.as_str())
                        })
                    {
                        return Some(Type::EnumVariant {
                            enum_name: name.to_string(),
                            variant_name: member.property.to_string(),
                            value: Some(Arc::new(value.clone())),
                        });
                    }
                }
            }
        }
        self.unknown_equality_candidate(value, 0)
    }

    fn unknown_equality_candidate(&self, value: &Type, depth: usize) -> Option<Type> {
        if depth >= 16 {
            return None;
        }
        match value {
            Type::String
            | Type::Number
            | Type::BigInt
            | Type::Boolean
            | Type::Symbol
            | Type::UniqueSymbol(_)
            | Type::StringLiteral(_)
            | Type::NumberLiteral(_)
            | Type::BooleanLiteral(_)
            | Type::BigIntLiteral(_)
            | Type::Null
            | Type::Undefined
            | Type::Void
            | Type::Object
            | Type::EnumType(_)
            | Type::EnumVariant { .. } => Some(value.clone()),
            Type::ObjectType(info)
                if info.properties.is_empty()
                    && info.call_signatures.is_empty()
                    && info.construct_signatures.is_empty()
                    && info.index_signature.is_none() =>
            {
                Some(value.clone())
            }
            Type::ObjectType(_)
            | Type::Array(_)
            | Type::Tuple(_)
            | Type::Function(_)
            | Type::Constructor(_)
            | Type::Module(_)
            | Type::Namespace(_) => Some(Type::Object),
            Type::TypeReference(name, _) if self.active_type_parameter_name(value).is_none() => {
                if self.enum_info.contains_key(name.as_str()) {
                    Some(value.clone())
                } else if let Some(resolved) = self
                    .resolve_type_for_assignability(value)
                    .filter(|resolved| resolved != value)
                {
                    self.unknown_equality_candidate(&resolved, depth + 1)
                } else if self.class_info.contains_key(name.as_str())
                    || self.interface_info.contains_key(name.as_str())
                    || name.starts_with("typeof ")
                    || self.builtins.instance_members.contains_key(name.as_str())
                {
                    Some(Type::Object)
                } else {
                    None
                }
            }
            _ => None,
        }
    }
}
