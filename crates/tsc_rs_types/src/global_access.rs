use super::*;

impl TypeChecker {
    /// Resolve intrinsic global-object aliases from this file's declarations,
    /// without consulting the donor's shared bindings from unrelated files.
    pub(super) fn file_global_object_alias(statements: &[Stmt], expression: &Expr) -> bool {
        fn resolve(statements: &[Stmt], expression: &Expr, seen: &mut HashSet<String>) -> bool {
            match &expression.kind {
                ExprKind::Ident(name) => {
                    if !seen.insert(name.to_string()) {
                        return false;
                    }
                    // Find an alias before checking import bindings, including
                    // aliases declared later in the file.
                    if let Some(alias) = statements.iter().find_map(|statement| {
                        match &TypeChecker::unwrap_export_stmt(statement).kind {
                            StmtKind::ImportEquals(alias) if alias.name == *name => Some(alias),
                            _ => None,
                        }
                    }) {
                        return resolve(statements, &alias.module_ref, seen);
                    }
                    if TypeChecker::import_binding_names(statements).contains(name.as_str()) {
                        return false;
                    }
                    for statement in statements {
                        let declaration = TypeChecker::unwrap_export_stmt(statement);
                        if TypeChecker::stmt_declares_value_name(declaration, name) {
                            return false;
                        }
                    }
                    name == "globalThis"
                }
                ExprKind::Member(member) if member.property == "globalThis" => {
                    resolve(statements, &member.object, seen)
                }
                _ => false,
            }
        }
        resolve(statements, expression, &mut HashSet::new())
    }

    pub(super) fn is_global_object_type(&self, ty: &Type) -> bool {
        match ty {
            Type::Typeof(name) => name == "globalThis",
            Type::TypeReference(_, _) => {
                matches!(self.simplify_type(ty), Type::Typeof(name) if name == "globalThis")
            }
            _ => false,
        }
    }

    pub(super) fn is_global_object_expression(&self, expression: &Expr) -> bool {
        match &expression.kind {
            ExprKind::Ident(name) => name == "globalThis" && self.lookup_var(name).is_none(),
            ExprKind::This => {
                self.current_file_is_script
                    && self.enclosing_class_names.is_empty()
                    && self.lookup_var("this").is_none()
            }
            ExprKind::Paren(inner) | ExprKind::NonNull(inner) => {
                self.is_global_object_expression(inner)
            }
            _ => false,
        }
    }

    pub(super) fn global_this_known_property(&self, name: &str) -> Option<Type> {
        if name == "globalThis" {
            return Some(Type::Typeof(name.to_string()));
        }
        let declared = match self.script_global_kinds.get(name) {
            Some(0) => true,
            Some(_) => false,
            None => {
                self.user_global_value_names.contains(name)
                    || crate::stdlib::stdlib_member_availability(&self.compiler_options)
                        .is_some_and(|lib| lib.declares_global_value(name))
            }
        };
        declared.then(|| {
            self.scopes[0]
                .narrowed
                .get(name)
                .or_else(|| self.scopes[0].vars.get(name))
                .map(|id| self.get_type(*id).clone())
                .unwrap_or(Type::Any)
        })
    }

    pub(super) fn global_this_indexed_type(&self, index: &Type) -> Option<Type> {
        match index {
            Type::StringLiteral(key) | Type::NumberLiteral(key) => {
                Some(self.global_this_known_property(key).unwrap_or(Type::Error))
            }
            Type::Union(keys) => keys
                .iter()
                .map(|key| self.global_this_indexed_type(key))
                .collect::<Option<Vec<_>>>()
                .map(Type::flatten_union),
            Type::Never => Some(Type::Never),
            _ => None,
        }
    }

    pub(super) fn check_global_indexed_access_type(&mut self, object: &TypeNode, index: &TypeNode) {
        if !self.is_global_object_type(&self.resolve_type_node(object)) {
            return;
        }
        let index_type = self.simplify_type(&self.resolve_type_node(index));
        let keys = match &index_type {
            Type::Union(keys) => keys.as_ref(),
            _ => std::slice::from_ref(&index_type),
        };
        for key in keys {
            let diagnostic = match key {
                Type::StringLiteral(name) | Type::NumberLiteral(name) => {
                    if self.global_this_known_property(name).is_some() {
                        continue;
                    }
                    diagnostics::error_property_not_exist(name, "typeof globalThis", index.span)
                }
                Type::String | Type::Number => Diagnostic {
                    code: 2537,
                    message: format!(
                        "Type 'typeof globalThis' has no matching index signature for type '{}'.",
                        key.display_string()
                    ),
                    category: DiagnosticCategory::Error,
                    file_name: None,
                    span: Some(index.span),
                    related: None,
                },
                Type::Any
                | Type::Unknown
                | Type::Boolean
                | Type::BooleanLiteral(_)
                | Type::Symbol
                | Type::UniqueSymbol(_)
                | Type::BigInt
                | Type::BigIntLiteral(_)
                | Type::Null
                | Type::Undefined
                | Type::Void
                | Type::Object
                | Type::ObjectType(_)
                | Type::Array(_)
                | Type::Tuple(_)
                | Type::Function(_)
                | Type::Constructor(_) => diagnostics::error_invalid_index_type(
                    &key.display_string_single_line(),
                    index.span,
                ),
                _ => continue,
            };
            if !self.diagnostics.iter().any(|existing| {
                existing.code == diagnostic.code
                    && existing.span == diagnostic.span
                    && existing.message == diagnostic.message
            }) {
                self.diagnostics.push(diagnostic);
            }
        }
    }

