//! Scopes where a local binding shadows a top-level import or declaration.
//!
//! CommonJS emit rewrites references to imported names by NAME
//! (`gate` -> `guard_1.gate`) through `cjs_import_map`, which is flat. Without
//! scope information, a parameter, `const`, `catch` binding or nested function
//! that reuses an import's name would have its references rewritten to the
//! import, silently changing behavior (an auth guard `const gate = check();
//! if (gate instanceof Response) return gate;` read the import instead).
//!
//! The same holds for exported top-level bindings rewritten to `exports.x`.
//!
//! This pre-pass walks the file once and records, per top-level name, the
//! source ranges of every scope that declares a binding with that name. The
//! rewrite sites then skip any reference whose start offset falls inside one
//! of those ranges. Ranges are conservative in the right direction: a scope
//! range always covers every reference that resolves to its binding.

use super::*;

#[derive(Default, Clone)]
pub(crate) struct ImportShadows {
    ranges: HashMap<AstString, Vec<Scope>>,
    /// Top-level import bindings. `cjs_import_map` also carries synthetic
    /// entries (e.g. a decorated class's `Foo` -> `Foo_1` alias) that must
    /// not be checked against shadowing scopes.
    imports: HashSet<AstString>,
}

/// A scope's source range, minus the decorator expressions inside it: those
/// are evaluated outside the function they decorate.
#[derive(Clone, PartialEq)]
struct Scope {
    start: u32,
    end: u32,
    holes: Vec<(u32, u32)>,
}

impl Scope {
    fn contains(&self, pos: u32) -> bool {
        self.start <= pos && pos < self.end && !self.holes.iter().any(|&(s, e)| s <= pos && pos < e)
    }
}

impl ImportShadows {
    pub(crate) fn is_empty(&self) -> bool {
        self.ranges.is_empty()
    }

    /// True when the reference to `name` at `pos` resolves to a local binding
    /// rather than the top-level import.
    pub(crate) fn is_shadowed(&self, name: &str, pos: u32) -> bool {
        self.ranges
            .get(name)
            .is_some_and(|scopes| scopes.iter().any(|scope| scope.contains(pos)))
    }

    pub(crate) fn collect(stmts: &[Stmt]) -> Self {
        let mut names: HashSet<AstString> = HashSet::new();
        for stmt in stmts {
            match &stmt.kind {
                StmtKind::Import(decl) => match &decl.specifiers {
                    ImportClause::Named {
                        default,
                        named,
                        namespace,
                    } => {
                        names.extend(default.iter().map(|n| AstString::from(n.as_str())));
                        names.extend(namespace.iter().map(|n| AstString::from(n.as_str())));
                        names.extend(named.iter().map(|s| AstString::from(s.local.as_str())));
                    }
                    ImportClause::Require(name) => {
                        names.insert(AstString::from(name.as_str()));
                    }
                },
                StmtKind::ImportEquals(decl) => {
                    names.insert(AstString::from(decl.name.as_str()));
                }
                _ => {}
            }
        }
        let imports = names.clone();
        // Top-level declarations too: exported ones are rewritten to
        // `exports.x`, which a nested local of the same name must escape.
        let mut top = Vec::new();
        hoisted_var_names(stmts, &mut top);
        lexical_names(stmts, &mut top);
        names.extend(top.into_iter().map(AstString::from));
        let mut walker = Walker {
            names: &names,
            out: ImportShadows {
                imports,
                ..Default::default()
            },
        };
        if !names.is_empty() {
            // Top-level statements are the import's own scope: only nested
            // scopes can shadow it.
            for stmt in stmts {
                walker.stmt(stmt);
            }
        }
        walker.out
    }
}

impl Emitter<'_> {
    /// The `cjs_import_map` entry for a reference to `name` at source offset
    /// `pos`, or None when a local binding shadows the import there.
    pub(crate) fn cjs_import_ref(&self, name: &str, pos: u32) -> Option<(AstString, AstString)> {
        if self.import_shadows.imports.contains(name) && self.import_shadows.is_shadowed(name, pos)
        {
            return None;
        }
        self.cjs_import_map.get(name).cloned()
    }
}

