use super::*;

impl TypeChecker {
    pub(super) fn report_unbound_private_member_on_any(
        &mut self,
        property: &str,
        span: Span,
    ) -> bool {
        if !property.starts_with('#') || self.private_name_lexical_owner(property).is_some() {
            return false;
        }
        let diagnostic = if self.enclosing_class_names.is_empty() {
            diagnostics::error_private_identifier_outside_class(span)
        } else {
            diagnostics::error_property_not_exist(property, "any", span)
        };
        self.diagnostics.push(diagnostic);
        true
    }

    pub(super) fn this_receiver_origin(&self, expression: &Expr) -> Option<Type> {
        match &expression.kind {
            ExprKind::This => {
                if let Some(class) = &self.static_this_class {
                    Some(Type::TypeReference(
                        format!("typeof {class}"),
                        Arc::from([]),
                    ))
                } else {
                    self.lookup_var(INSTANCE_MEMBER_MARKER)
                        .filter(|ty| matches!(ty, Type::TypeReference(..)))
                        .cloned()
                }
            }
            ExprKind::Ident(name) => {
                let scope = self.nearest_binding_scope(name)?;
                self.scopes[scope]
                    .this_alias_receivers
                    .get(name.as_str())
                    .cloned()
            }
            ExprKind::Paren(inner) | ExprKind::NonNull(inner) => self.this_receiver_origin(inner),
            ExprKind::Satisfies(expression) => self.this_receiver_origin(&expression.expr),
            _ => None,
        }
    }

    pub(super) fn private_name_lexical_owner(&self, property: &str) -> Option<&str> {
        self.enclosing_class_names.iter().rev().find_map(|name| {
            self.class_info.get(name).and_then(|info| {
                info.private_identifiers
                    .iter()
                    .any(|(member, _)| member == property)
                    .then_some(name.as_str())
            })
        })
    }

    /// A class's index signatures and quoted `"#name"` properties cannot
    /// declare private identifiers. Instance private slots are installed by
    /// base constructors; static private slots belong only to their owner.
    pub(super) fn report_missing_private_member(
        &mut self,
        receiver: &Type,
        property: &str,
        span: Span,
    ) -> bool {
        if !property.starts_with('#') {
            return false;
        }
        let (class, is_static) = match receiver {
            Type::This => {
                let Some(Type::TypeReference(class, _)) = self.lookup_var(INSTANCE_MEMBER_MARKER)
                else {
                    return false;
                };
                (class.as_str(), false)
            }
            Type::TypeReference(name, _) => name
                .strip_prefix("typeof ")
                .map_or((name.as_str(), false), |class| (class, true)),
            Type::Typeof(class) => (class.as_str(), true),
            _ => return false,
        };
        let mut current = class;
        let mut seen = HashSet::new();
        while seen.insert(current) {
            let Some(info) = self.class_info.get(current) else {
                // An unresolved base may contribute an instance private slot.
                return false;
            };
            if info
                .private_identifiers
                .contains(&(property.to_string(), is_static))
            {
                return false;
            }
            if !is_static {
                if let Some(base) = &info.extends {
                    current = base;
                    continue;
                }
            }
            let display = if matches!(receiver, Type::This) {
                if is_static {
                    format!("typeof {class}")
                } else {
                    class.to_string()
                }
            } else {
                receiver.display_string_single_line()
            };
            let diagnostic = self.missing_property_diagnostic(receiver, property, &display, span);
            self.diagnostics.push(diagnostic);
            return true;
        }
        false
    }
}
