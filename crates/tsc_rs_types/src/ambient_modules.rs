use super::*;

impl TypeChecker {
    pub(super) fn is_ambient_module_type_key(key: &str) -> bool {
        key.strip_prefix(FILE_INTERFACE_KEY_PREFIX)
            .is_some_and(|owner| owner.starts_with("\0ambient:"))
    }

    pub(super) fn namespace_member_type_key(owner: &str, name: &str) -> String {
        if owner.starts_with(FILE_INTERFACE_KEY_PREFIX) && owner.ends_with("::") {
            format!("{owner}{name}")
        } else {
            format!("{owner}.{name}")
        }
    }

    pub(super) fn register_ambient_augmentation_export(
        &mut self,
        specifier: &str,
        exported: &str,
        key: &str,
    ) {
        let Some(path) = self.ambient_module_paths.get(specifier).cloned() else {
            return;
        };
        Arc::make_mut(&mut self.namespace_type_names).insert(key.to_string());
        Arc::make_mut(&mut self.module_type_exports)
            .entry(path.clone())
            .or_default()
            .insert(exported.to_string());
        Arc::make_mut(&mut self.module_known_exports)
            .entry(path.clone())
            .or_default()
            .insert(exported.to_string());
        let exports = Arc::make_mut(&mut self.module_exports)
            .entry(path)
            .or_default();
        if !exports.iter().any(|(name, _)| name == exported) {
            exports.push((
                exported.to_string(),
                Type::TypeReference(key.to_string(), Arc::from([] as [Type; 0])),
            ));
        }
    }

    /// Ambient external modules own their declarations just as source-file
    /// modules do. Their registry keys must never bind the same-named globals.
    pub(super) fn prepare_ambient_modules(&mut self, files: &[&SourceFile], inject_types: bool) {
        let declarations: Vec<_> = files
            .iter()
            .filter(|file| !Self::file_is_module(file))
            .flat_map(|file| {
                file.statements.iter().filter_map(move |statement| {
                    let StmtKind::ModuleDecl(module) = &statement.kind else {
                        return None;
                    };
                    let ModuleName::String(name) = &module.name else {
                        return None;
                    };
                    let Some(ModuleBody::Block(_)) = &module.body else {
                        return None;
                    };
                    Some((*file, module.as_ref(), name.as_str()))
                })
            })
            .collect();
        for (_, module, name) in &declarations {
            let path = format!("\0ambient:{name}.d.ts");
            Arc::make_mut(&mut self.ambient_module_paths).insert(name.to_string(), path.clone());
            let owner = Self::file_interface_registry_key(&path, "");
            Self::collect_namespace_type_names(
                module,
                &owner,
                Arc::make_mut(&mut self.namespace_type_names),
            );
        }
        for (file, module, name) in declarations {
            let path = self.ambient_module_paths[name].clone();
            let owner = Self::file_interface_registry_key(&path, "");
            let previous_file = self.injected_decl_file.replace(file.file_name.clone());
            let previous_source = self.current_source.replace(file.text.clone());
            let previous_module = std::mem::replace(&mut self.file_is_module_flag, true);
            self.ambient_depth += 1;
            if inject_types {
                self.inject_namespace_type_members(module, &owner);
            }
            let (exports, type_names) = self.ambient_module_exports(module, &owner);
            let known = Arc::make_mut(&mut self.module_known_exports)
                .entry(path.clone())
                .or_default();
            known.insert(MODULE_MARKER_EXPORT.into());
            known.extend(exports.iter().map(|(name, _)| name.clone()));
            Arc::make_mut(&mut self.module_type_exports)
                .entry(path.clone())
                .or_default()
                .extend(type_names);
            let entry = Arc::make_mut(&mut self.module_exports)
                .entry(path)
                .or_default();
            for (name, ty) in exports {
                if let Some((_, existing)) =
                    entry.iter_mut().find(|(exported, _)| exported == &name)
                {
                    *existing = ty;
                } else {
                    entry.push((name, ty));
                }
            }
            self.ambient_depth -= 1;
            self.file_is_module_flag = previous_module;
            self.current_source = previous_source;
            self.injected_decl_file = previous_file;
        }
    }

