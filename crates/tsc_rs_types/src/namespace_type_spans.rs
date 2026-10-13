//! Namespace declaration meaning for the TypeScript type-baseline walker.
//!
//! Adapts getModuleInstanceState and its export-alias resolution from
//! pingdotgg/ts-rust, ast/utilities_p3.rs (see NOTICE.txt). Native statement
//! slices provide the ancestor scopes; AST addresses key the resolution cache.

use super::*;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Instance {
    None,
    Value,
    ConstEnum,
}

#[derive(Default)]
struct Modules {
    states: HashMap<(u8, usize), Option<Instance>>,
    parents: Vec<String>,
    external_module: Option<String>,
}

struct ModuleSymbol<'a> {
    scope: usize,
    name: &'a str,
    declarations: Vec<&'a ModuleDecl>,
}

impl Modules {
    fn resolve_module<'a>(
        expression: &Expr,
        scopes: &[&'a [Stmt]],
        seen: &mut HashSet<usize>,
    ) -> Option<ModuleSymbol<'a>> {
        match &expression.kind {
            ExprKind::Ident(name) => {
                for (index, scope) in scopes.iter().enumerate().rev() {
                    let declarations: Vec<_> = scope
                        .iter()
                        .map(TypeChecker::unwrap_export_stmt)
                        .filter(|statement| Self::has_name(statement, name))
                        .collect();
                    if declarations.is_empty() {
                        continue;
                    }
                    let mut modules = Vec::new();
                    for declaration in declarations {
                        match &declaration.kind {
                            StmtKind::ModuleDecl(module) => modules.push(module.as_ref()),
                            StmtKind::ImportEquals(import) => {
                                if !seen.insert(declaration as *const Stmt as usize) {
                                    return None;
                                }
                                return Self::resolve_module(
                                    &import.module_ref,
                                    &scopes[..=index],
                                    seen,
                                );
                            }
                            _ => {}
                        }
                    }
                    let ModuleName::Ident(name) = &modules.first()?.name else {
                        return None;
                    };
                    return Some(ModuleSymbol {
                        scope: scope.as_ptr() as usize,
                        name,
                        declarations: modules,
                    });
                }
                None
            }
            ExprKind::Member(member) => {
                let parent = Self::resolve_module(&member.object, scopes, seen)?;
                for declaration in parent.declarations {
                    match &declaration.body {
                        Some(ModuleBody::Block(body)) => {
                            let modules: Vec<_> = body
                                .iter()
                                .filter_map(|statement| {
                                    let StmtKind::ModuleDecl(module) =
                                        &TypeChecker::unwrap_export_stmt(statement).kind
                                    else {
                                        return None;
                                    };
                                    matches!(&module.name, ModuleName::Ident(name) if name == &member.property)
                                        .then_some(module.as_ref())
                                })
                                .collect();
                            if let Some(module) = modules.first() {
                                let ModuleName::Ident(name) = &module.name else {
                                    return None;
                                };
                                return Some(ModuleSymbol {
                                    scope: body.as_ptr() as usize,
                                    name,
                                    declarations: modules,
                                });
                            }
                        }
                        Some(ModuleBody::Module(inner)) => {
                            if let ModuleName::Ident(name) = &inner.name {
                                if name == &member.property {
                                    return Some(ModuleSymbol {
                                        scope: declaration as *const ModuleDecl as usize,
                                        name,
                                        declarations: vec![inner],
                                    });
                                }
                            }
                        }
                        None => {}
                    }
                }
                None
            }
            _ => None,
        }
    }

    fn has_name(statement: &Stmt, name: &str) -> bool {
        match &statement.kind {
            StmtKind::Var(variable) => variable.declarations.iter().any(
                |declaration| matches!(&declaration.name.kind, PatKind::Ident(n) if n == name),
            ),
            _ => TypeChecker::statement_declared_name(statement).as_deref() == Some(name),
        }
    }

    fn local_alias<'a>(
        module: &'a ModuleDecl,
        scope: usize,
        name: &str,
        ancestors: &[&'a [Stmt]],
    ) -> Option<String> {
        let Some(ModuleBody::Block(body)) = &module.body else {
            return None;
        };
        let mut scopes = ancestors.to_vec();
        scopes.push(body);
        for statement in body {
            let StmtKind::ImportEquals(import) = &TypeChecker::unwrap_export_stmt(statement).kind
            else {
                continue;
            };
            if Self::resolve_module(&import.module_ref, &scopes, &mut HashSet::new())
                .is_some_and(|target| target.scope == scope && target.name == name)
            {
                return Some(import.name.clone());
            }
        }
        None
    }

    fn module<'a>(&mut self, module: &'a ModuleDecl, scopes: &[&'a [Stmt]]) -> Instance {
        let key = (0, module as *const ModuleDecl as usize);
        if let Some(state) = self.states.get(&key) {
            return state.unwrap_or(Instance::None);
        }
        self.states.insert(key, None);
        let state = match &module.body {
            None => Instance::Value,
            Some(ModuleBody::Module(inner)) => self.module(inner, scopes),
            Some(ModuleBody::Block(body)) => {
                let mut scopes = scopes.to_vec();
                scopes.push(body);
                let mut state = Instance::None;
                for statement in body {
                    match self.statement(statement, &scopes, false) {
                        Instance::Value => {
                            state = Instance::Value;
                            break;
                        }
                        Instance::ConstEnum => state = Instance::ConstEnum,
                        Instance::None => {}
                    }
                }
                state
            }
        };
        self.states.insert(key, Some(state));
        state
    }

    fn namespace_name<'a>(
        &mut self,
        module: &'a ModuleDecl,
        scope: usize,
        name: &str,
        scopes: &[&'a [Stmt]],
    ) -> String {
        if let Some(alias) = Self::local_alias(module, scope, name, scopes) {
            return alias;
        }
        let shadowed = if let Some(ModuleBody::Block(body)) = &module.body {
            let mut inner_scopes = scopes.to_vec();
            inner_scopes.push(body);
            body.iter().any(|statement| {
                let declaration = TypeChecker::unwrap_export_stmt(statement);
                Self::has_name(declaration, name)
                    && match &declaration.kind {
                        StmtKind::InterfaceDecl(_) | StmtKind::TypeAlias(_) => false,
                        StmtKind::ModuleDecl(inner) => {
                            self.module(inner, &inner_scopes) == Instance::Value
                        }
                        _ => true,
                    }
            })
        } else {
            false
        };
        if !shadowed {
            return name.to_string();
        }
        // The declaration is printed from inside its own body. A local
        // value of the same name requires qualification through its parent.
        let parent = if self.parents.is_empty() {
            self.external_module
                .clone()
                .unwrap_or_else(|| "globalThis".into())
        } else {
            self.parents.join(".")
        };
        format!("{parent}.{name}")
    }

    fn statement<'a>(
        &mut self,
        statement: &'a Stmt,
        scopes: &[&'a [Stmt]],
        exported: bool,
    ) -> Instance {
        let key = (
            if exported { 2 } else { 1 },
            statement as *const Stmt as usize,
        );
        if let Some(state) = self.states.get(&key) {
            return state.unwrap_or(Instance::None);
        }
        self.states.insert(key, None);
        let state = match &statement.kind {
            StmtKind::InterfaceDecl(_) | StmtKind::TypeAlias(_) => Instance::None,
            StmtKind::EnumDecl(enumeration) if enumeration.is_const => Instance::ConstEnum,
            StmtKind::Import(_) if !exported => Instance::None,
            StmtKind::ImportEquals(import) if !exported && import.modifiers & MOD_EXPORT == 0 => {
                Instance::None
            }
            StmtKind::ModuleDecl(module) => self.module(module, scopes),
            StmtKind::Export(export) => match &export.kind {
                ExportDeclKind::Decl(inner) | ExportDeclKind::DefaultDecl(inner) => {
                    self.statement(inner, scopes, true)
                }
                ExportDeclKind::Named {
                    specifiers,
                    source: None,
                    ..
                } => {
                    let mut state = Instance::None;
                    for specifier in specifiers {
                        state = state.max(if specifier.local_is_string {
                            Instance::Value
                        } else {
                            self.alias(&specifier.local, scopes)
                        });
                        if state == Instance::Value {
                            break;
                        }
                    }
                    state
                }
                _ => Instance::Value,
            },
            _ => Instance::Value,
        };
        self.states.insert(key, Some(state));
        state
    }

    fn alias<'a>(&mut self, name: &str, scopes: &[&'a [Stmt]]) -> Instance {
        for (index, scope) in scopes.iter().enumerate().rev() {
            let mut found = None;
            for statement in *scope {
                let declaration = TypeChecker::unwrap_export_stmt(statement);
                if !Self::has_name(declaration, name) {
                    continue;
                }
                let state = self.statement(statement, &scopes[..=index], false);
                found = Some(found.map_or(state, |previous: Instance| previous.max(state)));
                if found == Some(Instance::Value) {
                    return Instance::Value;
                }
                // An import-equals alias can denote a value even when the
                // non-exported import itself does not instantiate its module.
                if matches!(declaration.kind, StmtKind::ImportEquals(_)) {
                    found = Some(Instance::Value);
                }
            }
            if let Some(state) = found {
                return state;
            }
        }
        Instance::Value
    }

    fn visit_module<'a>(
        &mut self,
        checker: &mut TypeChecker,
        module: &'a ModuleDecl,
        scopes: &[&'a [Stmt]],
        scope: usize,
    ) {
        if module.name_span.is_none()
            && matches!(&module.name, ModuleName::Ident(name) if name == "global")
        {
            if let Some(text) = checker
                .current_source
                .as_deref()
                .and_then(|source| source.get(module.span.start as usize..module.span.end as usize))
            {
                let mut scanner = tsc_rs_scanner::TsScanner::new(text);
                loop {
                    let token = scanner.scan();
                    if scanner.token_value() == "global" {
                        let span = Span::new(
                            module.span.start + scanner.token_pos() as u32,
                            module.span.start + scanner.text_pos() as u32,
                        );
                        checker.record_expression_type_span(span, &Type::Any);
                        break;
                    }
                    if matches!(
                        token,
                        tsc_rs_scanner::TokenKind::EndOfFile | tsc_rs_scanner::TokenKind::OpenBrace
                    ) {
                        break;
                    }
                }
            }
        }
        if let Some(span) = module.name_span {
            let name = match &module.name {
                ModuleName::Ident(name) if self.module(module, scopes) == Instance::Value => {
                    Some(self.namespace_name(module, scope, name, scopes))
                }
                ModuleName::String(name) => Some(format!(
                    "import({})",
                    Type::StringLiteral(name.clone()).display_string_single_line()
                )),
                _ => None,
            };
            if let Some(name) = name {
                checker.record_expression_type_span(span, &Type::Typeof(name));
            }
        }
        self.parents.push(match &module.name {
            ModuleName::Ident(name) => name.clone(),
            ModuleName::String(name) => format!(
                "import({})",
                Type::StringLiteral(name.clone()).display_string_single_line()
            ),
        });
        match &module.body {
            Some(ModuleBody::Block(body)) => self.visit(checker, body, scopes),
            Some(ModuleBody::Module(inner)) => {
                self.visit_module(checker, inner, scopes, module as *const ModuleDecl as usize)
            }
            None => {}
        }
        self.parents.pop();
    }

    fn visit<'a>(
        &mut self,
        checker: &mut TypeChecker,
        statements: &'a [Stmt],
        ancestors: &[&'a [Stmt]],
    ) {
        let mut scopes = ancestors.to_vec();
        scopes.push(statements);
        for statement in statements {
            match &TypeChecker::unwrap_export_stmt(statement).kind {
                StmtKind::ModuleDecl(module) => {
                    self.visit_module(checker, module, &scopes, statements.as_ptr() as usize)
                }
                StmtKind::Block(body) => self.visit(checker, body, &scopes),
                StmtKind::FnDecl(function) => {
                    if let Some(body) = &function.body {
                        self.visit(checker, body, &scopes);
                    }
                }
                _ => {}
            }
        }
    }
}

impl TypeChecker {
    pub(crate) fn record_namespace_declaration_types(&mut self, file: &SourceFile) {
        if self.expression_type_spans.is_some() {
            let external_module = Self::file_is_module(file).then(|| {
                let path = file.file_name.replace('\\', "/");
                let stem = [
                    ".d.ts", ".d.mts", ".d.cts", ".tsx", ".ts", ".mts", ".cts", ".jsx", ".js",
                ]
                .iter()
                .find_map(|extension| path.strip_suffix(extension))
                .unwrap_or(&path);
                let path = if stem.starts_with('.') || stem.starts_with('/') {
                    stem.to_string()
                } else {
                    format!("./{stem}")
                };
                format!(
                    "import({})",
                    Type::StringLiteral(path).display_string_single_line()
                )
            });
            Modules {
                external_module,
                ..Default::default()
            }
            .visit(self, &file.statements, &[]);
        }
    }
}
