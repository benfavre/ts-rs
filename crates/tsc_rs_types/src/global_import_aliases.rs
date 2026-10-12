use super::*;

impl TypeChecker {
    /// Global import aliases retain the enclosing module's declaration identity.
    /// Resolve them with module-local values in a temporary lexical scope, then
    /// publish only aliases explicitly exported by the augmentation.
    pub(super) fn inject_global_import_aliases(&mut self, file: &SourceFile) {
        if !Self::file_is_module(file) {
            return;
        }
        let bodies: Vec<_> = file
            .statements
            .iter()
            .filter_map(|statement| {
                let StmtKind::ModuleDecl(module) = &statement.kind else {
                    return None;
                };
                if module.name_span.is_some()
                    || !matches!(&module.name, ModuleName::Ident(name) if name == "global")
                {
                    return None;
                }
                let Some(ModuleBody::Block(body)) = &module.body else {
                    return None;
                };
                body.iter()
                    .any(|statement| {
                        matches!(
                            Self::unwrap_export_stmt(statement).kind,
                            StmtKind::ImportEquals(_)
                        )
                    })
                    .then_some(body)
            })
            .collect();
        if bodies.is_empty() {
            return;
        }

        let previous_file = self.current_file_name.replace(file.file_name.clone());
        let previous_injected = self.injected_decl_file.replace(file.file_name.clone());
        let previous_source = self.current_source.replace(file.text.clone());
        let previous_module = self.file_is_module_flag;
        self.file_is_module_flag = true;
        self.push_scope();

        let mut roots = HashMap::new();
        let mut namespaces = Vec::new();
        fn inventory<'a>(
            module: &'a ModuleDecl,
            path: String,
            out: &mut Vec<(String, &'a ModuleDecl)>,
        ) {
            out.push((path.clone(), module));
            match &module.body {
                Some(ModuleBody::Module(inner)) => {
                    if let ModuleName::Ident(name) = &inner.name {
                        inventory(inner, format!("{path}.{name}"), out);
                    }
                }
                Some(ModuleBody::Block(body)) => {
                    for statement in body {
                        if let StmtKind::ModuleDecl(inner) =
                            &TypeChecker::unwrap_export_stmt(statement).kind
                        {
                            if let ModuleName::Ident(name) = &inner.name {
                                inventory(inner, format!("{path}.{name}"), out);
                            }
                        }
                    }
                }
                None => {}
            }
        }
        for statement in &file.statements {
            let StmtKind::ModuleDecl(module) = &Self::unwrap_export_stmt(statement).kind else {
                continue;
            };
            let ModuleName::Ident(name) = &module.name else {
                continue;
            };
            if module.name_span.is_none() {
                continue;
            }
            let key = Self::file_interface_registry_key(&file.file_name, name);
            roots.insert(name.clone(), key.clone());
            inventory(module, name.clone(), &mut namespaces);
            let mut scoped_module = (**module).clone();
            scoped_module.name = ModuleName::Ident(key.clone());
            self.collect_namespace_module(&scoped_module, "", false);
            Self::collect_namespace_type_names(
                module,
                &key,
                Arc::make_mut(&mut self.namespace_type_names),
            );
        }
        // Inventory every declaration first, including namespaces appearing
        // after the augmentation or after another reopening of the namespace.
        for statement in &file.statements {
            let StmtKind::ModuleDecl(module) = &Self::unwrap_export_stmt(statement).kind else {
                continue;
            };
            let ModuleName::Ident(name) = &module.name else {
                continue;
            };
            let Some(key) = roots.get(name) else {
                continue;
            };
            self.inject_namespace_type_members(module, key);
        }
        for (path, module) in &namespaces {
            let Some(ModuleBody::Block(body)) = &module.body else {
                continue;
            };
            let owner = Self::file_interface_registry_key(&file.file_name, path);
            let previous_owner = self.type_resolution_namespace.replace(owner.clone());
            for statement in body {
                let (name, parameters) = match &Self::unwrap_export_stmt(statement).kind {
                    StmtKind::InterfaceDecl(declaration) => {
                        (&declaration.name, declaration.type_params.as_deref())
                    }
                    StmtKind::TypeAlias(declaration) => {
                        (&declaration.name, declaration.type_params.as_deref())
                    }
                    StmtKind::ClassDecl(declaration) => {
                        let Some(name) = &declaration.name else {
                            continue;
                        };
                        (name, declaration.type_params.as_deref())
                    }
                    _ => continue,
                };
                let _parameters = crate::type_params_in_resolution(parameters);
                let defaults = self.resolve_type_param_defaults(parameters);
                if defaults.iter().any(Option::is_some) {
                    self.alias_type_param_defaults
                        .insert(format!("{owner}.{name}"), defaults);
                }
            }
            self.type_resolution_namespace = previous_owner;
        }
        for statement in &file.statements {
            match &Self::unwrap_export_stmt(statement).kind {
                StmtKind::ModuleDecl(module) => {
                    let ModuleName::Ident(name) = &module.name else {
                        continue;
                    };
                    let Some(key) = roots.get(name) else {
                        continue;
                    };
                    let incoming = self.nested_namespace_value_with_seed(module, key, false, false);
                    let mut value = match self.scopes[self.current_scope].vars.get(name).copied() {
                        Some(id) => match self.get_type(id) {
                            Type::Namespace(namespace) => namespace.clone(),
                            _ => NamespaceType {
                                name: name.clone(),
                                exports: Vec::new(),
                                readonly_exports: Vec::new(),
                            },
                        },
                        None => NamespaceType {
                            name: name.clone(),
                            exports: Vec::new(),
                            readonly_exports: Vec::new(),
                        },
                    };
                    Self::merge_namespace_exports(&mut value, incoming);
                    self.declare_var(name, Type::Namespace(value));
                }
                StmtKind::Import(import) => self.register_import_decl(import),
                _ => {}
            }
        }
        let mut published = Vec::new();
        for body in bodies {
            self.push_scope();
            let mut aliases: Vec<_> = body
                .iter()
                .filter_map(|statement| {
                    let declaration = Self::unwrap_export_stmt(statement);
                    let StmtKind::ImportEquals(alias) = &declaration.kind else {
                        return None;
                    };
                    let exported = alias.modifiers & MOD_EXPORT != 0
                        || matches!(statement.kind, StmtKind::Export(_));
                    let mut segments = Vec::new();
                    Self::import_equals_entity_segments(&alias.module_ref, &mut segments)
                        .then_some((alias, exported, segments))
                })
                .collect();
            let mut targets = roots.clone();
            loop {
                let before = aliases.len();
                aliases.retain(|(alias, exported, segments)| {
                    let Some(root) = segments.first() else {
                        return false;
                    };
                    let target = targets.get(root).map(|root| {
                        if segments.len() == 1 {
                            root.clone()
                        } else {
                            format!("{root}.{}", segments[1..].join("."))
                        }
                    });
                    let value = self
                        .import_equals_namespace_target_type(&alias.module_ref)
                        .map(|value| match &value {
                            Type::TypeReference(name, args)
                                if self.class_info.contains_key(name) =>
                            {
                                Type::TypeReference(format!("typeof {name}"), args.clone())
                            }
                            _ => value,
                        });
                    let has_type = target.as_ref().is_some_and(|target| {
                        self.interface_info.contains_key(target)
                            || self.class_info.contains_key(target)
                            || self.type_aliases.contains_key(target)
                            || self.enum_info.contains_key(target)
                    });
                    if let Some(target) = &target {
                        if let Some((owner, member)) = target.rsplit_once('.') {
                            if self.namespace_paths.contains(owner)
                                && !self
                                    .namespace_exported_member_names
                                    .get(owner)
                                    .is_some_and(|members| members.contains(member))
                            {
                                // Failed aliases still bind an error symbol. Never
                                // expose the private declaration's actual type.
                                self.declare_var(&alias.name, Type::Error);
                                if *exported {
                                    published.push((alias.name.clone(), None, Some(Type::Error)));
                                }
                                return false;
                            }
                        }
                    }
                    if value.is_none() && !has_type {
                        return true;
                    }
                    if let Some(target) = &target {
                        targets.insert(alias.name.clone(), target.clone());
                    }
                    if let Some(value) = &value {
                        self.declare_var(&alias.name, value.clone());
                    }
                    if *exported {
                        published.push((alias.name.clone(), target, value));
                    }
                    false
                });
                if before == aliases.len() {
                    break;
                }
            }
            self.pop_scope();
        }
        self.pop_scope();
        for (name, target, value) in published {
            if let Some(target) = target {
                self.publish_global_alias_type(&name, &target);
                if matches!(value, Some(Type::Namespace(_))) {
                    let prefix = format!("{target}.");
                    let members: Vec<_> = self
                        .namespace_type_names
                        .iter()
                        .filter_map(|key| {
                            let suffix = key.strip_prefix(&prefix)?;
                            let (owner, member) = key.rsplit_once('.')?;
                            self.namespace_exported_member_names
                                .get(owner)
                                .is_some_and(|members| members.contains(member))
                                .then(|| (format!("{name}.{suffix}"), key.clone()))
                        })
                        .collect();
                    for (local, key) in members {
                        self.publish_global_alias_type(&local, &key);
                    }
                    let display = scoped_interface_display_name(&target);
                    for (_, module) in namespaces.iter().filter(|(path, _)| path == display) {
                        let mut module = (*module).clone();
                        module.name = ModuleName::Ident(name.clone());
                        self.collect_namespace_module(&module, "", false);
                    }
                }
            }
            if let Some(value) = value {
                self.user_global_value_names.insert(name.clone());
                self.declare_var(&name, value);
            }
        }
        self.file_is_module_flag = previous_module;
        self.current_file_name = previous_file;
        self.injected_decl_file = previous_injected;
        self.current_source = previous_source;
    }

