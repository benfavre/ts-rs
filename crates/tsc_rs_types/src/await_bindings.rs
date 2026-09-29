//! `await` used as a binding name where it is reserved: at the top level of
//! a module (TS1262) and inside an async function (TS1359). tsc's parser
//! reports these while parsing identifiers in an await context.

use tsc_rs_ast::*;

use crate::TypeChecker;

#[derive(Clone, Copy, PartialEq)]
enum Context {
    /// Neither a module's top level nor an async function body.
    Plain,
    ModuleTop,
    Async,
}

struct AwaitWalk<'a> {
    source: &'a str,
    found: Vec<(Context, Span)>,
}

impl AwaitWalk<'_> {
    fn name(&mut self, context: Context, name: &str, span: Span) {
        if context != Context::Plain && name == "await" {
            self.found.push((context, span));
        }
    }

    /// The first `await` token inside `span` (for names without their own
    /// span, such as a default import).
    fn await_token_in(&self, span: Span) -> Option<Span> {
        let text = self.source.get(span.start as usize..span.end as usize)?;
        let bytes = text.as_bytes();
        let mut from = 0;
        while let Some(at) = text[from..].find("await") {
            let start = from + at;
            let end = start + 5;
            let before_ok = start == 0 || !is_ident_byte(bytes[start - 1]);
            let after_ok = end >= bytes.len() || !is_ident_byte(bytes[end]);
            if before_ok && after_ok {
                return Some(Span::new(
                    span.start + start as u32,
                    span.start + end as u32,
                ));
            }
            from = end;
        }
        None
    }

    fn pattern(&mut self, context: Context, pattern: &Pat) {
        match &pattern.kind {
            PatKind::Ident(name) => self.name(context, name, pattern.span),
            PatKind::Array(elements) => {
                for element in elements.iter().flatten() {
                    match element {
                        ArrayPatElem::Pat(inner) | ArrayPatElem::Rest(inner) => {
                            self.pattern(context, inner)
                        }
                    }
                }
            }
            PatKind::Object(properties) => {
                for property in properties {
                    match property {
                        ObjPatProp::KeyValue(_, inner) | ObjPatProp::Rest(inner) => {
                            self.pattern(context, inner)
                        }
                        ObjPatProp::Shorthand(name, span)
                        | ObjPatProp::ShorthandAssign(name, _, span) => self.name(
                            context,
                            name,
                            Span::new(span.start, span.start + name.len() as u32),
                        ),
                    }
                }
            }
            PatKind::Assign(inner, init) => {
                self.pattern(context, inner);
                self.expr(context, init);
            }
            PatKind::Rest(inner) => self.pattern(context, inner),
        }
    }

    fn statements(&mut self, context: Context, statements: &[Stmt]) {
        for statement in statements {
            self.statement(context, statement);
        }
    }

    fn function(&mut self, is_async: bool, params: &[Param], body: Option<&[Stmt]>) {
        let inner = if is_async {
            Context::Async
        } else {
            Context::Plain
        };
        for param in params {
            self.pattern(inner, &param.name);
            if let Some(init) = &param.initializer {
                self.expr(inner, init);
            }
        }
        if let Some(body) = body {
            self.statements(inner, body);
        }
    }

    fn statement(&mut self, context: Context, statement: &Stmt) {
        match &statement.kind {
            StmtKind::Var(var) => {
                for declaration in &var.declarations {
                    self.pattern(context, &declaration.name);
                    if let Some(init) = &declaration.init {
                        self.expr(context, init);
                    }
                }
            }
            StmtKind::FnDecl(function) => {
                if let (Some(name), Some(span)) = (&function.name, function.name_span) {
                    self.name(context, name, span);
                }
                self.function(
                    function.is_async,
                    &function.params,
                    function.body.as_deref(),
                );
            }
            StmtKind::ClassDecl(class) => self.class(context, class),
            StmtKind::Import(import) if context == Context::ModuleTop => match &import.specifiers {
                ImportClause::Named {
                    default,
                    named,
                    namespace,
                } => {
                    if default.as_deref() == Some("await") || namespace.as_deref() == Some("await")
                    {
                        if let Some(span) = self.await_token_in(statement.span) {
                            self.found.push((context, span));
                        }
                    }
                    for specifier in named {
                        if specifier.local == "await" {
                            let end = specifier.span.end;
                            self.found.push((context, Span::new(end - 5, end)));
                        }
                    }
                }
                ImportClause::Require(name) => {
                    if name == "await" {
                        if let Some(span) = self.await_token_in(statement.span) {
                            self.found.push((context, span));
                        }
                    }
                }
            },
            StmtKind::ImportEquals(import) if context == Context::ModuleTop => {
                if import.name == "await" {
                    if let Some(span) = self.await_token_in(statement.span) {
                        self.found.push((context, span));
                    }
                }
            }
            StmtKind::Export(export) => match &export.kind {
                ExportDeclKind::Decl(declaration) | ExportDeclKind::DefaultDecl(declaration) => {
                    self.statement(context, declaration)
                }
                ExportDeclKind::Default(expr) => self.expr(context, expr),
                _ => {}
            },
            StmtKind::Expr(expr) | StmtKind::Throw(expr) => self.expr(context, expr),
            StmtKind::Return(Some(expr)) => self.expr(context, expr),
            StmtKind::Block(body) => self.statements(context, body),
            StmtKind::If(if_stmt) => {
                self.expr(context, &if_stmt.test);
                self.statement(context, &if_stmt.consequent);
                if let Some(alternate) = &if_stmt.alternate {
                    self.statement(context, alternate);
                }
            }
            StmtKind::For(f) => {
                if let Some(ForInit::Var(var)) = &f.init {
                    for declaration in &var.declarations {
                        self.pattern(context, &declaration.name);
                    }
                }
                self.statement(context, &f.body);
            }
            StmtKind::ForIn(f) => {
                if let ForInOfLeft::Var(var) = &f.left {
                    for declaration in &var.declarations {
                        self.pattern(context, &declaration.name);
                    }
                }
                self.statement(context, &f.body);
            }
            StmtKind::ForOf(f) => {
                if let ForInOfLeft::Var(var) = &f.left {
                    for declaration in &var.declarations {
                        self.pattern(context, &declaration.name);
                    }
                }
                self.statement(context, &f.body);
            }
            StmtKind::While(w) => self.statement(context, &w.body),
            StmtKind::DoWhile(d) => self.statement(context, &d.body),
            StmtKind::Labeled(l) => self.statement(context, &l.body),
            StmtKind::Try(t) => {
                self.statements(context, &t.block);
                if let Some(handler) = &t.handler {
                    if let Some(param) = &handler.param {
                        self.pattern(context, param);
                    }
                    self.statements(context, &handler.body);
                }
                if let Some(finalizer) = &t.finalizer {
                    self.statements(context, finalizer);
                }
            }
            StmtKind::Switch(s) => {
                for case in &s.cases {
                    self.statements(context, &case.consequent);
                }
            }
            _ => {}
        }
    }

    fn class(&mut self, context: Context, class: &ClassDecl) {
        if let (Some(name), Some(span)) = (&class.name, class.name_span) {
            self.name(context, name, span);
        }
        for member in &class.members {
            match &member.kind {
                ClassMemberKind::Method(method) => {
                    self.function(method.is_async, &method.params, method.body.as_deref())
                }
                ClassMemberKind::Constructor(ctor) => {
                    self.function(false, &ctor.params, ctor.body.as_deref())
                }
                ClassMemberKind::GetAccessor(a) | ClassMemberKind::SetAccessor(a) => {
                    self.function(false, &a.params, a.body.as_deref())
                }
                _ => {}
            }
        }
    }

    fn expr(&mut self, context: Context, expr: &Expr) {
        match &expr.kind {
            ExprKind::Arrow(arrow) => {
                let inner = if arrow.is_async {
                    Context::Async
                } else {
                    Context::Plain
                };
                for param in &arrow.params {
                    self.pattern(inner, &param.name);
                }
                match &arrow.body {
                    ArrowBody::Block(body) => self.statements(inner, body),
                    ArrowBody::Expr(body) => self.expr(inner, body),
                }
            }
            ExprKind::FnExpr(function) => {
                // A function expression's own name is bound in its own
                // (async) context.
                let own = if function.is_async {
                    Context::Async
                } else {
                    Context::Plain
                };
                if let (Some(name), Some(span)) = (&function.name, function.name_span) {
                    self.name(own, name, span);
                }
                self.function(
                    function.is_async,
                    &function.params,
                    function.body.as_deref(),
                );
            }
            ExprKind::ClassExpr(class) => self.class(context, class),
            ExprKind::Call(call) => {
                self.expr(context, &call.callee);
                for argument in &call.args {
                    self.expr(context, argument);
                }
            }
            ExprKind::New(new) => {
                for argument in new.args.iter().flatten() {
                    self.expr(context, argument);
                }
            }
            ExprKind::Paren(inner) | ExprKind::Await(inner) | ExprKind::Spread(inner) => {
                self.expr(context, inner)
            }
            ExprKind::Assign(assign) => self.expr(context, &assign.right),
            ExprKind::Binary(binary) => {
                self.expr(context, &binary.left);
                self.expr(context, &binary.right);
            }
            ExprKind::Cond(cond) => {
                self.expr(context, &cond.consequent);
                self.expr(context, &cond.alternate);
            }
            ExprKind::ArrayLit(elements) => {
                for element in elements.iter().flatten() {
                    self.expr(context, element);
                }
            }
            ExprKind::ObjectLit(properties) => {
                for property in properties {
                    match property {
                        ObjLitProp::Property(p) => self.expr(context, &p.value),
                        ObjLitProp::Method(m) => {
                            self.function(m.is_async, &m.params, Some(&m.body))
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
}

fn is_ident_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'$'
}

impl TypeChecker {
    pub(crate) fn check_await_bindings(&mut self, file: &SourceFile) {
        if tsc_rs_parser::has_syntax_errors(&file.diagnostics) || self.current_file_is_declaration()
        {
            return;
        }
        let is_module = file.statements.iter().any(|s| {
            matches!(
                s.kind,
                StmtKind::Import(_) | StmtKind::Export(_) | StmtKind::ExportAssign(_)
            )
        });
        let source = self.current_source.clone().unwrap_or_default();
        let mut walk = AwaitWalk {
            source: &source,
            found: Vec::new(),
        };
        let top = if is_module {
            Context::ModuleTop
        } else {
            Context::Plain
        };
        walk.statements(top, &file.statements);
        for (context, span) in walk.found {
            let (code, message) = if context == Context::ModuleTop {
                (
                    1262,
                    "Identifier expected. 'await' is a reserved word at the top-level of a module.",
                )
            } else {
                (
                    1359,
                    "Identifier expected. 'await' is a reserved word that cannot be used here.",
                )
            };
            if self
                .reported_duplicate_spans
                .insert((code, span.start, span.end))
            {
                self.diagnostics.push(Diagnostic {
                    code,
                    message: message.to_string(),
                    category: DiagnosticCategory::Error,
                    file_name: None,
                    span: Some(span),
                    related: None,
                });
            }
        }
    }
}
