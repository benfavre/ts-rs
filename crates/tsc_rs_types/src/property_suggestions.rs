use super::*;

impl TypeChecker {
    pub(super) fn record_enum_member_locations(&mut self, name: &str, declaration: &EnumDecl) {
        let file = self
            .injected_decl_file
            .as_ref()
            .or(self.current_file_name.as_ref())
            .cloned()
            .unwrap_or_default();
        let locations: Vec<_> = declaration
            .members
            .iter()
            .map(|member| {
                (
                    self.prop_name_to_string(&member.name),
                    (file.clone(), member.name.span()),
                )
            })
            .collect();
        let entries = self
            .enum_member_locations
            .get_or_insert_with(name.to_string(), Default::default);
        for (name, location) in locations {
            entries.entry(name).or_insert(location);
        }
    }

    fn property_suggestion_names(
        &self,
        receiver: &Type,
        seen: &mut HashSet<String>,
        depth: usize,
    ) -> Vec<String> {
        if depth > 32 {
            return Vec::new();
        }
        let clean = |name: &str| name.strip_prefix('?').unwrap_or(name).to_string();
        match receiver {
            Type::ObjectType(info) => info
                .properties
                .iter()
                .map(|(name, _)| clean(name))
                .collect(),
            Type::Namespace(namespace) => namespace
                .exports
                .iter()
                .map(|(name, _)| name.clone())
                .collect(),
            Type::Module(module) => module
                .exports
                .iter()
                .map(|(name, _)| name.clone())
                .collect(),
            Type::TypeReference(name, _) => {
                if !seen.insert(name.clone()) {
                    return Vec::new();
                }
                let (owner, is_static) = name
                    .strip_prefix("typeof ")
                    .map_or((name.as_str(), false), |owner| (owner, true));
                if is_static {
                    if let Some(members) = self.enum_info.get(owner) {
                        return members.iter().map(|(name, _)| name.clone()).collect();
                    }
                }
                if let Some(class) = self.class_info.get(owner) {
                    let modifiers = if is_static {
                        &class.static_member_modifiers
                    } else {
                        &class.instance_member_modifiers
                    };
                    let mut names: Vec<_> = modifiers
                        .iter()
                        .filter(|(member, flags)| {
                            if class
                                .private_identifiers
                                .contains(&(member.clone(), is_static))
                            {
                                return self.private_name_lexical_owner(member) == Some(owner);
                            }
                            if flags & MOD_PRIVATE != 0 {
                                return self
                                    .enclosing_class_names
                                    .iter()
                                    .any(|enclosing| enclosing == owner);
                            }
                            if flags & MOD_PROTECTED != 0 {
                                return self.enclosing_class_names.iter().any(|enclosing| {
                                    self.class_derives_from(enclosing, owner)
                                        && (is_static || seen.contains(enclosing))
                                });
                            }
                            true
                        })
                        .map(|(name, _)| clean(name))
                        .collect();
                    if let Some(base) = &class.extends {
                        let base = if is_static {
                            format!("typeof {base}")
                        } else {
                            base.clone()
                        };
                        let inherited = self.property_suggestion_names(
                            &Type::TypeReference(base, Arc::from([])),
                            seen,
                            depth + 1,
                        );
                        names.extend(inherited.into_iter().filter(|member| {
                            !is_static
                                || !class
                                    .extends
                                    .as_ref()
                                    .and_then(|base| self.class_info.get(base))
                                    .is_some_and(|base| {
                                        base.private_identifiers.contains(&(member.clone(), true))
                                    })
                        }));
                    }
                    return names;
                }
                if let Some(interface) = self.interface_info.get(owner) {
                    let mut names: Vec<_> = interface
                        .object_type
                        .properties
                        .iter()
                        .map(|(name, _)| clean(name))
                        .collect();
                    for (base, args) in &interface.extends {
                        names.extend(self.property_suggestion_names(
                            &Type::TypeReference(base.clone(), args.clone()),
                            seen,
                            depth + 1,
                        ));
                    }
                    return names;
                }
                if let Some(resolved) = self
                    .resolve_type_for_assignability(receiver)
                    .filter(|resolved| resolved != receiver)
                {
                    return self.property_suggestion_names(&resolved, seen, depth + 1);
                }
                if let Some(constraint) = self.declared_type_param_constraint(receiver) {
                    return self.property_suggestion_names(&constraint, seen, depth + 1);
                }
                self.library_property_suggestions(owner)
            }
            Type::Typeof(name) => self.property_suggestion_names(
                &Type::TypeReference(format!("typeof {name}"), Arc::from([])),
                seen,
                depth + 1,
            ),
            Type::This => self
                .enclosing_class_names
                .last()
                .map(|name| {
                    self.property_suggestion_names(
                        &Type::TypeReference(name.clone(), Arc::from([])),
                        seen,
                        depth + 1,
                    )
                })
                .unwrap_or_default(),
            Type::TypeParameter(_) => self
                .declared_type_param_constraint(receiver)
                .map(|constraint| self.property_suggestion_names(&constraint, seen, depth + 1))
                .unwrap_or_default(),
            Type::Union(members) => {
                let mut names = members
                    .first()
                    .map(|member| {
                        self.property_suggestion_names(member, &mut seen.clone(), depth + 1)
                    })
                    .unwrap_or_default();
                for member in members.iter().skip(1) {
                    let other =
                        self.property_suggestion_names(member, &mut seen.clone(), depth + 1);
                    names.retain(|name| other.contains(name));
                }
                names
            }
            Type::Intersection(members) => members
                .iter()
                .flat_map(|member| {
                    self.property_suggestion_names(member, &mut seen.clone(), depth + 1)
                })
                .collect(),
            Type::String | Type::StringLiteral(_) => self.library_property_suggestions("String"),
            Type::Number | Type::NumberLiteral(_) => self.library_property_suggestions("Number"),
            Type::Boolean | Type::BooleanLiteral(_) => self.library_property_suggestions("Boolean"),
            Type::Array(_) | Type::Tuple(_) => self.library_property_suggestions("Array"),
            Type::Function(_) => self.library_property_suggestions("Function"),
            _ => Vec::new(),
        }
    }

