//! Syntax-aware definition targets. Keep declaration identity and lexical
//! boundaries intact instead of searching for another occurrence of a name.

use tsc_rs_ast::*;
use tsc_rs_query::QueryEngine;

mod calls;
mod properties;

type Target = (String, Span);

fn contains(span: Span, offset: u32) -> bool {
    span.start <= offset && offset < span.end
}

fn property_name(name: &PropName) -> Option<&str> {
    match name {
        PropName::Ident(n, _)
        | PropName::String(n, _)
        | PropName::Number(n, _)
        | PropName::Private(n, _) => Some(n),
        _ => None,
    }
}

#[derive(Default)]
struct Index<'a> {
    symbols: tsc_rs_symbols::SymbolTable,
    classes: Vec<(&'a ClassDecl, bool)>,
    functions: Vec<&'a FnDecl>,
    variables: Vec<&'a VarDeclarator>,
    expressions: Vec<&'a Expr>,
    aliases: Vec<&'a TypeAliasDecl>,
    interfaces: Vec<&'a InterfaceDecl>,
    parameters: Vec<&'a Param>,
}

impl<'a> Index<'a> {
    fn new(sf: &'a SourceFile) -> Self {
        let mut index = Self {
            symbols: tsc_rs_symbols::bind(sf),
            ..Self::default()
        };
        index.statements(&sf.statements);
        index
    }

    fn statements(&mut self, statements: &'a [Stmt]) {
        for stmt in statements {
            self.statement(stmt);
        }
    }

    fn variables(&mut self, var: &'a VarStmt) {
        for decl in &var.declarations {
            self.variables.push(decl);
            if let Some(init) = &decl.init {
                self.expression(init);
            }
        }
    }

    fn statement(&mut self, stmt: &'a Stmt) {
        match &stmt.kind {
            StmtKind::Var(var) => self.variables(var),
            StmtKind::Expr(expr) | StmtKind::Throw(expr) | StmtKind::ExportAssign(expr) => {
                self.expression(expr)
            }
            StmtKind::Return(Some(expr)) => self.expression(expr),
            StmtKind::Block(body) => self.statements(body),
            StmtKind::FnDecl(f) => self.function(f),
            StmtKind::ClassDecl(c) => self.class(c, false),
            StmtKind::InterfaceDecl(i) => self.interfaces.push(i),
            StmtKind::TypeAlias(a) => self.aliases.push(a),
            StmtKind::ModuleDecl(m) => self.module(m),
            StmtKind::Export(e) => match &e.kind {
                ExportDeclKind::Decl(s) | ExportDeclKind::DefaultDecl(s) => self.statement(s),
                ExportDeclKind::Default(e) => self.expression(e),
                _ => {}
            },
            StmtKind::If(s) => {
                self.expression(&s.test);
                self.statement(&s.consequent);
                if let Some(alt) = &s.alternate {
                    self.statement(alt);
                }
            }
            StmtKind::While(s) => {
                self.expression(&s.test);
                self.statement(&s.body);
            }
            StmtKind::DoWhile(s) => {
                self.statement(&s.body);
                self.expression(&s.test);
            }
            StmtKind::For(s) => {
                if let Some(init) = &s.init {
                    match init {
                        ForInit::Var(v) => self.variables(v),
                        ForInit::Expr(e) => self.expression(e),
                    }
                }
                if let Some(e) = &s.test {
                    self.expression(e);
                }
                if let Some(e) = &s.update {
                    self.expression(e);
                }
                self.statement(&s.body);
            }
            StmtKind::ForIn(s) => {
                self.for_left(&s.left);
                self.expression(&s.right);
                self.statement(&s.body);
            }
            StmtKind::ForOf(s) => {
                self.for_left(&s.left);
                self.expression(&s.right);
                self.statement(&s.body);
            }
            StmtKind::Switch(s) => {
                self.expression(&s.discriminant);
                for c in &s.cases {
                    if let Some(e) = &c.test {
                        self.expression(e);
                    }
                    self.statements(&c.consequent);
                }
            }
            StmtKind::Try(s) => {
                self.statements(&s.block);
                if let Some(c) = &s.handler {
                    self.statements(&c.body);
                }
                if let Some(b) = &s.finalizer {
                    self.statements(b);
                }
            }
            StmtKind::Labeled(s) => self.statement(&s.body),
            StmtKind::With(s) => {
                self.expression(&s.object);
                self.statement(&s.body);
            }
            _ => {}
        }
    }

    fn for_left(&mut self, left: &'a ForInOfLeft) {
        match left {
            ForInOfLeft::Var(v) => self.variables(v),
            ForInOfLeft::Expr(e) => self.expression(e),
            ForInOfLeft::Pat(_) => {}
        }
    }

    fn module(&mut self, module: &'a ModuleDecl) {
        match &module.body {
            Some(ModuleBody::Block(body)) => self.statements(body),
            Some(ModuleBody::Module(m)) => self.module(m),
            None => {}
        }
    }

    fn params(&mut self, params: &'a [Param]) {
        self.parameters.extend(params);
        for p in params {
            if let Some(e) = &p.initializer {
                self.expression(e);
            }
        }
    }

