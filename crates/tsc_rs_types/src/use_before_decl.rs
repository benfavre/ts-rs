//! TS2448 / TS2449: a block-scoped `let`/`const` binding or a class used
//! before its declaration in a position that executes immediately (tsc's
//! `isBlockScopedNameDeclaredBeforeUse`). References inside functions,
//! arrows, methods and instance property initializers are deferred and
//! therefore fine; IIFE bodies, static initializers, computed property names
//! and heritage clauses are not.

use crate::TypeChecker;
use rustc_hash::FxHashMap;
use tsc_rs_ast::*;

#[derive(Clone, Copy, PartialEq)]
enum DeclKind {
    BlockVar,
    Class,
    Other,
}

#[derive(Clone)]
struct Decl {
    kind: DeclKind,
    name_span: Span,
    /// Uses at or after this offset are fine: the end of the declarator for
    /// `let`/`const` (its own initializer counts as "before"), the start of
    /// the declaration for a class.
    ready_at: u32,
    /// Index of the declaring scope; a use is deferred only when a function
    /// boundary lies between it and this scope.
    scope_depth: usize,
    /// Inside a destructuring pattern a later binding element may use an
    /// earlier one: uses within this window are fine.
    pattern_window: Option<(u32, u32)>,
}

struct Walker {
    scopes: Vec<FxHashMap<String, Decl>>,
    /// Scope depths at which deferred (function-like) contexts begin.
    boundaries: Vec<usize>,
    findings: Vec<(DeclKind, String, Span, Span)>,
    /// Names used immediately (outside any function) that no scope of this
    /// file declares: candidates for a declaration in a later outFile file.
    unresolved_immediate: Vec<(String, Span)>,
}

impl Walker {
    fn declare(&mut self, name: &str, mut decl: Decl) {
        decl.scope_depth = self.scopes.len().saturating_sub(1);
        if let Some(scope) = self.scopes.last_mut() {
            // The first declaration of a name in a scope wins (redeclaration
            // is a different diagnostic).
            scope.entry(name.to_string()).or_insert(decl);
        }
    }

    fn declare_pattern(&mut self, pattern: &Pat, kind: DeclKind, ready_at: u32) {
        let mut bound = Vec::new();
        TypeChecker::pattern_binding_spans(pattern, &mut bound);
        let destructuring = !matches!(pattern.kind, PatKind::Ident(_));
        // A destructured name is initialized once its own element (including
        // its default, `[x = x]` reads the uninitialized `x`) is done.
        let mut element_ends = Vec::new();
        binding_element_ends(pattern, &mut element_ends);
        for (name, span) in bound {
            let initialized_from = element_ends
                .iter()
                .find(|(start, _)| *start == span.start)
                .map_or(span.end, |(_, end)| *end);
            self.declare(
                &name,
                Decl {
                    kind,
                    name_span: span,
                    ready_at,
                    scope_depth: 0,
                    pattern_window: destructuring.then_some((initialized_from, pattern.span.end)),
                },
            );
        }
    }

    fn declare_params(&mut self, params: &[Param]) {
        for param in params {
            self.declare_pattern(&param.name, DeclKind::Other, 0);
        }
    }

