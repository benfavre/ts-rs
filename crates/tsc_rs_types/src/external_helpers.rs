//! tsc's `checkExternalEmitHelpers` (TS2354 / TS2343) under `importHelpers`.
//!
//! Syntax that down-levels to an emit helper asks the helpers module
//! (`tslib`) for it. The walk records each request, with its location and
//! the helper bits tsc uses, in tsc's check order. The first request resolves
//! `tslib` for the file: when it cannot be found, TS2354 is reported there
//! once. Otherwise each helper missing from `tslib`'s exports is reported
//! (TS2343) at the first request for it in the whole program.
//!
//! Check order matters only for which location is "first". Statements are
//! checked in source order, function and arrow expressions are deferred
//! (tsc's `checkNodeDeferred`), and function declarations are checked lazily
//! (`addLazyDiagnostic`) after everything else.

use tsc_rs_ast::*;

use crate::TypeChecker;

const EXTENDS: u32 = 1;
const ASSIGN: u32 = 2;
const REST: u32 = 4;
const DECORATE: u32 = 8;
const METADATA: u32 = 16;
const PARAM: u32 = 32;
const AWAITER: u32 = 64;
const GENERATOR: u32 = 128;
const VALUES: u32 = 256;
const READ: u32 = 512;
const SPREAD_ARRAY: u32 = 1024;
const AWAIT: u32 = 2048;
const ASYNC_GENERATOR: u32 = 4096;
const ASYNC_DELEGATOR: u32 = 8192;
const ASYNC_VALUES: u32 = 16384;
const EXPORT_STAR: u32 = 32768;
const IMPORT_STAR: u32 = 65536;
const IMPORT_DEFAULT: u32 = 131072;
const MAKE_TEMPLATE_OBJECT: u32 = 262144;
const PRIVATE_FIELD_GET: u32 = 524288;
const PRIVATE_FIELD_SET: u32 = 1048576;
const PRIVATE_FIELD_IN: u32 = 2097152;
const SET_FUNCTION_NAME: u32 = 4194304;
const PROP_KEY: u32 = 8388608;
const DISPOSABLE: u32 = 16777216;
const LAST_HELPER: u32 = DISPOSABLE;

/// tsc's getHelperNames.
fn helper_names(helper: u32, legacy_decorators: bool) -> &'static [&'static str] {
    match helper {
        EXTENDS => &["__extends"],
        ASSIGN => &["__assign"],
        REST => &["__rest"],
        DECORATE if legacy_decorators => &["__decorate"],
        DECORATE => &["__esDecorate", "__runInitializers"],
        METADATA => &["__metadata"],
        PARAM => &["__param"],
        AWAITER => &["__awaiter"],
        GENERATOR => &["__generator"],
        VALUES => &["__values"],
        READ => &["__read"],
        SPREAD_ARRAY => &["__spreadArray"],
        AWAIT => &["__await"],
        ASYNC_GENERATOR => &["__asyncGenerator"],
        ASYNC_DELEGATOR => &["__asyncDelegator"],
        ASYNC_VALUES => &["__asyncValues"],
        EXPORT_STAR => &["__exportStar"],
        IMPORT_STAR => &["__importStar"],
        IMPORT_DEFAULT => &["__importDefault"],
        MAKE_TEMPLATE_OBJECT => &["__makeTemplateObject"],
        PRIVATE_FIELD_GET => &["__classPrivateFieldGet"],
        PRIVATE_FIELD_SET => &["__classPrivateFieldSet"],
        PRIVATE_FIELD_IN => &["__classPrivateFieldIn"],
        SET_FUNCTION_NAME => &["__setFunctionName"],
        PROP_KEY => &["__propKey"],
        DISPOSABLE => &["__addDisposableResource", "__disposeResources"],
        _ => &[],
    }
}

/// When a request is checked: statements in order, then deferred function
/// and arrow expressions, then lazily checked function declarations.
const IMMEDIATE: u8 = 0;
const DEFERRED: u8 = 1;
const LAZY: u8 = 2;

struct HelperWalk<'a> {
    source: &'a str,
    target: ScriptTarget,
    legacy_decorators: bool,
    decorator_metadata: bool,
    downlevel_iteration: bool,
    private_names_transformed: bool,
    module_helpers: bool,
    es_module_interop: bool,
    resolvable: &'a dyn Fn(&str) -> bool,
    requests: Vec<(u8, usize, Span, u32)>,
    phase: u8,
    ambient: u32,
    /// Whether the innermost non-arrow function is async (for `for await`).
    in_async: bool,
}

