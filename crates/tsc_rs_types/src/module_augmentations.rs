use super::*;

#[derive(Clone, Copy)]
pub(super) enum ModuleBodyKind {
    Namespace,
    External,
    Augmentation { resolved: bool },
}

impl TypeChecker {
    pub(super) fn prepare_augmentation_module_detection(&mut self, file: &SourceFile) {
        if !matches!(
            self.compiler_options.jsx,
            Some(JsxEmit::ReactJSX | JsxEmit::ReactJSXDev)
        ) {
            return;
        }
        // This structural walk also inventories JSX, including JSX after the
        // augmentation. Name usage is collected separately for unused checks.
        let used = std::mem::take(&mut self.used_names);
        let written = std::mem::take(&mut self.written_member_names);
        self.file_has_jsx = false;
        self.file_has_jsx_fragment = false;
        self.collect_all_used_names(&file.statements);
        self.used_names = used;
        self.written_member_names = written;
    }

    pub(super) fn check_global_augmentation_context(&mut self, module: &ModuleDecl) {
        if module.name_span.is_some()
            || !matches!(&module.name, ModuleName::Ident(name) if name == "global")
        {
            return;
        }
        let Some(source) = self
            .current_source
            .as_deref()
            .and_then(|source| source.get(module.span.start as usize..module.span.end as usize))
        else {
            return;
        };
        let mut scanner = tsc_rs_scanner::TsScanner::new(source);
        let mut first_token = None;
        let global_span = loop {
            let kind = scanner.scan();
            let span = Span::new(
                module.span.start + scanner.token_pos() as u32,
                module.span.start + scanner.text_pos() as u32,
            );
            first_token.get_or_insert(span);
            match kind {
                tsc_rs_scanner::TokenKind::Global => break span,
                tsc_rs_scanner::TokenKind::EndOfFile => return,
                _ => {}
            }
        };
        let ambient = module.modifiers & MOD_DECLARE != 0 || self.ambient_depth > 0;
        if !ambient {
            self.diagnostics.push(Diagnostic {
                code: 2670,
                message: "Augmentations for the global scope should have 'declare' modifier unless they appear in already ambient context.".into(),
                category: DiagnosticCategory::Error,
                file_name: None,
                span: Some(global_span),
                related: None,
            });
        }
        let scope = &self.scopes[self.current_scope];
        if scope.parent.is_some() && scope.module_body_kind.is_none() {
            self.diagnostics.push(Diagnostic {
                code: 1234,
                message:
                    "An ambient module declaration is only allowed at the top level in a file."
                        .into(),
                category: DiagnosticCategory::Error,
                file_name: None,
                span: first_token,
                related: None,
            });
            return;
        }
        if !self.module_is_augmentation(module) {
            self.diagnostics.push(Diagnostic {
                code: 2669,
                message: "Augmentations for the global scope can only be directly nested in external modules or ambient module declarations.".into(),
                category: DiagnosticCategory::Error,
                file_name: None,
                span: Some(global_span),
                related: None,
            });
        }
    }

    pub(super) fn augmentation_file_is_external(&self) -> bool {
        let detection = self
            .compiler_options
            .module_detection
            .as_deref()
            .unwrap_or("auto");
        let implicit_module = !self.current_file_is_declaration()
            && (detection.eq_ignore_ascii_case("force")
                || (!detection.eq_ignore_ascii_case("legacy")
                    && ((self.current_file_module_format == Some(ModuleKind::ESNext)
                        && matches!(
                            self.compiler_options.module,
                            Some(
                                ModuleKind::Node16
                                    | ModuleKind::Node18
                                    | ModuleKind::Node20
                                    | ModuleKind::NodeNext
                            )
                        ))
                        || self.current_file_name.as_deref().is_some_and(|name| {
                            [".mts", ".cts", ".mjs", ".cjs"]
                                .iter()
                                .any(|extension| name.ends_with(extension))
                        })
                        || (self.file_has_jsx
                            && matches!(
                                self.compiler_options.jsx,
                                Some(JsxEmit::ReactJSX | JsxEmit::ReactJSXDev)
                            )))));
        self.file_is_module_flag || implicit_module
    }

    fn module_is_augmentation(&self, module: &ModuleDecl) -> bool {
        let ambient_name = matches!(module.name, ModuleName::String(_))
            || (module.name_span.is_none()
                && matches!(&module.name, ModuleName::Ident(name) if name == "global"));
        if !ambient_name {
            return false;
        }
        let scope = &self.scopes[self.current_scope];
        let external_file = self.augmentation_file_is_external();
        if scope.parent.is_none() {
            return external_file;
        }
        matches!(
            scope.module_body_kind,
            Some(ModuleBodyKind::External | ModuleBodyKind::Augmentation { .. })
        ) && scope
            .parent
            .is_some_and(|parent| self.scopes[parent].parent.is_none())
            && !external_file
    }