    fn hoist(&mut self, stmts: &[Stmt]) {
        for stmt in stmts {
            let inner = TypeChecker::unwrap_export_stmt(stmt);
            match &inner.kind {
                StmtKind::Var(var) => {
                    // An ambient `declare const` has no temporal dead zone.
                    let kind =
                        if matches!(var.kind, VarKind::Var) || var.modifiers & MOD_DECLARE != 0 {
                            DeclKind::Other
                        } else {
                            DeclKind::BlockVar
                        };
                    for declarator in &var.declarations {
                        self.declare_pattern(&declarator.name, kind, declarator.span.end);
                    }
                }
                StmtKind::ClassDecl(class) => {
                    if let Some(name) = &class.name {
                        self.declare(
                            name,
                            Decl {
                                kind: DeclKind::Class,
                                name_span: class.name_span.unwrap_or(inner.span),
                                ready_at: inner.span.start,
                                scope_depth: 0,
                                pattern_window: None,
                            },
                        );
                    }
                }
                StmtKind::FnDecl(function) => {
                    if let Some(name) = &function.name {
                        self.declare(
                            name,
                            Decl {
                                kind: DeclKind::Other,
                                name_span: function.name_span.unwrap_or(inner.span),
                                ready_at: 0,
                                scope_depth: 0,
                                pattern_window: None,
                            },
                        );
                    }
                }
                StmtKind::EnumDecl(enumeration) => self.declare(
                    &enumeration.name,
                    Decl {
                        kind: DeclKind::Other,
                        name_span: inner.span,
                        ready_at: 0,
                        scope_depth: 0,
                        pattern_window: None,
                    },
                ),
                StmtKind::ModuleDecl(module) => {
                    if let ModuleName::Ident(name) = &module.name {
                        self.declare(
                            name,
                            Decl {
                                kind: DeclKind::Other,
                                name_span: inner.span,
                                ready_at: 0,
                                scope_depth: 0,
                                pattern_window: None,
                            },
                        );
                    }
                }
                StmtKind::ImportEquals(alias) => self.declare(
                    &alias.name,
                    Decl {
                        kind: DeclKind::Other,
                        name_span: inner.span,
                        ready_at: 0,
                        scope_depth: 0,
                        pattern_window: None,
                    },
                ),
                StmtKind::Import(import) => {
                    if let ImportClause::Named { default, named, .. } = &import.specifiers {
                        if let Some(name) = default {
                            self.declare(
                                name,
                                Decl {
                                    kind: DeclKind::Other,
                                    name_span: inner.span,
                                    ready_at: 0,
                                    scope_depth: 0,
                                    pattern_window: None,
                                },
                            );
                        }
                        for specifier in named {
                            self.declare(
                                &specifier.local,
                                Decl {
                                    kind: DeclKind::Other,
                                    name_span: specifier.span,
                                    ready_at: 0,
                                    scope_depth: 0,
                                    pattern_window: None,
                                },
                            );
                        }
                    }
                }
                _ => {}
            }
        }
    }

    fn check(&mut self, name: &str, span: Span) {
        let Some(decl) = self
            .scopes
            .iter()
            .rev()
            .find_map(|scope| scope.get(name))
            .cloned()
        else {
            if self.boundaries.is_empty() {
                self.unresolved_immediate.push((name.to_string(), span));
            }
            return;
        };
        if decl.kind == DeclKind::Other || span.start >= decl.ready_at {
            return;
        }
        if decl
            .pattern_window
            .is_some_and(|(from, to)| span.start >= from && span.start < to)
        {
            return;
        }
        // Deferred: a function boundary was entered after the declaring scope.
        if self
            .boundaries
            .iter()
            .any(|&depth| depth > decl.scope_depth)
        {
            return;
        }
        self.findings
            .push((decl.kind, name.to_string(), span, decl.name_span));
    }

    fn visit_stmts(&mut self, stmts: &[Stmt]) {
        self.scopes.push(FxHashMap::default());
        self.hoist(stmts);
        for stmt in stmts {
            self.visit_stmt(stmt);
        }
        self.scopes.pop();
    }

    fn visit_body_stmt(&mut self, stmt: &Stmt) {
        match &stmt.kind {
            StmtKind::Block(stmts) => self.visit_stmts(stmts),
            _ => {
                self.scopes.push(FxHashMap::default());
                self.hoist(std::slice::from_ref(stmt));
                self.visit_stmt(stmt);
                self.scopes.pop();
            }
        }
    }

