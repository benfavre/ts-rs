use super::*;

impl TypeChecker {
    /// The three parts of the special unknown-like union `{}` | null |
    /// undefined. Only anonymous empty objects qualify; an empty interface
    /// and `void` do not acquire unknown's assignment behavior.
    pub(super) fn unknown_union_coverage(&self, ty: &Type, depth: usize) -> u8 {
        if depth >= 16 {
            return 0;
        }
        match ty {
            Type::Null => 1,
            Type::Undefined => 2,
            Type::ObjectType(info)
                if info.properties.is_empty()
                    && info.call_signatures.is_empty()
                    && info.construct_signatures.is_empty()
                    && info.index_signature.is_none() =>
            {
                4
            }
            Type::Any | Type::Unknown => 7,
            Type::Union(members) => members.iter().fold(0, |covered, member| {
                covered | self.unknown_union_coverage(member, depth + 1)
            }),
            Type::Intersection(members) => members.iter().fold(7, |covered, member| {
                covered & self.unknown_union_coverage(member, depth + 1)
            }),
            Type::TypeReference(name, _)
                if self.type_aliases.contains_key(name)
                    && self.alias_name_means_registered_alias(name) =>
            {
                self.resolve_type_for_assignability(ty)
                    .filter(|resolved| resolved != ty)
                    .map(|resolved| self.unknown_union_coverage(&resolved, depth + 1))
                    .unwrap_or(0)
            }
            _ => 0,
        }
    }

    pub(super) fn report_unknown_object_spread(&mut self, ty: &Type, span: Span) -> bool {
        if !self.is_unknown_receiver(ty)
            && !matches!(
                ty,
                Type::Boolean
                    | Type::BooleanLiteral(_)
                    | Type::Number
                    | Type::NumberLiteral(_)
                    | Type::String
                    | Type::StringLiteral(_)
                    | Type::BigInt
                    | Type::BigIntLiteral(_)
                    | Type::Symbol
                    | Type::UniqueSymbol(_)
            )
        {
            return false;
        }
        self.diagnostics.push(Diagnostic {
            code: 2698,
            message: "Spread types may only be created from object types.".to_string(),
            category: DiagnosticCategory::Error,
            file_name: None,
            span: Some(span),
            related: None,
        });
        true
    }

    pub(super) fn is_unknown_receiver(&self, ty: &Type) -> bool {
        match ty {
            Type::Unknown => true,
            Type::Intersection(members) => {
                !members.is_empty()
                    && members
                        .iter()
                        .all(|member| self.is_unknown_receiver(member))
            }
            Type::Union(members) => {
                members.iter().any(|member| matches!(member, Type::Unknown))
                    && !members
                        .iter()
                        .any(|member| matches!(member, Type::Any | Type::Error))
            }
            Type::TypeReference(name, _)
                if self.type_aliases.contains_key(name)
                    && self.alias_name_means_registered_alias(name) =>
            {
                matches!(self.resolve_type_for_assignability(ty), Some(Type::Unknown))
            }
            _ => false,
        }
    }

    /// Unknown values need a guard before property access or invocation.
    /// A type parameter merely constrained by `unknown` remains a distinct
    /// type, and optional access first removes the unknown's nullish part.
    pub(super) fn report_unknown_receiver(
        &mut self,
        expression: &Expr,
        ty: &Type,
        optional: bool,
    ) -> bool {
        if !self.strict_null_checks || optional || !self.is_unknown_receiver(ty) {
            return false;
        }
        fn entity_name(expression: &Expr) -> Option<String> {
            match &expression.kind {
                ExprKind::Ident(name) => Some(name.to_string()),
                ExprKind::Member(member) if !member.property.starts_with('#') => Some(format!(
                    "{}.{}",
                    entity_name(&member.object)?,
                    member.property
                )),
                _ => None,
            }
        }
        let diagnostic = if let Some(name) =
            entity_name(expression).filter(|name| name.encode_utf16().count() < 100)
        {
            diagnostics::error_value_unknown(&name, expression.span)
        } else {
            Diagnostic {
                code: 2571,
                message: "Object is of type 'unknown'.".to_string(),
                category: DiagnosticCategory::Error,
                file_name: None,
                span: Some(expression.span),
                related: None,
            }
        };
        if !self
            .diagnostics
            .iter()
            .any(|existing| existing.code == diagnostic.code && existing.span == diagnostic.span)
        {
            self.diagnostics.push(diagnostic);
        }
        true
    }

