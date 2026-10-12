use super::*;

impl TypeChecker {
    /// Diagnose an absent index only when the receiver's member table is
    /// complete. Unresolved generic and computed shapes retain their existing
    /// lookup until their index domains can be established.
    pub(super) fn report_missing_element_index(
        &mut self,
        expression: &Expr,
        receiver: &Type,
        index: &Type,
    ) -> Option<Type> {
        let ExprKind::ElemAccess(access) = &expression.kind else {
            return None;
        };
        if !self.no_implicit_any || self.current_file_is_js() {
            return None;
        }
        // Super currently uses an instance shape even in static containers.
        // That shape cannot establish absence on the constructor side.
        if matches!(access.object.kind, ExprKind::Super) {
            return None;
        }
        // An inferred `any` can stand for an unresolved generic property.
        // Only a directly bound any-valued key is conclusive here.
        if matches!(index, Type::Any)
            && !matches!(&access.index.kind, ExprKind::Ident(name)
                if matches!(self.lookup_declared_var(name), Some(Type::Any)))
        {
            return None;
        }
        if let ExprKind::Ident(name) = &access.object.kind {
            if self.lookup_declared_var(name).is_some_and(|declared| {
                declared != *receiver && self.element_index_domain(&declared, 0).is_none()
            }) {
                // Predicate narrowing can currently lose a declared index
                // signature; do not treat that approximation as a closed type.
                return None;
            }
        }
        let Some(numeric_only) = self.element_index_domain(receiver, 0) else {
            return None;
        };
        if numeric_only && self.is_numeric_for_in_key(&access.index) {
            return None;
        }
        let keys = self.element_index_keys(index);
        let writing = self.assignment_target_span == Some(expression.span);
        let literal = Self::index_receiver_literal(&access.object);
        let fresh = !writing
            && literal.is_some()
            && keys
                .iter()
                .all(|(key, _)| matches!(key, Type::StringLiteral(_) | Type::NumberLiteral(_)));
        if fresh && !numeric_only {
            let mut values = Vec::new();
            let mut missing = Vec::new();
            for (key, _) in &keys {
                if let Some(value) = self.known_literal_index_read(receiver, key, 0) {
                    values.push(value);
                } else {
                    let name = match key {
                        Type::StringLiteral(name) => name.clone(),
                        Type::NumberLiteral(name) => Self::canonical_numeric_property_name(name),
                        _ => return None,
                    };
                    if self.intersection_property_presence(receiver, &name, 0) != Some(false) {
                        return None;
                    }
                    missing.push(name);
                    values.push(Type::Undefined);
                }
            }
            if missing.is_empty() {
                return None;
            }
            let shown = receiver.display_string_single_line();
            missing.sort();
            missing.dedup();
            for property in missing {
                self.diagnostics.push(Diagnostic {
                    code: 2339,
                    message: format!("Property '{property}' does not exist on type '{shown}'."),
                    category: DiagnosticCategory::Error,
                    file_name: None,
                    span: Some(expression.span),
                    related: None,
                });
            }
            return Some(self.array_literal_union(values));
        }
        let mut missing = None;
        for (key, detail) in &keys {
            match key {
                Type::StringLiteral(name) | Type::NumberLiteral(name) => {
                    if numeric_only && self.index_applies_to_property(key, &Type::Number) {
                        continue;
                    }
                    if self.intersection_property_presence(receiver, name, 0) != Some(false) {
                        continue;
                    }
                    missing = Some(Some(detail.clone().unwrap_or_else(|| name.clone())));
                }
                Type::UniqueSymbol(identity) => {
                    let name = Self::unique_symbol_property_key(identity);
                    if self.intersection_property_presence(receiver, &name, 0) != Some(false) {
                        continue;
                    }
                    missing = Some(Some(format!(
                        "[{}]",
                        Self::unique_symbol_display_name(identity)
                    )));
                }
                Type::Any if numeric_only => continue,
                Type::Number if numeric_only => continue,
                Type::Any | Type::String | Type::Number | Type::Symbol if !fresh => {
                    missing = Some(None);
                }
                _ => return None,
            }
            if missing.is_some() {
                break;
            }
        }
        let Some(property) = missing else {
            return None;
        };
        let shown = match receiver {
            Type::TypeReference(name, _) if self.enum_info.contains_key(name.as_str()) => {
                format!("typeof {name}")
            }
            _ if fresh => self
                .fresh_index_receiver_display(literal.unwrap(), receiver)
                .display_string_single_line(),
            _ => self
                .index_diagnostic_display_type(receiver)
                .display_string_single_line(),
        };
        if let Some(property) = property.as_ref().filter(|_| !fresh) {
            if self.index_receiver_has_static_property(receiver, property) {
                let key = self
                    .current_source
                    .as_deref()
                    .and_then(|source| {
                        source.get(access.index.span.start as usize..access.index.span.end as usize)
                    })
                    .map(str::to_owned)
                    .unwrap_or_else(|| index.display_string_single_line());
                self.diagnostics.push(Diagnostic {
                    code: 2576,
                    message: format!("Property '{property}' does not exist on type '{shown}'. Did you mean to access the static member '{shown}[{key}]' instead?"),
                    category: DiagnosticCategory::Error,
                    file_name: None, span: Some(expression.span), related: None,
                });
                return Some(Type::Any);
            }
        }
        if let Some(property) = property.as_ref().filter(|_| !fresh && !numeric_only) {
            let suggested =
                self.missing_property_diagnostic(receiver, property, &shown, access.index.span);
            if suggested.code == 2551 {
                self.diagnostics.push(suggested);
                return Some(Type::Any);
            }
        }
        let (code, message, span) = if numeric_only {
            (7015, "Element implicitly has an 'any' type because index expression is not of type 'number'.".to_string(), access.index.span)
        } else if fresh {
            let Some(property) = property else {
                return None;
            };
            (
                2339,
                format!("Property '{property}' does not exist on type '{shown}'."),
                expression.span,
            )
        } else {
            if let Some(method) = self.index_method_suggestion(receiver, index, writing) {
                let suggestion = Self::index_receiver_name(&access.object)
                    .map(|name| format!("{name}.{method}"))
                    .unwrap_or_else(|| method.to_owned());
                self.diagnostics.push(Diagnostic {
                    code: 7052,
                    message: format!("Element implicitly has an 'any' type because type '{shown}' has no index signature. Did you mean to call '{suggestion}'?"),
                    category: DiagnosticCategory::Error,
                    file_name: None, span: Some(expression.span), related: None,
                });
                return Some(Type::Any);
            }
            let detail = if let Some(property) = property {
                format!("\n  Property '{property}' does not exist on type '{shown}'.")
            } else if matches!(index, Type::String | Type::Number) {
                format!("\n  No index signature with a parameter of type '{}' was found on type '{shown}'.", index.display_string_single_line())
            } else {
                String::new()
            };
            let key_shown = if matches!(index, Type::UniqueSymbol(_)) {
                "unique symbol".to_string()
            } else {
                index.display_string_single_line()
            };
            (7053, format!("Element implicitly has an 'any' type because expression of type '{key_shown}' can't be used to index type '{shown}'.{detail}"), expression.span)
        };
        self.diagnostics.push(Diagnostic {
            code,
            message,
            category: DiagnosticCategory::Error,
            file_name: None,
            span: Some(span),
            related: None,
        });
        Some(if fresh { Type::Undefined } else { Type::Any })
    }