    fn visit_pattern(&mut self, pattern: &Pat) {
        match &pattern.kind {
            PatKind::Ident(_) => {}
            PatKind::Array(elements) => {
                for element in elements.iter().flatten() {
                    match element {
                        ArrayPatElem::Pat(inner) | ArrayPatElem::Rest(inner) => {
                            self.visit_pattern(inner)
                        }
                    }
                }
            }
            PatKind::Object(properties) => {
                for property in properties {
                    match property {
                        ObjPatProp::KeyValue(key, inner) => {
                            if let PropName::Computed(expr, _) = key {
                                self.visit_expr(expr);
                            }
                            self.visit_pattern(inner);
                        }
                        ObjPatProp::Rest(inner) => self.visit_pattern(inner),
                        ObjPatProp::Shorthand(_, _) => {}
                        ObjPatProp::ShorthandAssign(_, init, _) => self.visit_expr(init),
                    }
                }
            }
            PatKind::Assign(inner, init) => {
                self.visit_pattern(inner);
                self.visit_expr(init);
            }
            PatKind::Rest(inner) => self.visit_pattern(inner),
        }
    }

    fn visit_function(&mut self, params: &[Param], body: Option<&[Stmt]>, deferred: bool) {
        if deferred {
            self.boundaries.push(self.scopes.len());
        }
        self.scopes.push(FxHashMap::default());
        self.declare_params(params);
        for param in params {
            self.visit_pattern(&param.name);
        }
        if let Some(body) = body {
            self.visit_stmts(body);
        }
        self.scopes.pop();
        if deferred {
            self.boundaries.pop();
        }
    }

    fn visit_class(&mut self, class: &ClassDecl) {
        // Ambient classes contain no executable code.
        if class.modifiers & MOD_DECLARE != 0 {
            return;
        }
        if let Some(extends) = &class.extends {
            self.visit_expr(extends);
        }
        for member in &class.members {
            match &member.kind {
                ClassMemberKind::Property(property) => {
                    if let PropName::Computed(expr, _) = &property.name {
                        self.visit_expr(expr);
                    }
                    if let Some(init) = &property.initializer {
                        let is_static = property.modifiers & MOD_STATIC != 0;
                        if !is_static {
                            self.boundaries.push(self.scopes.len());
                        }
                        self.visit_expr(init);
                        if !is_static {
                            self.boundaries.pop();
                        }
                    }
                }
                ClassMemberKind::Method(method) => {
                    if let PropName::Computed(expr, _) = &method.name {
                        self.visit_expr(expr);
                    }
                    self.visit_function(&method.params, method.body.as_deref(), true);
                }
                ClassMemberKind::Constructor(ctor) => {
                    self.visit_function(&ctor.params, ctor.body.as_deref(), true);
                }
                ClassMemberKind::GetAccessor(accessor) | ClassMemberKind::SetAccessor(accessor) => {
                    if let PropName::Computed(expr, _) = &accessor.name {
                        self.visit_expr(expr);
                    }
                    self.visit_function(&accessor.params, accessor.body.as_deref(), true);
                }
                ClassMemberKind::StaticBlock(stmts) => self.visit_stmts(stmts),
                ClassMemberKind::IndexSignature(_) | ClassMemberKind::SemicolonClassElement => {}
            }
        }
    }

