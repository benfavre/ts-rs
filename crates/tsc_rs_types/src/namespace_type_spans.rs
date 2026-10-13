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
    ambient_module_depth: usize,
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
                                    matches!(&module.name, ModuleName::Ident(name) if name == member.property)
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
                                if name == member.property {
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

    fn alias(&mut self, name: &str, scopes: &[&[Stmt]]) -> Instance {
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
                        let ty = if self.external_module.is_none() && self.ambient_module_depth == 0
                        {
                            Type::Typeof("global".into())
                        } else {
                            Type::Any
                        };
                        checker.record_expression_type_span(span, &ty);
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
        let ambient_module = matches!(module.name, ModuleName::String(_));
        self.ambient_module_depth += usize::from(ambient_module);
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
        self.ambient_module_depth -= usize::from(ambient_module);
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
    fn value_identifier_span(&self, name: &str, range: Span) -> Option<Span> {
        let text = self
            .current_source
            .as_deref()?
            .get(range.start as usize..range.end as usize)?;
        let mut scanner = tsc_rs_scanner::TsScanner::new(text);
        let mut result = None;
        loop {
            let token = scanner.scan();
            if token == tsc_rs_scanner::TokenKind::EndOfFile {
                break;
            }
            if scanner.token_value() == name {
                result = Some(Span::new(
                    range.start + scanner.token_pos() as u32,
                    range.start + scanner.text_pos() as u32,
                ));
            }
        }
        result
    }

    pub(crate) fn record_import_binding_type(&mut self, name: &str, range: Span) {
        if self.expression_type_spans.is_none() {
            return;
        }
        if let Some(span) = self.value_identifier_span(name, range) {
            let ty = self.lookup_var(name).cloned().unwrap_or(Type::Any);
            self.record_expression_type_span(span, &ty);
        }
    }

    pub(crate) fn record_import_equals_declaration(
        &mut self,
        alias: &ImportEqualsDecl,
        span: Span,
    ) {
        if self.expression_type_spans.is_none() {
            return;
        }
        self.record_import_binding_type(
            &alias.name,
            Span::new(span.start, alias.module_ref.span.start),
        );
        let ty = self.lookup_var(&alias.name).cloned().unwrap_or(Type::Any);
        self.record_import_entity(&alias.module_ref, &ty);
    }

    fn record_import_entity(&mut self, entity: &Expr, ty: &Type) {
        match &entity.kind {
            ExprKind::Ident(_) => self.record_expression_type_span(entity.span, ty),
            ExprKind::Member(member) => {
                let owner = self
                    .import_equals_namespace_target_type(&member.object)
                    .filter(|ty| matches!(ty, Type::Namespace(_) | Type::Module(_)))
                    .unwrap_or(Type::Any);
                self.record_import_entity(&member.object, &owner);
                if let Some(span) = self.value_identifier_span(
                    &member.property,
                    Span::new(member.object.span.end, entity.span.end),
                ) {
                    self.record_expression_type_span(span, ty);
                }
            }
            _ => {}
        }
    }

    pub(crate) fn record_type_query_entity(&mut self, entity: &Expr) {
        if self.expression_type_spans.is_none() {
            return;
        }
        if let ExprKind::Member(member) = &entity.kind {
            self.record_type_query_entity(&member.object);
        }
        let ty = self.infer_expr_type(entity);
        self.record_checked_expression_type(entity, &ty);
    }

    pub(crate) fn record_expression_member_names(&mut self, expression: &Expr, ty: &Type) {
        match (&expression.kind, ty) {
            (ExprKind::Member(member), _) => {
                if let Some(span) = self.value_identifier_span(
                    &member.property,
                    Span::new(member.object.span.end, expression.span.end),
                ) {
                    self.record_expression_type_span(span, ty);
                }
            }
            (ExprKind::ObjectLit(properties), Type::ObjectType(object)) => {
                for property in properties {
                    let name = match property {
                        ObjLitProp::Property(property) => &property.key,
                        ObjLitProp::Method(method) => &method.name,
                        ObjLitProp::Get(accessor) | ObjLitProp::Set(accessor) => &accessor.name,
                        _ => continue,
                    };
                    if matches!(name, PropName::Computed(..)) {
                        continue;
                    }
                    let key = self.prop_name_to_string(name);
                    if let Some((_, ty)) = object.properties.iter().find(|(name, _)| name == &key) {
                        self.record_expression_type_span(name.span(), &Self::member_read_type(ty));
                    }
                }
            }
            _ => {}
        }
    }

    /// Native equivalent of the value-symbol chain selected by the donor's
    /// symbolToTypeNode. Search lexical scopes from the inside out, retaining
    /// declaration identity when namespace values have identical exports.
    pub(crate) fn namespace_type_node_name(
        &self,
        namespace: &NamespaceType,
        parameters: &[String],
    ) -> String {
        let path = &namespace.qualified_name;
        let parts: Vec<_> = path.split('.').collect();
        let mut scope = self.current_scope;
        let mut shadowed: HashSet<&str> = parameters.iter().map(String::as_str).collect();
        loop {
            let current = &self.scopes[scope];
            for start in (0..parts.len()).rev() {
                let root = parts[start];
                if shadowed.contains(root) {
                    continue;
                }
                let Some(id) = current.vars.get(root) else {
                    continue;
                };
                let mut target = Some(self.arena[*id as usize].clone());
                for member in &parts[start + 1..] {
                    target = target.and_then(|ty| self.qualified_property_type(ty, member));
                }
                if matches!(target, Some(Type::Namespace(ref ns)) if ns.qualified_name == *path) {
                    return parts[start..].join(".");
                }
            }
            for alias in &current.namespace_alias_order {
                if !shadowed.contains(alias.as_str())
                    && current
                        .readonly_namespace_aliases
                        .get(alias)
                        .is_some_and(|ns| ns.qualified_name == *path)
                {
                    return alias.clone();
                }
            }
            shadowed.extend(current.vars.keys().map(String::as_str));
            let Some(parent) = current.parent else {
                break;
            };
            scope = parent;
        }
        if shadowed.contains(parts[0]) {
            // This flag is initialized before declaration hoisting, when
            // inferred function signatures may already need a type node.
            if !self.file_is_module_flag {
                return format!("globalThis.{path}");
            }
            if let Some(file) = self.current_file_name.as_deref() {
                return format!("{}.{path}", namespace_file_import(file));
            }
        }
        path.clone()
    }

    pub(crate) fn record_namespace_declaration_types(&mut self, file: &SourceFile) {
        if self.expression_type_spans.is_some() {
            let external_module =
                Self::file_is_module(file).then(|| namespace_file_import(&file.file_name));
            Modules {
                external_module,
                ..Default::default()
            }
            .visit(self, &file.statements, &[]);
        }
    }
}

fn namespace_file_import(file: &str) -> String {
    let path = file.replace('\\', "/");
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
}