    fn function(&mut self, f: &'a FnDecl) {
        self.functions.push(f);
        self.params(&f.params);
        if let Some(body) = &f.body {
            self.statements(body);
        }
    }

    fn property(&mut self, name: &'a PropName) {
        if let PropName::Computed(e, _) = name {
            self.expression(e);
        }
    }

    fn class(&mut self, c: &'a ClassDecl, expression: bool) {
        self.classes.push((c, expression));
        if let Some(e) = &c.extends {
            self.expression(e);
        }
        for member in &c.members {
            match &member.kind {
                ClassMemberKind::Method(m) => {
                    self.property(&m.name);
                    self.params(&m.params);
                    if let Some(b) = &m.body {
                        self.statements(b);
                    }
                }
                ClassMemberKind::Constructor(m) => {
                    self.params(&m.params);
                    if let Some(b) = &m.body {
                        self.statements(b);
                    }
                }
                ClassMemberKind::GetAccessor(m) | ClassMemberKind::SetAccessor(m) => {
                    self.property(&m.name);
                    self.params(&m.params);
                    if let Some(b) = &m.body {
                        self.statements(b);
                    }
                }
                ClassMemberKind::Property(p) => {
                    self.property(&p.name);
                    if let Some(e) = &p.initializer {
                        self.expression(e);
                    }
                }
                ClassMemberKind::StaticBlock(b) => self.statements(b),
                _ => {}
            }
        }
    }

    fn expression(&mut self, expr: &'a Expr) {
        self.expressions.push(expr);
        if let Some(inner) = expr.kind.type_layer_inner() {
            self.expression(inner);
            return;
        }
        match &expr.kind {
            ExprKind::FnExpr(f) => self.function(f),
            ExprKind::ClassExpr(c) => self.class(c, true),
            ExprKind::Arrow(f) => {
                self.params(&f.params);
                match &f.body {
                    ArrowBody::Expr(e) => self.expression(e),
                    ArrowBody::Block(b) => self.statements(b),
                }
            }
            ExprKind::Call(c) => {
                self.expression(&c.callee);
                for a in &c.args {
                    self.expression(a);
                }
            }
            ExprKind::New(c) => {
                self.expression(&c.callee);
                if let Some(args) = &c.args {
                    for a in args {
                        self.expression(a);
                    }
                }
            }
            ExprKind::Member(m) => self.expression(&m.object),
            ExprKind::ElemAccess(m) => {
                self.expression(&m.object);
                self.expression(&m.index);
            }
            ExprKind::Binary(b) => {
                self.expression(&b.left);
                self.expression(&b.right);
            }
            ExprKind::Assign(b) => {
                self.expression(&b.left);
                self.expression(&b.right);
            }
            ExprKind::Cond(c) => {
                self.expression(&c.test);
                self.expression(&c.consequent);
                self.expression(&c.alternate);
            }
            ExprKind::Unary(u) => self.expression(&u.argument),
            ExprKind::Update(u) => self.expression(&u.argument),
            ExprKind::Paren(e)
            | ExprKind::Spread(e)
            | ExprKind::Await(e)
            | ExprKind::Delete(e)
            | ExprKind::Typeof(e)
            | ExprKind::Void(e) => self.expression(e),
            ExprKind::Yield(_, Some(e)) => self.expression(e),
            ExprKind::Comma(es) => {
                for e in es {
                    self.expression(e);
                }
            }
            ExprKind::ArrayLit(es) => {
                for e in es.iter().flatten() {
                    self.expression(e);
                }
            }
            ExprKind::Template(t) => {
                for e in &t.exprs {
                    self.expression(e);
                }
            }
            ExprKind::TaggedTemplate(t) => {
                self.expression(&t.tag);
                for e in &t.quasi.exprs {
                    self.expression(e);
                }
            }
            ExprKind::ObjectLit(props) => {
                for p in props {
                    match p {
                        ObjLitProp::Property(p) => {
                            self.property(&p.key);
                            self.expression(&p.value);
                        }
                        ObjLitProp::Spread(e, _) | ObjLitProp::ShorthandDefault(_, e, _) => {
                            self.expression(e)
                        }
                        ObjLitProp::Method(m) => {
                            self.property(&m.name);
                            self.params(&m.params);
                            self.statements(&m.body);
                        }
                        ObjLitProp::Get(m) | ObjLitProp::Set(m) => {
                            self.property(&m.name);
                            self.params(&m.params);
                            self.statements(&m.body);
                        }
                        _ => {}
                    }
                }
            }
            ExprKind::JsxSelfClosing(e) => {
                self.expression(&e.name);
                for a in &e.attributes {
                    if let JsxAttribute::Normal { value: Some(e), .. } = a {
                        self.expression(e);
                    }
                }
            }
            ExprKind::JsxElement(e) => {
                self.expression(&e.name);
                for a in &e.attributes {
                    if let JsxAttribute::Normal { value: Some(e), .. } = a {
                        self.expression(e);
                    }
                }
            }
            _ => {}
        }
    }