impl HelperWalk<'_> {
    fn request(&mut self, span: Span, helpers: u32) {
        if self.ambient == 0 && helpers != 0 {
            let order = self.requests.len();
            self.requests.push((self.phase, order, span, helpers));
        }
    }

    fn below(&self, minimum: ScriptTarget) -> bool {
        self.target < minimum
    }

    fn with_phase(&mut self, phase: u8, f: impl FnOnce(&mut Self)) {
        let saved = self.phase;
        self.phase = self.phase.max(phase);
        f(self);
        self.phase = saved;
    }

    /// The span of `text` ending right before `end` (skipping whitespace),
    /// e.g. the `...` before a rest element.
    fn token_before(&self, end: u32, text: &str) -> Option<u32> {
        let before = self.source.get(..end as usize)?.trim_end();
        before
            .ends_with(text)
            .then(|| (before.len() - text.len()) as u32)
    }

    fn word_at(&self, start: u32) -> Span {
        let rest = self.source.get(start as usize..).unwrap_or_default();
        let len = rest
            .find(|c: char| !(c.is_alphanumeric() || c == '_' || c == '$'))
            .unwrap_or(rest.len())
            .max(1);
        Span::new(start, start + len as u32)
    }

    // ------------------------------------------------------------------
    // Statements
    // ------------------------------------------------------------------

    fn statements(&mut self, statements: &[Stmt]) {
        for statement in statements {
            self.statement(statement);
        }
    }

    fn statement(&mut self, statement: &Stmt) {
        match &statement.kind {
            StmtKind::Var(var) => self.variable(var, statement.span),
            StmtKind::Expr(expr) | StmtKind::Throw(expr) | StmtKind::ExportAssign(expr) => {
                self.expr(expr)
            }
            StmtKind::Return(Some(expr)) => self.expr(expr),
            StmtKind::If(if_stmt) => {
                self.expr(&if_stmt.test);
                self.statement(&if_stmt.consequent);
                if let Some(alternate) = &if_stmt.alternate {
                    self.statement(alternate);
                }
            }
            StmtKind::While(w) => {
                self.expr(&w.test);
                self.statement(&w.body);
            }
            StmtKind::DoWhile(d) => {
                self.statement(&d.body);
                self.expr(&d.test);
            }
            StmtKind::For(f) => {
                match &f.init {
                    Some(ForInit::Var(var)) => self.variable(var, statement.span),
                    Some(ForInit::Expr(expr)) => self.expr(expr),
                    None => {}
                }
                if let Some(test) = &f.test {
                    self.expr(test);
                }
                if let Some(update) = &f.update {
                    self.expr(update);
                }
                self.statement(&f.body);
            }
            StmtKind::ForIn(f) => {
                self.for_left(&f.left, statement.span);
                self.expr(&f.right);
                self.statement(&f.body);
            }
            StmtKind::ForOf(f) => {
                if f.is_await {
                    if self.in_async && self.below(ScriptTarget::ES2018) {
                        self.request(statement.span, ASYNC_VALUES);
                    }
                } else if self.downlevel_iteration && self.below(ScriptTarget::ES2015) {
                    self.request(statement.span, VALUES);
                }
                self.for_left(&f.left, statement.span);
                self.expr(&f.right);
                self.statement(&f.body);
            }
            StmtKind::Switch(s) => {
                self.expr(&s.discriminant);
                for case in &s.cases {
                    if let Some(test) = &case.test {
                        self.expr(test);
                    }
                    self.statements(&case.consequent);
                }
            }
            StmtKind::Try(t) => {
                self.statements(&t.block);
                if let Some(handler) = &t.handler {
                    if let Some(param) = &handler.param {
                        self.binding_pattern(param, None);
                    }
                    self.statements(&handler.body);
                }
                if let Some(finalizer) = &t.finalizer {
                    self.statements(finalizer);
                }
            }
            StmtKind::Block(body) => self.statements(body),
            StmtKind::Labeled(l) => self.statement(&l.body),
            StmtKind::With(w) => {
                self.expr(&w.object);
                self.statement(&w.body);
            }
            StmtKind::FnDecl(function) => self.function_declaration(function),
            StmtKind::ClassDecl(class) => self.class(class, false, None),
            StmtKind::EnumDecl(e) => {
                let ambient = e.modifiers & MOD_DECLARE != 0;
                self.ambient += u32::from(ambient);
                for member in &e.members {
                    if let Some(init) = &member.initializer {
                        self.expr(init);
                    }
                }
                self.ambient -= u32::from(ambient);
            }
            StmtKind::ModuleDecl(module) => {
                let ambient = module.modifiers & MOD_DECLARE != 0
                    || matches!(module.name, ModuleName::String(_));
                self.ambient += u32::from(ambient);
                let mut body = module.body.as_ref();
                while let Some(ModuleBody::Module(inner)) = body {
                    body = inner.body.as_ref();
                }
                if let Some(ModuleBody::Block(statements)) = body {
                    self.statements(statements);
                }
                self.ambient -= u32::from(ambient);
            }
            StmtKind::Import(import) => self.import(import),
            StmtKind::Export(export) => self.export(export, statement.span),
            _ => {}
        }
    }

    fn import(&mut self, import: &ImportDecl) {
        if !self.module_helpers {
            return;
        }
        let ImportClause::Named {
            named, namespace, ..
        } = &import.specifiers
        else {
            return;
        };
        if namespace.is_some() {
            if self.es_module_interop {
                self.request(import.span, IMPORT_STAR);
            }
        } else if !named.is_empty() && (self.resolvable)(&import.source) {
            for specifier in named {
                let name = specifier.imported.as_deref().unwrap_or(&specifier.local);
                if name == "default" && self.es_module_interop {
                    self.request(specifier.span, IMPORT_DEFAULT);
                }
            }
        }
    }

    fn export(&mut self, export: &ExportDecl, span: Span) {
        match &export.kind {
            ExportDeclKind::Decl(declaration) => self.statement(declaration),
            ExportDeclKind::DefaultDecl(declaration) => match &declaration.kind {
                StmtKind::ClassDecl(class) => self.class(class, false, None),
                _ => self.statement(declaration),
            },
            ExportDeclKind::Default(expr) => {
                self.named_evaluation(expr, None);
                self.expr(expr);
            }
            ExportDeclKind::All { alias, .. } => {
                if self.module_helpers {
                    if alias.is_some() {
                        if self.es_module_interop {
                            self.request(span, IMPORT_STAR);
                        }
                    } else {
                        self.request(span, EXPORT_STAR);
                    }
                }
            }
            ExportDeclKind::Named {
                specifiers,
                source: Some(_),
                ..
            } => {
                if self.module_helpers && self.es_module_interop {
                    for specifier in specifiers {
                        if specifier.local == "default" {
                            self.request(specifier.span, IMPORT_DEFAULT);
                        }
                    }
                }
            }
            ExportDeclKind::Named { .. } => {}
        }
    }

    fn variable(&mut self, var: &VarStmt, span: Span) {
        let ambient = var.modifiers & MOD_DECLARE != 0;
        self.ambient += u32::from(ambient);
        if matches!(var.kind, VarKind::Using | VarKind::AwaitUsing)
            && self.below(ScriptTarget::ESNext)
        {
            let end = self
                .source
                .get(span.start as usize..span.end as usize)
                .map_or(span.end, |text| {
                    span.start + text.trim_end_matches(';').len() as u32
                });
            self.request(Span::new(span.start, end), DISPOSABLE);
        }
        for declaration in &var.declarations {
            self.binding_pattern(&declaration.name, None);
            if let Some(init) = &declaration.init {
                if matches!(declaration.name.kind, PatKind::Ident(_)) {
                    self.named_evaluation(init, None);
                }
                self.expr(init);
            }
        }
        self.ambient -= u32::from(ambient);
    }

    fn for_left(&mut self, left: &ForInOfLeft, span: Span) {
        match left {
            ForInOfLeft::Var(var) => self.variable(var, span),
            ForInOfLeft::Pat(pattern) => self.binding_pattern(pattern, None),
            ForInOfLeft::Expr(expr) => self.assignment_target(expr),
        }
    }

    // ------------------------------------------------------------------
    // Binding patterns
    // ------------------------------------------------------------------

    /// `declaration`: the span of the declaration owning an array pattern
    /// (tsc reports READ at the declaration's name, i.e. the pattern).
    fn binding_pattern(&mut self, pattern: &Pat, _declaration: Option<Span>) {
        match &pattern.kind {
            PatKind::Ident(_) => {}
            PatKind::Array(elements) => {
                if self.downlevel_iteration && self.below(ScriptTarget::ES2015) {
                    self.request(pattern.span, READ);
                }
                for element in elements.iter().flatten() {
                    match element {
                        ArrayPatElem::Pat(inner) | ArrayPatElem::Rest(inner) => {
                            self.binding_pattern(inner, None)
                        }
                    }
                }
            }
            PatKind::Object(properties) => {
                for property in properties {
                    match property {
                        ObjPatProp::KeyValue(key, value) => {
                            self.prop_name(key);
                            self.binding_pattern(value, None);
                        }
                        ObjPatProp::Rest(inner) => {
                            // tsc reports at the binding element's name.
                            if self.below(ScriptTarget::ES2018) {
                                self.request(inner.span, REST);
                            }
                            self.binding_pattern(inner, None);
                        }
                        ObjPatProp::ShorthandAssign(_, default, _) => {
                            self.named_evaluation(default, None);
                            self.expr(default);
                        }
                        ObjPatProp::Shorthand(..) => {}
                    }
                }
            }
            PatKind::Assign(inner, default) => {
                self.binding_pattern(inner, None);
                if matches!(inner.kind, PatKind::Ident(_)) {
                    self.named_evaluation(default, None);
                }
                self.expr(default);
            }
            PatKind::Rest(inner) => self.binding_pattern(inner, None),
        }
    }

    /// The left side of a destructuring assignment.
    fn assignment_target(&mut self, target: &Expr) {
        match &target.kind {
            ExprKind::ObjectLit(properties) => {
                for property in properties {
                    match property {
                        ObjLitProp::Spread(inner, span) => {
                            if self.below(ScriptTarget::ES2018) {
                                self.request(*span, REST);
                            }
                            self.assignment_target(inner);
                        }
                        ObjLitProp::Property(property) => {
                            self.prop_name(&property.key);
                            self.assignment_target(&property.value);
                        }
                        ObjLitProp::ShorthandDefault(_, default, _) => {
                            self.named_evaluation(default, None);
                            self.expr(default);
                        }
                        _ => {}
                    }
                }
            }
            ExprKind::ArrayLit(elements) => {
                if self.downlevel_iteration && self.below(ScriptTarget::ES2015) {
                    self.request(target.span, READ);
                }
                for element in elements.iter().flatten() {
                    match &element.kind {
                        ExprKind::Spread(inner) => self.assignment_target(inner),
                        _ => self.assignment_target(element),
                    }
                }
            }
            ExprKind::Assign(assign) if assign.op == AssignOp::Assign => {
                self.assignment_target(&assign.left);
                if matches!(assign.left.kind, ExprKind::Ident(_)) {
                    self.named_evaluation(&assign.right, None);
                }
                self.expr(&assign.right);
            }
            ExprKind::Member(member) if member.property.starts_with('#') => {
                self.expr(&member.object);
                if self.private_names_transformed {
                    self.request(target.span, PRIVATE_FIELD_SET);
                }
            }
            ExprKind::Paren(inner) => self.assignment_target(inner),
            _ => self.expr(target),
        }
    }

    // ------------------------------------------------------------------
    // Functions and classes
    // ------------------------------------------------------------------

    /// tsc's error span for a function-like node: its name, else its first
    /// token; an arrow spans to the end of its body's first line.
    fn function_span(&self, name: Option<Span>, span: Span, arrow: bool) -> Span {
        if let Some(name) = name {
            return name;
        }
        if arrow {
            let text = self
                .source
                .get(span.start as usize..span.end as usize)
                .unwrap_or_default();
            if let Some(arrow_at) = text.find("=>") {
                let after = &text[arrow_at + 2..];
                if after.trim_start().starts_with('{') {
                    let brace = arrow_at + 2 + (after.len() - after.trim_start().len());
                    if let Some(newline) = text[brace..].find('\n') {
                        let line = &text[..brace + newline];
                        return Span::new(
                            span.start,
                            span.start + line.trim_end_matches('\r').len() as u32,
                        );
                    }
                }
            }
            return span;
        }
        self.word_at(span.start)
    }

    fn signature(&mut self, is_async: bool, is_generator: bool, location: Span) {
        if is_async && is_generator && self.below(ScriptTarget::ES2018) {
            self.request(location, AWAIT | ASYNC_GENERATOR);
        }
        if is_async && !is_generator && self.below(ScriptTarget::ES2017) {
            self.request(location, AWAITER);
        }
        if (is_async || is_generator) && self.below(ScriptTarget::ES2015) {
            self.request(location, GENERATOR);
        }
    }

    fn params(&mut self, params: &[Param], decorate_params: bool) {
        for param in params {
            if decorate_params {
                if let Some(first) = param.decorators.first() {
                    self.request(first.span, DECORATE | PARAM);
                    if self.decorator_metadata {
                        self.request(first.span, METADATA);
                    }
                }
            }
            for decorator in &param.decorators {
                self.expr(decorator);
            }
            self.binding_pattern(&param.name, None);
            if let Some(init) = &param.initializer {
                if matches!(param.name.kind, PatKind::Ident(_)) {
                    self.named_evaluation(init, None);
                }
                self.expr(init);
            }
        }
    }

    fn body(&mut self, body: &[Stmt], is_async: bool) {
        let saved = self.in_async;
        self.in_async = is_async;
        self.statements(body);
        self.in_async = saved;
    }

    fn function_declaration(&mut self, function: &FnDecl) {
        let ambient = function.modifiers & MOD_DECLARE != 0 || function.body.is_none();
        self.ambient += u32::from(ambient);
        self.with_phase(LAZY, |walk| {
            let location = walk.function_span(function.name_span, function.span, false);
            walk.signature(function.is_async, function.is_generator, location);
            walk.params(&function.params, false);
            if let Some(body) = &function.body {
                walk.body(body, function.is_async);
            }
        });
        self.ambient -= u32::from(ambient);
    }

    fn function_expression(&mut self, function: &FnDecl) {
        self.with_phase(DEFERRED, |walk| {
            let location = walk.function_span(function.name_span, function.span, false);
            walk.signature(function.is_async, function.is_generator, location);
            walk.params(&function.params, false);
            if let Some(body) = &function.body {
                walk.body(body, function.is_async);
            }
        });
    }

    fn arrow(&mut self, arrow: &ArrowFn, span: Span) {
        self.with_phase(DEFERRED, |walk| {
            let location = walk.function_span(None, span, true);
            walk.signature(arrow.is_async, false, location);
            walk.params(&arrow.params, false);
            match &arrow.body {
                ArrowBody::Block(body) => {
                    let saved = walk.in_async;
                    walk.in_async = arrow.is_async || saved;
                    walk.statements(body);
                    walk.in_async = saved;
                }
                ArrowBody::Expr(expr) => walk.expr(expr),
            }
        });
    }

    /// tsc's getFirstTransformableStaticClassElement.
    fn first_transformable_static_element(&self, class: &ClassDecl) -> Option<Span> {
        let decorated_class = !self.legacy_decorators
            && self.below(ScriptTarget::ESNext)
            && (!class.decorators.is_empty()
                || class.members.iter().any(|member| match &member.kind {
                    ClassMemberKind::Constructor(ctor) => {
                        ctor.params.iter().any(|p| !p.decorators.is_empty())
                    }
                    _ => false,
                }));
        let transforms_statics = self.below(ScriptTarget::ESNext);
        if !decorated_class && !transforms_statics {
            return None;
        }
        for member in &class.members {
            if decorated_class && !member_decorators(member).is_empty() {
                return Some(class.decorators.first().map_or(class.span, |d| d.span));
            }
            if transforms_statics {
                match &member.kind {
                    ClassMemberKind::StaticBlock(_) => return Some(member.span),
                    _ if member_is_static(member) => {
                        return Some(member_name_span(member).unwrap_or(member.span));
                    }
                    _ => {}
                }
            }
        }
        None
    }

    fn class(&mut self, class: &ClassDecl, is_expression: bool, _name_key: Option<Span>) {
        let ambient = class.modifiers & MOD_DECLARE != 0;
        self.ambient += u32::from(ambient);
        // Class decorators.
        let class_decoratable = !is_expression || !self.legacy_decorators;
        if class_decoratable {
            if let Some(first) = class.decorators.first() {
                if self.legacy_decorators {
                    self.request(first.span, DECORATE);
                } else if self.below(ScriptTarget::ESNext) {
                    self.request(first.span, DECORATE);
                    if !is_expression
                        && (class.name.is_none()
                            || self.first_transformable_static_element(class).is_some())
                    {
                        self.request(first.span, SET_FUNCTION_NAME);
                    }
                }
                if self.decorator_metadata {
                    self.request(first.span, METADATA);
                }
            }
        }
        for decorator in &class.decorators {
            self.expr(decorator);
        }
        if let Some(extends) = &class.extends {
            if self.below(ScriptTarget::ES2015) {
                let start = self
                    .token_before(extends.span.start, "extends")
                    .unwrap_or(extends.span.start);
                let end = class
                    .extends_type_args
                    .as_ref()
                    .and_then(|args| args.last())
                    .map_or(extends.span.end, |_| {
                        let text = self
                            .source
                            .get(extends.span.end as usize..)
                            .unwrap_or_default();
                        extends.span.end + text.find('>').map_or(0, |at| at as u32 + 1)
                    });
                self.request(Span::new(start, end), EXTENDS);
            }
            self.expr(extends);
        }
        for member in &class.members {
            self.class_member(class, member, is_expression);
        }
        self.ambient -= u32::from(ambient);
    }

    fn member_decorators(&mut self, class: &ClassDecl, member: &ClassMember, is_expression: bool) {
        let decorators = member_decorators(member);
        let Some(first) = decorators.first() else {
            return;
        };
        let private = matches!(member_name(member), Some(PropName::Private(..)));
        let decoratable = match &member.kind {
            ClassMemberKind::Property(property) => {
                if self.legacy_decorators {
                    !is_expression && !private
                } else {
                    property.modifiers & (MOD_ABSTRACT | MOD_DECLARE) == 0
                }
            }
            ClassMemberKind::Method(method) => {
                method.body.is_some() && !(self.legacy_decorators && (is_expression || private))
            }
            ClassMemberKind::GetAccessor(accessor) | ClassMemberKind::SetAccessor(accessor) => {
                accessor.body.is_some() && !(self.legacy_decorators && (is_expression || private))
            }
            _ => false,
        };
        let _ = class;
        if !decoratable {
            return;
        }
        let span = first.span;
        if self.legacy_decorators {
            self.request(span, DECORATE);
        } else if self.below(ScriptTarget::ESNext) {
            self.request(span, DECORATE);
            let method_like = matches!(
                member.kind,
                ClassMemberKind::Method(_)
                    | ClassMemberKind::GetAccessor(_)
                    | ClassMemberKind::SetAccessor(_)
            ) || matches!(&member.kind, ClassMemberKind::Property(p) if p.modifiers & MOD_ACCESSOR != 0);
            if private && method_like {
                self.request(span, SET_FUNCTION_NAME);
            }
            if matches!(member_name(member), Some(PropName::Computed(..))) {
                self.request(span, PROP_KEY);
            }
        }
        if self.decorator_metadata {
            self.request(span, METADATA);
        }
    }

    fn class_member(&mut self, class: &ClassDecl, member: &ClassMember, is_expression: bool) {
        match &member.kind {
            ClassMemberKind::Property(property) => {
                self.member_decorators(class, member, is_expression);
                for decorator in &property.decorators {
                    self.expr(decorator);
                }
                self.prop_name(&property.name);
                if let Some(init) = &property.initializer {
                    self.named_evaluation(init, Some(&property.name));
                    self.expr(init);
                }
            }
            ClassMemberKind::Method(method) => {
                let ambient = method.body.is_none();
                self.ambient += u32::from(ambient);
                self.member_decorators(class, member, is_expression);
                for decorator in &method.decorators {
                    self.expr(decorator);
                }
                self.prop_name(&method.name);
                self.signature(method.is_async, method.is_generator, method.name.span());
                let decorate_params =
                    self.legacy_decorators && !is_expression && method.body.is_some();
                self.params(&method.params, decorate_params);
                if let Some(body) = &method.body {
                    self.body(body, method.is_async);
                }
                self.ambient -= u32::from(ambient);
            }
            ClassMemberKind::GetAccessor(accessor) | ClassMemberKind::SetAccessor(accessor) => {
                let ambient = accessor.body.is_none();
                self.ambient += u32::from(ambient);
                self.member_decorators(class, member, is_expression);
                for decorator in &accessor.decorators {
                    self.expr(decorator);
                }
                self.prop_name(&accessor.name);
                let decorate_params = self.legacy_decorators
                    && !is_expression
                    && accessor.body.is_some()
                    && matches!(member.kind, ClassMemberKind::SetAccessor(_));
                self.params(&accessor.params, decorate_params);
                if let Some(body) = &accessor.body {
                    self.body(body, false);
                }
                self.ambient -= u32::from(ambient);
            }
            ClassMemberKind::Constructor(ctor) => {
                let ambient = ctor.body.is_none();
                self.ambient += u32::from(ambient);
                let decorate_params =
                    self.legacy_decorators && !is_expression && ctor.body.is_some();
                self.params(&ctor.params, decorate_params);
                if let Some(body) = &ctor.body {
                    self.body(body, false);
                }
                self.ambient -= u32::from(ambient);
            }
            ClassMemberKind::StaticBlock(body) => self.body(body, false),
            _ => {}
        }
    }

    /// tsc's checkClassExpressionExternalHelpers: an anonymous class that
    /// takes its name from the surrounding binding needs `__setFunctionName`
    /// when its statics are transformed.
    fn named_evaluation(&mut self, value: &Expr, computed_key: Option<&PropName>) {
        let mut inner = value;
        loop {
            match &inner.kind {
                ExprKind::Paren(e) | ExprKind::NonNull(e) => inner = e,
                ExprKind::As(a) => inner = &a.expr,
                ExprKind::Satisfies(s) => inner = &s.expr,
                ExprKind::TypeAssertion(t) => inner = &t.expr,
                _ => break,
            }
        }
        let ExprKind::ClassExpr(class) = &inner.kind else {
            return;
        };
        if class.name.is_some() {
            return;
        }
        let decorated = !self.legacy_decorators
            && self.below(ScriptTarget::ESNext)
            && !class.decorators.is_empty();
        let location = if decorated {
            class.decorators.first().map(|d| d.span)
        } else {
            self.first_transformable_static_element(class)
        };
        if let Some(location) = location {
            self.request(location, SET_FUNCTION_NAME);
            if matches!(computed_key, Some(PropName::Computed(..))) {
                self.request(location, PROP_KEY);
            }
        }
    }

    fn prop_name(&mut self, name: &PropName) {
        if let PropName::Computed(expr, _) = name {
            self.expr(expr);
        }
    }

    // ------------------------------------------------------------------
    // Expressions
    // ------------------------------------------------------------------

    fn spread_element(&mut self, span: Span) {
        if self.below(ScriptTarget::ES2015) {
            let helpers = if self.downlevel_iteration {
                SPREAD_ARRAY | READ
            } else {
                SPREAD_ARRAY
            };
            self.request(span, helpers);
        }
    }

    fn private_access(&mut self, span: Span, read: bool, write: bool) {
        if !self.private_names_transformed {
            return;
        }
        if write {
            self.request(span, PRIVATE_FIELD_SET);
        }
        if read {
            self.request(span, PRIVATE_FIELD_GET);
        }
    }

    fn expr(&mut self, expr: &Expr) {
        match &expr.kind {
            ExprKind::Member(member) => {
                self.expr(&member.object);
                if member.property.starts_with('#') {
                    self.private_access(expr.span, true, false);
                }
            }
            ExprKind::Assign(assign) => {
                match &assign.left.kind {
                    ExprKind::Member(member) if member.property.starts_with('#') => {
                        self.expr(&member.object);
                        let compound = assign.op != AssignOp::Assign;
                        self.private_access(assign.left.span, compound, true);
                    }
                    ExprKind::ObjectLit(_) | ExprKind::ArrayLit(_)
                        if assign.op == AssignOp::Assign =>
                    {
                        self.assignment_target(&assign.left)
                    }
                    _ => self.expr(&assign.left),
                }
                if matches!(
                    assign.op,
                    AssignOp::Assign
                        | AssignOp::LogAndAssign
                        | AssignOp::LogOrAssign
                        | AssignOp::NullCoalAssign
                ) && matches!(assign.left.kind, ExprKind::Ident(_))
                {
                    self.named_evaluation(&assign.right, None);
                }
                self.expr(&assign.right);
            }
            ExprKind::Update(update) => match &update.argument.kind {
                ExprKind::Member(member) if member.property.starts_with('#') => {
                    self.expr(&member.object);
                    self.private_access(update.argument.span, true, true);
                }
                _ => self.expr(&update.argument),
            },
            ExprKind::Binary(binary) => {
                if binary.op == BinaryOp::In
                    && matches!(&binary.left.kind, ExprKind::Ident(name) if name.starts_with('#'))
                {
                    if self.private_names_transformed {
                        self.request(binary.left.span, PRIVATE_FIELD_IN);
                    }
                } else {
                    self.expr(&binary.left);
                }
                self.expr(&binary.right);
            }
            ExprKind::ObjectLit(properties) => {
                for property in properties {
                    match property {
                        ObjLitProp::Property(property) => {
                            self.prop_name(&property.key);
                            self.named_evaluation(&property.value, Some(&property.key));
                            self.expr(&property.value);
                        }
                        ObjLitProp::Spread(inner, span) => {
                            if self.below(ScriptTarget::ES2015) {
                                self.request(*span, ASSIGN);
                            }
                            self.expr(inner);
                        }
                        ObjLitProp::ShorthandDefault(_, value, _) => self.expr(value),
                        ObjLitProp::Method(method) => {
                            self.prop_name(&method.name);
                            let (is_async, is_generator, name) =
                                (method.is_async, method.is_generator, method.name.span());
                            self.with_phase(DEFERRED, |walk| {
                                walk.signature(is_async, is_generator, name);
                                walk.params(&method.params, false);
                                walk.body(&method.body, is_async);
                            });
                        }
                        ObjLitProp::Get(accessor) | ObjLitProp::Set(accessor) => {
                            self.prop_name(&accessor.name);
                            self.with_phase(DEFERRED, |walk| {
                                walk.params(&accessor.params, false);
                                walk.body(&accessor.body, false);
                            });
                        }
                        ObjLitProp::Shorthand(..) => {}
                    }
                }
            }
            ExprKind::ArrayLit(elements) => {
                for element in elements.iter().flatten() {
                    self.expr(element);
                }
            }
            ExprKind::Spread(inner) => {
                self.spread_element(expr.span);
                self.expr(inner);
            }
            ExprKind::FnExpr(function) => self.function_expression(function),
            ExprKind::Arrow(arrow) => self.arrow(arrow, expr.span),
            ExprKind::ClassExpr(class) => self.class(class, true, None),
            ExprKind::Call(call) => {
                self.expr(&call.callee);
                for argument in &call.args {
                    self.expr(argument);
                }
            }
            ExprKind::New(new) => {
                self.expr(&new.callee);
                for argument in new.args.iter().flatten() {
                    self.expr(argument);
                }
            }
            ExprKind::TaggedTemplate(tagged) => {
                if self.below(ScriptTarget::ES2015) {
                    self.request(expr.span, MAKE_TEMPLATE_OBJECT);
                }
                self.expr(&tagged.tag);
                for part in &tagged.quasi.exprs {
                    self.expr(part);
                }
            }
            ExprKind::Template(template) => {
                for part in &template.exprs {
                    self.expr(part);
                }
            }
            ExprKind::Yield(delegate, argument) => {
                if *delegate {
                    let location = self.word_at(expr.span.start);
                    if self.in_async && self.below(ScriptTarget::ES2018) {
                        self.request(location, AWAIT | ASYNC_DELEGATOR | ASYNC_VALUES);
                    }
                    if !self.in_async
                        && self.below(ScriptTarget::ES2015)
                        && self.downlevel_iteration
                    {
                        self.request(location, VALUES);
                    }
                }
                if let Some(argument) = argument {
                    self.expr(argument);
                }
            }
            ExprKind::ElemAccess(access) => {
                self.expr(&access.object);
                self.expr(&access.index);
            }
            ExprKind::Cond(conditional) => {
                self.expr(&conditional.test);
                self.expr(&conditional.consequent);
                self.expr(&conditional.alternate);
            }
            ExprKind::Unary(unary) => self.expr(&unary.argument),
            ExprKind::TypeAssertion(assertion) => self.expr(&assertion.expr),
            ExprKind::As(assertion) => self.expr(&assertion.expr),
            ExprKind::Satisfies(assertion) => self.expr(&assertion.expr),
            ExprKind::Instantiation(instantiation) => self.expr(&instantiation.expr),
            ExprKind::Paren(inner)
            | ExprKind::NonNull(inner)
            | ExprKind::Await(inner)
            | ExprKind::Delete(inner)
            | ExprKind::Typeof(inner)
            | ExprKind::Void(inner) => self.expr(inner),
            ExprKind::Comma(parts) => {
                for part in parts {
                    self.expr(part);
                }
            }
            ExprKind::JsxElement(element) => {
                self.jsx_attributes(&element.attributes);
                self.jsx_children(&element.children);
            }
            ExprKind::JsxSelfClosing(element) => self.jsx_attributes(&element.attributes),
            ExprKind::JsxFragment(fragment) => self.jsx_children(&fragment.children),
            _ => {}
        }
    }

    fn jsx_attributes(&mut self, attributes: &[JsxAttribute]) {
        for attribute in attributes {
            match attribute {
                JsxAttribute::Normal {
                    value: Some(value), ..
                } => self.expr(value),
                JsxAttribute::Spread(value, _) => self.expr(value),
                _ => {}
            }
        }
    }

    fn jsx_children(&mut self, children: &[JsxChild]) {
        for child in children {
            match child {
                JsxChild::Element(element) => self.expr(element),
                JsxChild::Expression(Some(expr), _) => self.expr(expr),
                JsxChild::Fragment(fragment) => self.jsx_children(&fragment.children),
                _ => {}
            }
        }
    }
}