    pub(super) fn element_index_keys(&self, index: &Type) -> Vec<(Type, Option<String>)> {
        match index {
            Type::Union(keys) => keys
                .iter()
                .flat_map(|key| self.element_index_keys(key))
                .collect(),
            Type::TypeReference(name, _) => {
                if let Some(members) = self.enum_info.get(name) {
                    return members
                        .iter()
                        .map(|(member, value)| {
                            (
                                value.clone(),
                                Some(if members.len() == 1 {
                                    format!("[{name}]")
                                } else {
                                    format!("[{name}.{member}]")
                                }),
                            )
                        })
                        .collect();
                }
                if let Some((owner, member)) = name.rsplit_once('.') {
                    if let Some(value) = self
                        .enum_info
                        .get(owner)
                        .and_then(|members| members.iter().find(|(key, _)| key == member))
                    {
                        return vec![(value.1.clone(), Some(format!("[{name}]")))];
                    }
                }
                vec![(index.clone(), None)]
            }
            Type::EnumVariant {
                enum_name,
                variant_name,
                value: Some(value),
            } => {
                vec![(
                    Type::clone(value),
                    Some(format!("[{enum_name}.{variant_name}]")),
                )]
            }
            _ => vec![(index.clone(), None)],
        }
    }

    pub(super) fn is_named_object_index(&self, index: &Type) -> bool {
        let Type::TypeReference(name, args) = index else {
            return false;
        };
        self.active_type_parameter_name(index).is_none()
            && self.enum_target_member_values(name).is_none()
            && matches!(
                self.resolve_type_reference_to_object(name, args),
                Some(Type::ObjectType(_))
            )
    }

