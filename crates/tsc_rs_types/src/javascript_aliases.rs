use super::*;

const VALUE: u8 = 1;
const ERASED: u8 = 2;
const NAMESPACE: u8 = 4;

pub(super) type AliasMeanings = rustc_hash::FxHashMap<(String, bool, String), u8>;

struct AliasModuleIndex {
    files: ProgramFileIndex,
    ambient: rustc_hash::FxHashSet<String>,
}

#[derive(Default)]
struct AliasGraph {
    nodes: rustc_hash::FxHashMap<(String, bool, String), usize>,
    meanings: Vec<u8>,
    edges: Vec<(usize, usize)>,
}

impl AliasGraph {
    fn node(&mut self, file: &str, exported: bool, name: &str) -> usize {
        let key = (file.to_string(), exported, name.to_string());
        *self.nodes.entry(key).or_insert_with(|| {
            let index = self.meanings.len();
            self.meanings.push(0);
            index
        })
    }

    fn fact(&mut self, file: &str, exported: bool, name: &str, meaning: u8) {
        let node = self.node(file, exported, name);
        self.meanings[node] |= meaning;
    }

    fn alias(
        &mut self,
        file: &str,
        exported: bool,
        name: &str,
        target: &str,
        target_exported: bool,
        remote: &str,
    ) {
        let from = self.node(target, target_exported, remote);
        let to = self.node(file, exported, name);
        self.edges.push((from, to));
    }

    fn finish(mut self) -> AliasMeanings {
        // Facts only accumulate. Cycles without declarations retain unknown
        // meaning; a runtime declaration wins over merged type declarations.
        let mut successors = vec![Vec::new(); self.meanings.len()];
        for (from, to) in self.edges {
            successors[from].push(to);
        }
        let mut pending: std::collections::VecDeque<_> = self
            .meanings
            .iter()
            .enumerate()
            .filter_map(|(index, meaning)| (*meaning != 0).then_some(index))
            .collect();
        while let Some(from) = pending.pop_front() {
            for &to in &successors[from] {
                let merged = self.meanings[to] | self.meanings[from];
                if merged != self.meanings[to] {
                    self.meanings[to] = merged;
                    pending.push_back(to);
                }
            }
        }
        self.nodes
            .into_iter()
            .map(|(key, index)| (key, self.meanings[index]))
            .collect()
    }
}

fn namespace_meaning(module: &ModuleDecl) -> u8 {
    let runtime =
        match &module.body {
            None => false,
            Some(ModuleBody::Module(inner)) => namespace_meaning(inner) & VALUE != 0,
            Some(ModuleBody::Block(body)) => body.iter().any(|statement| {
                match &TypeChecker::unwrap_export_stmt(statement).kind {
                    StmtKind::InterfaceDecl(_) | StmtKind::TypeAlias(_) | StmtKind::Empty => false,
                    StmtKind::ModuleDecl(inner) => namespace_meaning(inner) & VALUE != 0,
                    StmtKind::Export(export) => !matches!(
                        export.kind,
                        ExportDeclKind::Named { .. } | ExportDeclKind::All { .. }
                    ),
                    _ => true,
                }
            }),
        };
    NAMESPACE | if runtime { VALUE } else { ERASED }
}

impl TypeChecker {
    pub(super) fn build_javascript_alias_meanings(&mut self, files: &[&SourceFile]) {
        let mut index = AliasModuleIndex {
            files: ProgramFileIndex::new(files.iter().map(|file| file.file_name.clone()).collect()),
            ambient: Default::default(),
        };
        for file in files.iter().filter(|file| !Self::file_is_module(file)) {
            for statement in &file.statements {
                if let StmtKind::ModuleDecl(module) = &statement.kind {
                    if let ModuleName::String(name) = &module.name {
                        index.ambient.insert(name.clone());
                    }
                }
            }
        }
        let mut graph = AliasGraph::default();
        for file in files {
            let desugared = [".js", ".jsx", ".mjs", ".cjs"]
                .iter()
                .any(|ext| file.file_name.ends_with(ext))
                .then(|| jsdoc::desugar(file))
                .flatten();
            let file = desugared.as_ref().unwrap_or(file);
            self.collect_javascript_alias_meanings(
                &mut graph,
                &index,
                &file.file_name,
                &file.file_name,
                &file.statements,
                false,
            );
            if !Self::file_is_module(file) {
                for statement in &file.statements {
                    let StmtKind::ModuleDecl(module) = &statement.kind else {
                        continue;
                    };
                    let ModuleName::String(name) = &module.name else {
                        continue;
                    };
                    let Some(ModuleBody::Block(body)) = &module.body else {
                        continue;
                    };
                    let path = format!("\0ambient:{name}.d.ts");
                    self.collect_javascript_alias_meanings(
                        &mut graph,
                        &index,
                        &path,
                        &file.file_name,
                        body,
                        true,
                    );
                }
            }
        }
        self.javascript_alias_meanings = Arc::new(graph.finish());
    }