    fn publish_global_alias_type(&mut self, name: &str, target: &str) {
        let parameters = self
            .interface_info
            .get(target)
            .map(|info| (info.type_params.clone(), info.required_type_params))
            .or_else(|| {
                self.class_info
                    .get(target)
                    .map(|info| (info.type_params.clone(), info.required_type_params))
            })
            .or_else(|| {
                self.type_aliases
                    .get(target)
                    .map(|(parameters, _, required)| (parameters.clone(), *required))
            })
            .or_else(|| self.enum_info.contains_key(target).then(|| (Vec::new(), 0)));
        let Some((parameters, required)) = parameters else {
            return;
        };
        let arguments = parameters
            .iter()
            .map(|name| Type::TypeParameter(name.clone()))
            .collect();
        self.type_aliases.insert(
            name.to_string(),
            (
                parameters,
                Type::TypeReference(target.to_string(), arguments),
                required,
            ),
        );
        self.global_import_type_targets
            .insert(name.to_string(), target.to_string());
        Arc::make_mut(&mut self.global_type_names).insert(name.to_string());
        self.user_global_type_names.insert(name.to_string());
        if let Some(defaults) = self.alias_type_param_defaults.get(target).cloned() {
            self.alias_type_param_defaults
                .insert(name.to_string(), defaults);
        }
    }
}