    fn enclosing_class(&self, offset: u32) -> Option<&'a ClassDecl> {
        self.classes
            .iter()
            .map(|(c, _)| *c)
            .filter(|c| contains(c.span, offset))
            .min_by_key(|c| c.span.end - c.span.start)
    }

    fn binding(&self, name: &str, offset: u32) -> Option<&tsc_rs_symbols::Symbol> {
        let graph = &self.symbols.scope_graph;
        let mut scope = graph.innermost_scope_at(offset).or(Some(0));
        while let Some(id) = scope {
            let current = graph.scope(id)?;
            if let Some(id) = current.bindings.get(name) {
                return self.symbols.get_symbol(*id);
            }
            scope = current.parent;
        }
        None
    }

    fn variable(&self, name: &str, offset: u32) -> Option<&'a VarDeclarator> {
        let symbol = self.binding(name, offset)?;
        self.variables.iter().copied().find(|v| {
            symbol
                .declarations
                .iter()
                .any(|d| v.name.span.start <= d.span.start && d.span.end <= v.name.span.end)
        })
    }

    fn named_class(&self, name: &str) -> Option<&'a ClassDecl> {
        self.classes
            .iter()
            .find_map(|(c, expression)| {
                (!expression && c.name.as_deref() == Some(name)).then_some(*c)
            })
            .or_else(|| {
                self.variables.iter().find_map(|v| {
                    if matches!(&v.name.kind, PatKind::Ident(n) if n == name) {
                        if let ExprKind::ClassExpr(c) = &v.init.as_ref()?.kind {
                            return Some(c.as_ref());
                        }
                    }
                    None
                })
            })
    }

    fn constructor(&self, class: &'a ClassDecl, seen: &mut Vec<u32>) -> Option<Span> {
        if seen.contains(&class.span.start) {
            return None;
        }
        seen.push(class.span.start);
        if let Some(span) = class.members.iter().find_map(|m| match &m.kind {
            ClassMemberKind::Constructor(c) if c.body.is_some() => {
                Some(Span::new(c.keyword_span.start, m.span.end))
            }
            _ => None,
        }) {
            return Some(span);
        }
        if let Some(Expr {
            kind: ExprKind::Ident(name),
            ..
        }) = class.extends.as_deref()
        {
            return self.constructor(self.named_class(name)?, seen);
        }
        class.name_span
    }
}