fn member_decorators(member: &ClassMember) -> &[Expr] {
    match &member.kind {
        ClassMemberKind::Property(p) => &p.decorators,
        ClassMemberKind::Method(m) => &m.decorators,
        ClassMemberKind::GetAccessor(a) | ClassMemberKind::SetAccessor(a) => &a.decorators,
        _ => &[],
    }
}

fn member_name(member: &ClassMember) -> Option<&PropName> {
    match &member.kind {
        ClassMemberKind::Property(p) => Some(&p.name),
        ClassMemberKind::Method(m) => Some(&m.name),
        ClassMemberKind::GetAccessor(a) | ClassMemberKind::SetAccessor(a) => Some(&a.name),
        _ => None,
    }
}

fn member_name_span(member: &ClassMember) -> Option<Span> {
    member_name(member).map(PropName::span)
}

fn member_is_static(member: &ClassMember) -> bool {
    let modifiers = match &member.kind {
        ClassMemberKind::Property(p) => p.modifiers,
        ClassMemberKind::Method(m) => m.modifiers,
        ClassMemberKind::GetAccessor(a) | ClassMemberKind::SetAccessor(a) => a.modifiers,
        _ => 0,
    };
    modifiers & MOD_STATIC != 0
}

impl TypeChecker {
    /// Record where `file`'s helpers module ('tslib') resolves: `None` when
    /// it cannot be found. Files without an entry are not checked.
    pub fn register_helpers_module(&mut self, file: &str, resolved: Option<String>) {
        std::sync::Arc::make_mut(&mut self.helpers_modules)
            .insert(Self::resolvable_specifier_key(file), resolved);
    }