    fn visit_stmt(&mut self, stmt: &Stmt) {
        match &stmt.kind {
            StmtKind::Expr(expr) | StmtKind::Throw(expr) => self.visit_expr(expr),
            StmtKind::Return(Some(expr)) => self.visit_expr(expr),
            StmtKind::Var(var) => {
                for declarator in &var.declarations {
                    self.visit_pattern(&declarator.name);
                    if let Some(init) = &declarator.init {
                        self.visit_expr(init);
                    }
                }
            }
            StmtKind::If(if_stmt) => {
                self.visit_expr(&if_stmt.test);
                self.visit_body_stmt(&if_stmt.consequent);
                if let Some(alternate) = &if_stmt.alternate {
                    self.visit_body_stmt(alternate);
                }
            }
            StmtKind::While(while_stmt) => {
                self.visit_expr(&while_stmt.test);
                self.visit_body_stmt(&while_stmt.body);
            }
            StmtKind::DoWhile(do_while) => {
                self.visit_body_stmt(&do_while.body);
                self.visit_expr(&do_while.test);
            }
            StmtKind::For(for_stmt) => {
                self.scopes.push(FxHashMap::default());
                match &for_stmt.init {
                    Some(ForInit::Var(var)) => {
                        let kind = if matches!(var.kind, VarKind::Var) {
                            DeclKind::Other
                        } else {
                            DeclKind::BlockVar
                        };
                        for declarator in &var.declarations {
                            self.declare_pattern(&declarator.name, kind, declarator.span.end);
                        }
                        for declarator in &var.declarations {
                            self.visit_pattern(&declarator.name);
                            if let Some(init) = &declarator.init {
                                self.visit_expr(init);
                            }
                        }
                    }
                    Some(ForInit::Expr(expr)) => self.visit_expr(expr),
                    None => {}
                }
                if let Some(test) = &for_stmt.test {
                    self.visit_expr(test);
                }
                if let Some(update) = &for_stmt.update {
                    self.visit_expr(update);
                }
                self.visit_body_stmt(&for_stmt.body);
                self.scopes.pop();
            }
            StmtKind::ForIn(for_in) => {
                self.visit_for_in_of(&for_in.left, &for_in.right, &for_in.body)
            }
            StmtKind::ForOf(for_of) => {
                self.visit_for_in_of(&for_of.left, &for_of.right, &for_of.body)
            }
            StmtKind::Switch(switch) => {
                self.visit_expr(&switch.discriminant);
                self.scopes.push(FxHashMap::default());
                for case in &switch.cases {
                    self.hoist(&case.consequent);
                }
                for case in &switch.cases {
                    if let Some(test) = &case.test {
                        self.visit_expr(test);
                    }
                    for inner in &case.consequent {
                        self.visit_stmt(inner);
                    }
                }
                self.scopes.pop();
            }
            StmtKind::Try(try_stmt) => {
                self.visit_stmts(&try_stmt.block);
                if let Some(handler) = &try_stmt.handler {
                    self.scopes.push(FxHashMap::default());
                    if let Some(param) = &handler.param {
                        self.declare_pattern(param, DeclKind::Other, 0);
                    }
                    self.visit_stmts(&handler.body);
                    self.scopes.pop();
                }
                if let Some(finalizer) = &try_stmt.finalizer {
                    self.visit_stmts(finalizer);
                }
            }
            StmtKind::Labeled(labeled) => self.visit_body_stmt(&labeled.body),
            StmtKind::With(with) => {
                self.visit_expr(&with.object);
                self.visit_body_stmt(&with.body);
            }
            StmtKind::Block(stmts) => self.visit_stmts(stmts),
            StmtKind::FnDecl(function) => {
                self.visit_function(&function.params, function.body.as_deref(), true);
            }
            StmtKind::ClassDecl(class) => self.visit_class(class),
            StmtKind::EnumDecl(enumeration) => {
                for member in &enumeration.members {
                    if let Some(init) = &member.initializer {
                        self.visit_expr(init);
                    }
                }
            }
            StmtKind::ModuleDecl(module) => {
                let mut body = module.body.as_ref();
                while let Some(current) = body {
                    match current {
                        ModuleBody::Block(stmts) => {
                            self.visit_stmts(stmts);
                            break;
                        }
                        ModuleBody::Module(inner) => body = inner.body.as_ref(),
                    }
                }
            }
            StmtKind::Export(export) => match &export.kind {
                ExportDeclKind::Decl(inner) | ExportDeclKind::DefaultDecl(inner) => {
                    self.visit_stmt(inner)
                }
                ExportDeclKind::Default(expr) => self.visit_expr(expr),
                _ => {}
            },
            StmtKind::ExportAssign(_)
            | StmtKind::Return(None)
            | StmtKind::Break(_)
            | StmtKind::Continue(_)
            | StmtKind::Empty
            | StmtKind::Debugger
            | StmtKind::InterfaceDecl(_)
            | StmtKind::TypeAlias(_)
            | StmtKind::Import(_)
            | StmtKind::ImportEquals(_) => {}
        }
    }