pub(super) fn resolve(
    qe: &QueryEngine,
    sf: &SourceFile,
    offset: u32,
    output: &tsc_rs_types::TypeCheckOutput,
) -> Option<Vec<Target>> {
    let source = qe.get_source_file(&sf.file_name)?;
    let token = super::extract_token_at(source, offset);
    let index = Index::new(sf);
    let local = |span| (sf.file_name.clone(), span);
    if let Some(targets) = calls::resolve(qe, sf, &index, offset, output) {
        return Some(targets);
    }
    if let Some(comment) = sf
        .comments
        .iter()
        .find(|c| c.pos <= offset && offset < c.end)
    {
        if source[comment.pos as usize..comment.end as usize].contains("@see") {
            if let Some(function) = index
                .functions
                .iter()
                .filter(|f| f.span.start >= comment.end)
                .min_by_key(|f| f.span.start)
            {
                if source[comment.end as usize..function.span.start as usize]
                    .trim()
                    .is_empty()
                {
                    if let Some(param) = function
                        .params
                        .iter()
                        .find(|p| matches!(&p.name.kind, PatKind::Ident(n) if n == &token))
                    {
                        return Some(vec![local(param.name.span)]);
                    }
                }
            }
        }
    }
    if let Some(class) = index.enclosing_class(offset) {
        for member in &class.members {
            let ClassMemberKind::Method(method) = &member.kind else {
                continue;
            };
            if method.modifiers & MOD_OVERRIDE == 0
                || !contains(member.span, offset)
                || offset > method.name.span().end
            {
                continue;
            }
            let mut parent_expr = class.extends.as_deref()?;
            while let ExprKind::Paren(inner) = &parent_expr.kind {
                parent_expr = inner;
            }
            let parent = match &parent_expr.kind {
                ExprKind::ClassExpr(c) => c.as_ref(),
                ExprKind::Ident(n) => index.named_class(n)?,
                _ => return None,
            };
            let name = property_name(&method.name)?;
            let targets: Vec<_> = parent
                .members
                .iter()
                .filter_map(|m| match &m.kind {
                    ClassMemberKind::Method(m) if property_name(&m.name) == Some(name) => {
                        Some(local(m.name.span()))
                    }
                    _ => None,
                })
                .collect();
            if !targets.is_empty() {
                return Some(targets);
            }
        }
    }
    // A destructured Promise callback receives the exports of import(...).
    for outer in &index.expressions {
        let ExprKind::Call(call) = &outer.kind else {
            continue;
        };
        let ExprKind::Member(member) = &call.callee.kind else {
            continue;
        };
        if member.property != "then" {
            continue;
        }
        let ExprKind::Call(import) = &member.object.kind else {
            continue;
        };
        if !matches!(&import.callee.kind, ExprKind::Ident(n) if n == "import") {
            continue;
        }
        let Some(Expr {
            kind: ExprKind::StrLit(spec),
            ..
        }) = import.args.first().map(|e| e.as_ref())
        else {
            continue;
        };
        for arg in &call.args {
            let ExprKind::Arrow(arrow) = &arg.kind else {
                continue;
            };
            for p in &arrow.params {
                let PatKind::Object(props) = &p.name.kind else {
                    continue;
                };
                for prop in props {
                    if let ObjPatProp::Shorthand(name, span) = prop {
                        if span.start <= offset && offset <= span.end {
                            let file = module_path(qe, spec, &sf.file_name)?;
                            return export_target(qe, &file, Some(name), &mut vec![])
                                .map(|t| vec![t]);
                        }
                    }
                }
            }
        }
    }

    if let Some(target) = reference_directive(qe, &sf.file_name, source, offset) {
        return Some(target.into_iter().collect());
    }
    if let Some(spec) = module_specifier(sf, &index, offset) {
        let mut ambient = ambient_modules(qe, spec);
        if !ambient.is_empty() {
            if let Some(file) = qe.resolve_import_path(spec, &sf.file_name) {
                if !file.ends_with(".ts")
                    && !file.ends_with(".js")
                    && !file.ends_with(".tsx")
                    && !file.ends_with(".jsx")
                {
                    ambient.insert(0, (file, Span::new(0, 0)));
                }
            }
        }
        if !ambient.is_empty() {
            return Some(ambient);
        }
    }
    if let Some(target) = quoted_alias(qe, sf, offset) {
        return Some(vec![target]);
    }
    if !super::is_inside_string_or_comment(source, offset) && !token.is_empty() {
        // A property named like an imported binding is not that binding.
        let start = super::find_identifier_occurrences(source, &token)
            .into_iter()
            .find(|s| s.start <= offset && offset <= s.end)
            .map(|s| s.start as usize)?;
        if !source[..start].trim_end().ends_with('.') {
            let binding = index.binding(&token, start as u32);
            if binding.is_some_and(|s| s.flags & tsc_rs_symbols::SYM_PARAMETER != 0)
                && imported_binding(qe, sf, &index, &token, &mut vec![]).is_some()
            {
                return Some(
                    binding
                        .into_iter()
                        .flat_map(|s| &s.declarations)
                        .map(|d| local(d.span))
                        .collect(),
                );
            }
            let is_import = binding.is_some_and(|s| s.flags & tsc_rs_symbols::SYM_IMPORT != 0)
                || index
                    .variable(&token, start as u32)
                    .is_some_and(|v| v.init.as_deref().and_then(require_spec).is_some());
            if is_import {
                if let Some(target) = imported_binding(qe, sf, &index, &token, &mut vec![]) {
                    return Some(all_declarations(qe, target));
                }
            }
        }
    }
    if let Some(targets) = merged_declarations(qe, sf, offset, &token) {
        return Some(targets);
    }

    // Only the property part of new.target is navigable. import.meta has no
    // declaration; an ordinary function nested in a class owns its new.target.
    if let Some(e) = index
        .expressions
        .iter()
        .find(|e| matches!(e.kind, ExprKind::MetaProp(_)) && contains(e.span, offset))
    {
        let ExprKind::MetaProp(meta) = &e.kind else {
            unreachable!()
        };
        if meta.meta != "new" || token != "target" {
            return Some(vec![]);
        }
        let class = index.enclosing_class(offset);
        let function = index
            .functions
            .iter()
            .filter(|f| contains(f.span, offset))
            .min_by_key(|f| f.span.end - f.span.start);
        let span = match (class, function) {
            (Some(c), Some(f)) if f.span.start > c.span.start => f.name_span,
            (Some(c), _) => c.name_span,
            (None, Some(f)) => f.name_span,
            _ => None,
        };
        return Some(span.map(local).into_iter().collect());
    }

    if token == "static" {
        if let Some(c) = index.enclosing_class(offset) {
            if c.members.iter().any(|m| {
                matches!(m.kind, ClassMemberKind::StaticBlock(_))
                    && contains(Span::new(m.span.start, m.span.start + 6), offset)
            }) {
                return Some(
                    c.members
                        .iter()
                        .filter(|m| matches!(m.kind, ClassMemberKind::StaticBlock(_)))
                        .map(|m| local(Span::new(m.span.start, m.span.start + 6)))
                        .collect(),
                );
            }
        }
    }

    if token == "super" {
        let class = index.enclosing_class(offset)?;
        let Some(Expr {
            kind: ExprKind::Ident(name),
            ..
        }) = class.extends.as_deref()
        else {
            return Some(vec![]);
        };
        let parent = index.named_class(name)?;
        let is_call = index.expressions.iter().any(|e| matches!(&e.kind, ExprKind::Call(c) if matches!(c.callee.kind, ExprKind::Super) && contains(c.callee.span, offset)));
        let target = if is_call {
            index.constructor(parent, &mut vec![])
        } else {
            parent.name_span
        };
        let mut targets: Vec<_> = target.map(local).into_iter().collect();
        if is_call
            && !parent
                .members
                .iter()
                .any(|m| matches!(m.kind, ClassMemberKind::Constructor(_)))
        {
            if let Some(span) = parent.name_span {
                if Some(span) != target {
                    targets.push(local(span));
                }
            }
        }
        return Some(targets);
    }

    if token == "instanceof" {
        if let Some(binary) = index.expressions.iter().find_map(|e| match &e.kind {
            ExprKind::Binary(b)
                if b.op == BinaryOp::InstanceOf
                    && b.left.span.end <= offset
                    && offset < b.right.span.start =>
            {
                Some(b)
            }
            _ => None,
        }) {
            if let ExprKind::Ident(name) = &binary.right.kind {
                if let Some(c) = index.named_class(name) {
                    let spans: Vec<_> = c.members.iter().filter_map(|m| {
                        let ClassMemberKind::Method(method) = &m.kind else { return None; };
                        let PropName::Computed(expr, span) = &method.name else { return None; };
                        (method.modifiers & MOD_STATIC != 0 && matches!(&expr.kind, ExprKind::Member(m) if m.property == "hasInstance" && matches!(&m.object.kind, ExprKind::Ident(n) if n == "Symbol"))).then(|| local(*span))
                    }).collect();
                    if !spans.is_empty() {
                        return Some(spans);
                    }
                }
            }
        }
    }

    // A keyword in a computed method name belongs to that member, even if
    // the recovery grammar also parsed a yield expression inside the name.
    if token == "yield" {
        if let Some(c) = index.enclosing_class(offset) {
            for m in &c.members {
                if let ClassMemberKind::Method(method) = &m.kind {
                    if let PropName::Computed(_, span) = &method.name {
                        if contains(*span, offset) {
                            return Some(vec![local(*span)]);
                        }
                    }
                }
            }
        }
    }

    // The name of a class expression exists only inside that expression.
    if index
        .classes
        .iter()
        .any(|(c, expression)| *expression && c.name.as_deref() == Some(token.as_str()))
    {
        if let Some((c, _)) = index
            .classes
            .iter()
            .filter(|(c, expression)| {
                *expression && c.name.as_deref() == Some(token.as_str()) && contains(c.span, offset)
            })
            .min_by_key(|(c, _)| c.span.end - c.span.start)
        {
            return Some(c.name_span.map(local).into_iter().collect());
        }
        let symbols = qe.get_file_symbols(&sf.file_name)?;
        if !symbols
            .scope_graph
            .references
            .iter()
            .any(|r| contains(r.span, offset))
        {
            return Some(vec![]);
        }
    }
    properties::resolve(&index, offset).map(|spans| spans.into_iter().map(local).collect())
}

