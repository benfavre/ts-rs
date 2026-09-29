//! Decorator scanning and emission for the emitter.

use super::*;

impl<'a> Emitter<'a> {
    pub(crate) fn scan_for_decorators(&mut self, file: &SourceFile) {
        for stmt in &file.statements {
            self.scan_stmt_for_decorators(stmt);
        }
    }

    pub(crate) fn scan_for_simple_standard_decorator_wrappers(&mut self, file: &SourceFile) {
        for stmt in &file.statements {
            self.scan_stmt_for_simple_standard_decorator_wrappers(stmt);
        }
    }

    pub(crate) fn scan_stmt_for_decorators(&mut self, stmt: &Stmt) {
        match &stmt.kind {
            StmtKind::ClassDecl(c) => {
                self.scan_class_for_decorators(c);
            }
            StmtKind::Export(e) => {
                if let ExportDeclKind::Decl(ref decl) | ExportDeclKind::DefaultDecl(ref decl) =
                    e.kind
                {
                    self.scan_stmt_for_decorators(decl);
                }
            }
            StmtKind::ModuleDecl(m) => {
                if let Some(ModuleBody::Block(body)) = &m.body {
                    for s in body {
                        self.scan_stmt_for_decorators(s);
                    }
                }
            }
            StmtKind::FnDecl(f) => {
                if let Some(body) = &f.body {
                    for s in body {
                        self.scan_stmt_for_decorators(s);
                    }
                }
            }
            StmtKind::Block(stmts) => {
                for s in stmts {
                    self.scan_stmt_for_decorators(s);
                }
            }
            StmtKind::If(if_stmt) => {
                self.scan_stmt_for_decorators(&if_stmt.consequent);
                if let Some(alt) = &if_stmt.alternate {
                    self.scan_stmt_for_decorators(alt);
                }
            }
            StmtKind::While(w) => self.scan_stmt_for_decorators(&w.body),
            StmtKind::DoWhile(w) => self.scan_stmt_for_decorators(&w.body),
            StmtKind::For(f) => self.scan_stmt_for_decorators(&f.body),
            StmtKind::ForIn(f) => self.scan_stmt_for_decorators(&f.body),
            StmtKind::ForOf(f) => self.scan_stmt_for_decorators(&f.body),
            StmtKind::Labeled(l) => self.scan_stmt_for_decorators(&l.body),
            StmtKind::With(w) => self.scan_stmt_for_decorators(&w.body),
            StmtKind::Switch(sw) => {
                for case in &sw.cases {
                    for s in &case.consequent {
                        self.scan_stmt_for_decorators(s);
                    }
                }
            }
            StmtKind::Try(t) => {
                for s in &t.block {
                    self.scan_stmt_for_decorators(s);
                }
                if let Some(catch) = &t.handler {
                    for s in &catch.body {
                        self.scan_stmt_for_decorators(s);
                    }
                }
                if let Some(finally) = &t.finalizer {
                    for s in finally {
                        self.scan_stmt_for_decorators(s);
                    }
                }
            }
            StmtKind::Expr(expr) => {
                self.scan_expr_for_decorated_classes(expr);
            }
            StmtKind::Var(var_stmt) => {
                for decl in &var_stmt.declarations {
                    if let Some(init) = &decl.init {
                        self.scan_expr_for_decorated_classes(init);
                    }
                }
            }
            StmtKind::Return(Some(expr)) | StmtKind::Throw(expr) => {
                self.scan_expr_for_decorated_classes(expr);
            }
            _ => {}
        }
    }

    fn scan_stmt_for_simple_standard_decorator_wrappers(&mut self, stmt: &Stmt) {
        match &stmt.kind {
            StmtKind::ClassDecl(class_decl) => {
                if class_can_emit_simple_standard_decorator_wrapper(class_decl)
                    || class_can_emit_narrow_standard_decorator_member_decl(class_decl)
                    || class_can_emit_narrow_standard_decorator_member_expr(class_decl)
                    || class_can_emit_method_only_decorator_wrapper(class_decl)
                    || (self.effective_target() >= ScriptTarget::ES2015
                        && self.can_emit_public_multi_method_decorator_wrapper(class_decl))
                    || class_native_standard_decorator_shape(class_decl).is_some()
                {
                    self.needs_es_decorate_helper = true;
                    self.needs_run_initializers_helper = true;
                    if class_can_emit_simple_standard_decorator_wrapper(class_decl)
                        && self.needs_downlevel("static-blocks")
                    {
                        self.needs_set_function_name_helper = true;
                    }
                    if let Some(shape) = class_native_standard_decorator_shape(class_decl) {
                        if shape.needs_prop_key_helper() {
                            self.needs_prop_key_helper = true;
                        }
                        if shape.needs_set_function_name_helper() {
                            self.needs_set_function_name_helper = true;
                        }
                    }
                    if class_has_decorated_methods(class_decl) {
                        self.has_es_decorated_methods = true;
                    }
                    if self.effective_target() >= ScriptTarget::ES2015
                        && self.can_emit_public_multi_method_decorator_wrapper(class_decl)
                        && class_method_decorator_wrapper_needs_prop_key(class_decl)
                    {
                        self.needs_prop_key_helper = true;
                    }
                }
                self.scan_class_for_simple_standard_decorator_wrappers(class_decl);
            }
            StmtKind::Export(e) => match &e.kind {
                ExportDeclKind::Decl(ref decl) | ExportDeclKind::DefaultDecl(ref decl) => {
                    self.scan_stmt_for_simple_standard_decorator_wrappers(decl);
                }
                ExportDeclKind::Default(ref expr) => {
                    self.scan_expr_for_simple_standard_decorator_wrappers(expr);
                }
                _ => {}
            },
            StmtKind::ModuleDecl(m) => {
                if let Some(ModuleBody::Block(body)) = &m.body {
                    for s in body {
                        self.scan_stmt_for_simple_standard_decorator_wrappers(s);
                    }
                }
            }
            StmtKind::FnDecl(f) => {
                // Scan parameter defaults for decorated class expressions
                for param in &f.params {
                    if let Some(init) = &param.initializer {
                        self.scan_expr_for_simple_standard_decorator_wrappers(init);
                    }
                }
                if let Some(body) = &f.body {
                    for s in body {
                        self.scan_stmt_for_simple_standard_decorator_wrappers(s);
                    }
                }
            }
            StmtKind::Block(stmts) => {
                for s in stmts {
                    self.scan_stmt_for_simple_standard_decorator_wrappers(s);
                }
            }
            StmtKind::If(if_stmt) => {
                self.scan_stmt_for_simple_standard_decorator_wrappers(&if_stmt.consequent);
                if let Some(alt) = &if_stmt.alternate {
                    self.scan_stmt_for_simple_standard_decorator_wrappers(alt);
                }
            }
            StmtKind::While(w) => self.scan_stmt_for_simple_standard_decorator_wrappers(&w.body),
            StmtKind::DoWhile(w) => self.scan_stmt_for_simple_standard_decorator_wrappers(&w.body),
            StmtKind::For(f) => self.scan_stmt_for_simple_standard_decorator_wrappers(&f.body),
            StmtKind::ForIn(f) => self.scan_stmt_for_simple_standard_decorator_wrappers(&f.body),
            StmtKind::ForOf(f) => self.scan_stmt_for_simple_standard_decorator_wrappers(&f.body),
            StmtKind::Labeled(l) => self.scan_stmt_for_simple_standard_decorator_wrappers(&l.body),
            StmtKind::With(w) => self.scan_stmt_for_simple_standard_decorator_wrappers(&w.body),
            StmtKind::Switch(sw) => {
                for case in &sw.cases {
                    for s in &case.consequent {
                        self.scan_stmt_for_simple_standard_decorator_wrappers(s);
                    }
                }
            }
            StmtKind::Try(t) => {
                for s in &t.block {
                    self.scan_stmt_for_simple_standard_decorator_wrappers(s);
                }
                if let Some(catch) = &t.handler {
                    for s in &catch.body {
                        self.scan_stmt_for_simple_standard_decorator_wrappers(s);
                    }
                }
                if let Some(finally) = &t.finalizer {
                    for s in finally {
                        self.scan_stmt_for_simple_standard_decorator_wrappers(s);
                    }
                }
            }
            StmtKind::Expr(expr) => self.scan_expr_for_simple_standard_decorator_wrappers(expr),
            StmtKind::Var(var_stmt) => {
                for decl in &var_stmt.declarations {
                    if let Some(init) = &decl.init {
                        self.scan_expr_for_simple_standard_decorator_wrappers(init);
                    }
                }
            }
            StmtKind::Return(Some(expr)) | StmtKind::Throw(expr) => {
                self.scan_expr_for_simple_standard_decorator_wrappers(expr);
            }
            _ => {}
        }
    }

