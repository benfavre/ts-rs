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
    ) -> bool {
        let ExprKind::ElemAccess(access) = &expression.kind else {
            return false;
        };
        if !self.no_implicit_any || self.current_file_is_js() {
            return false;
        }
        // Super currently uses an instance shape even in static containers.
        // That shape cannot establish absence on the constructor side.
        if matches!(access.object.kind, ExprKind::Super) {
            return false;
        }
        // An inferred `any` can stand for an unresolved generic property.
        // Only a directly bound any-valued key is conclusive here.
        if matches!(index, Type::Any)
            && !matches!(&access.index.kind, ExprKind::Ident(name)
                if matches!(self.lookup_declared_var(name), Some(Type::Any)))
        {
            return false;
        }
        if let ExprKind::Ident(name) = &access.object.kind {
            if self.lookup_declared_var(name).is_some_and(|declared| {
                declared != *receiver && self.element_index_domain(&declared, 0).is_none()
            }) {
                // Predicate narrowing can currently lose a declared index
                // signature; do not treat that approximation as a closed type.
                return false;
            }
        }
        let Some(numeric_only) = self.element_index_domain(receiver, 0) else {
            return false;
        };
        if numeric_only && self.is_numeric_for_in_key(&access.index) {
            return false;
        }
        let keys = match index {
            Type::Union(keys) => keys.to_vec(),
            key => vec![key.clone()],
        };
        let fresh = Self::is_fresh_object_literal(&access.object);
        let mut missing = None;
        for key in &keys {
            match key {
                Type::StringLiteral(name) | Type::NumberLiteral(name) => {
                    if numeric_only && self.index_applies_to_property(key, &Type::Number) {
                        continue;
                    }
                    if self.intersection_property_presence(receiver, name, 0) != Some(false) {
                        continue;
                    }
                    missing = Some(Some(name.clone()));
                }
                Type::Any if numeric_only => continue,
                Type::Number if numeric_only => continue,
                Type::Any | Type::String | Type::Number | Type::Symbol if !fresh => {
                    missing = Some(None);
                }
                _ => return false,
            }
            if missing.is_some() {
                break;
            }
        }
        let Some(property) = missing else {
            return false;
        };
        let shown = match receiver {
            Type::TypeReference(name, _) if self.enum_info.contains_key(name.as_str()) => {
                format!("typeof {name}")
            }
            _ if fresh => self
                .fresh_index_receiver_display(&access.object, receiver)
                .display_string_single_line(),
            _ => receiver.display_string_single_line(),
        };
        if let Some(property) = property.as_ref().filter(|_| !fresh && !numeric_only) {
            let suggested =
                self.missing_property_diagnostic(receiver, property, &shown, access.index.span);
            if suggested.code == 2551 {
                self.diagnostics.push(suggested);
                return true;
            }
        }
        let (code, message, span) = if numeric_only {
            (7015, "Element implicitly has an 'any' type because index expression is not of type 'number'.".to_string(), access.index.span)
        } else if fresh {
            let Some(property) = property else {
                return false;
            };
            (
                2339,
                format!("Property '{property}' does not exist on type '{shown}'."),
                expression.span,
            )
        } else {
            let detail = if let Some(property) = property {
                format!("\n  Property '{property}' does not exist on type '{shown}'.")
            } else if matches!(index, Type::String | Type::Number) {
                format!("\n  No index signature with a parameter of type '{}' was found on type '{shown}'.", index.display_string_single_line())
            } else {
                String::new()
            };
            (7053, format!("Element implicitly has an 'any' type because expression of type '{}' can't be used to index type '{shown}'.{detail}", index.display_string_single_line()), expression.span)
        };
        self.diagnostics.push(Diagnostic {
            code,
            message,
            category: DiagnosticCategory::Error,
            file_name: None,
            span: Some(span),
            related: None,
        });
        true
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
