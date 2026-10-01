use super::*;

impl TypeChecker {
    /// Destructuring reads named properties even when the local binding has a
    /// different name. Object rest copies only public properties.
    pub(crate) fn check_binding_pattern_access(&mut self, pattern: &Pat, ty: &Type) {
        use tsc_rs_ast::{ArrayPatElem, ObjPatProp};
        match &pattern.kind {
            PatKind::Object(properties) => {
                for property in properties {
                    match property {
                        ObjPatProp::Shorthand(name, span)
                        | ObjPatProp::ShorthandAssign(name, _, span) => {
                            self.check_destructured_member_access(ty, name, *span);
                        }
                        ObjPatProp::KeyValue(key, nested) => {
                            if let Some(name) = self.pattern_property_name(key) {
                                self.check_destructured_member_access(ty, &name, key.span());
                                let member = self.extract_member_type(ty, &name);
                                self.check_binding_pattern_access(nested, &member);
                            }
                        }
                        ObjPatProp::Rest(_) => {}
                    }
                }
            }
            PatKind::Array(elements) => {
                for (index, element) in elements.iter().enumerate() {
                    if let Some(ArrayPatElem::Pat(nested)) = element {
                        let member = self.extract_element_type(ty, index);
                        self.check_binding_pattern_access(nested, &member);
                    }
                }
            }
            PatKind::Assign(nested, _) | PatKind::Rest(nested) => {
                self.check_binding_pattern_access(nested, ty);
            }
            PatKind::Ident(_) => {}
        }
    }

    fn check_destructured_member_access(&mut self, ty: &Type, name: &str, span: Span) {
        let apparent = self.typeparam_constraint_apparent(ty);
        let receiver = apparent.as_ref().unwrap_or(ty);
        self.check_private_member_access(receiver, name, span);
        self.check_protected_member_access(receiver, name, span, false, false);
    }

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
        // Class/namespace merges retain a nominal constructor alongside
        // the namespace's exported properties.
        if let Type::Intersection(members) = receiver {
            if members.len() == 2 && matches!(members[0], Type::ObjectType(_)) {
                if matches!(&members[1], Type::TypeReference(name, _) if name.starts_with("typeof "))
                {
                    self.check_protected_member_access(
                        &members[1],
                        property,
                        span,
                        is_super,
                        writing,
                    );
                }
            }
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

        let mut pending = vec![receiver_class.clone()];
        let mut seen = rustc_hash::FxHashSet::default();
        let declaring = loop {
            let Some(class) = pending.pop() else { return };
            if !seen.insert(class.clone()) {
                continue;
            }
            let Some(info) = self.class_info.get(&class) else {
                if !is_static {
                    if let Some(info) = self.interface_info.get(&class) {
                        // Own interface members are public, even when the
                        // heritage checker diagnoses an incompatible override.
                        if info.member_names.contains_key(property) {
                            return;
                        }
                        // Resolve conflicting inherited members in source order.
                        pending.extend(info.extends.iter().enumerate().rev().map(
                            |(index, (name, _))| {
                                self.interface_lookup_name_for_source(
                                    name,
                                    info.extends_sources
                                        .get(index)
                                        .and_then(|source| source.as_deref()),
                                )
                            },
                        ));
                    }
                }
                continue;
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
            pending.extend(info.extends.iter().cloned());
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
                    Type::TypeReference(name, _)
                        if self.protected_receiver_derives_from(name, &declaring) =>
                    {
                        Some(name.clone())
                    }
                    _ => None,
                }
            });

        let (code, message) = if let Some(enclosing) = enclosing {
            if is_static || self.protected_receiver_derives_from(&receiver_class, &enclosing) {
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

    // An interface extending a class retains the class's protected identity.
    // Walk heritage rather than structural assignability: a public lookalike
    // must not gain access to a protected member.
    fn protected_receiver_derives_from(&self, receiver: &str, base: &str) -> bool {
        let mut pending = vec![receiver.to_string()];
        let mut seen = rustc_hash::FxHashSet::default();
        while let Some(name) = pending.pop() {
            if !seen.insert(name.clone()) {
                continue;
            }
            if name == base {
                return true;
            }
            if let Some(info) = self.class_info.get(&name) {
                pending.extend(info.extends.iter().cloned());
            } else if let Some(info) = self.interface_info.get(&name) {
                pending.extend(info.extends.iter().enumerate().map(|(index, (name, _))| {
                    self.interface_lookup_name_for_source(
                        name,
                        info.extends_sources
                            .get(index)
                            .and_then(|source| source.as_deref()),
                    )
                }));
            }
        }
        false
    }

    fn protected_class_display(&self, class: &str) -> String {
        let name = class.rsplit('.').next().unwrap_or(class);
        let params = self
            .class_info
            .get(class)
            .map(|info| &info.type_params)
            .or_else(|| self.interface_info.get(class).map(|info| &info.type_params));
        match params {
            Some(params) if !params.is_empty() => format!("{name}<{}>", params.join(", ")),
            _ => name.to_string(),
        }
    }
}