    fn scan_expr_for_simple_standard_decorator_wrappers(&mut self, expr: &Expr) {
        match &expr.kind {
            ExprKind::ClassExpr(class_decl) => {
                if class_can_emit_narrow_standard_decorator_member_expr(class_decl)
                    || class_expr_can_emit_simple_standard_decorator_wrapper(class_decl)
                {
                    self.needs_es_decorate_helper = true;
                    self.needs_run_initializers_helper = true;
                    self.needs_set_function_name_helper = true;
                    if class_has_decorated_methods(class_decl) {
                        self.has_es_decorated_methods = true;
                    }
                }
                self.scan_class_for_simple_standard_decorator_wrappers(class_decl);
            }
            ExprKind::Paren(inner) => self.scan_expr_for_simple_standard_decorator_wrappers(inner),
            ExprKind::Comma(exprs) => {
                for expr in exprs {
                    self.scan_expr_for_simple_standard_decorator_wrappers(expr);
                }
            }
            ExprKind::Assign(assign) => {
                self.scan_expr_for_simple_standard_decorator_wrappers(&assign.left);
                self.scan_expr_for_simple_standard_decorator_wrappers(&assign.right);
            }
            ExprKind::Cond(cond) => {
                self.scan_expr_for_simple_standard_decorator_wrappers(&cond.consequent);
                self.scan_expr_for_simple_standard_decorator_wrappers(&cond.alternate);
            }
            ExprKind::ArrayLit(elements) => {
                for e in elements.iter().flatten() {
                    self.scan_expr_for_simple_standard_decorator_wrappers(e);
                }
            }
            ExprKind::ObjectLit(props) => {
                for p in props {
                    match p {
                        ObjLitProp::Property(kv) => {
                            if let PropName::Computed(expr, _) = &kv.key {
                                if Self::expr_needs_class_expr_binding_name(&kv.value)
                                    && !Self::computed_name_is_simple_literal(expr)
                                {
                                    self.needs_prop_key_helper = true;
                                }
                            }
                            self.scan_expr_for_simple_standard_decorator_wrappers(&kv.value);
                        }
                        ObjLitProp::ShorthandDefault(_, default_expr, _) => {
                            self.scan_expr_for_simple_standard_decorator_wrappers(default_expr);
                        }
                        ObjLitProp::Spread(inner, _) => {
                            self.scan_expr_for_simple_standard_decorator_wrappers(inner);
                        }
                        _ => {}
                    }
                }
            }
            ExprKind::Arrow(arrow) => {
                // Scan parameter defaults for decorated class expressions
                for param in &arrow.params {
                    if let Some(init) = &param.initializer {
                        self.scan_expr_for_simple_standard_decorator_wrappers(init);
                    }
                }
                match &arrow.body {
                    ArrowBody::Expr(expr) => {
                        self.scan_expr_for_simple_standard_decorator_wrappers(expr);
                    }
                    ArrowBody::Block(stmts) => {
                        for s in stmts {
                            self.scan_stmt_for_simple_standard_decorator_wrappers(s);
                        }
                    }
                }
            }
            ExprKind::FnExpr(f) => {
                // Scan parameter defaults for decorated class expressions
                for param in &f.params {
                    if let Some(init) = &param.initializer {
                        self.scan_expr_for_simple_standard_decorator_wrappers(init);
                    }
                }
                if let Some(body) = &f.body {
                    for s in body {
                        self.scan_stmt_for_simple_standard_decorator_wrappers(s);
                    }
                }
            }
            ExprKind::Call(call) => {
                self.scan_expr_for_simple_standard_decorator_wrappers(&call.callee);
                for arg in &call.args {
                    self.scan_expr_for_simple_standard_decorator_wrappers(arg);
                }
            }
            ExprKind::New(new_expr) => {
                self.scan_expr_for_simple_standard_decorator_wrappers(&new_expr.callee);
                if let Some(args) = &new_expr.args {
                    for arg in args {
                        self.scan_expr_for_simple_standard_decorator_wrappers(arg);
                    }
                }
            }
            _ => {}
        }
    }