struct Walker<'n> {
    names: &'n HashSet<AstString>,
    out: ImportShadows,
}

/// Collect every binding name introduced by a pattern.
fn pat_names<'p>(pat: &'p Pat, out: &mut Vec<&'p str>) {
    match &pat.kind {
        PatKind::Ident(name) => out.push(name.as_str()),
        PatKind::Array(elems) => {
            for elem in elems.iter().flatten() {
                match elem {
                    ArrayPatElem::Pat(p) | ArrayPatElem::Rest(p) => pat_names(p, out),
                }
            }
        }
        PatKind::Object(props) => {
            for prop in props {
                match prop {
                    ObjPatProp::KeyValue(_, p) | ObjPatProp::Rest(p) => pat_names(p, out),
                    ObjPatProp::Shorthand(name, _) | ObjPatProp::ShorthandAssign(name, _, _) => {
                        out.push(name.as_str())
                    }
                }
            }
        }
        PatKind::Assign(p, _) | PatKind::Rest(p) => pat_names(p, out),
    }
}

/// `var` declarations hoisted to the enclosing function, without entering
/// nested functions or classes.
fn hoisted_var_names<'s>(stmts: &'s [Stmt], out: &mut Vec<&'s str>) {
    for stmt in stmts {
        hoisted_var_names_stmt(stmt, out);
    }
}

fn hoisted_var_names_stmt<'s>(stmt: &'s Stmt, out: &mut Vec<&'s str>) {
    let var_decl = |v: &'s VarStmt, out: &mut Vec<&'s str>| {
        if matches!(v.kind, VarKind::Var) {
            for d in &v.declarations {
                pat_names(&d.name, out);
            }
        }
    };
    match &stmt.kind {
        StmtKind::Var(v) => var_decl(v, out),
        StmtKind::If(s) => {
            hoisted_var_names_stmt(&s.consequent, out);
            if let Some(alt) = &s.alternate {
                hoisted_var_names_stmt(alt, out);
            }
        }
        StmtKind::While(s) => hoisted_var_names_stmt(&s.body, out),
        StmtKind::DoWhile(s) => hoisted_var_names_stmt(&s.body, out),
        StmtKind::For(s) => {
            if let Some(ForInit::Var(v)) = &s.init {
                var_decl(v, out);
            }
            hoisted_var_names_stmt(&s.body, out);
        }
        StmtKind::ForIn(s) => {
            if let ForInOfLeft::Var(v) = &s.left {
                var_decl(v, out);
            }
            hoisted_var_names_stmt(&s.body, out);
        }
        StmtKind::ForOf(s) => {
            if let ForInOfLeft::Var(v) = &s.left {
                var_decl(v, out);
            }
            hoisted_var_names_stmt(&s.body, out);
        }
        StmtKind::Switch(s) => {
            for case in &s.cases {
                hoisted_var_names(&case.consequent, out);
            }
        }
        StmtKind::Try(s) => {
            hoisted_var_names(&s.block, out);
            if let Some(h) = &s.handler {
                hoisted_var_names(&h.body, out);
            }
            if let Some(f) = &s.finalizer {
                hoisted_var_names(f, out);
            }
        }
        StmtKind::Block(b) => hoisted_var_names(b, out),
        StmtKind::Labeled(s) => hoisted_var_names_stmt(&s.body, out),
        StmtKind::With(s) => hoisted_var_names_stmt(&s.body, out),
        StmtKind::Export(e) => {
            if let ExportDeclKind::Decl(inner) = &e.kind {
                hoisted_var_names_stmt(inner, out);
            }
        }
        _ => {}
    }
}

