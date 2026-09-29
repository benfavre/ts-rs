//! TS2440 for `import x = <entity>` that shares its name with a plain `var`.
//!
//! The binder treats an import-equals alias as a function-scoped variable, so
//! it merges silently with `var x`. tsc decides in `checkAliasSymbol`: the
//! pair conflicts when the alias target has a value meaning (a variable,
//! function, class or regular enum, an instantiated namespace, or any
//! `require`). Other colliding declarations (let/const, functions, classes)
//! are already reported by the binder.

use tsc_rs_ast::*;

use crate::TypeChecker;

/// Names declared by plain `var` statements in `statements`, and the subset
/// that is exported.
fn var_names(statements: &[Stmt]) -> (Vec<String>, Vec<String>) {
    fn pattern_names(pattern: &Pat, out: &mut Vec<String>) {
        match &pattern.kind {
            PatKind::Ident(name) => out.push(name.to_string()),
            PatKind::Array(elements) => {
                for element in elements.iter().flatten() {
                    match element {
                        ArrayPatElem::Pat(p) | ArrayPatElem::Rest(p) => pattern_names(p, out),
                    }
                }
            }
            PatKind::Object(properties) => {
                for property in properties {
                    match property {
                        ObjPatProp::KeyValue(_, p) | ObjPatProp::Rest(p) => pattern_names(p, out),
                        ObjPatProp::Shorthand(name, _)
                        | ObjPatProp::ShorthandAssign(name, _, _) => out.push(name.to_string()),
                    }
                }
            }
            PatKind::Assign(p, _) | PatKind::Rest(p) => pattern_names(p, out),
        }
    }
    let mut all = Vec::new();
    let mut exported = Vec::new();
    for statement in statements {
        let (inner, is_exported) = match &statement.kind {
            StmtKind::Export(export) => match &export.kind {
                ExportDeclKind::Decl(declaration) => (declaration.as_ref(), true),
                _ => continue,
            },
            _ => (statement, false),
        };
        if let StmtKind::Var(var) = &inner.kind {
            if var.kind == VarKind::Var {
                let mut names = Vec::new();
                for declaration in &var.declarations {
                    pattern_names(&declaration.name, &mut names);
                }
                if is_exported {
                    exported.extend(names.iter().cloned());
                }
                all.extend(names);
            }
        }
    }
    (all, exported)
}

fn module_blocks<'a>(statements: &'a [Stmt], name: &str) -> Vec<&'a [Stmt]> {
    let mut blocks = Vec::new();
    for statement in statements {
        let inner = TypeChecker::unwrap_export_stmt(statement);
        let StmtKind::ModuleDecl(module) = &inner.kind else {
            continue;
        };
        if !matches!(&module.name, ModuleName::Ident(n) if n == name) {
            continue;
        }
        if let Some(ModuleBody::Block(body)) = &module.body {
            blocks.push(body.as_slice());
        }
    }
    blocks
}

fn namespace_is_instantiated(blocks: &[&[Stmt]]) -> bool {
    blocks.iter().any(|body| {
        body.iter().any(
            |statement| match &TypeChecker::unwrap_export_stmt(statement).kind {
                StmtKind::InterfaceDecl(_) | StmtKind::TypeAlias(_) | StmtKind::Empty => false,
                StmtKind::EnumDecl(e) => !e.is_const,
                StmtKind::ModuleDecl(module) => match &module.name {
                    ModuleName::Ident(name) => {
                        namespace_is_instantiated(&module_blocks(body, name))
                    }
                    ModuleName::String(_) => false,
                },
                StmtKind::ImportEquals(_) => false,
                _ => true,
            },
        )
    })
}

/// Whether `name` in `statements` (possibly several merged blocks) has a
/// value meaning: `Some(true)` / `Some(false)`, or `None` when not declared.
fn member_has_value(blocks: &[&[Stmt]], name: &str) -> Option<bool> {
    let mut found = None;
    for body in blocks {
        for statement in body.iter() {
            match &TypeChecker::unwrap_export_stmt(statement).kind {
                StmtKind::Var(var) => {
                    if var
                        .declarations
                        .iter()
                        .any(|d| matches!(&d.name.kind, PatKind::Ident(n) if n.as_str() == name))
                    {
                        return Some(true);
                    }
                }
                StmtKind::FnDecl(f) if f.name.as_deref() == Some(name) => return Some(true),
                StmtKind::ClassDecl(c) if c.name.as_deref() == Some(name) => return Some(true),
                StmtKind::EnumDecl(e) if e.name == name => {
                    if !e.is_const {
                        return Some(true);
                    }
                    found = Some(false);
                }
                StmtKind::ModuleDecl(m) if matches!(&m.name, ModuleName::Ident(n) if n == name) => {
                    if namespace_is_instantiated(&module_blocks(body, name)) {
                        return Some(true);
                    }
                    found = Some(false);
                }
                StmtKind::InterfaceDecl(i) if i.name == name => found = found.or(Some(false)),
                StmtKind::TypeAlias(t) if t.name == name => found = found.or(Some(false)),
                _ => {}
            }
        }
    }
    found
}

fn entity_segments(expr: &Expr, out: &mut Vec<String>) -> bool {
    match &expr.kind {
        ExprKind::Ident(name) => {
            out.push(name.to_string());
            true
        }
        ExprKind::Member(member) => {
            if !entity_segments(&member.object, out) {
                return false;
            }
            out.push(member.property.to_string());
            true
        }
        _ => false,
    }
}