    fn scan_class_for_simple_standard_decorator_wrappers(&mut self, class: &ClassDecl) {
        for decorator in &class.decorators {
            self.scan_expr_for_simple_standard_decorator_wrappers(decorator);
        }
        if let Some(extends) = &class.extends {
            self.scan_expr_for_simple_standard_decorator_wrappers(extends);
        }
        for member in &class.members {
            match &member.kind {
                ClassMemberKind::Property(prop) => {
                    for decorator in &prop.decorators {
                        self.scan_expr_for_simple_standard_decorator_wrappers(decorator);
                    }
                    if let PropName::Computed(expr, _) = &prop.name {
                        if prop.initializer.as_ref().is_some_and(|init| {
                            Self::expr_needs_class_expr_binding_name(init)
                                && !Self::computed_name_is_simple_literal(expr)
                        }) {
                            self.needs_prop_key_helper = true;
                        }
                        self.scan_expr_for_simple_standard_decorator_wrappers(expr);
                    }
                    if let Some(init) = &prop.initializer {
                        self.scan_expr_for_simple_standard_decorator_wrappers(init);
                    }
                }
                ClassMemberKind::Method(method) => {
                    for decorator in &method.decorators {
                        self.scan_expr_for_simple_standard_decorator_wrappers(decorator);
                    }
                    if let PropName::Computed(expr, _) = &method.name {
                        self.scan_expr_for_simple_standard_decorator_wrappers(expr);
                    }
                    for param in &method.params {
                        if let Some(init) = &param.initializer {
                            self.scan_expr_for_simple_standard_decorator_wrappers(init);
                        }
                    }
                    if let Some(body) = &method.body {
                        for stmt in body {
                            self.scan_stmt_for_simple_standard_decorator_wrappers(stmt);
                        }
                    }
                }
                ClassMemberKind::GetAccessor(acc) | ClassMemberKind::SetAccessor(acc) => {
                    for decorator in &acc.decorators {
                        self.scan_expr_for_simple_standard_decorator_wrappers(decorator);
                    }
                    if let PropName::Computed(expr, _) = &acc.name {
                        self.scan_expr_for_simple_standard_decorator_wrappers(expr);
                    }
                    for param in &acc.params {
                        if let Some(init) = &param.initializer {
                            self.scan_expr_for_simple_standard_decorator_wrappers(init);
                        }
                    }
                    if let Some(body) = &acc.body {
                        for stmt in body {
                            self.scan_stmt_for_simple_standard_decorator_wrappers(stmt);
                        }
                    }
                }
                ClassMemberKind::Constructor(ctor) => {
                    for param in &ctor.params {
                        if let Some(init) = &param.initializer {
                            self.scan_expr_for_simple_standard_decorator_wrappers(init);
                        }
                    }
                    if let Some(body) = &ctor.body {
                        for stmt in body {
                            self.scan_stmt_for_simple_standard_decorator_wrappers(stmt);
                        }
                    }
                }
                ClassMemberKind::StaticBlock(stmts) => {
                    for stmt in stmts {
                        self.scan_stmt_for_simple_standard_decorator_wrappers(stmt);
                    }
                }
                ClassMemberKind::IndexSignature(_) | ClassMemberKind::SemicolonClassElement => {}
            }
        }
    }

    fn scan_expr_for_decorated_classes(&mut self, expr: &Expr) {
        match &expr.kind {
            ExprKind::ClassExpr(c) => {
                // For class expressions, only scan MEMBER decorators.
                // Class-level decorators on expressions are dropped in experimental
                // mode (TypeScript doesn't support them) and handled separately
                // by the standard decorator IIFE wrapper.
                self.scan_class_members_for_decorators(c);
            }
            ExprKind::Paren(inner) => self.scan_expr_for_decorated_classes(inner),
            ExprKind::Comma(exprs) => {
                for e in exprs {
                    self.scan_expr_for_decorated_classes(e);
                }
            }
            ExprKind::Assign(a) => {
                self.scan_expr_for_decorated_classes(&a.right);
            }
            ExprKind::Cond(c) => {
                self.scan_expr_for_decorated_classes(&c.consequent);
                self.scan_expr_for_decorated_classes(&c.alternate);
            }
            _ => {}
        }
    }

    /// Scan only class members for experimental decorators (skip class-level).
    /// Used for class expressions where class-level decorators are either dropped
    /// (experimental mode) or handled by the standard IIFE wrapper.
    fn scan_class_members_for_decorators(&mut self, class: &ClassDecl) {
        let emit_metadata = self.options.emit_decorator_metadata == Some(true);
        let mut has_member_decorator = false;
        // Skip class-level decorator check — only scan members
        self.scan_class_member_decorators(class, &mut has_member_decorator);
        if emit_metadata && has_member_decorator {
            self.needs_metadata_helper = true;
        }
    }

    fn scan_class_member_decorators(&mut self, class: &ClassDecl, has_member_decorator: &mut bool) {
        for member in &class.members {
            match &member.kind {
                ClassMemberKind::Method(m) => {
                    if m.body.is_none() {
                        continue;
                    }
                    // Private names cannot be decorated with legacy decorators
                    if matches!(&m.name, PropName::Private(..)) {
                        // Still recurse into body for nested decorated classes
                        if let Some(body) = &m.body {
                            for s in body {
                                self.scan_stmt_for_decorators(s);
                            }
                        }
                        continue;
                    }
                    if !m.decorators.is_empty() {
                        self.needs_decorate_helper = true;
                        *has_member_decorator = true;
                    }
                    for param in &m.params {
                        if !param.decorators.is_empty() {
                            self.needs_decorate_helper = true;
                            self.needs_param_helper = true;
                            *has_member_decorator = true;
                        }
                    }
                    // Recurse into method bodies to find nested decorated classes
                    if let Some(body) = &m.body {
                        for s in body {
                            self.scan_stmt_for_decorators(s);
                        }
                    }
                }
                ClassMemberKind::Property(p) => {
                    if matches!(&p.name, PropName::Private(..)) {
                        continue;
                    }
                    if !p.decorators.is_empty() {
                        self.needs_decorate_helper = true;
                        *has_member_decorator = true;
                    }
                }
                ClassMemberKind::Constructor(ctor) => {
                    for param in &ctor.params {
                        if !param.decorators.is_empty() {
                            self.needs_decorate_helper = true;
                            self.needs_param_helper = true;
                            *has_member_decorator = true;
                        }
                    }
                    // Recurse into constructor body to find nested decorated classes
                    if let Some(body) = &ctor.body {
                        for s in body {
                            self.scan_stmt_for_decorators(s);
                        }
                    }
                }
                ClassMemberKind::GetAccessor(a) | ClassMemberKind::SetAccessor(a) => {
                    if matches!(&a.name, PropName::Private(..)) {
                        // Still recurse into body for nested decorated classes
                        if let Some(body) = &a.body {
                            for s in body {
                                self.scan_stmt_for_decorators(s);
                            }
                        }
                        continue;
                    }
                    if !a.decorators.is_empty() {
                        self.needs_decorate_helper = true;
                        *has_member_decorator = true;
                    }
                    for param in &a.params {
                        if !param.decorators.is_empty() {
                            self.needs_decorate_helper = true;
                            self.needs_param_helper = true;
                            *has_member_decorator = true;
                        }
                    }
                    // Recurse into accessor bodies to find nested decorated classes
                    if let Some(body) = &a.body {
                        for s in body {
                            self.scan_stmt_for_decorators(s);
                        }
                    }
                }
                _ => {}
            }
        }
    }

