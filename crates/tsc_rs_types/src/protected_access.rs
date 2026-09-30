use super::*;

impl TypeChecker {
    pub(crate) fn check_protected_member_access(
        &mut self,
        receiver: &Type,
        property: &str,
        span: Span,
        is_super: bool,
        writing: bool,
    ) {
        // Other checks enforce the target/field restrictions on `super`.
        // Protected visibility itself permits a super access.
        if is_super || property.starts_with('#') {
            return;
        }
        let (receiver_class, is_static) = match receiver {
            Type::This => match self.enclosing_class_names.last() {
                Some(name) => (name.clone(), false),
                None => return,
            },
            Type::TypeReference(name, _) => match name.strip_prefix("typeof ") {
                Some(name) => (name.to_string(), true),
                None => (name.clone(), false),
            },
            Type::Typeof(name) => (name.clone(), true),
            _ => return,
        };

        let mut current = Some(receiver_class.clone());
        let mut seen = rustc_hash::FxHashSet::default();
        let declaring = loop {
            let Some(class) = current.take() else { return };
            if !seen.insert(class.clone()) {
                return;
            }
            let Some(info) = self.class_info.get(&class) else {
                return;
            };
            let accessor_flags = info
                .accessor_modifiers
                .get(&(property.to_string(), is_static))
                .and_then(|(getter, setter)| {
                    if writing {
                        setter.or(*getter)
                    } else {
                        getter.or(*setter)
                    }
                });
            let protected = if let Some(flags) = accessor_flags {
                flags & tsc_rs_ast::MOD_PROTECTED != 0
            } else if is_static {
                info.static_member_modifiers
                    .iter()
                    .any(|(name, flags)| name == property && flags & tsc_rs_ast::MOD_PROTECTED != 0)
            } else {
                info.instance_member_modifiers
                    .iter()
                    .any(|(name, flags)| name == property && flags & tsc_rs_ast::MOD_PROTECTED != 0)
            };
            if protected {
                break class;
            }
            // A nearer public/private declaration replaces inherited visibility.
            let declared = accessor_flags.is_some()
                || if is_static {
                    info.static_member_modifiers
                        .iter()
                        .any(|(name, _)| name == property)
                } else {
                    info.instance_properties
                        .iter()
                        .any(|(name, _)| name == property)
                        || info
                            .instance_methods
                            .iter()
                            .any(|(name, _)| name == property)
                        || info.accessor_props.contains(property)
                        || info.own_private_members.contains(property)
                };
            if declared {
                return;
            }
            current = info.extends.clone();
        };

        // Nested functions/classes retain access afforded by an enclosing class.
        // An explicit or contextual `this` parameter can grant instance access,
        // but never grants access to protected static members.
        let enclosing = self
            .enclosing_class_names
            .iter()
            .rev()
            .find(|name| self.class_derives_from(name, &declaring))
            .cloned()
            .or_else(|| {
                if is_static {
                    return None;
                }
                if self.nearer_binding_is("this", SUPER_PROPERTY_BARRIER_MARKER) == Some(false)
                    || self.nearer_binding_is("this", SUPER_PROPERTY_OK_MARKER) == Some(false)
                {
                    return None;
                }
                let this_type = self.lookup_var("this")?;
                let apparent = self.typeparam_constraint_apparent(this_type);
                match apparent.as_ref().unwrap_or(this_type) {
                    Type::TypeReference(name, _) if self.class_derives_from(name, &declaring) => {
                        Some(name.clone())
                    }
                    _ => None,
                }
            });

        let (code, message) = if let Some(enclosing) = enclosing {
            if is_static || self.class_derives_from(&receiver_class, &enclosing) {
                return;
            }
            (2446, format!(
                "Property '{property}' is protected and only accessible through an instance of class '{}'. This is an instance of class '{}'.",
                self.protected_class_display(&enclosing), receiver.display_string(),
            ))
        } else {
            (2445, format!(
                "Property '{property}' is protected and only accessible within class '{}' and its subclasses.",
                self.protected_class_display(&declaring),
            ))
        };
        if self
            .reported_duplicate_spans
            .insert((code, span.start, span.end))
        {
            self.diagnostics.push(Diagnostic {
                code,
                message,
                category: DiagnosticCategory::Error,
                file_name: None,
                span: Some(span),
                related: None,
            });
        }
    }

    fn protected_class_display(&self, class: &str) -> String {
        let name = class.rsplit('.').next().unwrap_or(class);
        match self.class_info.get(class) {
            Some(info) if !info.type_params.is_empty() => {
                format!("{name}<{}>", info.type_params.join(", "))
            }
            _ => name.to_string(),
        }
    }
}