    fn visit_for_in_of(&mut self, left: &ForInOfLeft, right: &Expr, body: &Stmt) {
        self.scopes.push(FxHashMap::default());
        match left {
            ForInOfLeft::Var(var) => {
                let kind = if matches!(var.kind, VarKind::Var) {
                    DeclKind::Other
                } else {
                    DeclKind::BlockVar
                };
                for declarator in &var.declarations {
                    // The iteration binding is initialized by the loop itself,
                    // after the iterated expression (`for (let v of v)` reads
                    // the uninitialized `v`).
                    self.declare_pattern(&declarator.name, kind, right.span.end);
                }
            }
            ForInOfLeft::Pat(pattern) => self.visit_pattern(pattern),
            ForInOfLeft::Expr(expr) => self.visit_expr(expr),
        }
        self.visit_expr(right);
        self.visit_body_stmt(body);
        self.scopes.pop();
    }

    fn visit_expr(&mut self, expr: &Expr) {
        match &expr.kind {
            ExprKind::Ident(name) => self.check(name, expr.span),
            ExprKind::Member(member) => self.visit_expr(&member.object),
            ExprKind::ElemAccess(access) => {
                self.visit_expr(&access.object);
                self.visit_expr(&access.index);
            }
            ExprKind::Call(call) => {
                // An immediately invoked function runs right away.
                let mut callee: &Expr = &call.callee;
                while let ExprKind::Paren(inner) = &callee.kind {
                    callee = inner;
                }
                match &callee.kind {
                    ExprKind::FnExpr(function) => {
                        self.visit_function(&function.params, function.body.as_deref(), false)
                    }
                    ExprKind::Arrow(arrow) => {
                        self.scopes.push(FxHashMap::default());
                        self.declare_params(&arrow.params);
                        for param in &arrow.params {
                            self.visit_pattern(&param.name);
                        }
                        match &arrow.body {
                            ArrowBody::Expr(body) => self.visit_expr(body),
                            ArrowBody::Block(stmts) => self.visit_stmts(stmts),
                        }
                        self.scopes.pop();
                    }
                    _ => self.visit_expr(&call.callee),
                }
                for arg in &call.args {
                    self.visit_expr(arg);
                }
            }
            ExprKind::New(new_expr) => {
                self.visit_expr(&new_expr.callee);
                if let Some(args) = &new_expr.args {
                    for arg in args {
                        self.visit_expr(arg);
                    }
                }
            }
            ExprKind::Binary(binary) => {
                self.visit_expr(&binary.left);
                self.visit_expr(&binary.right);
            }
            ExprKind::Assign(assign) => {
                self.visit_expr(&assign.left);
                self.visit_expr(&assign.right);
            }
            ExprKind::Cond(cond) => {
                self.visit_expr(&cond.test);
                self.visit_expr(&cond.consequent);
                self.visit_expr(&cond.alternate);
            }
            ExprKind::Unary(unary) => self.visit_expr(&unary.argument),
            ExprKind::Update(update) => self.visit_expr(&update.argument),
            ExprKind::Paren(inner)
            | ExprKind::NonNull(inner)
            | ExprKind::Spread(inner)
            | ExprKind::Await(inner)
            | ExprKind::Delete(inner)
            | ExprKind::Typeof(inner)
            | ExprKind::Void(inner) => self.visit_expr(inner),
            ExprKind::Yield(_, Some(inner)) => self.visit_expr(inner),
            ExprKind::TypeAssertion(assertion) => self.visit_expr(&assertion.expr),
            ExprKind::As(as_expr) => self.visit_expr(&as_expr.expr),
            ExprKind::Satisfies(satisfies) => self.visit_expr(&satisfies.expr),
            ExprKind::Instantiation(instantiation) => self.visit_expr(&instantiation.expr),
            ExprKind::ArrayLit(items) => {
                for item in items.iter().flatten() {
                    self.visit_expr(item);
                }
            }
            ExprKind::ObjectLit(properties) => {
                for property in properties {
                    match property {
                        ObjLitProp::Property(prop) => {
                            if let PropName::Computed(key, _) = &prop.key {
                                self.visit_expr(key);
                            }
                            self.visit_expr(&prop.value);
                        }
                        ObjLitProp::Shorthand(name, span) => self.check(name, *span),
                        ObjLitProp::ShorthandDefault(name, init, span) => {
                            self.check(name, *span);
                            self.visit_expr(init);
                        }
                        ObjLitProp::Spread(inner, _) => self.visit_expr(inner),
                        ObjLitProp::Method(method) => {
                            if let PropName::Computed(key, _) = &method.name {
                                self.visit_expr(key);
                            }
                            self.visit_function(&method.params, Some(&method.body), true);
                        }
                        ObjLitProp::Get(accessor) | ObjLitProp::Set(accessor) => {
                            if let PropName::Computed(key, _) = &accessor.name {
                                self.visit_expr(key);
                            }
                            self.visit_function(&accessor.params, Some(&accessor.body), true);
                        }
                    }
                }
            }
            ExprKind::Template(template) => {
                for inner in &template.exprs {
                    self.visit_expr(inner);
                }
            }
            ExprKind::TaggedTemplate(tagged) => {
                self.visit_expr(&tagged.tag);
                for inner in &tagged.quasi.exprs {
                    self.visit_expr(inner);
                }
            }
            ExprKind::FnExpr(function) => {
                self.scopes.push(FxHashMap::default());
                if let Some(name) = &function.name {
                    self.declare(
                        name,
                        Decl {
                            kind: DeclKind::Other,
                            name_span: expr.span,
                            ready_at: 0,
                            scope_depth: 0,
                            pattern_window: None,
                        },
                    );
                }
                self.visit_function(&function.params, function.body.as_deref(), true);
                self.scopes.pop();
            }
            ExprKind::Arrow(arrow) => {
                self.boundaries.push(self.scopes.len());
                self.scopes.push(FxHashMap::default());
                self.declare_params(&arrow.params);
                for param in &arrow.params {
                    self.visit_pattern(&param.name);
                }
                match &arrow.body {
                    ArrowBody::Expr(body) => self.visit_expr(body),
                    ArrowBody::Block(stmts) => self.visit_stmts(stmts),
                }
                self.scopes.pop();
                self.boundaries.pop();
            }
            ExprKind::ClassExpr(class) => {
                self.scopes.push(FxHashMap::default());
                if let Some(name) = &class.name {
                    self.declare(
                        name,
                        Decl {
                            kind: DeclKind::Other,
                            name_span: expr.span,
                            ready_at: 0,
                            scope_depth: 0,
                            pattern_window: None,
                        },
                    );
                }
                self.visit_class(class);
                self.scopes.pop();
            }
            ExprKind::Comma(items) => {
                for item in items {
                    self.visit_expr(item);
                }
            }
            _ => {}
        }
    }
}

