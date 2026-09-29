use super::*;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum IndexKeyKind {
    Valid,
    Invalid,
    LiteralOrGeneric,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum TemplatePartKind {
    Finite,
    Pattern,
    Generic,
    Invalid,
}

impl TypeChecker {
    pub(crate) fn check_index_signature_type_grammar(
        &mut self,
        signature: &IndexSignature,
        span: Span,
    ) {
        if !self.check_index_grammar
            || self.current_file_is_javascript()
            || signature.modifiers & !(MOD_READONLY | MOD_STATIC) != 0
            || signature.params.len() != 1
        {
            return;
        }
        let param = &signature.params[0];
        if param.dotdotdot || param.optional || param.initializer.is_some() || param.modifiers != 0
        {
            return;
        }
        let Some(annotation) = &param.type_ann else {
            return;
        };
        let key = self.resolve_type_node(annotation);
        let diagnostic = match self.index_key_kind(&key, 0) {
            IndexKeyKind::LiteralOrGeneric => Some((1337,
                "An index signature parameter type cannot be a literal type or generic type. Consider using a mapped object type instead.", param.name.span)),
            IndexKeyKind::Invalid => Some((1268,
                "An index signature parameter type must be 'string', 'number', 'symbol', or a template literal type.", param.name.span)),
            IndexKeyKind::Valid if signature.type_ann.is_none() => Some((1021,
                "An index signature must have a type annotation.", span)),
            IndexKeyKind::Valid => None,
        };
        if let Some((code, message, span)) = diagnostic {
            if !self
                .diagnostics
                .iter()
                .any(|d| d.code == code && d.span == Some(span))
            {
                self.diagnostics.push(Diagnostic {
                    code,
                    message: message.into(),
                    category: DiagnosticCategory::Error,
                    file_name: None,
                    span: Some(span),
                    related: None,
                });
            }
        }
    }

    fn index_key_kind(&self, key: &Type, depth: usize) -> IndexKeyKind {
        use IndexKeyKind::*;
        if depth > 32 {
            return Invalid;
        }
        let classify = |key: &Type| self.index_key_kind(key, depth + 1);
        match key {
            Type::String | Type::Number | Type::Symbol => Valid,
            Type::StringLiteral(_)
            | Type::NumberLiteral(_)
            | Type::UniqueSymbol(_)
            | Type::EnumType(_)
            | Type::EnumVariant { .. }
            | Type::TypeParameter(_)
            | Type::Infer(_) => LiteralOrGeneric,
            Type::TypeReference(name, args) => {
                let enum_member = name.rsplit_once('.').is_some_and(|(owner, member)| {
                    self.enum_info
                        .get(owner)
                        .is_some_and(|members| members.iter().any(|(name, _)| name == member))
                });
                if self.type_param_name_is_active(name)
                    || self.enum_info.contains_key(name)
                    || enum_member
                {
                    return LiteralOrGeneric;
                }
                if let Some(expanded) = self
                    .expand_type_alias_once(name, args)
                    .or_else(|| self.resolve_type_for_assignability(key))
                {
                    return classify(&expanded);
                }
                Invalid
            }
            Type::Union(parts) => {
                let parts: Vec<_> = parts
                    .iter()
                    .map(|part| self.index_key_alias(part, depth + 1))
                    .collect();
                let normalized = crate::type_ops::simplify_union(parts.clone());
                if &normalized != key {
                    return classify(&normalized);
                }
                if parts
                    .iter()
                    .any(|part| matches!(part, Type::Any | Type::Unknown))
                {
                    return Invalid;
                }
                let string = parts.iter().any(|p| matches!(p, Type::String));
                let number = parts.iter().any(|p| matches!(p, Type::Number));
                let strict_null = self
                    .compiler_options
                    .strict_null_checks
                    .or(self.compiler_options.strict)
                    .unwrap_or(false);
                parts
                    .iter()
                    .filter(|part| {
                        !matches!(part, Type::Never)
                            && !(string && matches!(part, Type::StringLiteral(_)))
                            && !(number && matches!(part, Type::NumberLiteral(_)))
                            && (strict_null || !matches!(part, Type::Null | Type::Undefined))
                    })
                    .map(classify)
                    .max()
                    .unwrap_or(Invalid)
            }
            Type::Intersection(parts) => {
                let parts: Vec<_> = parts
                    .iter()
                    .map(|part| self.index_key_alias(part, depth + 1))
                    .collect();
                let normalized = crate::type_ops::simplify_intersection(parts.clone());
                if &normalized != key {
                    return classify(&normalized);
                }
                let primitive_domains = parts.iter().fold(0u32, |domains, part| {
                    domains
                        | match part {
                            Type::String | Type::StringLiteral(_) => 1,
                            Type::Number | Type::NumberLiteral(_) => 2,
                            Type::Symbol | Type::UniqueSymbol(_) => 4,
                            Type::Boolean | Type::BooleanLiteral(_) => 8,
                            Type::BigInt | Type::BigIntLiteral(_) => 16,
                            Type::Null => 32,
                            Type::Undefined => 64,
                            _ => 0,
                        }
                });
                if primitive_domains.count_ones() > 1 {
                    return Invalid;
                }
                let kinds: Vec<_> = parts.iter().map(classify).collect();
                if kinds.contains(&LiteralOrGeneric) {
                    LiteralOrGeneric
                } else if kinds.contains(&Valid) {
                    Valid
                } else {
                    Invalid
                }
            }
            Type::TemplateLiteral { types, .. } => {
                match types
                    .iter()
                    .map(|part| self.index_template_part(part, depth + 1))
                    .max()
                    .unwrap_or(TemplatePartKind::Finite)
                {
                    TemplatePartKind::Pattern => Valid,
                    TemplatePartKind::Finite | TemplatePartKind::Generic => LiteralOrGeneric,
                    TemplatePartKind::Invalid => Invalid,
                }
            }
            Type::StringMapping { inner, .. } => classify(inner),
            Type::Typeof(name) => self.lookup_var(name).map_or(Invalid, |ty| classify(&ty)),
            Type::Keyof(inner) => {
                if matches!(inner.as_ref(), Type::Any | Type::Never | Type::Error) {
                    return Valid;
                }
                if let Type::TypeReference(name, args) = inner.as_ref() {
                    if self.type_param_name_is_active(name) {
                        return LiteralOrGeneric;
                    }
                    let expanded = self.resolve_type_reference_to_object(name, args);
                    return match expanded {
                        Some(expanded) => classify(&Type::Keyof(Arc::new(expanded))),
                        // An unresolved name has the error-any key domain.
                        None => Valid,
                    };
                }
                let resolved = crate::type_ops::resolve_keyof(inner);
                if &resolved == key {
                    LiteralOrGeneric
                } else {
                    classify(&resolved)
                }
            }
            Type::Conditional { .. } | Type::IndexedAccess(_, _) => {
                let resolved = self.simplify_type(key);
                if &resolved == key {
                    LiteralOrGeneric
                } else {
                    classify(&resolved)
                }
            }
            _ => Invalid,
        }
    }

    fn index_key_alias(&self, key: &Type, depth: usize) -> Type {
        if depth <= 32 {
            if let Type::TypeReference(name, args) = key {
                if !self.type_param_name_is_active(name) {
                    if let Some(expanded) = self.expand_type_alias_once(name, args) {
                        return self.index_key_alias(&expanded, depth + 1);
                    }
                }
            }
        }
        key.clone()
    }

    fn index_template_part(&self, key: &Type, depth: usize) -> TemplatePartKind {
        use TemplatePartKind::*;
        if depth > 32 {
            return Invalid;
        }
        let key = self.index_key_alias(key, depth + 1);
        match &key {
            Type::Any | Type::String | Type::Number | Type::BigInt => Pattern,
            Type::Boolean
            | Type::BooleanLiteral(_)
            | Type::StringLiteral(_)
            | Type::NumberLiteral(_)
            | Type::BigIntLiteral(_)
            | Type::Null
            | Type::Undefined => Finite,
            Type::Union(parts) | Type::TemplateLiteral { types: parts, .. } => parts
                .iter()
                .map(|part| self.index_template_part(part, depth + 1))
                .max()
                .unwrap_or(Finite),
            Type::StringMapping { inner, .. } => self.index_template_part(inner, depth + 1),
            _ => match self.index_key_kind(&key, depth + 1) {
                IndexKeyKind::Valid => Pattern,
                IndexKeyKind::LiteralOrGeneric => Generic,
                IndexKeyKind::Invalid => Invalid,
            },
        }
    }
}