    pub(super) fn global_this_element_access(&mut self, index: &Type, span: Span) -> Option<Type> {
        let key = match index {
            Type::StringLiteral(key) | Type::NumberLiteral(key) => Some(key.as_str()),
            Type::Any | Type::String | Type::Number | Type::Symbol | Type::UniqueSymbol(_) => None,
            Type::Never => return Some(Type::Never),
            _ => return None,
        };
        if let Some(key) = key {
            if let Some(ty) = self.global_this_known_property(key) {
                return Some(ty);
            }
            if matches!(self.script_global_kinds.get(key), Some(1)) {
                self.diagnostics.push(diagnostics::error_property_not_exist(
                    key,
                    "typeof globalThis",
                    span,
                ));
                return Some(Type::Error);
            }
        }
        if self.no_implicit_any {
            let detail = if let Some(key) = key {
                format!("\n  Property '{key}' does not exist on type 'typeof globalThis'.")
            } else if matches!(index, Type::String | Type::Number | Type::Symbol) {
                format!("\n  No index signature with a parameter of type '{}' was found on type 'typeof globalThis'.", index.display_string())
            } else {
                String::new()
            };
            self.diagnostics.push(Diagnostic {
                code: 7053,
                message: format!("Element implicitly has an 'any' type because expression of type '{}' can't be used to index type 'typeof globalThis'.{detail}", index.display_string_single_line()),
                category: DiagnosticCategory::Error, file_name: None, span: Some(span), related: None,
            });
        }
        Some(Type::Any)
    }

    fn index_accepts_property(kinds: u8, property: &str) -> bool {
        kinds & 2 != 0
            || (kinds & 1 != 0
                && (matches!(property, "NaN" | "Infinity" | "-Infinity")
                    || property.parse::<f64>().is_ok_and(|number| {
                        number.is_finite()
                            && Self::canonical_numeric_property_name(property) == property
                    })))
    }

    fn index_kind(key: &Type) -> u8 {
        match key {
            Type::Number => 1,
            Type::String => 2,
            Type::Symbol => 4,
            Type::Union(members) => members
                .iter()
                .fold(0, |kinds, member| kinds | Self::index_kind(member)),
            _ => 0,
        }
    }

    pub(super) fn intersection_property_presence(
        &self,
        ty: &Type,
        property: &str,
        depth: usize,
    ) -> Option<bool> {
        if depth >= 16 {
            return None;
        }
        match ty {
            Type::Typeof(name) if name == "globalThis" => {
                Some(self.global_this_known_property(property).is_some())
            }
            Type::ObjectType(info) => {
                if info.properties.iter().any(|(name, _)| name == "__spread__") {
                    return None;
                }
                Some(
                    info.properties.iter().any(|(name, _)| name == property)
                        || info.index_signature.as_ref().is_some_and(|(key, _)| {
                            self.index_applies_to_property(
                                &Type::StringLiteral(property.to_string()),
                                key,
                            )
                        })
                        || self.unknown_apparent_member(property).is_some(),
                )
            }
            Type::TypeReference(name, args) => {
                if let Some(resolved) = self
                    .resolve_type_reference_to_object(name, args)
                    .or_else(|| self.resolve_type_for_assignability(ty))
                    .filter(|resolved| resolved != ty)
                {
                    return self.intersection_property_presence(&resolved, property, depth + 1);
                }
                if let Some(lib) = crate::stdlib::stdlib_member_availability(&self.compiler_options)
                {
                    if let Some(present) = lib.declares_inherited_member(name, property) {
                        let kinds = lib.inherited_index_kinds(name).unwrap_or(0);
                        if present
                            || Self::index_accepts_property(kinds, property)
                            || self.unknown_apparent_member(property).is_some()
                        {
                            return Some(true);
                        }
                        // Computed index domains are not necessarily absent
                        // just because no named declaration matches.
                        return (kinds & 8 == 0).then_some(false);
                    }
                }
                self.type_property_presence(ty, property)
            }
            Type::Intersection(members) => {
                let facts: Vec<_> = members
                    .iter()
                    .map(|member| self.intersection_property_presence(member, property, depth + 1))
                    .collect();
                if facts.contains(&Some(true)) {
                    Some(true)
                } else if facts.iter().all(|fact| *fact == Some(false)) {
                    Some(false)
                } else {
                    None
                }
            }
            _ => self.type_property_presence(ty, property),
        }
    }

    pub(super) fn intersection_index_kinds(&self, ty: &Type, depth: usize) -> Option<u8> {
        if depth >= 16 {
            return None;
        }
        match ty {
            Type::Typeof(name) if name == "globalThis" => Some(0),
            Type::ObjectType(info) => match &info.index_signature {
                None => Some(0),
                Some((key, _)) => {
                    let kinds = Self::index_kind(key);
                    (kinds != 0).then_some(kinds)
                }
            },
            Type::TypeReference(name, args) => {
                if let Some(resolved) = self
                    .resolve_type_reference_to_object(name, args)
                    .or_else(|| self.resolve_type_for_assignability(ty))
                    .filter(|resolved| resolved != ty)
                {
                    return self.intersection_index_kinds(&resolved, depth + 1);
                }
                crate::stdlib::stdlib_member_availability(&self.compiler_options)?
                    .inherited_index_kinds(name)
                    .filter(|kinds| kinds & 8 == 0)
            }
            Type::Intersection(members) => members.iter().try_fold(0, |kinds, member| {
                self.intersection_index_kinds(member, depth + 1)
                    .map(|member| kinds | member)
            }),
            Type::Array(_) | Type::Tuple(_) => Some(1),
            _ => None,
        }
    }
}