    /// Export names of a helpers module with no file of its own (an ambient
    /// `declare module "tslib"`), under the key registered as its path.
    pub fn register_helpers_module_exports(&mut self, path: &str, names: &[String]) {
        std::sync::Arc::make_mut(&mut self.helpers_module_exports)
            .insert(path.to_string(), names.iter().cloned().collect());
    }

    pub(crate) fn check_external_emit_helpers(&mut self, file: &SourceFile) {
        if self.compiler_options.import_helpers != Some(true) || self.current_file_is_declaration()
        {
            return;
        }
        let Some(file_name) = self.current_file_name.clone() else {
            return;
        };
        let Some(helpers_module) = self
            .helpers_modules
            .get(&Self::resolvable_specifier_key(&file_name))
            .cloned()
        else {
            return;
        };
        let options = &self.compiler_options;
        let target = match options.target {
            None | Some(ScriptTarget::ES3) => ScriptTarget::ES2025,
            Some(target) => target,
        };
        let module_kind = self
            .current_file_module_format
            .or(options.module)
            .unwrap_or(if target == ScriptTarget::ESNext {
                ModuleKind::ESNext
            } else if target >= ScriptTarget::ES2022 {
                ModuleKind::ES2022
            } else if target >= ScriptTarget::ES2020 {
                ModuleKind::ES2020
            } else if target >= ScriptTarget::ES2015 {
                ModuleKind::ES2015
            } else {
                ModuleKind::CommonJS
            });
        let force_detection = matches!(
            options.module,
            Some(
                ModuleKind::Node16 | ModuleKind::Node18 | ModuleKind::Node20 | ModuleKind::NodeNext
            )
        );
        let is_module = force_detection
            || file.statements.iter().any(|statement| {
                matches!(
                    statement.kind,
                    StmtKind::Import(_)
                        | StmtKind::Export(_)
                        | StmtKind::ImportEquals(_)
                        | StmtKind::ExportAssign(_)
                )
            });
        if !is_module {
            return;
        }
        let use_define = options
            .use_define_for_class_fields
            .unwrap_or(target >= ScriptTarget::ES2022);
        let legacy_decorators = options.experimental_decorators == Some(true);
        let source = self.current_source.clone().unwrap_or_default();
        let resolvable = |specifier: &str| self.can_resolve_module(specifier);
        let mut walk = HelperWalk {
            source: &source,
            target,
            legacy_decorators,
            decorator_metadata: options.emit_decorator_metadata == Some(true),
            downlevel_iteration: options.down_level_iteration == Some(true),
            private_names_transformed: target < ScriptTarget::ESNext || !use_define,
            module_helpers: matches!(
                module_kind,
                ModuleKind::None | ModuleKind::CommonJS | ModuleKind::AMD | ModuleKind::UMD
            ),
            es_module_interop: options.es_module_interop != Some(false),
            resolvable: &resolvable,
            requests: Vec::new(),
            phase: IMMEDIATE,
            ambient: 0,
            in_async: false,
        };
        walk.statements(&file.statements);
        let mut requests = walk.requests;
        requests.sort_by_key(|(phase, order, _, _)| (*phase, *order));
        let Some(&(_, _, first_span, _)) = requests.first() else {
            return;
        };
        let Some(path) = helpers_module else {
            self.diagnostics.push(Diagnostic {
                code: 2354,
                message:
                    "This syntax requires an imported helper but module 'tslib' cannot be found."
                        .to_string(),
                category: DiagnosticCategory::Error,
                file_name: None,
                span: Some(first_span),
                related: None,
            });
            return;
        };
        let exports = self
            .helpers_module_exports
            .get(&path)
            .map(|names| names.iter().cloned().collect::<rustc_hash::FxHashSet<_>>())
            .or_else(|| self.lookup_module_known_exports(&path).cloned());
        let Some(exports) = exports else {
            return;
        };
        let mut requested = self
            .helpers_requested
            .lock()
            .expect("helpers_requested lock poisoned");
        let already = requested.entry(path).or_insert(0);
        let mut missing = Vec::new();
        for (_, _, span, helpers) in requests {
            let unchecked = helpers & !*already;
            let mut helper = 1;
            while helper <= LAST_HELPER {
                if unchecked & helper != 0 {
                    for name in helper_names(helper, legacy_decorators) {
                        if !exports.contains(*name) {
                            missing.push((span, *name));
                        }
                    }
                }
                helper <<= 1;
            }
            *already |= helpers;
        }
        drop(requested);
        // tsc sorts diagnostics at the same location by message text.
        missing.sort_by(|a, b| (a.0.start, a.0.end, a.1).cmp(&(b.0.start, b.0.end, b.1)));
        for (span, name) in missing {
            self.diagnostics.push(Diagnostic {
                code: 2343,
                message: format!(
                    "This syntax requires an imported helper named '{name}' which does not exist in 'tslib'. Consider upgrading your version of 'tslib'."
                ),
                category: DiagnosticCategory::Error,
                file_name: None,
                span: Some(span),
                related: None,
            });
        }
    }
}