impl TypeChecker {
    /// TS2448 (`let`/`const`) and TS2449 (class) for uses that precede the
    /// declaration in the same file and execute immediately.
    /// Under `outFile`, script files run in program order: a block-scoped
    /// binding of a later script file is not yet initialized when an earlier
    /// file's top-level code runs (tsc's isBlockScopedNameDeclaredBeforeUse).
    fn report_later_file_block_scoped_uses(&mut self, stmts: &[Stmt], uses: &[(String, Span)]) {
        if uses.is_empty() || self.compiler_options.out_file.is_none() {
            return;
        }
        let is_module = stmts.iter().any(|s| {
            matches!(
                s.kind,
                StmtKind::Import(_)
                    | StmtKind::Export(_)
                    | StmtKind::ImportEquals(_)
                    | StmtKind::ExportAssign(_)
            )
        });
        if is_module {
            return;
        }
        let Some(current) = self
            .current_file_name
            .as_deref()
            .map(Self::normalized_file_key)
        else {
            return;
        };
        let Some(current_index) = self.external_file_order.iter().position(|f| *f == current)
        else {
            return;
        };
        for (name, span) in uses {
            let Some(bindings) = self.external_top_level_bindings.get(name) else {
                continue;
            };
            let later = bindings.iter().find(|(file, block_scoped, _)| {
                *block_scoped
                    && self
                        .external_file_order
                        .iter()
                        .position(|f| f == file)
                        .is_some_and(|index| index > current_index)
            });
            let Some((file, _, name_span)) = later.cloned() else {
                continue;
            };
            if !self
                .reported_duplicate_spans
                .insert((2448, span.start, span.end))
            {
                continue;
            }
            self.diagnostics.push(Diagnostic {
                code: 2448,
                message: format!("Block-scoped variable '{name}' used before its declaration."),
                category: DiagnosticCategory::Error,
                file_name: None,
                span: Some(*span),
                related: Some(vec![RelatedDiagnostic {
                    code: 2728,
                    message: format!("'{name}' is declared here."),
                    file_name: Some(file),
                    span: Some(name_span),
                }]),
            });
        }
    }