    pub(crate) fn scan_class_for_decorators(&mut self, class: &ClassDecl) {
        let emit_metadata = self.options.emit_decorator_metadata == Some(true);
        let mut has_member_decorator = false;
        if !class.decorators.is_empty() {
            self.needs_decorate_helper = true;
        }
        self.scan_class_member_decorators(class, &mut has_member_decorator);
        // __metadata is needed when:
        // 1. Member-level decorators exist (method/property/accessor/param), OR
        // 2. Class-level decorator + constructor has typed parameters
        let class_ctor_has_params = !class.decorators.is_empty()
            && class.members.iter().any(|m| {
                matches!(&m.kind, ClassMemberKind::Constructor(ctor) if !ctor.params.is_empty())
            });
        if emit_metadata && (has_member_decorator || class_ctor_has_params) {
            self.needs_metadata_helper = true;
        }

        // Check if the decorated class needs a self-reference alias.
        if class_needs_let_wrapper(class) && decorated_class_has_self_reference(class) {
            if let Some(ref name) = class.name {
                // Find the next available suffix: _1, _2, _3, ...
                let mut suffix = 1u32;
                loop {
                    let candidate = format!("{}_{}", name, suffix);
                    let candidate_ast = AstString::from(candidate.as_str());
                    if !self.hoisted_decorated_aliases.contains(&candidate_ast) {
                        self.hoisted_decorated_aliases.push(candidate_ast);
                        break;
                    }
                    suffix += 1;
                }
            }
        }
    }

    pub(crate) fn emit_decorate_helper(&mut self) {
        self.writeln("var __decorate = (this && this.__decorate) || function (decorators, target, key, desc) {");
        self.indent += 1;
        self.writeln("var c = arguments.length, r = c < 3 ? target : desc === null ? desc = Object.getOwnPropertyDescriptor(target, key) : desc, d;");
        self.writeln("if (typeof Reflect === \"object\" && typeof Reflect.decorate === \"function\") r = Reflect.decorate(decorators, target, key, desc);");
        self.writeln("else for (var i = decorators.length - 1; i >= 0; i--) if (d = decorators[i]) r = (c < 3 ? d(r) : c > 3 ? d(target, key, r) : d(target, key)) || r;");
        self.writeln("return c > 3 && r && Object.defineProperty(target, key, r), r;");
        self.indent -= 1;
        self.writeln("};");
    }

    pub(crate) fn emit_metadata_helper(&mut self) {
        self.writeln("var __metadata = (this && this.__metadata) || function (k, v) {");
        self.indent += 1;
        self.writeln("if (typeof Reflect === \"object\" && typeof Reflect.metadata === \"function\") return Reflect.metadata(k, v);");
        self.indent -= 1;
        self.writeln("};");
    }

    pub(crate) fn emit_param_helper(&mut self) {
        self.writeln("var __param = (this && this.__param) || function (paramIndex, decorator) {");
        self.indent += 1;
        self.writeln("return function (target, key) { decorator(target, key, paramIndex); }");
        self.indent -= 1;
        self.writeln("};");
    }

    pub(crate) fn emit_run_initializers_helper(&mut self) {
        self.writeln(
            "var __runInitializers = (this && this.__runInitializers) || function (thisArg, initializers, value) {",
        );
        self.indent += 1;
        self.writeln("var useValue = arguments.length > 2;");
        self.writeln("for (var i = 0; i < initializers.length; i++) {");
        self.indent += 1;
        self.writeln(
            "value = useValue ? initializers[i].call(thisArg, value) : initializers[i].call(thisArg);",
        );
        self.indent -= 1;
        self.writeln("}");
        self.writeln("return useValue ? value : void 0;");
        self.indent -= 1;
        self.writeln("};");
    }

    pub(crate) fn emit_es_decorate_helper(&mut self) {
        self.writeln(
            "var __esDecorate = (this && this.__esDecorate) || function (ctor, descriptorIn, decorators, contextIn, initializers, extraInitializers) {",
        );
        self.indent += 1;
        self.writeln(
            "function accept(f) { if (f !== void 0 && typeof f !== \"function\") throw new TypeError(\"Function expected\"); return f; }",
        );
        self.writeln(
            "var kind = contextIn.kind, key = kind === \"getter\" ? \"get\" : kind === \"setter\" ? \"set\" : \"value\";",
        );
        self.writeln(
            "var target = !descriptorIn && ctor ? contextIn[\"static\"] ? ctor : ctor.prototype : null;",
        );
        self.writeln(
            "var descriptor = descriptorIn || (target ? Object.getOwnPropertyDescriptor(target, contextIn.name) : {});",
        );
        self.writeln("var _, done = false;");
        self.writeln("for (var i = decorators.length - 1; i >= 0; i--) {");
        self.indent += 1;
        self.writeln("var context = {};");
        self.writeln("for (var p in contextIn) context[p] = p === \"access\" ? {} : contextIn[p];");
        self.writeln("for (var p in contextIn.access) context.access[p] = contextIn.access[p];");
        self.writeln(
            "context.addInitializer = function (f) { if (done) throw new TypeError(\"Cannot add initializers after decoration has completed\"); extraInitializers.push(accept(f || null)); };",
        );
        self.writeln(
            "var result = (0, decorators[i])(kind === \"accessor\" ? { get: descriptor.get, set: descriptor.set } : descriptor[key], context);",
        );
        self.writeln("if (kind === \"accessor\") {");
        self.indent += 1;
        self.writeln("if (result === void 0) continue;");
        self.writeln(
            "if (result === null || typeof result !== \"object\") throw new TypeError(\"Object expected\");",
        );
        self.writeln("if (_ = accept(result.get)) descriptor.get = _;");
        self.writeln("if (_ = accept(result.set)) descriptor.set = _;");
        self.writeln("if (_ = accept(result.init)) initializers.unshift(_);");
        self.indent -= 1;
        self.writeln("}");
        self.writeln("else if (_ = accept(result)) {");
        self.indent += 1;
        self.writeln("if (kind === \"field\") initializers.unshift(_);");
        self.writeln("else descriptor[key] = _;");
        self.indent -= 1;
        self.writeln("}");
        self.indent -= 1;
        self.writeln("}");
        self.writeln("if (target) Object.defineProperty(target, contextIn.name, descriptor);");
        self.writeln("done = true;");
        self.indent -= 1;
        self.writeln("};");
    }