    pub(super) fn unknown_apparent_member(&self, property: &str) -> Option<Type> {
        self.builtins
            .lookup_instance_property("Object", property)
            .or_else(|| {
                let Type::ObjectType(info) =
                    self.resolve_type_reference_to_object("Object", &[])?
                else {
                    return None;
                };
                info.properties
                    .iter()
                    .find(|(name, _)| name == property)
                    .map(|(_, ty)| Type::clone(ty))
            })
    }

    pub(super) fn unknown_element_access(
        &mut self,
        index: &Type,
        index_span: Span,
        span: Span,
    ) -> Type {
        if matches!(index, Type::Error) {
            return Type::Any;
        }
        if matches!(index, Type::Never) {
            return Type::Never;
        }
        let keys = match index {
            Type::Union(members) => members.as_ref(),
            _ => std::slice::from_ref(index),
        };
        let mut invalid = false;
        for key in keys {
            if matches!(
                key,
                Type::Boolean
                    | Type::BooleanLiteral(_)
                    | Type::Object
                    | Type::ObjectType(_)
                    | Type::Array(_)
                    | Type::Tuple(_)
                    | Type::Function(_)
                    | Type::Constructor(_)
                    | Type::BigInt
                    | Type::BigIntLiteral(_)
                    | Type::Null
                    | Type::Undefined
                    | Type::Void
                    | Type::Unknown
            ) {
                let shown = if matches!(key, Type::BigIntLiteral(_)) {
                    "bigint".to_string()
                } else {
                    key.display_string()
                };
                self.diagnostics
                    .push(diagnostics::error_invalid_index_type(&shown, index_span));
                invalid = true;
            }
        }
        if invalid {
            return Type::Any;
        }
        let mut selected = Vec::new();
        let mut missing = None;
        for key in keys {
            if let Type::StringLiteral(property) | Type::NumberLiteral(property) = key {
                if let Some(member) = self.unknown_apparent_member(property) {
                    selected.push(member);
                    continue;
                }
            }
            missing.get_or_insert(key);
        }
        let Some(missing) = missing else {
            return Type::flatten_union(selected);
        };
        if self.active_type_parameter_name(index).is_some() {
            if self.strict_null_checks {
                self.diagnostics.push(Diagnostic {
                    code: 2536,
                    message: format!(
                        "Type '{}' cannot be used to index type '{{}}'.",
                        index.display_string()
                    ),
                    category: DiagnosticCategory::Error,
                    file_name: None,
                    span: Some(span),
                    related: None,
                });
            }
            return Type::Any;
        }
        if !self.no_implicit_any {
            return Type::Any;
        }
        let shown = index.display_string_single_line();
        let detail = if let Type::StringLiteral(property) | Type::NumberLiteral(property) = missing
        {
            format!("\n  Property '{property}' does not exist on type '{{}}'.")
        } else if matches!(missing, Type::String | Type::Number | Type::Symbol) {
            format!(
                "\n  No index signature with a parameter of type '{}' was found on type '{{}}'.",
                missing.display_string()
            )
        } else {
            String::new()
        };
        self.diagnostics.push(Diagnostic {
            code: 7053,
            message: format!("Element implicitly has an 'any' type because expression of type '{shown}' can't be used to index type '{{}}'.{detail}"),
            category: DiagnosticCategory::Error, file_name: None, span: Some(span), related: None,
        });
        Type::Any
    }
}
