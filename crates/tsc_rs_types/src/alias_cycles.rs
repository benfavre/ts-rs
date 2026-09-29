//! Program-wide alias resolution, retaining declaration identity across modules.
use super::*;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Binding {
    Alias(usize),
    Namespace(usize),
    Value,
}

#[derive(Clone)]
enum Reference {
    Entity(Vec<String>),
    External(String, Option<String>),
    Module(usize),
}

#[derive(Default)]
struct Scope {
    parent: Option<usize>,
    bindings: rustc_hash::FxHashMap<String, Binding>,
    exports: rustc_hash::FxHashMap<String, Binding>,
    assignment: Option<Reference>,
    stars: Vec<(usize, String)>,
    external_imports: bool,
}

enum AliasPlacement {
    Local,
    Export,
    ExportedLocal,
}

struct Alias {
    file: usize,
    scope: usize,
    name: String,
    span: Span,
    reference: Reference,
}

struct AliasGraph<'a> {
    checker: &'a TypeChecker,
    files: &'a [&'a SourceFile],
    keys: Vec<String>,
    scopes: Vec<Scope>,
    roots: Vec<usize>,
    ambient: rustc_hash::FxHashMap<String, usize>,
    aliases: Vec<Alias>,
    dependencies: Vec<Vec<String>>,
    external_targets: rustc_hash::FxHashMap<(usize, String), Option<usize>>,
    state: Vec<u8>,
    resolved: Vec<Option<Binding>>,
    export_stack: rustc_hash::FxHashSet<(usize, String)>,
    diagnostics: rustc_hash::FxHashMap<String, Vec<Diagnostic>>,
}