    pub(super) fn module_declaration_body_kind(&mut self, module: &ModuleDecl) -> ModuleBodyKind {
        if self.module_is_augmentation(module) {
            let resolved = match &module.name {
                ModuleName::Ident(_) => true,
                ModuleName::String(name) => self.can_resolve_module(name),
            };
            if self.check_module_resolution && !resolved {
                let ModuleName::String(name) = &module.name else {
                    unreachable!()
                };
                self.diagnostics.push(Diagnostic {
                    code: 2664,
                    message: format!(
                        "Invalid module name in augmentation, module '{name}' cannot be found."
                    ),
                    category: DiagnosticCategory::Error,
                    file_name: None,
                    span: module.name_span,
                    related: None,
                });
            }
            ModuleBodyKind::Augmentation { resolved }
        } else if matches!(module.name, ModuleName::String(_)) || module.name_span.is_none() {
            ModuleBodyKind::External
        } else {
            ModuleBodyKind::Namespace
        }
    }

    pub(super) fn augmentation_declaration<'a>(
        body: &[Stmt],
        statement: &'a Stmt,
    ) -> Option<&'a Stmt> {
        let explicit_exports = body.iter().any(|statement| match &statement.kind {
            StmtKind::ExportAssign(_) => true,
            StmtKind::Export(export) => matches!(
                export.kind,
                ExportDeclKind::Named { .. }
                    | ExportDeclKind::All { .. }
                    | ExportDeclKind::Default(_)
            ),
            _ => false,
        });
        match &statement.kind {
            StmtKind::Export(export) => match &export.kind {
                ExportDeclKind::Decl(inner) | ExportDeclKind::DefaultDecl(inner) => Some(inner),
                _ => None,
            },
            _ if !explicit_exports => Some(statement),
            _ => None,
        }
    }

    pub(super) fn augmentation_import_resolution(&self, source: &str) -> Option<bool> {
        if !matches!(
            self.scopes[self.current_scope].module_body_kind,
            Some(ModuleBodyKind::Augmentation { .. })
        ) {
            return None;
        }
        let cached = self.current_file_name.as_deref().is_some_and(|file| {
            let file = Self::resolvable_specifier_key(file);
            self.resolvable_specifiers
                .get(&file)
                .is_some_and(|names| names.contains(source))
                || self
                    .virtual_module_paths
                    .get(&file)
                    .is_some_and(|paths| paths.contains_key(source))
        });
        let relative = source.starts_with('.') || source.starts_with('/');
        Some(
            cached
                || (!relative
                    && (self.module_exports.contains_key(source)
                        || self
                            .available_modules
                            .contains(&source.to_ascii_lowercase()))),
        )
    }

    pub(super) fn check_augmentation_statement(&mut self, statement: &Stmt) {
        if !matches!(
            self.scopes[self.current_scope].module_body_kind,
            Some(ModuleBodyKind::Augmentation { resolved: true })
        ) {
            return;
        }
        let code = match &statement.kind {
            StmtKind::Import(_) => 2667,
            StmtKind::ImportEquals(import)
                if matches!(import.module_ref.kind, ExprKind::Call(_)) =>
            {
                2667
            }
            StmtKind::ExportAssign(_) => 2666,
            StmtKind::Export(export)
                if !matches!(
                    export.kind,
                    ExportDeclKind::Decl(_) | ExportDeclKind::DefaultDecl(_)
                ) =>
            {
                2666
            }
            _ => return,
        };
        let span = self
            .current_source
            .as_deref()
            .and_then(|text| text.get(statement.span.start as usize..statement.span.end as usize))
            .map(|source| {
                let mut scanner = tsc_rs_scanner::TsScanner::new(source);
                scanner.scan();
                Span::new(
                    statement.span.start + scanner.token_pos() as u32,
                    statement.span.start + scanner.text_pos() as u32,
                )
            })
            .unwrap_or(statement.span);
        self.diagnostics.push(Diagnostic {
            code,
            message: if code == 2667 { "Imports are not permitted in module augmentations. Consider moving them to the enclosing external module." } else { "Exports and export assignments are not permitted in module augmentations." }.into(),
            category: DiagnosticCategory::Error,
            file_name: None,
            span: Some(span),
            related: None,
        });
    }

    pub(super) fn check_augmentation_reexport_resolution(
        &mut self,
        declaration: &ExportDecl,
        source: &str,
    ) -> bool {
        if !self.check_module_resolution
            || self.augmentation_import_resolution(source) != Some(false)
        {
            return false;
        }
        let span = self
            .current_source
            .as_deref()
            .and_then(|text| {
                text.get(declaration.span.start as usize..declaration.span.end as usize)
            })
            .and_then(|source| {
                let mut scanner = tsc_rs_scanner::TsScanner::new(source);
                let mut from = false;
                loop {
                    match scanner.scan() {
                        tsc_rs_scanner::TokenKind::From => from = true,
                        tsc_rs_scanner::TokenKind::StringLiteral if from => {
                            return Some(Span::new(
                                declaration.span.start + scanner.token_pos() as u32,
                                declaration.span.start + scanner.text_pos() as u32,
                            ))
                        }
                        tsc_rs_scanner::TokenKind::EndOfFile => return None,
                        _ => {}
                    }
                }
            })
            .unwrap_or(declaration.span);
        self.diagnostics
            .push(self.missing_module_diagnostic(source, span));
        true
    }
}