    fn collect_javascript_alias_meanings(
        &self,
        graph: &mut AliasGraph,
        index: &AliasModuleIndex,
        owner: &str,
        containing: &str,
        statements: &[Stmt],
        ambient: bool,
    ) {
        let implicit_exports = ambient
            && !statements.iter().any(|statement| match &statement.kind {
                StmtKind::ExportAssign(_) => true,
                StmtKind::Export(export) => !matches!(
                    export.kind,
                    ExportDeclKind::Decl(_) | ExportDeclKind::DefaultDecl(_)
                ),
                _ => false,
            });
        let is_js = [".js", ".jsx", ".mjs", ".cjs"]
            .iter()
            .any(|ext| containing.ends_with(ext));
        let resolve = |source: &str| {
            index
                .ambient
                .contains(source)
                .then(|| format!("\0ambient:{source}.d.ts"))
                .or_else(|| self.resolve_module_to_path(source, containing))
                .or_else(|| {
                    Self::javascript_program_file_for_specifier(source, containing, &index.files)
                })
        };
        let automatic_exports: rustc_hash::FxHashSet<_> = statements
            .iter()
            .filter_map(|statement| {
                let StmtKind::TypeAlias(alias) = &statement.kind else {
                    return None;
                };
                (is_js && alias.name_span.is_none()).then(|| {
                    alias
                        .name
                        .split('.')
                        .next()
                        .unwrap_or(&alias.name)
                        .to_string()
                })
            })
            .collect();
        let mut explicit = rustc_hash::FxHashSet::default();
        for statement in statements {
            let declaration = match &statement.kind {
                StmtKind::Export(export) => match &export.kind {
                    ExportDeclKind::DefaultDecl(declaration) => declaration.as_ref(),
                    _ => Self::unwrap_export_stmt(statement),
                },
                _ => statement,
            };
            let mut declared = Vec::new();
            match &declaration.kind {
                StmtKind::InterfaceDecl(interface) => {
                    declared.push((interface.name.clone(), ERASED))
                }
                StmtKind::TypeAlias(alias) => {
                    declared.push((alias.name.clone(), ERASED));
                    if is_js && alias.name_span.is_none() {
                        let (root, meaning) = alias
                            .name
                            .split_once('.')
                            .map_or((alias.name.as_str(), ERASED), |(root, _)| {
                                (root, ERASED | NAMESPACE)
                            });
                        graph.fact(owner, true, root, meaning);
                    }
                }
                StmtKind::ClassDecl(class) => {
                    declared.extend(class.name.iter().map(|name| (name.clone(), VALUE)))
                }
                StmtKind::EnumDecl(enumeration) => declared.push((enumeration.name.clone(), VALUE)),
                StmtKind::FnDecl(function) => {
                    declared.extend(function.name.iter().map(|name| (name.clone(), VALUE)))
                }
                StmtKind::Var(variable) => {
                    for declaration in &variable.declarations {
                        let mut names = Vec::new();
                        Self::pattern_binding_spans(&declaration.name, &mut names);
                        declared.extend(names.into_iter().map(|(name, _)| (name, VALUE)));
                    }
                }
                StmtKind::ModuleDecl(module) => {
                    if let ModuleName::Ident(name) = &module.name {
                        declared.push((name.clone(), namespace_meaning(module)));
                    }
                }
                StmtKind::Import(import) => {
                    if let Some(path) = resolve(&import.source) {
                        match &import.specifiers {
                            ImportClause::Require(local) => {
                                graph.alias(owner, false, local, &path, true, "export=")
                            }
                            ImportClause::Named {
                                default,
                                named,
                                namespace,
                            } => {
                                if let Some(local) = default {
                                    graph.alias(owner, false, local, &path, true, "default");
                                }
                                if let Some(local) = namespace {
                                    graph.fact(owner, false, local, VALUE | NAMESPACE);
                                }
                                for specifier in named {
                                    graph.alias(
                                        owner,
                                        false,
                                        &specifier.local,
                                        &path,
                                        true,
                                        specifier.imported.as_deref().unwrap_or(&specifier.local),
                                    );
                                }
                            }
                        }
                    }
                }
                StmtKind::ImportEquals(alias) => {
                    if let ExprKind::Ident(target) = &alias.module_ref.kind {
                        graph.alias(owner, false, &alias.name, owner, false, target);
                    }
                }
                _ => {}
            }
            for (name, meaning) in declared {
                graph.fact(owner, false, &name, meaning);
                if (implicit_exports
                    && !matches!(&statement.kind, StmtKind::Export(export) if matches!(export.kind, ExportDeclKind::DefaultDecl(_))))
                    || matches!(&statement.kind, StmtKind::Export(export) if matches!(export.kind, ExportDeclKind::Decl(_)))
                {
                    graph.alias(owner, true, &name, owner, false, &name);
                    explicit.insert(name);
                }
            }
            match &statement.kind {
                StmtKind::Export(export) => match &export.kind {
                    ExportDeclKind::Named {
                        specifiers, source, ..
                    } => {
                        for specifier in specifiers {
                            let name = specifier.exported.as_deref().unwrap_or(&specifier.local);
                            explicit.insert(name.to_string());
                            // An automatically exported JSDoc declaration already
                            // owns this export symbol; an export clause naming a
                            // private value does not replace the type declaration.
                            if automatic_exports.contains(name) {
                                continue;
                            }
                            if let Some(source) = source {
                                if let Some(path) = resolve(source) {
                                    graph.alias(owner, true, name, &path, true, &specifier.local);
                                }
                            } else {
                                graph.alias(owner, true, name, owner, false, &specifier.local);
                            }
                        }
                    }
                    ExportDeclKind::Default(expression) => {
                        explicit.insert("default".into());
                        if let ExprKind::Ident(name) = &expression.kind {
                            graph.alias(owner, true, "default", owner, false, name);
                        } else {
                            graph.fact(owner, true, "default", VALUE);
                        }
                    }
                    ExportDeclKind::DefaultDecl(declaration) => {
                        explicit.insert("default".into());
                        let meaning = if matches!(
                            declaration.kind,
                            StmtKind::InterfaceDecl(_) | StmtKind::TypeAlias(_)
                        ) {
                            ERASED
                        } else {
                            VALUE
                        };
                        graph.fact(owner, true, "default", meaning);
                    }
                    ExportDeclKind::All {
                        alias: Some(name), ..
                    } => {
                        explicit.insert(name.clone());
                        graph.fact(owner, true, name, VALUE | NAMESPACE);
                    }
                    _ => {}
                },
                StmtKind::ExportAssign(expression) => {
                    if let ExprKind::Ident(name) = &expression.kind {
                        graph.alias(owner, true, "export=", owner, false, name);
                    } else {
                        graph.fact(owner, true, "export=", VALUE);
                    }
                }
                _ => {}
            }
        }
        for statement in statements {
            let StmtKind::Export(export) = &statement.kind else {
                continue;
            };
            let ExportDeclKind::All {
                source,
                alias: None,
                ..
            } = &export.kind
            else {
                continue;
            };
            let Some(path) = resolve(source) else {
                continue;
            };
            let Some(names) = self.lookup_module_known_exports(&path) else {
                continue;
            };
            for name in names {
                if name != "default" && name != "export=" && !explicit.contains(name) {
                    graph.alias(owner, true, name, &path, true, name);
                }
            }
        }
    }