    /// In `isolatedModules + noLib`, runtime constructors like `Map` may be
    /// unavailable. Emit a guarded metadata expression matching TypeScript's
    /// fallback style for this case.
    fn maybe_wrap_no_lib_metadata_ctor(&mut self, type_expr: String) -> String {
        if !self.options.isolated_modules.unwrap_or(false) || self.options.no_lib != Some(true) {
            return type_expr;
        }
        if type_expr == "Map" {
            let tmp = self.next_temp_var();
            return format!(
                "typeof ({tmp} = typeof Map !== \"undefined\" && Map) === \"function\" ? {tmp} : Object"
            );
        }
        type_expr
    }

    /// In CJS, metadata for qualified references off a default import
    /// (`Foo.Bar` where `Foo` is imported as default) needs a runtime guard.
    /// This also avoids collisions when a local value shadows the import name.
    fn maybe_wrap_cjs_default_member_metadata(
        &mut self,
        type_ann: &TypeNode,
        serialized: String,
        cjs_map: &HashMap<AstString, (AstString, AstString)>,
    ) -> String {
        fn unwrap_paren_type<'t>(ty: &'t TypeNode) -> &'t TypeNode {
            match &ty.kind {
                TypeNodeKind::Paren(inner) => unwrap_paren_type(inner),
                _ => ty,
            }
        }

        let ty = unwrap_paren_type(type_ann);
        let TypeNodeKind::Reference(type_ref) = &ty.kind else {
            return serialized;
        };

        let start = type_ref.name.span.start as usize;
        let end = type_ref.name.span.end as usize;
        if start >= end || end > self.source.len() {
            return serialized;
        }

        let raw_name = &self.source[start..end];
        let Some((root, member_path)) = raw_name.split_once('.') else {
            return serialized;
        };
        let Some((req_var, imported_name)) = cjs_map.get(root) else {
            return serialized;
        };
        if imported_name != "default" {
            return serialized;
        }

        let qualified = format!("{req_var}.default.{member_path}");
        if serialized != raw_name && serialized != qualified {
            return serialized;
        }

        let tmp = self.next_temp_var();
        format!(
            "typeof ({tmp} = typeof {req_var}.default !== \"undefined\" && {qualified}) === \"function\" ? {tmp} : Object"
        )
    }

    /// For unresolved qualified type metadata references (e.g. `A.B.C` where
    /// `A` has no local runtime binding), TypeScript emits a guarded expression
    /// that falls back to `Object`.
    fn maybe_wrap_unresolved_qualified_metadata(
        &mut self,
        type_ann: &TypeNode,
        serialized: String,
    ) -> String {
        fn unwrap_paren_type<'t>(ty: &'t TypeNode) -> &'t TypeNode {
            match &ty.kind {
                TypeNodeKind::Paren(inner) => unwrap_paren_type(inner),
                _ => ty,
            }
        }

        let ty = unwrap_paren_type(type_ann);
        let TypeNodeKind::Reference(type_ref) = &ty.kind else {
            return serialized;
        };

        let start = type_ref.name.span.start as usize;
        let end = type_ref.name.span.end as usize;
        if start >= end || end > self.source.len() {
            return serialized;
        }
        let raw_name = &self.source[start..end];
        if serialized != raw_name {
            return serialized;
        }

        let mut parts = raw_name.split('.').collect::<Vec<_>>();
        if parts.len() < 2 {
            return serialized;
        }

        let root = parts[0];
        if self.file_value_bound_names.contains(root) || self.cjs_import_map.contains_key(root) {
            return serialized;
        }

        let last = parts.pop().unwrap_or_default();
        let mut chain_expr = root.to_string();
        let mut guard = format!("typeof {root} !== \"undefined\"");
        for part in parts.iter().skip(1) {
            let tmp = self.next_temp_var();
            guard.push_str(&format!(" && ({tmp} = {chain_expr}.{part}) !== void 0"));
            chain_expr = tmp;
        }
        let terminal = format!("{chain_expr}.{last}");
        let out_tmp = self.next_temp_var();
        format!("typeof ({out_tmp} = {guard} && {terminal}) === \"function\" ? {out_tmp} : Object")
    }

    fn serialize_metadata_type(
        &mut self,
        type_ann: &TypeNode,
        metadata_type_only: &HashSet<AstString>,
        cjs_map: &HashMap<AstString, (AstString, AstString)>,
    ) -> String {
        let serialized = serialize_type_for_metadata_cjs(
            type_ann,
            self.source,
            metadata_type_only,
            &self.type_only_import_names,
            &self.global_type_only_names,
            cjs_map,
            &self.cjs_string_import_locals,
            &self.enum_decl_names,
            self.options
                .strict_null_checks
                .unwrap_or(self.options.strict.unwrap_or(false)),
        );
        let serialized = self.maybe_wrap_cjs_default_member_metadata(type_ann, serialized, cjs_map);
        self.maybe_wrap_unresolved_qualified_metadata(type_ann, serialized)
    }

    fn serialize_metadata_param_type(
        &mut self,
        param: &Param,
        metadata_type_only: &HashSet<AstString>,
        cjs_map: &HashMap<AstString, (AstString, AstString)>,
    ) -> String {
        let serialized = serialize_param_type_for_metadata_cjs(
            param,
            self.source,
            metadata_type_only,
            &self.type_only_import_names,
            &self.global_type_only_names,
            cjs_map,
            &self.cjs_string_import_locals,
            &self.enum_decl_names,
            self.options
                .strict_null_checks
                .unwrap_or(self.options.strict.unwrap_or(false)),
        );

        let maybe_ann = if param.dotdotdot {
            if let Some(type_ann) = &param.type_ann {
                if let TypeNodeKind::Array(inner) = &type_ann.kind {
                    Some(inner.as_ref())
                } else {
                    Some(type_ann)
                }
            } else {
                None
            }
        } else {
            param.type_ann.as_ref()
        };

        if let Some(type_ann) = maybe_ann {
            let serialized =
                self.maybe_wrap_cjs_default_member_metadata(type_ann, serialized, cjs_map);
            self.maybe_wrap_unresolved_qualified_metadata(type_ann, serialized)
        } else {
            serialized
        }
    }

    pub(crate) fn emit_decorator_applications(
        &mut self,
        class: &ClassDecl,
        cjs_export_target: Option<&str>,
    ) {
        let class_name = match &class.name {
            Some(name) => name.clone(),
            None => return,
        };
        let insert_in_legacy_iife =
            self.can_emit_legacy_es5_class_decl(class) && !class.decorators.is_empty();
        let application_output_start = self.output.len();
        let application_temp_start = self.temp_var_names.len();
        if insert_in_legacy_iife {
            self.indent += 1;
        }
        let emit_metadata = self.options.emit_decorator_metadata == Some(true);
        let is_static = |modifiers: ModifierFlags| modifiers & MOD_STATIC != 0;
        // In CJS mode, metadata type references must use the qualified name
        // (e.g. `observable_1.Observable` instead of bare `Observable`).
        let cjs_map = if self.is_commonjs() {
            self.cjs_import_map.clone()
        } else {
            HashMap::new()
        };
        // Collect class type parameter names -- these should serialize as Object
        // in metadata because they have no runtime value.
        let mut metadata_type_only = self.type_only_decl_names.clone();
        if let Some(ref type_params) = class.type_params {
            for tp in type_params {
                metadata_type_only.insert(AstString::from(tp.name.as_str()));
            }
        }

        // Reuse class key temps already allocated during class emission.
        let decorated_prop_computed_temps: HashMap<u32, String> = self
            .class_general_computed_temps
            .iter()
            .map(|(temp, start)| (*start, temp.clone()))
            .collect();

        // TypeScript emits instance (prototype) member decorators first,
        // then static member decorators. Two passes over the members.
        for pass in 0..2 {
            let want_static = pass == 1;
            for member in &class.members {
                match &member.kind {
                    ClassMemberKind::Method(method) => {
                        if is_static(method.modifiers) != want_static {
                            continue;
                        }
                        // Overload signatures (no body) are erased — skip decorators
                        if method.body.is_none() {
                            continue;
                        }
                        // Private names cannot be decorated with legacy decorators
                        if matches!(&method.name, PropName::Private(..)) {
                            continue;
                        }
                        let mut decorators = if method.is_async && self.needs_downlevel("async") {
                            collect_member_decorator_strings_await_to_yield(
                                self.source,
                                self.options,
                                &method.decorators,
                                &method.params,
                                self.helper_prefix(),
                                &cjs_map,
                                &self.cjs_string_import_locals,
                            )
                        } else {
                            collect_member_decorator_strings(
                                self.source,
                                self.options,
                                &method.decorators,
                                &method.params,
                                self.helper_prefix(),
                                &cjs_map,
                                &self.cjs_string_import_locals,
                            )
                        };
                        if decorators.is_empty() {
                            continue;
                        }
                        // Add metadata entries if emitDecoratorMetadata is on
                        if emit_metadata {
                            decorators.push(format!(
                                "{}__metadata(\"design:type\", Function)",
                                self.helper_prefix()
                            ));
                            let param_types: Vec<String> = method
                                .params
                                .iter()
                                .filter(|p| {
                                    if let PatKind::Ident(ref name) = p.name.kind {
                                        name != "this" && name != "<error>"
                                    } else {
                                        true
                                    }
                                })
                                .map(|p| {
                                    self.serialize_metadata_param_type(
                                        p,
                                        &metadata_type_only,
                                        &cjs_map,
                                    )
                                })
                                .collect();
                            decorators.push(format!(
                                "{}__metadata(\"design:paramtypes\", [{}])",
                                self.helper_prefix(),
                                param_types.join(", ")
                            ));
                            let return_type = method
                                .return_type
                                .as_ref()
                                .map(|t| {
                                    self.serialize_metadata_type(t, &metadata_type_only, &cjs_map)
                                })
                                .unwrap_or_else(|| {
                                    if method.is_async {
                                        "Promise".to_string()
                                    } else {
                                        "void 0".to_string()
                                    }
                                });
                            decorators.push(format!(
                                "{}__metadata(\"design:returntype\", {})",
                                self.helper_prefix(),
                                return_type
                            ));
                        }
                        let target = if is_static(method.modifiers) {
                            class_name.clone()
                        } else {
                            format!("{}.prototype", class_name)
                        };
                        let computed = prop_name_is_computed(&method.name);
                        let name = if computed {
                            if let PropName::Computed(expr, _) = &method.name {
                                if let Some(temp) =
                                    decorated_prop_computed_temps.get(&expr.span.start)
                                {
                                    temp.clone()
                                } else {
                                    prop_name_computed_str(&method.name, self.source, self.options)
                                }
                            } else {
                                prop_name_computed_str(&method.name, self.source, self.options)
                            }
                        } else {
                            prop_name_str(&method.name)
                        };
                        self.emit_decorate_call_inner(
                            &decorators,
                            &target,
                            &name,
                            computed,
                            "null",
                        );
                    }
                    ClassMemberKind::Property(prop) => {
                        if is_static(prop.modifiers) != want_static {
                            continue;
                        }
                        if prop.decorators.is_empty() {
                            continue;
                        }
                        // Private names cannot be decorated with legacy decorators
                        if matches!(&prop.name, PropName::Private(..)) {
                            continue;
                        }
                        let mut decorators: Vec<String> = prop
                            .decorators
                            .iter()
                            .map(|dec| {
                                emit_expr_to_string(
                                    self.source,
                                    self.options,
                                    dec,
                                    &cjs_map,
                                    &self.cjs_string_import_locals,
                                )
                            })
                            .collect();
                        // Add metadata entry if emitDecoratorMetadata is on
                        if emit_metadata {
                            let type_str = prop
                                .type_ann
                                .as_ref()
                                .map(|t| {
                                    self.serialize_metadata_type(t, &metadata_type_only, &cjs_map)
                                })
                                .unwrap_or_else(|| "Object".to_string());
                            let type_str = self.maybe_wrap_no_lib_metadata_ctor(type_str);
                            decorators.push(format!(
                                "{}__metadata(\"design:type\", {})",
                                self.helper_prefix(),
                                type_str
                            ));
                        }
                        let target = if is_static(prop.modifiers) {
                            class_name.clone()
                        } else {
                            format!("{}.prototype", class_name)
                        };
                        let computed = prop_name_is_computed(&prop.name);
                        let name = if computed {
                            if let PropName::Computed(expr, _) = &prop.name {
                                if let Some(temp) =
                                    decorated_prop_computed_temps.get(&expr.span.start)
                                {
                                    temp.clone()
                                } else {
                                    prop_name_computed_str(&prop.name, self.source, self.options)
                                }
                            } else {
                                prop_name_computed_str(&prop.name, self.source, self.options)
                            }
                        } else {
                            prop_name_str(&prop.name)
                        };
                        // Auto-accessor properties (with `accessor` keyword) use `null`
                        // like methods/accessors, not `void 0` like regular properties.
                        let desc = if prop.modifiers & MOD_ACCESSOR != 0 {
                            "null"
                        } else {
                            "void 0"
                        };
                        self.emit_decorate_call_inner(&decorators, &target, &name, computed, desc);
                    }
                    ClassMemberKind::GetAccessor(acc) | ClassMemberKind::SetAccessor(acc) => {
                        if is_static(acc.modifiers) != want_static {
                            continue;
                        }
                        // Private names cannot be decorated with legacy decorators
                        if matches!(&acc.name, PropName::Private(..)) {
                            continue;
                        }
                        // Find paired get/set accessor (decorators are merged into
                        // one __decorate call using the decorated accessor).
                        let acc_name_str = prop_name_str(&acc.name);
                        let is_getter = matches!(&member.kind, ClassMemberKind::GetAccessor(_));
                        // Find the paired accessor to get full type info
                        let paired_setter = if is_getter {
                            class.members.iter().find_map(|m| {
                                if let ClassMemberKind::SetAccessor(s) = &m.kind {
                                    if prop_name_str(&s.name) == acc_name_str {
                                        return Some(s);
                                    }
                                }
                                None
                            })
                        } else {
                            None
                        };
                        let paired_getter = if !is_getter {
                            class.members.iter().find_map(|m| {
                                if let ClassMemberKind::GetAccessor(g) = &m.kind {
                                    if prop_name_str(&g.name) == acc_name_str {
                                        return Some(g);
                                    }
                                }
                                None
                            })
                        } else {
                            None
                        };
                        // Only emit __decorate on the decorated accessor (not the paired one).
                        // When both get and set have decorators, only the first one
                        // (in source order) gets the __decorate call.
                        if acc.decorators.is_empty() {
                            continue;
                        }
                        {
                            let paired_also_decorated = if is_getter {
                                paired_setter.is_some_and(|s| !s.decorators.is_empty())
                            } else {
                                paired_getter.is_some_and(|g| !g.decorators.is_empty())
                            };
                            if paired_also_decorated {
                                // Find the paired member's span to check source order
                                let paired_member_span = class.members.iter().find_map(|m| {
                                    let is_match = if is_getter {
                                        matches!(&m.kind, ClassMemberKind::SetAccessor(s) if prop_name_str(&s.name) == acc_name_str)
                                    } else {
                                        matches!(&m.kind, ClassMemberKind::GetAccessor(g) if prop_name_str(&g.name) == acc_name_str)
                                    };
                                    is_match.then_some(m.span)
                                });
                                if let Some(paired_span) = paired_member_span {
                                    if paired_span.start < member.span.start {
                                        continue;
                                    }
                                }
                            }
                        }
                        let mut decorators: Vec<String> = acc
                            .decorators
                            .iter()
                            .map(|dec| {
                                emit_expr_to_string(
                                    self.source,
                                    self.options,
                                    dec,
                                    &cjs_map,
                                    &self.cjs_string_import_locals,
                                )
                            })
                            .collect();
                        // Include __param entries from the paired setter's parameters
                        let param_source_for_param_decs = if is_getter {
                            paired_setter.map(|s| &s.params[..])
                        } else {
                            Some(&acc.params[..])
                        };
                        if let Some(params) = param_source_for_param_decs {
                            let mut param_idx = 0usize;
                            for param in params.iter() {
                                let is_this = matches!(&param.name.kind, PatKind::Ident(name) if name == "this" || name == "<error>");
                                if is_this {
                                    continue;
                                }
                                for dec in &param.decorators {
                                    let dec_str = emit_expr_to_string(
                                        self.source,
                                        self.options,
                                        dec,
                                        &cjs_map,
                                        &self.cjs_string_import_locals,
                                    );
                                    decorators.push(format!(
                                        "{}__param({}, {})",
                                        self.helper_prefix(),
                                        param_idx,
                                        dec_str
                                    ));
                                }
                                param_idx += 1;
                            }
                        }
                        // Add metadata entries if emitDecoratorMetadata is on
                        if emit_metadata {
                            // design:type uses the getter return type, or if not
                            // available, the setter's first param type.
                            let design_type = if is_getter {
                                acc.return_type
                                    .as_ref()
                                    .map(|t| {
                                        self.serialize_metadata_type(
                                            t,
                                            &metadata_type_only,
                                            &cjs_map,
                                        )
                                    })
                                    .or_else(|| {
                                        // Getter with no return type: use paired setter's param type
                                        paired_setter
                                            .and_then(|s| s.params.first())
                                            .and_then(|p| p.type_ann.as_ref())
                                            .map(|t| {
                                                self.serialize_metadata_type(
                                                    t,
                                                    &metadata_type_only,
                                                    &cjs_map,
                                                )
                                            })
                                    })
                                    .unwrap_or_else(|| "Object".to_string())
                            } else {
                                // Setter: use the first param type, or paired getter's return type
                                acc.params
                                    .first()
                                    .and_then(|p| p.type_ann.as_ref())
                                    .map(|t| {
                                        self.serialize_metadata_type(
                                            t,
                                            &metadata_type_only,
                                            &cjs_map,
                                        )
                                    })
                                    .or_else(|| {
                                        paired_getter.and_then(|g| g.return_type.as_ref()).map(
                                            |t| {
                                                self.serialize_metadata_type(
                                                    t,
                                                    &metadata_type_only,
                                                    &cjs_map,
                                                )
                                            },
                                        )
                                    })
                                    .unwrap_or_else(|| "Object".to_string())
                            };
                            decorators.push(format!(
                                "{}__metadata(\"design:type\", {})",
                                self.helper_prefix(),
                                design_type
                            ));
                            // design:paramtypes: for getter with paired setter,
                            // use the setter's param types; otherwise use the
                            // accessor's own params.
                            let param_source = if is_getter {
                                paired_setter.map(|s| &s.params[..]).unwrap_or(&[])
                            } else {
                                &acc.params[..]
                            };
                            let param_types: Vec<String> = param_source
                                .iter()
                                .filter(|p| {
                                    if let PatKind::Ident(ref name) = p.name.kind {
                                        name != "this" && name != "<error>"
                                    } else {
                                        true
                                    }
                                })
                                .map(|p| {
                                    self.serialize_metadata_param_type(
                                        p,
                                        &metadata_type_only,
                                        &cjs_map,
                                    )
                                })
                                .collect();
                            decorators.push(format!(
                                "{}__metadata(\"design:paramtypes\", [{}])",
                                self.helper_prefix(),
                                param_types.join(", ")
                            ));
                            // TypeScript does NOT emit design:returntype for
                            // get/set accessors.
                        }
                        let target = if is_static(acc.modifiers) {
                            class_name.clone()
                        } else {
                            format!("{}.prototype", class_name)
                        };
                        self.emit_decorate_call(&decorators, &target, &acc_name_str, "null");
                    }
                    _ => {}
                }
            }
        } // end for pass in 0..2
          // Class decorators: class decorators first, then constructor __param entries
        let mut class_decs: Vec<String> = Vec::new();
        let remove_comments = self.options.remove_comments == Some(true);
        for dec in &class.decorators {
            let mut dec_str = emit_expr_to_string(
                self.source,
                self.options,
                dec,
                &cjs_map,
                &self.cjs_string_import_locals,
            );
            if !remove_comments {
                let comment = crate::analysis::trailing_line_comment(self.source, dec.span.end);
                if !comment.is_empty() {
                    dec_str.push(' ');
                    dec_str.push_str(comment);
                }
            }
            class_decs.push(dec_str);
        }
        for member in &class.members {
            if let ClassMemberKind::Constructor(ctor) = &member.kind {
                let mut param_idx = 0usize;
                for param in ctor.params.iter() {
                    let is_this = matches!(&param.name.kind, PatKind::Ident(name) if name == "this" || name == "<error>");
                    if is_this {
                        continue;
                    }
                    for dec in &param.decorators {
                        let dec_str = emit_expr_to_string(
                            self.source,
                            self.options,
                            dec,
                            &cjs_map,
                            &self.cjs_string_import_locals,
                        );
                        class_decs.push(format!(
                            "{}__param({}, {})",
                            self.helper_prefix(),
                            param_idx,
                            dec_str
                        ));
                    }
                    param_idx += 1;
                }
            }
        }
        // Add constructor metadata AFTER class decorators -- only when the class
        // has an explicit constructor (TypeScript omits design:paramtypes when
        // there is no constructor declaration).
        if emit_metadata && !class.decorators.is_empty() {
            let mut has_explicit_ctor = false;
            let mut param_types: Vec<String> = Vec::new();
            for member in &class.members {
                if let ClassMemberKind::Constructor(ctor) = &member.kind {
                    has_explicit_ctor = true;
                    param_types = ctor
                        .params
                        .iter()
                        .map(|p| {
                            self.serialize_metadata_param_type(p, &metadata_type_only, &cjs_map)
                        })
                        .collect();
                    break;
                }
            }
            if has_explicit_ctor {
                class_decs.push(format!(
                    "{}__metadata(\"design:paramtypes\", [{}])",
                    self.helper_prefix(),
                    param_types.join(", ")
                ));
            }
        }
        if !class_decs.is_empty() {
            // In CJS mode, exported classes use combined pattern:
            //   exports.X = X = __decorate([...], X);
            if let Some(target) = cjs_export_target.filter(|_| !insert_in_legacy_iife) {
                let exported_name = self
                    .cjs_export_alias_map
                    .get(class_name.as_str())
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| class_name.clone());
                self.write_cjs_export_access(target, &exported_name);
                self.write(" = ");
            }
            self.write(&class_name);
            self.write(" = ");
            if let Some(alias) = self.decorated_class_self_ref_alias.clone() {
                self.write(&alias);
                self.write(" = ");
            }
            self.write(self.helper_prefix());
            self.write("__decorate([");
            self.newline();
            self.indent += 1;
            if !class_decs.iter().all(String::is_empty) {
                for (i, dec) in class_decs.iter().enumerate() {
                    self.write_multiline_decorator_fragment(dec);
                    let has_line_comment = dec.contains("//");
                    if i < class_decs.len() - 1 {
                        if has_line_comment {
                            self.newline();
                            self.writeln(",");
                        } else {
                            self.writeln(",");
                        }
                    } else {
                        self.newline();
                    }
                }
            }
            self.indent -= 1;
            self.write("], ");
            self.write(&class_name);
            self.writeln(");");
        }
        if insert_in_legacy_iife {
            self.indent -= 1;
            let applications = self.output.split_off(application_output_start);
            let local_temps = self.temp_var_names.split_off(application_temp_start);
            let return_marker = format!("return {class_name};");
            let Some(return_pos) = self.output.rfind(&return_marker) else {
                self.temp_var_names.extend(local_temps);
                self.output.push_str(&applications);
                return;
            };
            let insertion_pos = self.output[..return_pos]
                .rfind('\n')
                .map_or(0, |newline| newline + 1);
            let mut insertion = String::new();
            if !local_temps.is_empty() {
                insertion.push_str("    var ");
                insertion.push_str(&local_temps.join(", "));
                insertion.push_str(";\n");
            }
            insertion.push_str(&applications);
            self.output.insert_str(insertion_pos, &insertion);
            self.at_line_start = true;
        }
    }

    /// Helper to emit a `__decorate([...], target, "name", desc)` call.
    fn emit_decorate_call(&mut self, decorators: &[String], target: &str, name: &str, desc: &str) {
        self.emit_decorate_call_inner(decorators, target, name, false, desc);
    }

    fn emit_decorate_call_inner(
        &mut self,
        decorators: &[String],
        target: &str,
        name: &str,
        name_is_computed: bool,
        desc: &str,
    ) {
        self.write(self.helper_prefix());
        self.write("__decorate([");
        self.newline();
        self.indent += 1;
        if !decorators.iter().all(String::is_empty) {
            for (i, dec) in decorators.iter().enumerate() {
                self.write_multiline_decorator_fragment(dec);
                let has_line_comment = dec.contains("//");
                if i < decorators.len() - 1 {
                    if has_line_comment {
                        // Comment runs to end of line; put comma on next line
                        self.newline();
                        self.writeln(",");
                    } else {
                        self.writeln(",");
                    }
                } else {
                    self.newline();
                }
            }
        }
        self.indent -= 1;
        self.write("], ");
        self.write(target);
        if name_is_computed {
            self.write(", ");
            self.write(name);
        } else {
            self.write(", \"");
            self.write(name);
            self.write("\"");
        }
        self.write(", ");
        self.write(desc);
        self.writeln(");");
    }

    /// Write a pre-rendered decorator expression, preserving its internal
    /// relative indentation while also applying current output indentation.
    fn write_multiline_decorator_fragment(&mut self, text: &str) {
        if !text.contains('\n') {
            self.write(text);
            return;
        }
        let normalized = text.replace("\r\n", "\n");
        let mut lines = normalized.split('\n');
        if let Some(first) = lines.next() {
            self.write(first);
        }
        for line in lines {
            self.newline();
            self.write(line);
        }
    }
}