fn require_spec(expr: &Expr) -> Option<&str> {
    let ExprKind::Call(call) = &expr.kind else {
        return None;
    };
    if !matches!(&call.callee.kind, ExprKind::Ident(n) if n == "require") {
        return None;
    }
    match &call.args.first()?.kind {
        ExprKind::StrLit(s) => Some(s),
        _ => None,
    }
}

fn module_path(qe: &QueryEngine, spec: &str, from: &str) -> Option<String> {
    if let Some(path) = qe.resolve_import_path(spec, from) {
        return Some(path);
    }
    if spec.starts_with('.') || spec.starts_with('/') {
        return None;
    }
    let mut dir = from.rsplit_once('/').map(|(d, _)| d);
    while let Some(base) = dir {
        let package = normalize(&format!("{base}/node_modules/{spec}"));
        let metadata = qe
            .get_source_file(&format!("{package}/package.json"))
            .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok());
        let mut candidates = vec![];
        if let Some(meta) = metadata {
            for key in ["types", "typings", "main"] {
                if let Some(path) = meta[key].as_str() {
                    candidates.push(normalize(&format!("{package}/{path}")));
                }
            }
        }
        for suffix in [
            ".ts",
            ".d.ts",
            ".js",
            "/index.ts",
            "/index.d.ts",
            "/index.js",
        ] {
            candidates.push(format!("{package}{suffix}"));
        }
        if let Some(path) = candidates
            .into_iter()
            .find(|p| qe.get_source_file(p).is_some())
        {
            return Some(path);
        }
        dir = base.rsplit_once('/').map(|(d, _)| d);
    }
    None
}

fn external_module(sf: &SourceFile) -> bool {
    sf.statements.iter().any(|s| {
        matches!(
            s.kind,
            StmtKind::Import(_)
                | StmtKind::ImportEquals(_)
                | StmtKind::Export(_)
                | StmtKind::ExportAssign(_)
        )
    })
}

fn namespace_path(statements: &[Stmt], offset: u32, path: &mut Vec<String>) {
    for stmt in statements {
        if contains(stmt.span, offset) {
            if let StmtKind::ModuleDecl(m) = &stmt.kind {
                if let ModuleName::Ident(n) = &m.name {
                    path.push(n.clone());
                    if let Some(ModuleBody::Block(body)) = &m.body {
                        namespace_path(body, offset, path);
                    }
                }
            }
        }
    }
}