/// Block-scoped declarations directly in a statement list (`let`, `const`,
/// `using`, `class`, `function`, `enum`).
fn lexical_names<'s>(stmts: &'s [Stmt], out: &mut Vec<&'s str>) {
    for stmt in stmts {
        let stmt = match &stmt.kind {
            StmtKind::Export(e) => match &e.kind {
                ExportDeclKind::Decl(inner) | ExportDeclKind::DefaultDecl(inner) => inner,
                _ => continue,
            },
            _ => stmt,
        };
        match &stmt.kind {
            StmtKind::Var(v) if !matches!(v.kind, VarKind::Var) => {
                for d in &v.declarations {
                    pat_names(&d.name, out);
                }
            }
            StmtKind::FnDecl(f) => out.extend(f.name.as_deref()),
            StmtKind::ClassDecl(c) => out.extend(c.name.as_deref()),
            StmtKind::EnumDecl(e) => out.push(e.name.as_str()),
            _ => {}
        }
    }
}

fn stmts_range(stmts: &[Stmt]) -> Option<(u32, u32)> {
    Some((stmts.first()?.span.start, stmts.last()?.span.end))
}

impl Walker<'_> {
    fn shadow<'s>(&mut self, bound: impl IntoIterator<Item = &'s str>, range: (u32, u32)) {
        self.shadow_with_holes(bound, range, &[]);
    }

    fn shadow_with_holes<'s>(
        &mut self,
        bound: impl IntoIterator<Item = &'s str>,
        (start, end): (u32, u32),
        holes: &[(u32, u32)],
    ) {
        for name in bound {
            if self.names.contains(name) {
                let scope = Scope {
                    start,
                    end,
                    holes: holes.to_vec(),
                };
                let scopes = self.out.ranges.entry(AstString::from(name)).or_default();
                if !scopes.contains(&scope) {
                    scopes.push(scope);
                }
            }
        }
    }

    /// A function-like scope: params, hoisted `var`s and top-level lexical
    /// declarations of the body are all visible across `range`.
    fn function(&mut self, params: &[Param], body: Option<&[Stmt]>, range: (u32, u32)) {
        self.function_decorated(params, body, range, &[]);
    }

    /// `decorators` are the member's own decorators; together with parameter
    /// decorators they resolve in the enclosing scope.
    fn function_decorated(
        &mut self,
        params: &[Param],
        body: Option<&[Stmt]>,
        range: (u32, u32),
        decorators: &[Expr],
    ) {
        let mut bound = Vec::new();
        for p in params {
            pat_names(&p.name, &mut bound);
        }
        if let Some(body) = body {
            hoisted_var_names(body, &mut bound);
            lexical_names(body, &mut bound);
        }
        let holes: Vec<(u32, u32)> = decorators
            .iter()
            .chain(params.iter().flat_map(|p| p.decorators.iter()))
            .map(|d| (d.span.start, d.span.end))
            .collect();
        self.shadow_with_holes(bound, range, &holes);
        for p in params {
            self.param(p);
        }
        if let Some(body) = body {
            for s in body {
                self.stmt(s);
            }
        }
    }

    fn param(&mut self, p: &Param) {
        for d in &p.decorators {
            self.expr(d);
        }
        self.pat(&p.name);
        if let Some(init) = &p.initializer {
            self.expr(init);
        }
    }

    /// A non-function block: only its lexical declarations are scoped to it.
    fn block(&mut self, stmts: &[Stmt]) {
        if let Some(range) = stmts_range(stmts) {
            let mut bound = Vec::new();
            lexical_names(stmts, &mut bound);
            self.shadow(bound, range);
        }
        for s in stmts {
            self.stmt(s);
        }
    }

    fn var_stmt(&mut self, v: &VarStmt) {
        for d in &v.declarations {
            self.pat(&d.name);
            if let Some(init) = &d.init {
                self.expr(init);
            }
        }
    }

    fn stmt(&mut self, stmt: &Stmt) {
        match &stmt.kind {
            StmtKind::Var(v) => self.var_stmt(v),
            StmtKind::Expr(e) | StmtKind::Throw(e) | StmtKind::ExportAssign(e) => self.expr(e),
            StmtKind::Return(e) => {
                if let Some(e) = e {
                    self.expr(e);
                }
            }
            StmtKind::If(s) => {
                self.expr(&s.test);
                self.stmt(&s.consequent);
                if let Some(alt) = &s.alternate {
                    self.stmt(alt);
                }
            }
            StmtKind::While(s) => {
                self.expr(&s.test);
                self.stmt(&s.body);
            }
            StmtKind::DoWhile(s) => {
                self.stmt(&s.body);
                self.expr(&s.test);
            }
            StmtKind::For(s) => {
                if let Some(init) = &s.init {
                    match init {
                        ForInit::Var(v) => {
                            if !matches!(v.kind, VarKind::Var) {
                                let mut bound = Vec::new();
                                for d in &v.declarations {
                                    pat_names(&d.name, &mut bound);
                                }
                                self.shadow(bound, (stmt.span.start, stmt.span.end));
                            }
                            self.var_stmt(v);
                        }
                        ForInit::Expr(e) => self.expr(e),
                    }
                }
                if let Some(t) = &s.test {
                    self.expr(t);
                }
                if let Some(u) = &s.update {
                    self.expr(u);
                }
                self.stmt(&s.body);
            }
            StmtKind::ForIn(s) => {
                self.for_in_of_left(&s.left, stmt.span);
                self.expr(&s.right);
                self.stmt(&s.body);
            }
            StmtKind::ForOf(s) => {
                self.for_in_of_left(&s.left, stmt.span);
                self.expr(&s.right);
                self.stmt(&s.body);
            }
            StmtKind::Switch(s) => {
                self.expr(&s.discriminant);
                // All case clauses share one block scope.
                let mut bound = Vec::new();
                for case in &s.cases {
                    lexical_names(&case.consequent, &mut bound);
                }
                self.shadow(bound, (stmt.span.start, stmt.span.end));
                for case in &s.cases {
                    if let Some(t) = &case.test {
                        self.expr(t);
                    }
                    for c in &case.consequent {
                        self.stmt(c);
                    }
                }
            }
            StmtKind::Try(s) => {
                self.block(&s.block);
                if let Some(h) = &s.handler {
                    if let Some(p) = &h.param {
                        let mut bound = Vec::new();
                        pat_names(p, &mut bound);
                        self.shadow(bound, (h.span.start, h.span.end));
                        self.pat(p);
                    }
                    self.block(&h.body);
                }
                if let Some(f) = &s.finalizer {
                    self.block(f);
                }
            }
            StmtKind::Block(b) => self.block(b),
            StmtKind::FnDecl(f) => self.fn_decl(f),
            StmtKind::ClassDecl(c) => self.class(c),
            StmtKind::EnumDecl(e) => {
                for m in &e.members {
                    if let Some(init) = &m.initializer {
                        self.expr(init);
                    }
                }
            }
            StmtKind::ModuleDecl(m) => self.module(m),
            StmtKind::Export(e) => match &e.kind {
                ExportDeclKind::Decl(inner) | ExportDeclKind::DefaultDecl(inner) => {
                    self.stmt(inner)
                }
                ExportDeclKind::Default(expr) => self.expr(expr),
                _ => {}
            },
            StmtKind::Labeled(s) => self.stmt(&s.body),
            StmtKind::With(s) => {
                self.expr(&s.object);
                self.stmt(&s.body);
            }
            StmtKind::Break(_)
            | StmtKind::Continue(_)
            | StmtKind::Empty
            | StmtKind::Debugger
            | StmtKind::InterfaceDecl(_)
            | StmtKind::TypeAlias(_)
            | StmtKind::Import(_)
            | StmtKind::ImportEquals(_) => {}
        }
    }

    fn for_in_of_left(&mut self, left: &ForInOfLeft, span: Span) {
        match left {
            ForInOfLeft::Var(v) => {
                if !matches!(v.kind, VarKind::Var) {
                    let mut bound = Vec::new();
                    for d in &v.declarations {
                        pat_names(&d.name, &mut bound);
                    }
                    self.shadow(bound, (span.start, span.end));
                }
                self.var_stmt(v);
            }
            ForInOfLeft::Pat(p) => self.pat(p),
            ForInOfLeft::Expr(e) => self.expr(e),
        }
    }

    fn module(&mut self, m: &ModuleDecl) {
        match &m.body {
            // A namespace's own exported members are rewritten to `NS.x` by
            // the namespace emitter, which tracks them itself; only nested
            // scopes are recorded here.
            Some(ModuleBody::Block(stmts)) => {
                for s in stmts {
                    self.stmt(s);
                }
            }
            Some(ModuleBody::Module(inner)) => self.module(inner),
            None => {}
        }
    }

    fn fn_decl(&mut self, f: &FnDecl) {
        for d in &f.decorators {
            self.expr(d);
        }
        self.function_decorated(
            &f.params,
            f.body.as_deref(),
            (f.span.start, f.span.end),
            &f.decorators,
        );
    }

    fn class(&mut self, c: &ClassDecl) {
        for d in &c.decorators {
            self.expr(d);
        }
        if let Some(ext) = &c.extends {
            self.expr(ext);
        }
        for member in &c.members {
            let range = (member.span.start, member.span.end);
            match &member.kind {
                ClassMemberKind::Property(p) => {
                    for d in &p.decorators {
                        self.expr(d);
                    }
                    self.prop_name(&p.name);
                    if let Some(init) = &p.initializer {
                        self.expr(init);
                    }
                }
                ClassMemberKind::Method(m) => {
                    for d in &m.decorators {
                        self.expr(d);
                    }
                    self.prop_name(&m.name);
                    self.function_decorated(&m.params, m.body.as_deref(), range, &m.decorators);
                }
                ClassMemberKind::GetAccessor(a) | ClassMemberKind::SetAccessor(a) => {
                    for d in &a.decorators {
                        self.expr(d);
                    }
                    self.prop_name(&a.name);
                    self.function_decorated(&a.params, a.body.as_deref(), range, &a.decorators);
                }
                ClassMemberKind::Constructor(ctor) => {
                    for d in &ctor.decorators {
                        self.expr(d);
                    }
                    self.function_decorated(
                        &ctor.params,
                        ctor.body.as_deref(),
                        range,
                        &ctor.decorators,
                    );
                }
                ClassMemberKind::StaticBlock(body) => self.function(&[], Some(body), range),
                ClassMemberKind::IndexSignature(_) | ClassMemberKind::SemicolonClassElement => {}
            }
        }
    }

    fn prop_name(&mut self, name: &PropName) {
        if let PropName::Computed(e, _) = name {
            self.expr(e);
        }
    }

    fn pat(&mut self, pat: &Pat) {
        match &pat.kind {
            PatKind::Ident(_) => {}
            PatKind::Array(elems) => {
                for elem in elems.iter().flatten() {
                    match elem {
                        ArrayPatElem::Pat(p) | ArrayPatElem::Rest(p) => self.pat(p),
                    }
                }
            }
            PatKind::Object(props) => {
                for prop in props {
                    match prop {
                        ObjPatProp::KeyValue(key, p) => {
                            self.prop_name(key);
                            self.pat(p);
                        }
                        ObjPatProp::Rest(p) => self.pat(p),
                        ObjPatProp::ShorthandAssign(_, e, _) => self.expr(e),
                        ObjPatProp::Shorthand(..) => {}
                    }
                }
            }
            PatKind::Assign(p, e) => {
                self.pat(p);
                self.expr(e);
            }
            PatKind::Rest(p) => self.pat(p),
        }
    }

    fn expr(&mut self, expr: &Expr) {
        match &expr.kind {
            ExprKind::FnExpr(f) => {
                // A named function expression binds its own name inside itself.
                if let Some(name) = f.name.as_deref() {
                    self.shadow([name], (f.span.start, f.span.end));
                }
                self.fn_decl(f);
            }
            ExprKind::Arrow(a) => {
                let range = (a.span.start, a.span.end);
                match &a.body {
                    ArrowBody::Block(body) => self.function(&a.params, Some(body), range),
                    ArrowBody::Expr(body) => {
                        self.function(&a.params, None, range);
                        self.expr(body);
                    }
                }
            }
            ExprKind::ClassExpr(c) => {
                if let Some(name) = c.name.as_deref() {
                    self.shadow([name], (c.span.start, c.span.end));
                }
                self.class(c);
            }
            ExprKind::ObjectLit(props) => {
                for prop in props {
                    match prop {
                        ObjLitProp::Property(p) => {
                            self.prop_name(&p.key);
                            self.expr(&p.value);
                        }
                        ObjLitProp::ShorthandDefault(_, e, _) | ObjLitProp::Spread(e, _) => {
                            self.expr(e)
                        }
                        ObjLitProp::Method(m) => {
                            self.prop_name(&m.name);
                            self.function(&m.params, Some(&m.body), (m.span.start, m.span.end));
                        }
                        ObjLitProp::Get(a) | ObjLitProp::Set(a) => {
                            self.prop_name(&a.name);
                            self.function(&a.params, Some(&a.body), (a.span.start, a.span.end));
                        }
                        ObjLitProp::Shorthand(..) => {}
                    }
                }
            }
            ExprKind::Template(t) => {
                for e in &t.exprs {
                    self.expr(e);
                }
            }
            ExprKind::TaggedTemplate(t) => {
                self.expr(&t.tag);
                for e in &t.quasi.exprs {
                    self.expr(e);
                }
            }
            ExprKind::ArrayLit(elems) => {
                for e in elems.iter().flatten() {
                    self.expr(e);
                }
            }
            ExprKind::Call(c) => {
                self.expr(&c.callee);
                for a in &c.args {
                    self.expr(a);
                }
            }
            ExprKind::New(n) => {
                self.expr(&n.callee);
                for a in n.args.iter().flatten() {
                    self.expr(a);
                }
            }
            ExprKind::Member(m) => self.expr(&m.object),
            ExprKind::ElemAccess(a) => {
                self.expr(&a.object);
                self.expr(&a.index);
            }
            ExprKind::Cond(c) => {
                self.expr(&c.test);
                self.expr(&c.consequent);
                self.expr(&c.alternate);
            }
            ExprKind::Binary(b) => {
                self.expr(&b.left);
                self.expr(&b.right);
            }
            ExprKind::Assign(a) => {
                self.expr(&a.left);
                self.expr(&a.right);
            }
            ExprKind::Unary(u) => self.expr(&u.argument),
            ExprKind::Update(u) => self.expr(&u.argument),
            ExprKind::Paren(e)
            | ExprKind::NonNull(e)
            | ExprKind::Spread(e)
            | ExprKind::Await(e)
            | ExprKind::Delete(e)
            | ExprKind::Typeof(e)
            | ExprKind::Void(e) => self.expr(e),
            ExprKind::Yield(_, e) => {
                if let Some(e) = e {
                    self.expr(e);
                }
            }
            ExprKind::TypeAssertion(t) => self.expr(&t.expr),
            ExprKind::As(a) => self.expr(&a.expr),
            ExprKind::Satisfies(s) => self.expr(&s.expr),
            ExprKind::Instantiation(i) => self.expr(&i.expr),
            ExprKind::Comma(items) => {
                for e in items {
                    self.expr(e);
                }
            }
            ExprKind::JsxElement(el) => {
                self.jsx_attrs(&el.attributes);
                self.jsx_children(&el.children);
            }
            ExprKind::JsxSelfClosing(el) => self.jsx_attrs(&el.attributes),
            ExprKind::JsxFragment(f) => self.jsx_children(&f.children),
            _ => {}
        }
    }

    fn jsx_attrs(&mut self, attrs: &[JsxAttribute]) {
        for attr in attrs {
            match attr {
                JsxAttribute::Normal { value: Some(v), .. } => self.expr(v),
                JsxAttribute::Spread(v, _) => self.expr(v),
                _ => {}
            }
        }
    }

    fn jsx_children(&mut self, children: &[JsxChild]) {
        for child in children {
            match child {
                JsxChild::Element(e) => self.expr(e),
                JsxChild::Expression(Some(e), _) => self.expr(e),
                JsxChild::Fragment(f) => self.jsx_children(&f.children),
                _ => {}
            }
        }
    }
}