    fn javascript_alias_meaning(&self, file: &str, exported: bool, name: &str) -> u8 {
        Self::module_path_alternatives(file)
            .iter()
            .find_map(|path| {
                self.javascript_alias_meanings
                    .get(&(path.clone(), exported, name.to_string()))
                    .copied()
            })
            .unwrap_or(0)
    }

    fn javascript_program_file_for_specifier(
        source: &str,
        containing: &str,
        index: &ProgramFileIndex,
    ) -> Option<String> {
        Self::program_file_for_specifier(source, containing, index).or_else(|| {
            [".js", ".jsx", ".mjs", ".cjs"]
                .iter()
                .find_map(|extension| {
                    Self::program_file_for_specifier(
                        &format!("{source}{extension}"),
                        containing,
                        index,
                    )
                })
        })
    }

    pub(super) fn javascript_alias_is_erased(
        &self,
        file: &str,
        exported: bool,
        name: &str,
    ) -> bool {
        self.javascript_alias_meaning(file, exported, name) & (VALUE | ERASED) == ERASED
    }

    pub(super) fn javascript_import_is_erased(&self, source: &str, name: &str) -> bool {
        self.javascript_import_meaning(source, name) & (VALUE | ERASED) == ERASED
    }

    fn javascript_import_meaning(&self, source: &str, name: &str) -> u8 {
        let ambient =
            self.javascript_alias_meaning(&format!("\0ambient:{source}.d.ts"), true, name);
        if ambient != 0 {
            return ambient;
        }
        self.current_file_name
            .as_deref()
            .and_then(|file| {
                self.resolve_module_to_path(source, file)
                    .or_else(|| self.program_file_for_relative_specifier(source))
                    .or_else(|| {
                        [".js", ".jsx", ".mjs", ".cjs"]
                            .iter()
                            .find_map(|extension| {
                                self.program_file_for_relative_specifier(&format!(
                                    "{source}{extension}"
                                ))
                            })
                    })
            })
            .map_or(0, |path| self.javascript_alias_meaning(&path, true, name))
    }