fn declaration_spans(statements: &[Stmt], path: &[String], name: &str, out: &mut Vec<Span>) {
    for stmt in statements {
        match &stmt.kind {
            StmtKind::Export(e) => {
                if let ExportDeclKind::Decl(stmt) = &e.kind {
                    declaration_spans(std::slice::from_ref(stmt.as_ref()), path, name, out);
                }
            }
            StmtKind::InterfaceDecl(i) if path.is_empty() && i.name == name => {
                if let Some(s) = i.name_span {
                    out.push(s);
                }
            }
            StmtKind::Var(v) if path.is_empty() => {
                out.extend(
                    v.declarations
                        .iter()
                        .filter(|v| matches!(&v.name.kind, PatKind::Ident(n) if n == name))
                        .map(|v| v.name.span),
                );
            }
            StmtKind::ModuleDecl(m) => {
                if let ModuleName::Ident(n) = &m.name {
                    if path.is_empty() && n == name {
                        if let Some(s) = m.name_span {
                            out.push(s);
                        }
                    } else if path.first() == Some(n) {
                        if let Some(ModuleBody::Block(body)) = &m.body {
                            declaration_spans(body, &path[1..], name, out);
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

fn merged_declarations(
    qe: &QueryEngine,
    sf: &SourceFile,
    offset: u32,
    token: &str,
) -> Option<Vec<Target>> {
    if token.is_empty() || super::KEYWORDS.contains(&token) {
        return None;
    }
    let source = qe.get_source_file(&sf.file_name)?;
    let token_span = super::find_identifier_occurrences(source, token)
        .into_iter()
        .find(|s| s.start <= offset && offset <= s.end)?;
    if source[..token_span.start as usize]
        .trim_end()
        .ends_with('.')
    {
        return None;
    }
    let in_comment = sf
        .comments
        .iter()
        .any(|c| c.pos <= offset && offset < c.end);
    if super::is_inside_string_or_comment(source, offset) && !in_comment {
        return None;
    }
    if let Some(symbols) = qe.get_file_symbols(&sf.file_name) {
        if let Some(r) = symbols.scope_graph.reference_at(token_span.start) {
            if let Some(sym) = symbols.get_symbol(r.symbol_id) {
                if sym.flags
                    & (tsc_rs_symbols::SYM_INTERFACE
                        | tsc_rs_symbols::SYM_MODULE
                        | tsc_rs_symbols::SYM_VARIABLE)
                    == 0
                {
                    return None;
                }
                if symbols
                    .scope_graph
                    .binding_scope(sym.id)
                    .and_then(|id| symbols.scope_graph.scope(id))
                    .is_some_and(|scope| {
                        !matches!(
                            scope.kind,
                            tsc_rs_symbols::ScopeKind::Global | tsc_rs_symbols::ScopeKind::Module
                        )
                    })
                {
                    return None;
                }
            }
        }
    }
    let mut path = vec![];
    namespace_path(&sf.statements, offset, &mut path);
    let mut files = qe.source_file_names();
    files.sort();
    loop {
        let mut targets = vec![];
        for file in &files {
            if file == "__lib__.d.ts" {
                continue;
            }
            if file != &sf.file_name && external_module(sf) {
                continue;
            }
            let Some(source) = qe.get_source_file(file) else {
                continue;
            };
            if !source.contains(token) {
                continue;
            }
            let other = tsc_rs_parser::parse(file, source);
            if file != &sf.file_name && external_module(&other) {
                continue;
            }
            let mut spans = vec![];
            declaration_spans(&other.statements, &path, token, &mut spans);
            targets.extend(spans.into_iter().map(|s| (file.clone(), s)));
        }
        if targets.len() > 1 || (in_comment && !targets.is_empty()) {
            return Some(targets);
        }
        if path.pop().is_none() {
            break;
        }
    }
    None
}

fn all_declarations(qe: &QueryEngine, target: Target) -> Vec<Target> {
    let Some(source) = qe.get_source_file(&target.0) else {
        return vec![target];
    };
    let file = tsc_rs_parser::parse(&target.0, source);
    let table = tsc_rs_symbols::bind(&file);
    let Some(symbol) = table.symbols.iter().find(|s| {
        s.declarations
            .iter()
            .any(|d| d.span.start == target.1.start)
    }) else {
        return vec![target];
    };
    let scope = table.scope_graph.binding_scope(symbol.id);
    let mut results: Vec<_> = table
        .symbols
        .iter()
        .filter(|s| s.name == symbol.name && table.scope_graph.binding_scope(s.id) == scope)
        .flat_map(|s| &s.declarations)
        .map(|d| (target.0.clone(), d.span))
        .collect();
    results.sort_by_key(|(_, s)| s.start);
    results.dedup();
    results
}

fn module_specifier<'a>(sf: &'a SourceFile, index: &Index<'a>, offset: u32) -> Option<&'a str> {
    for s in &sf.statements {
        if let StmtKind::Import(i) = &s.kind {
            if contains(i.source_span, offset) {
                return Some(&i.source);
            }
        }
    }
    index.expressions.iter().find_map(|e| {
        let ExprKind::Call(c) = &e.kind else {
            return None;
        };
        let arg = c.args.first()?;
        contains(arg.span, offset)
            .then(|| require_spec(e))
            .flatten()
    })
}

fn quoted_alias(qe: &QueryEngine, sf: &SourceFile, offset: u32) -> Option<Target> {
    for stmt in &sf.statements {
        if !contains(stmt.span, offset) {
            continue;
        }
        match &stmt.kind {
            StmtKind::Import(i) => {
                if let ImportClause::Named { named, .. } = &i.specifiers {
                    for spec in named {
                        if spec.imported_is_string && contains(spec.span, offset) {
                            let target = module_path(qe, &i.source, &sf.file_name)?;
                            return export_target(
                                qe,
                                &target,
                                spec.imported.as_deref(),
                                &mut vec![],
                            );
                        }
                    }
                }
            }
            StmtKind::Export(e) => {
                if let ExportDeclKind::Named {
                    specifiers, source, ..
                } = &e.kind
                {
                    for spec in specifiers {
                        if (spec.local_is_string || spec.exported_is_string)
                            && contains(spec.span, offset)
                        {
                            let target = match source {
                                Some(s) => module_path(qe, s, &sf.file_name)?,
                                None => sf.file_name.clone(),
                            };
                            return export_target(qe, &target, Some(&spec.local), &mut vec![]);
                        }
                    }
                }
            }
            _ => {}
        }
    }
    None
}

fn ambient_modules(qe: &QueryEngine, spec: &str) -> Vec<Target> {
    let mut candidates = vec![];
    let mut files = qe.source_file_names();
    files.sort();
    for file in files {
        let Some(source) = qe.get_source_file(&file) else {
            continue;
        };
        // Parsing declarations also avoids matches in comments and string values.
        if !source.contains("module") {
            continue;
        }
        let sf = tsc_rs_parser::parse(&file, source);
        for stmt in &sf.statements {
            let StmtKind::ModuleDecl(m) = &stmt.kind else {
                continue;
            };
            let ModuleName::String(pattern) = &m.name else {
                continue;
            };
            let Some(span) = m.name_span else {
                continue;
            };
            let score = if pattern == spec {
                usize::MAX
            } else if let Some((prefix, suffix)) = pattern.split_once('*') {
                if suffix.contains('*')
                    || spec.len() < prefix.len() + suffix.len()
                    || !spec.starts_with(prefix)
                    || !spec.ends_with(suffix)
                {
                    continue;
                }
                prefix.len()
            } else {
                continue;
            };
            candidates.push((score, file.clone(), span));
        }
    }
    let best = candidates.iter().map(|(score, _, _)| *score).max();
    candidates
        .into_iter()
        .filter(|(score, _, _)| Some(*score) == best)
        .map(|(_, f, s)| (f, s))
        .collect()
}

fn normalize(path: &str) -> String {
    let mut parts = vec![];
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            _ => parts.push(part),
        }
    }
    format!(
        "{}{}",
        if path.starts_with('/') { "/" } else { "" },
        parts.join("/")
    )
}

fn reference_directive(
    qe: &QueryEngine,
    file: &str,
    source: &str,
    offset: u32,
) -> Option<Option<Target>> {
    let off = offset as usize;
    let before = source.get(..off)?;
    let start = before.rfind('\n').map_or(0, |p| p + 1);
    let end = source[off..].find('\n').map_or(source.len(), |p| off + p);
    let line = &source[start..end];
    if !line.trim_start().starts_with("///") || !line.contains("<reference ") {
        return None;
    }
    for attr in ["path", "types"] {
        let Some(at) = line.find(&format!("{attr}=")) else {
            continue;
        };
        let value_start = at + attr.len() + 1;
        let quote = *line.as_bytes().get(value_start)?;
        if !matches!(quote, b'\'' | b'"') {
            continue;
        }
        let value_end = value_start + 1 + line[value_start + 1..].find(quote as char)?;
        if off < start + value_start || off > start + value_end {
            continue;
        }
        let spec = &line[value_start + 1..value_end];
        let dir = file.rsplit_once('/').map_or("", |(d, _)| d);
        if attr == "path" {
            let path = normalize(&if spec.starts_with('/') {
                spec.to_string()
            } else {
                format!("{dir}/{spec}")
            });
            return Some(qe.get_source_file(&path).map(|_| (path, Span::new(0, 0))));
        }
        let mut dir = Some(dir);
        while let Some(base) = dir {
            let roots = qe
                .options()
                .type_roots
                .clone()
                .unwrap_or_else(|| vec!["node_modules/@types".into()]);
            for root in roots {
                let root = if root.starts_with('/') {
                    root
                } else {
                    format!("{base}/{root}")
                };
                for suffix in [format!("{spec}/index.d.ts"), format!("{spec}.d.ts")] {
                    let path = normalize(&format!("{root}/{suffix}"));
                    if qe.get_source_file(&path).is_some() {
                        return Some(Some((path, Span::new(0, 0))));
                    }
                }
            }
            dir = base.rsplit_once('/').map(|(d, _)| d);
        }
        return Some(None);
    }
    None
}

/// Deduplicated package identities share declarations, just as they share type
/// identity. Include the path within the package; equal names alone are not enough.
fn canonical_package(qe: &QueryEngine, target: String) -> String {
    let Some((folder, _)) = target.rsplit_once('/') else {
        return target;
    };
    let mut ancestor = Some(folder);
    while let Some(dir) = ancestor {
        let package = format!("{dir}/package.json");
        if let Some(json) = qe
            .get_source_file(&package)
            .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok())
        {
            if let (Some(name), Some(version)) = (json["name"].as_str(), json["version"].as_str()) {
                let suffix = &target[dir.len()..];
                let mut files = qe.source_file_names();
                files.sort();
                for file in files.iter().filter(|f| f.ends_with("/package.json")) {
                    let Some(other) = qe
                        .get_source_file(file)
                        .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok())
                    else {
                        continue;
                    };
                    if other["name"] == name
                        && other["version"] == version
                        && other["peerDependencies"] == json["peerDependencies"]
                    {
                        let candidate =
                            format!("{}{suffix}", file.trim_end_matches("/package.json"));
                        if qe.get_source_file(&candidate).is_some() {
                            return candidate;
                        }
                    }
                }
            }
            break;
        }
        ancestor = dir.rsplit_once('/').map(|(d, _)| d);
    }
    target
}

fn imported_binding(
    qe: &QueryEngine,
    sf: &SourceFile,
    index: &Index<'_>,
    name: &str,
    visited: &mut Vec<(String, String)>,
) -> Option<Target> {
    for stmt in &sf.statements {
        match &stmt.kind {
            StmtKind::Import(i) => {
                let remote = match &i.specifiers {
                    ImportClause::Require(n) if n == name => Some(None),
                    ImportClause::Named {
                        default,
                        named,
                        namespace,
                    } => {
                        if default.as_deref() == Some(name) {
                            Some(Some("default"))
                        }
                        // Named namespace imports navigate to their local alias;
                        // import-equals and require bindings navigate to exports.
                        else if namespace.as_deref() == Some(name) {
                            None
                        } else {
                            named
                                .iter()
                                .find(|s| s.local == name)
                                .map(|s| Some(s.imported.as_deref().unwrap_or(&s.local)))
                        }
                    }
                    _ => None,
                };
                if let Some(remote) = remote {
                    let target = canonical_package(qe, module_path(qe, &i.source, &sf.file_name)?);
                    return export_target(qe, &target, remote, visited);
                }
            }
            StmtKind::ImportEquals(i) if i.name == name => {
                let target = module_path(qe, require_spec(&i.module_ref)?, &sf.file_name)?;
                return export_target(qe, &target, None, visited);
            }
            _ => {}
        }
    }
    for var in &index.variables {
        let Some(spec) = var.init.as_deref().and_then(require_spec) else {
            continue;
        };
        let remote = match &var.name.kind {
            PatKind::Ident(n) if n == name => Some(None),
            PatKind::Object(props) => props.iter().find_map(|p| match p {
                ObjPatProp::Shorthand(n, _) if n == name => Some(Some(n.as_str())),
                ObjPatProp::KeyValue(k, p) if matches!(&p.kind, PatKind::Ident(n) if n == name) => {
                    property_name(k).map(Some)
                }
                _ => None,
            }),
            _ => None,
        };
        if let Some(remote) = remote {
            let target = module_path(qe, spec, &sf.file_name)?;
            return export_target(qe, &target, remote, visited);
        }
    }
    None
}

fn export_target(
    qe: &QueryEngine,
    file: &str,
    remote: Option<&str>,
    visited: &mut Vec<(String, String)>,
) -> Option<Target> {
    let key = (file.to_string(), remote.unwrap_or("*").to_string());
    if visited.contains(&key) || visited.len() > 32 {
        return None;
    }
    visited.push(key);
    let source = qe.get_source_file(file)?;
    let sf = tsc_rs_parser::parse(file, source);
    let index = Index::new(&sf);
    if let Some(name) = remote {
        for stmt in &sf.statements {
            if let StmtKind::Export(e) = &stmt.kind {
                if let ExportDeclKind::Named {
                    specifiers, source, ..
                } = &e.kind
                {
                    for spec in specifiers {
                        if spec.exported.as_deref().unwrap_or(&spec.local) == name {
                            let target = match source {
                                Some(s) => module_path(qe, s, file)?,
                                None => file.to_string(),
                            };
                            if target != file || spec.local != name {
                                return export_target(qe, &target, Some(&spec.local), visited);
                            }
                        }
                    }
                }
            }
        }
    }
    let mut local = remote.map(str::to_string);
    if remote == Some("default") {
        local = super::default_export_local_name(source);
    }
    if remote.is_none() {
        // `export = value` and `module.exports = value` export the value;
        // a module with named exports alone exports its namespace object.
        let value = sf.statements.iter().find_map(|s| match &s.kind {
            StmtKind::ExportAssign(e) => Some(e.as_ref()),
            StmtKind::Expr(e) => match &e.kind {
                ExprKind::Assign(a) if matches!(&a.left.kind, ExprKind::Member(m) if m.property == "exports" && matches!(&m.object.kind, ExprKind::Ident(n) if n == "module")) => Some(a.right.as_ref()),
                _ => None,
            },
            _ => None,
        });
        local = value.and_then(|e| match &e.kind {
            ExprKind::Ident(n) => Some(n.to_string()),
            ExprKind::FnExpr(f) => f.name.clone(),
            ExprKind::ClassExpr(c) => c.name.clone(),
            _ => None,
        });
        if local.is_none() {
            return Some((file.to_string(), Span::new(0, source.len() as u32)));
        }
    }
    let name = local?;
    if let Some(target) = imported_binding(qe, &sf, &index, &name, visited) {
        return Some(target);
    }
    let symbols = tsc_rs_symbols::bind(&sf);
    symbols
        .symbols
        .iter()
        .filter(|s| s.name == name && s.flags & tsc_rs_symbols::SYM_IMPORT == 0)
        .flat_map(|s| &s.declarations)
        .next()
        .map(|d| (file.to_string(), d.span))
}