    fn ambient_module_exports(
        &mut self,
        module: &ModuleDecl,
        owner: &str,
    ) -> (Vec<(String, Type)>, rustc_hash::FxHashSet<String>) {
        let Some(ModuleBody::Block(body)) = &module.body else {
            unreachable!()
        };
        // First resolve all locals, including declarations after an export
        // clause. Export assignments and clauses select from this table.
        let mut local_module = module.clone();
        local_module.body = Some(ModuleBody::Block(
            body.iter()
                .filter_map(|statement| match &statement.kind {
                    StmtKind::Export(export) => match &export.kind {
                        ExportDeclKind::Decl(inner) | ExportDeclKind::DefaultDecl(inner) => {
                            Some(inner.as_ref().clone())
                        }
                        _ => None,
                    },
                    StmtKind::ExportAssign(_) => None,
                    _ => Some(statement.clone()),
                })
                .collect(),
        ));
        let namespace = self.build_namespace_value_exports_core(&local_module, owner, true, false);
        let mut locals: Vec<(String, Type)> = Vec::new();
        for (name, mut ty) in namespace.exports {
            if let Type::TypeReference(key, args) = &ty {
                if self.class_info.contains_key(key) {
                    ty = Type::TypeReference(format!("typeof {key}"), args.clone());
                }
            }
            if let Type::Namespace(namespace) = ty {
                Self::push_namespace_export(&mut locals, &name, namespace);
            } else if let Some((_, old @ Type::Namespace(_))) =
                locals.iter_mut().find(|(local, _)| local == &name)
            {
                if let Type::Namespace(namespace) = old.clone() {
                    let _ = Self::fold_namespace_into_value(&mut ty, namespace);
                    *old = ty;
                }
            } else {
                locals.push((name, ty));
            }
        }
        let mut local_types = rustc_hash::FxHashSet::default();
        for statement in body {
            let declaration = Self::unwrap_export_stmt(statement);
            let name = match &declaration.kind {
                StmtKind::ClassDecl(class) => class.name.as_deref(),
                StmtKind::InterfaceDecl(interface) => Some(interface.name.as_str()),
                StmtKind::TypeAlias(alias) => Some(alias.name.as_str()),
                StmtKind::EnumDecl(enumeration) => Some(enumeration.name.as_str()),
                _ => None,
            };
            if let Some(name) = name {
                local_types.insert(name.to_string());
                if !locals.iter().any(|(exported, _)| exported == name) {
                    locals.push((
                        name.to_string(),
                        Type::TypeReference(
                            Self::namespace_member_type_key(owner, name),
                            Arc::from([] as [Type; 0]),
                        ),
                    ));
                }
            }
        }
        // Entity aliases can precede their targets and can refer to merged
        // class/namespace values, so resolve them after collecting locals.
        for _ in 0..body.len() {
            let mut changed = false;
            for statement in body {
                let StmtKind::ImportEquals(alias) = &Self::unwrap_export_stmt(statement).kind
                else {
                    continue;
                };
                if let Some(ty) =
                    Self::namespace_binding_type(&locals, &self.expr_to_name(&alias.module_ref))
                {
                    if let Some((_, old)) = locals.iter_mut().find(|(name, _)| name == &alias.name)
                    {
                        if *old != ty {
                            *old = ty;
                            changed = true;
                        }
                    }
                }
            }
            if !changed {
                break;
            }
        }
        let mut exports = self
            .build_namespace_value_exports_core(module, owner, true, false)
            .exports;
        for (name, ty) in &mut exports {
            if let Some((_, local)) = locals.iter().find(|(local, _)| local == name) {
                *ty = local.clone();
            }
        }
        let mut type_names = rustc_hash::FxHashSet::default();
        for statement in body {
            if let Some(declaration) = Self::augmentation_declaration(body, statement) {
                if let Some(name) = Self::statement_declared_name(declaration) {
                    if local_types.contains(&name) {
                        type_names.insert(name.clone());
                        if !exports.iter().any(|(exported, _)| exported == &name) {
                            if let Some((_, ty)) = locals.iter().find(|(local, _)| local == &name) {
                                exports.push((name, ty.clone()));
                            }
                        }
                    }
                }
            }
            match &statement.kind {
                StmtKind::ExportAssign(expression) => {
                    let target = self.expr_to_name(expression);
                    let ty = Self::namespace_binding_type(&locals, &target)
                        .or_else(|| self.resolve_value_path_type(&target))
                        .unwrap_or(Type::Any);
                    exports.push(("export=".into(), ty));
                }
                StmtKind::Export(export) => {
                    if let ExportDeclKind::Named {
                        specifiers,
                        source: None,
                        ..
                    } = &export.kind
                    {
                        for specifier in specifiers {
                            let name = specifier.exported.as_ref().unwrap_or(&specifier.local);
                            if let Some((_, ty)) =
                                locals.iter().find(|(local, _)| local == &specifier.local)
                            {
                                if !exports.iter().any(|(exported, _)| exported == name) {
                                    exports.push((name.clone(), ty.clone()));
                                }
                            }
                            if local_types.contains(&specifier.local) {
                                type_names.insert(name.clone());
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        (exports, type_names)
    }
}