    fn index_receiver_literal(expression: &Expr) -> Option<&Expr> {
        match &expression.kind {
            ExprKind::ObjectLit(_) => Some(expression),
            ExprKind::Paren(inner) => Self::index_receiver_literal(inner),
            ExprKind::Member(member) => {
                let object = Self::index_receiver_literal(&member.object)?;
                let ExprKind::ObjectLit(properties) = &object.kind else {
                    return None;
                };
                Self::index_literal_property(properties, &member.property)
                    .and_then(Self::index_receiver_literal)
            }
            _ => None,
        }
    }

    fn index_literal_property<'a>(properties: &'a [ObjLitProp], key: &str) -> Option<&'a Expr> {
        for property in properties.iter().rev() {
            match property {
                ObjLitProp::Property(property) => {
                    if matches!(&property.key, PropName::Computed(..)) {
                        return None;
                    }
                    if matches!(&property.key, PropName::Ident(name, _) | PropName::String(name, _) if name == key)
                    {
                        return Some(&property.value);
                    }
                }
                ObjLitProp::Spread(expression, _) => {
                    let literal = Self::index_receiver_literal(expression)?;
                    let ExprKind::ObjectLit(properties) = &literal.kind else {
                        return None;
                    };
                    if let Some(value) = Self::index_literal_property(properties, key) {
                        return Some(value);
                    }
                }
                ObjLitProp::Shorthand(name, _) | ObjLitProp::ShorthandDefault(name, _, _)
                    if name == key =>
                {
                    return None
                }
                _ => {}
            }
        }
        None
    }

    pub(super) fn fresh_index_receiver_type(&self, expression: &Expr, ty: Type) -> Type {
        Self::index_receiver_literal(expression)
            .map(|literal| self.fresh_index_receiver_display(literal, &ty))
            .unwrap_or(ty)
    }

    pub(super) fn fresh_dynamic_index_type(
        &self,
        expression: &Expr,
        receiver: &Type,
        index: &Type,
    ) -> Option<Type> {
        if !matches!(index, Type::String | Type::Number)
            || Self::index_receiver_literal(expression).is_none()
        {
            return None;
        }
        let Type::ObjectType(info) = receiver else {
            return None;
        };
        if info.index_signature.is_some() {
            return None;
        }
        let mut values = info
            .properties
            .iter()
            .map(|(_, ty)| Type::clone(ty))
            .collect::<Vec<_>>();
        values.push(Type::Undefined);
        Some(self.array_literal_union(values))
    }

    fn index_receiver_name(expression: &Expr) -> Option<String> {
        match &expression.kind {
            ExprKind::Ident(name) => Some(name.to_string()),
            ExprKind::Member(member) => Some(format!(
                "{}.{}",
                Self::index_receiver_name(&member.object)?,
                member.property
            )),
            ExprKind::ElemAccess(access) => {
                let key = match &access.index.kind {
                    ExprKind::Ident(key) | ExprKind::StrLit(key) | ExprKind::NumLit(key) => key,
                    _ => return None,
                };
                Some(format!(
                    "{}.{key}",
                    Self::index_receiver_name(&access.object)?
                ))
            }
            _ => None,
        }
    }

    fn index_method_suggestion(
        &self,
        receiver: &Type,
        index: &Type,
        writing: bool,
    ) -> Option<&'static str> {
        let method = if writing { "set" } else { "get" };
        let shape = match receiver {
            Type::TypeReference(name, args) => self.resolve_type_reference_to_object(name, args),
            _ => None,
        };
        if let Type::ObjectType(info) = shape.as_ref().unwrap_or(receiver) {
            if info
                .properties
                .iter()
                .filter(|(name, _)| name == method)
                .count()
                > 1
            {
                return None;
            }
        }
        let ty = self.extract_member_type_opt(receiver, method)?;
        let ty = match &ty {
            Type::TypeReference(name, args) => self
                .resolve_type_reference_to_object(name, args)
                .or_else(|| self.resolve_type_for_assignability(&ty))
                .unwrap_or(ty),
            _ => ty,
        };
        let signature = match ty {
            Type::Function(signature) => signature,
            Type::ObjectType(info)
                if info.call_signatures.len() == 1
                    && info.properties.is_empty()
                    && info.construct_signatures.is_empty()
                    && info.index_signature.is_none() =>
            {
                info.call_signatures[0].clone()
            }
            _ => return None,
        };
        let params = Self::relation_params(&signature.params);
        let (name, first) = params.first()?;
        fn accepts_void(ty: &Type) -> bool {
            matches!(ty, Type::Void)
                || matches!(ty, Type::Union(parts) if parts.iter().any(accepts_void))
        }
        if !params.iter().any(|(name, ty)| {
            !name.starts_with('?')
                && !name.starts_with("...")
                && !matches!(ty, Type::Optional(_))
                && !accepts_void(ty)
        }) {
            return None;
        }
        let first = if name.starts_with("...") {
            match first {
                Type::Array(element) => element.as_ref(),
                Type::Tuple(elements) => elements.first()?,
                _ => first,
            }
        } else {
            first
        };
        let generic_key = matches!(first, Type::TypeReference(name, _) | Type::TypeParameter(name) if signature.type_params.contains(name));
        ((!generic_key || matches!(index, Type::Any | Type::Never))
            && self.is_assignable_to(index, first))
        .then_some(method)
    }

    fn index_diagnostic_display_type(&self, ty: &Type) -> Type {
        match ty {
            Type::ObjectType(info) => {
                let mut info = info.clone();
                for (_, ty) in &mut info.properties {
                    *ty = Arc::new(self.index_diagnostic_display_type(ty));
                }
                Type::ObjectType(info)
            }
            Type::Optional(inner)
                if self.strict_null_checks
                    && !self
                        .compiler_options
                        .exact_optional_property_types
                        .unwrap_or(false) =>
            {
                Type::Optional(Arc::new(Type::flatten_union(vec![
                    self.index_diagnostic_display_type(inner),
                    Type::Undefined,
                ])))
            }
            Type::Function(signature) => {
                let mut signature = signature.clone();
                signature.params = Self::expand_rest_tuple_params(&signature.params);
                if self.strict_null_checks {
                    for (name, ty) in &mut signature.params {
                        if name.starts_with('?') {
                            *ty = Type::flatten_union(vec![ty.clone(), Type::Undefined]);
                        }
                    }
                }
                Type::Function(signature)
            }
            _ => ty.clone(),
        }
    }

    fn index_receiver_has_static_property(&self, receiver: &Type, property: &str) -> bool {
        let Type::TypeReference(name, _) = receiver else {
            return false;
        };
        let mut current = Some(name.as_str());
        let mut seen = HashSet::new();
        while let Some(name) = current.take().filter(|name| seen.insert(*name)) {
            let Some(info) = self.class_info.get(name) else {
                break;
            };
            if info
                .static_properties
                .iter()
                .any(|(name, _)| name == property)
                || info.static_methods.iter().any(|(name, _)| name == property)
            {
                return true;
            }
            current = info.extends.as_deref();
        }
        false
    }

    fn fresh_index_receiver_display(&self, expression: &Expr, receiver: &Type) -> Type {
        if let ExprKind::Paren(inner) = &expression.kind {
            return self.fresh_index_receiver_display(inner, receiver);
        }
        let (ExprKind::ObjectLit(properties), Type::ObjectType(info)) =
            (&expression.kind, receiver)
        else {
            return receiver.clone();
        };
        let mut shown = info.clone();
        for property in properties {
            let ObjLitProp::Property(property) = property else {
                continue;
            };
            let name = self.prop_name_to_string(&property.key);
            if let Some((_, ty)) = shown.properties.iter_mut().find(|(key, _)| *key == name) {
                let nested = self.fresh_index_receiver_display(&property.value, ty);
                *ty = Arc::new(if Self::is_fresh_literal_expr(&property.value) {
                    self.widen_type(&nested)
                } else {
                    nested
                });
            }
        }
        // A later spread can replace a shorthand, method, or earlier regular
        // literal. Use the last known initializer to preserve its freshness.
        for (name, original) in &info.properties {
            if let Some(value) = Self::index_literal_property(properties, name) {
                let nested = self.fresh_index_receiver_display(value, original);
                let ty = if Self::is_fresh_literal_expr(value) {
                    self.widen_type(&nested)
                } else {
                    nested
                };
                if let Some((_, shown)) = shown.properties.iter_mut().find(|(key, _)| key == name) {
                    *shown = Arc::new(ty);
                }
            }
        }
        Type::ObjectType(shown)
    }

    /// Some(true) is a numeric index domain, Some(false) a closed property
    /// table with no index signature, and None an open or unresolved shape.
    fn element_index_domain(&self, receiver: &Type, depth: usize) -> Option<bool> {
        if depth >= 16 || self.active_type_parameter_name(receiver).is_some() {
            return None;
        }
        match receiver {
            Type::Array(_) => Some(true),
            Type::ObjectType(info) => {
                // Function expandos and computed members are not completely
                // represented in the structural property table yet.
                if !info.call_signatures.is_empty()
                    || !info.construct_signatures.is_empty()
                    || info
                        .properties
                        .iter()
                        .any(|(name, _)| name == "__spread__" || name.starts_with('['))
                {
                    return None;
                }
                match &info.index_signature {
                    None => Some(false),
                    Some((key, _)) if matches!(key.as_ref(), Type::Number) => Some(true),
                    _ => None,
                }
            }
            Type::TypeReference(name, args) => {
                if let Some(members) = self.enum_info.get(name.as_str()) {
                    return Some(
                        members.is_empty()
                            || members.iter().any(|(_, value)| {
                                matches!(value, Type::Number | Type::NumberLiteral(_))
                            }),
                    );
                }
                if self.class_info.contains_key(name.as_str())
                    && !self.class_chain_fully_resolved(name)
                {
                    return None;
                }
                let keys = self.inherited_element_index_keys(receiver);
                if !keys.is_empty() {
                    return keys
                        .iter()
                        .all(|key| matches!(key, Type::Number))
                        .then_some(true);
                }
                self.resolve_type_reference_to_object(name, args)
                    .or_else(|| self.resolve_type_for_assignability(receiver))
                    .filter(|resolved| resolved != receiver)
                    .and_then(|resolved| self.element_index_domain(&resolved, depth + 1))
            }
            _ => None,
        }
    }

    pub(super) fn mark_numeric_for_in_key(&mut self, left: &ForInOfLeft, receiver: &Type) {
        if self.element_index_domain(receiver, 0) != Some(true) {
            return;
        }
        let name = match left {
            ForInOfLeft::Var(declarations) => declarations.declarations.first().and_then(|decl| {
                if let PatKind::Ident(name) = &decl.name.kind {
                    Some(name)
                } else {
                    None
                }
            }),
            ForInOfLeft::Pat(pattern) => {
                if let PatKind::Ident(name) = &pattern.kind {
                    Some(name)
                } else {
                    None
                }
            }
            ForInOfLeft::Expr(expression) => {
                if let ExprKind::Ident(name) = &expression.kind {
                    Some(name)
                } else {
                    None
                }
            }
        };
        if let Some(name) = name {
            self.declare_var(&format!("#for-in-number:{name}"), Type::Never);
        }
    }

    fn is_numeric_for_in_key(&self, expression: &Expr) -> bool {
        match &expression.kind {
            ExprKind::Ident(name) => {
                self.nearer_binding_is(&format!("#for-in-number:{name}"), name) == Some(true)
            }
            ExprKind::Paren(inner) => self.is_numeric_for_in_key(inner),
            _ => false,
        }
    }
}