    fn library_property_suggestions(&self, owner: &str) -> Vec<String> {
        stdlib::stdlib_member_availability(&self.compiler_options)
            .map(|library| library.member_names(owner))
            .unwrap_or_default()
    }

    fn property_suggestion_location(
        &self,
        receiver: &Type,
        property: &str,
        depth: usize,
    ) -> Option<(String, Option<Span>)> {
        if depth > 32 {
            return None;
        }
        if let Some((file, span)) = self.named_member_location(receiver, property) {
            return Some((file, Some(span)));
        }
        let owner = match receiver {
            Type::TypeReference(name, _) => {
                if let Some(owner) = name.strip_prefix("typeof ") {
                    if let Some(location) = self
                        .enum_member_locations
                        .get(owner)
                        .and_then(|members| members.get(property))
                    {
                        return Some((location.0.clone(), Some(location.1)));
                    }
                    if let Some((file, span)) = self.inherited_member_location(owner, property, 0) {
                        return Some((file, Some(span)));
                    }
                }
                if let Some(resolved) = self
                    .resolve_type_for_assignability(receiver)
                    .filter(|resolved| resolved != receiver)
                {
                    if let Some(location) =
                        self.property_suggestion_location(&resolved, property, depth + 1)
                    {
                        return Some(location);
                    }
                }
                name.as_str()
            }
            Type::Union(members) | Type::Intersection(members) => {
                return members.iter().find_map(|member| {
                    self.property_suggestion_location(member, property, depth + 1)
                })
            }
            Type::String | Type::StringLiteral(_) => "String",
            Type::Number | Type::NumberLiteral(_) => "Number",
            Type::Boolean | Type::BooleanLiteral(_) => "Boolean",
            Type::Array(_) | Type::Tuple(_) => "Array",
            Type::Function(_) => "Function",
            _ => return None,
        };
        stdlib::stdlib_member_availability(&self.compiler_options)?
            .member_declaring_lib(owner, property)
            .map(|file| (file, None))
    }

    pub(super) fn missing_property_diagnostic(
        &self,
        receiver: &Type,
        property: &str,
        display: &str,
        span: Span,
    ) -> Diagnostic {
        let named_this = matches!(receiver, Type::This)
            .then(|| Type::TypeReference(display.to_string(), Arc::from([])));
        let receiver = named_this.as_ref().unwrap_or(receiver);
        let names = self.property_suggestion_names(receiver, &mut HashSet::new(), 0);
        let Some(suggestion) = spelling_suggestion(property, names.iter().map(String::as_str))
        else {
            return diagnostics::error_property_not_exist(property, display, span);
        };
        let related = self
            .property_suggestion_location(receiver, &suggestion, 0)
            .map(|(file, span)| {
                vec![RelatedDiagnostic {
                    code: 2728,
                    message: format!("'{suggestion}' is declared here."),
                    file_name: Some(file),
                    span,
                }]
            });
        Diagnostic {
            code: 2551,
            message: format!("Property '{property}' does not exist on type '{display}'. Did you mean '{suggestion}'?"),
            category: DiagnosticCategory::Error, file_name: None, span: Some(span), related,
        }
    }
}