impl<'a> AliasGraph<'a> {
    fn new(checker: &'a TypeChecker, files: &'a [&'a SourceFile]) -> Self {
        Self {
            checker,
            files,
            keys: files.iter().map(|f| f.file_name.clone()).collect(),
            scopes: vec![Scope::default()],
            roots: Vec::new(),
            ambient: Default::default(),
            aliases: Vec::new(),
            dependencies: vec![Vec::new(); files.len()],
            external_targets: Default::default(),
            state: Vec::new(),
            resolved: Vec::new(),
            export_stack: Default::default(),
            diagnostics: Default::default(),
        }
    }

    fn scope(&mut self, parent: Option<usize>, external_imports: bool) -> usize {
        let id = self.scopes.len();
        self.scopes.push(Scope {
            parent,
            external_imports,
            ..Scope::default()
        });
        id
    }

    fn bind(&mut self, scope: usize, name: String, binding: Binding, exported: bool) {
        let name = Self::symbol_key(&name);
        self.scopes[scope].bindings.insert(name.clone(), binding);
        if exported {
            self.scopes[scope].exports.insert(name, binding);
        }
    }

    fn alias(
        &mut self,
        file: usize,
        scope: usize,
        name: String,
        span: Span,
        reference: Reference,
        placement: AliasPlacement,
    ) {
        let local = matches!(
            placement,
            AliasPlacement::Local | AliasPlacement::ExportedLocal
        );
        let exported = matches!(
            placement,
            AliasPlacement::Export | AliasPlacement::ExportedLocal
        );
        let id = self.aliases.len();
        self.aliases.push(Alias {
            file,
            scope,
            name: name.clone(),
            span,
            reference,
        });
        if local {
            self.scopes[scope]
                .bindings
                .insert(Self::symbol_key(&name), Binding::Alias(id));
        }
        if exported {
            self.scopes[scope].exports.insert(name, Binding::Alias(id));
        }
    }

    fn entity(expression: &Expr) -> Option<Reference> {
        let mut path = Vec::new();
        TypeChecker::import_equals_entity_segments(expression, &mut path)
            .then(|| Reference::Entity(path.iter().map(|name| Self::symbol_key(name)).collect()))
    }

    fn import_name_span(&self, file: usize, span: Span, namespace: bool) -> Span {
        use tsc_rs_scanner::TokenKind;
        let Some(text) = self.files[file]
            .text
            .get(span.start as usize..span.end as usize)
        else {
            return span;
        };
        let mut scanner = tsc_rs_scanner::TsScanner::new(text);
        scanner.scan();
        let mut start = None;
        let mut previous = (TokenKind::Unknown, 0);
        let mut before_previous = 0;
        while scanner.scan() != TokenKind::EndOfFile {
            start.get_or_insert(scanner.token_pos());
            if namespace && previous.0 == TokenKind::Asterisk && scanner.token() == TokenKind::As {
                scanner.scan();
                return Span::new(
                    span.start + scanner.token_pos() as u32,
                    span.start + scanner.text_pos() as u32,
                );
            }
            if !namespace
                && scanner.token() == TokenKind::StringLiteral
                && previous.0 == TokenKind::From
            {
                return Span::new(
                    span.start + start.unwrap_or(0) as u32,
                    span.start + before_previous as u32,
                );
            }
            before_previous = previous.1;
            previous = (scanner.token(), scanner.text_pos());
        }
        span
    }

    fn symbol_key(name: &str) -> String {
        if !name.contains('\\') {
            return name.to_owned();
        }
        let mut scanner = tsc_rs_scanner::TsScanner::new(name);
        scanner.scan();
        scanner.token_value().to_owned()
    }

    fn module(&mut self, file: usize, parent: usize, declaration: &ModuleDecl, exported: bool) {
        let scope = match &declaration.name {
            ModuleName::String(name) => {
                if let Some(&scope) = self.ambient.get(name) {
                    scope
                } else {
                    let scope = self.scope(Some(0), true);
                    self.ambient.insert(name.clone(), scope);
                    scope
                }
            }
            ModuleName::Ident(name) => {
                if declaration.body.is_none() {
                    let text = self.files[file]
                        .text
                        .get(declaration.span.start as usize..declaration.span.end as usize)
                        .unwrap_or("");
                    let mut scanner = tsc_rs_scanner::TsScanner::new(text);
                    if scanner.scan() == tsc_rs_scanner::TokenKind::Export
                        && scanner.scan() == tsc_rs_scanner::TokenKind::As
                        && scanner.scan() == tsc_rs_scanner::TokenKind::Namespace
                    {
                        self.alias(
                            file,
                            0,
                            name.clone(),
                            declaration.span,
                            Reference::Module(self.roots[file]),
                            AliasPlacement::Local,
                        );
                    }
                    return;
                }
                if name == "global" && declaration.name_span.is_none() {
                    0
                } else {
                    let scope = match self.scopes[parent].bindings.get(name) {
                        Some(Binding::Namespace(scope)) => *scope,
                        _ => self.scope(Some(parent), false),
                    };
                    self.bind(parent, name.clone(), Binding::Namespace(scope), exported);
                    scope
                }
            }
        };
        match &declaration.body {
            Some(ModuleBody::Block(body)) => self.statements(file, scope, body),
            Some(ModuleBody::Module(module)) => self.module(file, scope, module, true),
            None => {}
        }
    }

    fn statements(&mut self, file: usize, scope: usize, statements: &[Stmt]) {
        for statement in statements {
            self.statement(file, scope, statement, false);
        }
    }

    fn statement(&mut self, file: usize, scope: usize, statement: &Stmt, exported: bool) {
        match &statement.kind {
            StmtKind::Export(export) => match &export.kind {
                ExportDeclKind::Decl(inner) | ExportDeclKind::DefaultDecl(inner) => {
                    let first_alias = self.aliases.len();
                    self.statement(file, scope, inner, true);
                    if matches!(inner.kind, StmtKind::ImportEquals(_) | StmtKind::Import(_)) {
                        for alias in &mut self.aliases[first_alias..] {
                            alias.span = statement.span;
                        }
                    }
                }
                ExportDeclKind::Default(expression) => {
                    if let Some(reference) = Self::entity(expression) {
                        self.alias(
                            file,
                            scope,
                            "default".to_owned(),
                            statement.span,
                            reference,
                            AliasPlacement::Export,
                        );
                    } else {
                        self.scopes[scope]
                            .exports
                            .insert("default".to_owned(), Binding::Value);
                    }
                }
                ExportDeclKind::Named {
                    specifiers, source, ..
                } => {
                    if let Some(source) = source {
                        self.dependencies[file].push(source.clone());
                    }
                    for specifier in specifiers {
                        let name = specifier
                            .exported
                            .as_ref()
                            .unwrap_or(&specifier.local)
                            .clone();
                        let reference = match source {
                            Some(source) => {
                                Reference::External(source.clone(), Some(specifier.local.clone()))
                            }
                            None => Reference::Entity(vec![specifier.local.clone()]),
                        };
                        self.alias(
                            file,
                            scope,
                            name,
                            specifier.span,
                            reference,
                            AliasPlacement::Export,
                        );
                    }
                }
                ExportDeclKind::All { source, alias, .. } => {
                    self.dependencies[file].push(source.clone());
                    if let Some(name) = alias {
                        let span = self.import_name_span(file, statement.span, true);
                        self.alias(
                            file,
                            scope,
                            name.clone(),
                            span,
                            Reference::External(source.clone(), None),
                            AliasPlacement::Export,
                        );
                    } else {
                        self.scopes[scope].stars.push((file, source.clone()));
                    }
                }
            },
            StmtKind::ExportAssign(expression) => {
                self.scopes[scope].assignment = Self::entity(expression);
            }
            StmtKind::Import(import) if self.scopes[scope].external_imports => {
                self.dependencies[file].push(import.source.clone());
                match &import.specifiers {
                    ImportClause::Require(name) => self.alias(
                        file,
                        scope,
                        name.clone(),
                        statement.span,
                        Reference::External(import.source.clone(), None),
                        if exported {
                            AliasPlacement::ExportedLocal
                        } else {
                            AliasPlacement::Local
                        },
                    ),
                    ImportClause::Named {
                        default,
                        named,
                        namespace,
                    } => {
                        for (name, member) in default
                            .iter()
                            .map(|n| (n, Some("default".to_owned())))
                            .chain(namespace.iter().map(|n| (n, None)))
                        {
                            let span =
                                self.import_name_span(file, statement.span, member.is_none());
                            self.alias(
                                file,
                                scope,
                                name.clone(),
                                span,
                                Reference::External(import.source.clone(), member),
                                AliasPlacement::Local,
                            );
                        }
                        for specifier in named {
                            self.alias(
                                file,
                                scope,
                                specifier.local.clone(),
                                specifier.span,
                                Reference::External(
                                    import.source.clone(),
                                    Some(
                                        specifier
                                            .imported
                                            .as_ref()
                                            .unwrap_or(&specifier.local)
                                            .clone(),
                                    ),
                                ),
                                AliasPlacement::Local,
                            );
                        }
                    }
                }
            }
            StmtKind::ImportEquals(alias) => {
                if let Some(reference) = Self::entity(&alias.module_ref) {
                    self.alias(
                        file,
                        scope,
                        alias.name.clone(),
                        statement.span,
                        reference,
                        if exported || alias.modifiers & MOD_EXPORT != 0 {
                            AliasPlacement::ExportedLocal
                        } else {
                            AliasPlacement::Local
                        },
                    );
                }
            }
            StmtKind::ModuleDecl(module) => self.module(
                file,
                scope,
                module,
                exported || module.modifiers & MOD_EXPORT != 0,
            ),
            StmtKind::ClassDecl(class) => {
                if let Some(name) = &class.name {
                    self.bind(
                        scope,
                        name.clone(),
                        Binding::Value,
                        exported || class.modifiers & MOD_EXPORT != 0,
                    );
                }
            }
            StmtKind::FnDecl(function) => {
                if let Some(name) = &function.name {
                    self.bind(
                        scope,
                        name.clone(),
                        Binding::Value,
                        exported || function.modifiers & MOD_EXPORT != 0,
                    );
                }
            }
            StmtKind::InterfaceDecl(interface) => self.bind(
                scope,
                interface.name.clone(),
                Binding::Value,
                exported || interface.modifiers & MOD_EXPORT != 0,
            ),
            StmtKind::TypeAlias(alias) => self.bind(
                scope,
                alias.name.clone(),
                Binding::Value,
                exported || alias.modifiers & MOD_EXPORT != 0,
            ),
            StmtKind::EnumDecl(enumeration) => self.bind(
                scope,
                enumeration.name.clone(),
                Binding::Value,
                exported || enumeration.modifiers & MOD_EXPORT != 0,
            ),
            StmtKind::Var(variable) => {
                for declaration in &variable.declarations {
                    if let PatKind::Ident(name) = &declaration.name.kind {
                        self.bind(
                            scope,
                            name.to_string(),
                            Binding::Value,
                            exported || variable.modifiers & MOD_EXPORT != 0,
                        );
                    }
                }
            }
            _ => {}
        }
    }

    fn external_scope(&self, file: usize, source: &str) -> Option<usize> {
        self.external_targets
            .get(&(file, source.to_owned()))
            .copied()
            .flatten()
    }

    fn index_external_targets(&mut self) {
        let paths: rustc_hash::FxHashMap<_, _> = self
            .keys
            .iter()
            .enumerate()
            .map(|(file, path)| {
                (
                    TypeChecker::normalize_module_path(Path::new(path)),
                    self.roots[file],
                )
            })
            .collect();
        for file in 0..self.files.len() {
            for source in &self.dependencies[file] {
                let key = (file, source.clone());
                if self.external_targets.contains_key(&key) {
                    continue;
                }
                let target = self.ambient.get(source).copied().or_else(|| {
                    let mut candidates = Vec::new();
                    if source.starts_with("./")
                        || source.starts_with("../")
                        || Path::new(source).is_absolute()
                    {
                        let path = Path::new(&self.keys[file])
                            .parent()
                            .unwrap_or(Path::new(""))
                            .join(source);
                        let base = TypeChecker::normalize_module_path(&path);
                        for (runtime, extensions) in [
                            (".js", &[".ts", ".tsx", ".d.ts"][..]),
                            (".jsx", &[".ts", ".tsx", ".d.ts"][..]),
                            (".mjs", &[".mts", ".d.mts"][..]),
                            (".cjs", &[".cts", ".d.cts"][..]),
                        ] {
                            if let Some(stem) = base.strip_suffix(runtime) {
                                candidates.extend(
                                    extensions
                                        .iter()
                                        .map(|extension| format!("{stem}{extension}")),
                                );
                            }
                        }
                        candidates.push(base.clone());
                        for extension in [
                            ".ts", ".tsx", ".d.ts", ".mts", ".d.mts", ".cts", ".d.cts", ".js",
                            ".jsx",
                        ] {
                            candidates.push(format!("{base}{extension}"));
                        }
                        for extension in [
                            ".ts", ".tsx", ".d.ts", ".mts", ".d.mts", ".cts", ".d.cts", ".js",
                            ".jsx",
                        ] {
                            candidates.push(format!("{base}/index{extension}"));
                        }
                    }
                    candidates
                        .iter()
                        .find_map(|path| paths.get(path).copied())
                        .or_else(|| {
                            let path = self
                                .checker
                                .resolve_module_to_path(source, &self.keys[file])?;
                            paths
                                .get(&TypeChecker::normalize_module_path(Path::new(&path)))
                                .copied()
                        })
                });
                self.external_targets.insert(key, target);
            }
        }
    }

    fn lookup(&self, mut scope: usize, name: &str) -> Option<Binding> {
        loop {
            if let Some(binding) = self.scopes[scope].bindings.get(name) {
                return Some(*binding);
            }
            scope = self.scopes[scope].parent?;
        }
    }

    fn binding(&mut self, binding: Binding) -> Option<Binding> {
        match binding {
            Binding::Alias(alias) => self.resolve_alias(alias),
            other => Some(other),
        }
    }

    fn module_value(&mut self, file: usize, scope: usize) -> Option<Binding> {
        if let Some(reference) = self.scopes[scope].assignment.clone() {
            self.reference(file, scope, reference)
        } else {
            Some(Binding::Namespace(scope))
        }
    }

    fn exported(&mut self, file: usize, scope: usize, name: &str) -> Option<Binding> {
        if let Some(binding) = self.scopes[scope].exports.get(name).copied() {
            return self.binding(binding);
        }
        if name == "default" && self.scopes[scope].assignment.is_some() {
            return self.module_value(file, scope);
        }
        let key = (scope, name.to_owned());
        if !self.export_stack.insert(key.clone()) {
            return None;
        }
        let mut found = None;
        for (file, source) in self.scopes[scope].stars.clone() {
            if let Some(other) = self.external_scope(file, &source) {
                found = self.exported(file, other, name);
                if found.is_some() {
                    break;
                }
            }
        }
        self.export_stack.remove(&key);
        found
    }

    fn reference(&mut self, file: usize, scope: usize, reference: Reference) -> Option<Binding> {
        match reference {
            Reference::Module(scope) => self.module_value(file, scope),
            Reference::External(source, member) => {
                let scope = self.external_scope(file, &source)?;
                match member {
                    Some(name) => self.exported(file, scope, &name),
                    None => self.module_value(file, scope),
                }
            }
            Reference::Entity(path) => {
                let (name, rest) = path.split_first()?;
                let mut binding = self.binding(self.lookup(scope, name)?)?;
                for member in rest {
                    let Binding::Namespace(namespace) = binding else {
                        return Some(Binding::Value);
                    };
                    binding = self.exported(file, namespace, member)?;
                }
                Some(binding)
            }
        }
    }

    fn star_bindings(
        &self,
        scope: usize,
        seen: &mut rustc_hash::FxHashSet<usize>,
        names: &mut rustc_hash::FxHashMap<String, Vec<Binding>>,
    ) {
        if !seen.insert(scope) {
            return;
        }
        for (name, binding) in &self.scopes[scope].exports {
            if name == "default" {
                continue;
            }
            let bindings = names.entry(name.clone()).or_default();
            if !bindings.contains(binding) {
                bindings.push(*binding);
            }
        }
        for (file, source) in &self.scopes[scope].stars {
            if let Some(target) = self.external_scope(*file, source) {
                self.star_bindings(target, seen, names);
            }
        }
    }

    fn resolve_star_conflicts(&mut self, scope: usize) {
        let mut names = rustc_hash::FxHashMap::default();
        for (owner, source) in &self.scopes[scope].stars {
            if let Some(target) = self.external_scope(*owner, source) {
                self.star_bindings(target, &mut Default::default(), &mut names);
            }
        }
        let mut conflicts: Vec<_> = names
            .into_iter()
            .filter(|(name, bindings)| {
                bindings.len() > 1 && !self.scopes[scope].exports.contains_key(name)
            })
            .collect();
        conflicts.sort_by(|(left, _), (right, _)| left.cmp(right));
        for (_, bindings) in conflicts {
            for binding in bindings {
                self.binding(binding);
            }
        }
    }

    fn resolve_alias(&mut self, alias: usize) -> Option<Binding> {
        match self.state[alias] {
            2 => return self.resolved[alias],
            1 => {
                let alias = &self.aliases[alias];
                let mut diagnostic =
                    diagnostics::error_circular_import_alias(&alias.name, alias.span);
                diagnostic.file_name = Some(self.keys[alias.file].clone());
                self.diagnostics
                    .entry(TypeChecker::normalized_file_key(&self.keys[alias.file]))
                    .or_default()
                    .push(diagnostic);
                return None;
            }
            _ => {}
        }
        self.state[alias] = 1;
        let node = &self.aliases[alias];
        let result = self.reference(node.file, node.scope, node.reference.clone());
        self.state[alias] = 2;
        self.resolved[alias] = result;
        result
    }

    fn finish(mut self) -> rustc_hash::FxHashMap<String, Vec<Diagnostic>> {
        for file in self.files {
            let external = TypeChecker::file_is_module(file) || file.statements.iter().any(|statement| {
                matches!(statement.kind, StmtKind::ExportAssign(_))
                    || matches!(&statement.kind, StmtKind::ImportEquals(alias) if alias.modifiers & MOD_EXPORT != 0)
            });
            let root = if external {
                self.scope(Some(0), true)
            } else {
                0
            };
            self.roots.push(root);
        }
        self.scopes[0].external_imports = true;
        for file in 0..self.files.len() {
            self.statements(file, self.roots[file], &self.files[file].statements);
        }
        if self.aliases.is_empty() {
            return self.diagnostics;
        }
        self.index_external_targets();
        self.state.resize(self.aliases.len(), 0);
        self.resolved.resize(self.aliases.len(), None);
        // Match dependency-first source checking, preserving root and import order.
        let scope_files: rustc_hash::FxHashMap<_, _> = self
            .roots
            .iter()
            .enumerate()
            .filter(|(_, scope)| **scope != 0)
            .map(|(file, scope)| (*scope, file))
            .collect();
        let mut visited = vec![false; self.files.len()];
        let mut order = Vec::new();
        for root in 0..self.files.len() {
            let mut stack = vec![(root, false)];
            while let Some((file, finish)) = stack.pop() {
                if finish {
                    order.push(file);
                    continue;
                }
                if std::mem::replace(&mut visited[file], true) {
                    continue;
                }
                stack.push((file, true));
                for source in self.dependencies[file].iter().rev() {
                    if let Some(scope) = self.external_scope(file, source) {
                        if let Some(&dependency) = scope_files.get(&scope) {
                            if !visited[dependency] {
                                stack.push((dependency, false));
                            }
                        }
                    }
                }
            }
        }
        let mut aliases_by_file = vec![Vec::new(); self.files.len()];
        for (index, alias) in self.aliases.iter().enumerate() {
            aliases_by_file[alias.file].push(index);
        }
        let mut star_scopes_by_file = vec![Vec::new(); self.files.len()];
        for (scope, info) in self.scopes.iter().enumerate() {
            if info.stars.len() < 2 {
                continue;
            }
            for (file, _) in &info.stars {
                if !star_scopes_by_file[*file].contains(&scope) {
                    star_scopes_by_file[*file].push(scope);
                }
            }
        }
        for file in order {
            for &scope in &star_scopes_by_file[file] {
                self.resolve_star_conflicts(scope);
            }
            for &alias in &aliases_by_file[file] {
                self.resolve_alias(alias);
            }
        }
        self.diagnostics
    }
}

impl TypeChecker {
    pub(crate) fn index_alias_cycles(&mut self, files: &[&SourceFile]) {
        fn has_alias(statement: &Stmt) -> bool {
            match &statement.kind {
                StmtKind::Import(import) => !import.is_side_effect,
                StmtKind::ImportEquals(_) => true,
                StmtKind::Export(export) => match &export.kind {
                    ExportDeclKind::Decl(inner) | ExportDeclKind::DefaultDecl(inner) => {
                        has_alias(inner)
                    }
                    ExportDeclKind::Named { specifiers, .. } => !specifiers.is_empty(),
                    ExportDeclKind::All { alias, .. } => alias.is_some(),
                    ExportDeclKind::Default(_) => false,
                },
                StmtKind::ModuleDecl(module) => module_has_alias(module),
                _ => false,
            }
        }
        fn module_has_alias(module: &ModuleDecl) -> bool {
            match &module.body {
                Some(ModuleBody::Block(body)) => body.iter().any(has_alias),
                Some(ModuleBody::Module(module)) => module_has_alias(module),
                None => true,
            }
        }
        let diagnostics = if files
            .iter()
            .any(|file| file.statements.iter().any(has_alias))
        {
            AliasGraph::new(self, files).finish()
        } else {
            Default::default()
        };
        self.alias_cycle_diagnostics = Some(Arc::new(diagnostics));
    }

    pub(crate) fn check_alias_cycles(&mut self, file: &SourceFile) {
        if self.alias_cycle_diagnostics.is_none() {
            self.index_alias_cycles(&[file]);
        }
        if let Some(diagnostics) = self
            .alias_cycle_diagnostics
            .as_ref()
            .and_then(|index| index.get(&Self::normalized_file_key(&file.file_name)))
        {
            self.diagnostics.extend(diagnostics.iter().cloned());
        }
    }
}