    pub(super) fn check_javascript_type_import(&mut self, statement: &Stmt) {
        if !self.current_file_is_js() {
            return;
        }
        let StmtKind::Import(import) = &statement.kind else {
            return;
        };
        if import.type_only {
            return;
        }
        let mut bindings = Vec::new();
        match &import.specifiers {
            ImportClause::Named { default, named, .. } => {
                if let Some(name) = default {
                    bindings.push((
                        "default",
                        name.as_str(),
                        self.javascript_alias_token_span(
                            import.span,
                            1 + usize::from(import.defer),
                        ),
                    ));
                }
                for specifier in named {
                    if specifier.is_type {
                        continue;
                    }
                    let remote = specifier.imported.as_deref().unwrap_or(&specifier.local);
                    bindings.push((
                        remote,
                        if specifier.imported_is_string {
                            specifier.local.as_str()
                        } else {
                            remote
                        },
                        self.javascript_alias_token_span(specifier.span, 0),
                    ));
                }
            }
            ImportClause::Require(_) => {}
        }
        for (remote, displayed, span) in bindings {
            let meaning = self.javascript_import_meaning(&import.source, remote);
            if meaning & (VALUE | ERASED) == ERASED {
                let displayed = self.cooked_identifier_name(displayed, span);
                self.diagnostics.push(Diagnostic {
                    code: 18042,
                    message: format!("'{displayed}' is a type and cannot be imported in JavaScript files. Use 'import(\"{}\").{displayed}' in a JSDoc type annotation.", import.source),
                    category: DiagnosticCategory::Error,
                    file_name: None,
                    span: Some(span),
                    related: None,
                });
            }
        }
    }

    pub(super) fn javascript_alias_token_span(&self, span: Span, skip_tokens: usize) -> Span {
        let Some(text) = self
            .current_source
            .as_deref()
            .and_then(|text| text.get(span.start as usize..span.end as usize))
        else {
            return span;
        };
        let mut scanner = tsc_rs_scanner::TsScanner::new(text);
        for _ in 0..=skip_tokens {
            if scanner.scan() == tsc_rs_scanner::TokenKind::EndOfFile {
                return span;
            }
        }
        Span::new(
            span.start + scanner.token_pos() as u32,
            span.start + scanner.text_pos() as u32,
        )
    }

    pub(super) fn collect_javascript_erased_imports(&mut self, file: &SourceFile) {
        for statement in &file.statements {
            let StmtKind::Import(import) = &statement.kind else {
                continue;
            };
            let ImportClause::Named { default, named, .. } = &import.specifiers else {
                continue;
            };
            let bindings = default
                .iter()
                .map(String::as_str)
                .chain(named.iter().map(|specifier| specifier.local.as_str()));
            for local in bindings {
                // Use the local symbol's merged meaning: a same-named value
                // declaration can supply a runtime binding even when the import
                // itself names an erased declaration.
                let meaning = self.javascript_alias_meaning(&file.file_name, false, local);
                if meaning & (VALUE | ERASED) != ERASED {
                    continue;
                }
                if meaning & NAMESPACE != 0 {
                    self.uninstantiated_namespace_names
                        .insert(local.to_string());
                } else {
                    self.javascript_erased_type_imports
                        .insert(local.to_string());
                }
            }
        }
    }

    pub(super) fn javascript_import_has_no_value_at(
        &self,
        file: &SourceFile,
        name: &str,
        span: Span,
    ) -> bool {
        file.statements.iter().any(|statement| {
            let StmtKind::Import(import) = &statement.kind else {
                return false;
            };
            if span.start < import.span.start || span.end > import.span.end {
                return false;
            }
            let ImportClause::Named { default, named, .. } = &import.specifiers else {
                return false;
            };
            if default.as_deref() == Some(name) {
                return self.javascript_import_is_erased(&import.source, "default");
            }
            named.iter().any(|specifier| {
                specifier.local == name
                    && self.javascript_import_is_erased(
                        &import.source,
                        specifier.imported.as_deref().unwrap_or(&specifier.local),
                    )
            })
        })
    }
}