impl TypeChecker {
    /// Whether an import-equals target has a value meaning, resolved through
    /// the file's own namespaces (`None` when it cannot be resolved here).
    fn alias_target_has_value(root: &[Stmt], module_ref: &Expr) -> Option<bool> {
        if let ExprKind::Call(call) = &module_ref.kind {
            return matches!(&call.callee.kind, ExprKind::Ident(n) if n == "require")
                .then_some(true);
        }
        let mut segments = Vec::new();
        if !entity_segments(module_ref, &mut segments) {
            return None;
        }
        let (last, path) = segments.split_last()?;
        let mut blocks: Vec<&[Stmt]> = vec![root];
        for segment in path {
            let next: Vec<&[Stmt]> = blocks
                .iter()
                .flat_map(|body| module_blocks(body, segment))
                .collect();
            if next.is_empty() {
                return None;
            }
            blocks = next;
        }
        member_has_value(&blocks, last)
    }

    pub(crate) fn check_import_alias_value_conflicts(&mut self, root: &[Stmt]) {
        if self.current_file_is_declaration() {
            return;
        }
        self.import_alias_conflicts_in(root, root, &[]);
    }

    /// `merged_exported_vars`: exported `var`s of other blocks of the same
    /// namespace, which share a symbol with this block's exported aliases.
    fn import_alias_conflicts_in(
        &mut self,
        root: &[Stmt],
        statements: &[Stmt],
        merged_exported_vars: &[String],
    ) {
        let (vars, _) = var_names(statements);
        for statement in statements {
            let (inner, exported) = match &statement.kind {
                StmtKind::Export(export) => match &export.kind {
                    ExportDeclKind::Decl(declaration) => (declaration.as_ref(), true),
                    _ => (statement, false),
                },
                _ => (statement, false),
            };
            if let StmtKind::ImportEquals(import) = &inner.kind {
                self.check_alias_root_hidden(root, statements, import);
                let exported = exported || import.modifiers & MOD_EXPORT != 0;
                let collides = vars.contains(&import.name)
                    || (exported && merged_exported_vars.contains(&import.name));
                if collides
                    && Self::alias_target_has_value(root, &import.module_ref) == Some(true)
                    && self.reported_duplicate_spans.insert((
                        2440,
                        statement.span.start,
                        statement.span.end,
                    ))
                {
                    self.diagnostics.push(Diagnostic {
                        code: 2440,
                        message: format!(
                            "Import declaration conflicts with local declaration of '{}'.",
                            import.name
                        ),
                        category: DiagnosticCategory::Error,
                        file_name: None,
                        span: Some(statement.span),
                        related: None,
                    });
                }
            }
        }
        // Namespace blocks, with the exported vars of their merged siblings.
        let mut seen: Vec<&str> = Vec::new();
        for statement in statements {
            let StmtKind::ModuleDecl(module) = &Self::unwrap_export_stmt(statement).kind else {
                continue;
            };
            let ModuleName::Ident(name) = &module.name else {
                continue;
            };
            if module.modifiers & MOD_DECLARE != 0 || seen.contains(&name.as_str()) {
                continue;
            }
            seen.push(name);
            let blocks = module_blocks(statements, name);
            for (index, block) in blocks.iter().enumerate() {
                let siblings: Vec<String> = blocks
                    .iter()
                    .enumerate()
                    .filter(|(other, _)| *other != index)
                    .flat_map(|(_, body)| var_names(body).1)
                    .collect();
                self.import_alias_conflicts_in(root, block, &siblings);
            }
        }
    }

    /// TS2437: `import X = A.b` whose target is a value, where `A` itself
    /// resolves (value or namespace meaning) to a local non-namespace
    /// declaration of this block that hides the namespace `A`.
    fn check_alias_root_hidden(
        &mut self,
        root: &[Stmt],
        statements: &[Stmt],
        import: &ImportEqualsDecl,
    ) {
        let mut segments = Vec::new();
        if !entity_segments(&import.module_ref, &mut segments) {
            return;
        }
        let first = segments[0].as_str();
        let mut local_value = false;
        for statement in statements {
            match &Self::unwrap_export_stmt(statement).kind {
                StmtKind::ModuleDecl(m) if matches!(&m.name, ModuleName::Ident(n) if n == first) => {
                    return;
                }
                StmtKind::Var(var) => {
                    if var
                        .declarations
                        .iter()
                        .any(|d| matches!(&d.name.kind, PatKind::Ident(n) if n.as_str() == first))
                    {
                        local_value = true;
                    }
                }
                StmtKind::FnDecl(f) if f.name.as_deref() == Some(first) => local_value = true,
                StmtKind::ClassDecl(c) if c.name.as_deref() == Some(first) => local_value = true,
                _ => {}
            }
        }
        if !local_value || std::ptr::eq(statements, root) {
            return;
        }
        if Self::alias_target_has_value(root, &import.module_ref) != Some(true) {
            return;
        }
        // The first identifier's span: the start of the entity expression.
        let mut first_expr = import.module_ref.as_ref();
        while let ExprKind::Member(member) = &first_expr.kind {
            first_expr = &member.object;
        }
        if self
            .reported_duplicate_spans
            .insert((2437, first_expr.span.start, first_expr.span.end))
        {
            self.diagnostics.push(Diagnostic {
                code: 2437,
                message: format!(
                    "Module '{first}' is hidden by a local declaration with the same name."
                ),
                category: DiagnosticCategory::Error,
                file_name: None,
                span: Some(first_expr.span),
                related: None,
            });
        }
    }
}