    pub(crate) fn check_block_scoped_use_before_declaration(&mut self, stmts: &[Stmt]) {
        if self.current_file_is_declaration() {
            return;
        }
        let mut walker = Walker {
            scopes: Vec::new(),
            boundaries: Vec::new(),
            findings: Vec::new(),
            unresolved_immediate: Vec::new(),
        };
        walker.visit_stmts(stmts);
        self.report_later_file_block_scoped_uses(stmts, &walker.unresolved_immediate);
        let file = self.current_file_name.clone();
        for (kind, name, span, name_span) in walker.findings {
            let code = if kind == DeclKind::Class { 2449 } else { 2448 };
            if !self
                .reported_duplicate_spans
                .insert((code, span.start, span.end))
            {
                continue;
            }
            let message = if code == 2449 {
                format!("Class '{name}' used before its declaration.")
            } else {
                format!("Block-scoped variable '{name}' used before its declaration.")
            };
            self.diagnostics.push(Diagnostic {
                code,
                message,
                category: DiagnosticCategory::Error,
                file_name: None,
                span: Some(span),
                related: Some(vec![RelatedDiagnostic {
                    code: 2728,
                    message: format!("'{name}' is declared here."),
                    file_name: file.clone(),
                    span: Some(name_span),
                }]),
            });
        }
    }
}

/// (name start, element end) for each binding name with a default
/// (`name = init`): the name is initialized only after its default.
fn binding_element_ends(pattern: &Pat, out: &mut Vec<(u32, u32)>) {
    match &pattern.kind {
        PatKind::Ident(_) => {}
        PatKind::Assign(inner, init) => {
            if matches!(inner.kind, PatKind::Ident(_)) {
                out.push((inner.span.start, init.span.end));
            } else {
                binding_element_ends(inner, out);
            }
        }
        PatKind::Array(elements) => {
            for element in elements.iter().flatten() {
                match element {
                    ArrayPatElem::Pat(inner) | ArrayPatElem::Rest(inner) => {
                        binding_element_ends(inner, out)
                    }
                }
            }
        }
        PatKind::Object(properties) => {
            for property in properties {
                match property {
                    ObjPatProp::KeyValue(_, inner) | ObjPatProp::Rest(inner) => {
                        binding_element_ends(inner, out)
                    }
                    ObjPatProp::ShorthandAssign(_, init, span) => {
                        out.push((span.start, init.span.end));
                    }
                    ObjPatProp::Shorthand(..) => {}
                }
            }
        }
        PatKind::Rest(inner) => binding_element_ends(inner, out),
    }
}
