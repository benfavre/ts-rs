use tsc_rs_ast::*;

use crate::TypeChecker;

/// ECMAScript reserved words: never an identifier reference, so never a
/// shorthand property name (parser recovery can still produce one).
pub(crate) fn is_reserved_word(name: &str) -> bool {
    matches!(
        name,
        "break"
            | "case"
            | "catch"
            | "class"
            | "const"
            | "continue"
            | "debugger"
            | "default"
            | "delete"
            | "do"
            | "else"
            | "enum"
            | "export"
            | "extends"
            | "false"
            | "finally"
            | "for"
            | "function"
            | "if"
            | "import"
            | "in"
            | "instanceof"
            | "new"
            | "null"
            | "return"
            | "super"
            | "switch"
            | "this"
            | "throw"
            | "true"
            | "try"
            | "typeof"
            | "var"
            | "void"
            | "while"
            | "with"
    )
}

struct ReservedWordCheck<'a> {
    diagnostics: Vec<Diagnostic>,
    es2015_or_later: bool,
    current_function_is_generator: bool,
    ambient_depth: u32,
    class_depth: u32,
    external_module: bool,
    source: &'a str,
}

impl<'a> ReservedWordCheck<'a> {
    fn new(options: &CompilerOptions, source: &'a str) -> Self {
        Self {
            diagnostics: Vec::new(),
            es2015_or_later: options.always_strict != Some(false)
                && options
                    .target
                    .is_some_and(|target| target >= ScriptTarget::ES2015),
            current_function_is_generator: false,
            ambient_depth: 0,
            class_depth: 0,
            external_module: false,
            source,
        }
    }

    fn has_use_strict(&self, statements: &[Stmt]) -> bool {
        for statement in statements {
            let StmtKind::Expr(expression) = &statement.kind else {
                break;
            };
            let ExprKind::StrLit(value) = &expression.kind else {
                break;
            };
            let raw = self
                .source
                .get(expression.span.start as usize..expression.span.end as usize);
            if value.as_str() == "use strict"
                && matches!(raw, Some("\"use strict\"") | Some("'use strict'"))
            {
                return true;
            }
        }
        false
    }

    fn is_reserved(name: &str) -> bool {
        matches!(
            name,
            "implements"
                | "interface"
                | "let"
                | "package"
                | "private"
                | "protected"
                | "public"
                | "static"
                | "yield"
        )
    }

    fn name(&mut self, name: &str, span: Span, strict: bool) {
        if self.ambient_depth > 0 {
            return;
        }
        let reserved_for_target = self.es2015_or_later && matches!(name, "let" | "yield");
        if !Self::is_reserved(name) || (!strict && !reserved_for_target) {
            return;
        }
        let (code, context) = if self.class_depth > 0 {
            (1213, " Class definitions are automatically in strict mode.")
        } else if self.external_module {
            (1214, " Modules are automatically in strict mode.")
        } else {
            (1212, "")
        };
        if self.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == code && diagnostic.span.is_some_and(|existing| existing == span)
        }) {
            return;
        }
        self.diagnostics.push(Diagnostic {
            code,
            message: format!(
                "Identifier expected. '{}' is a reserved word in strict mode.{}",
                name, context
            ),
            category: DiagnosticCategory::Error,
            file_name: None,
            span: Some(span),
            related: None,
        });
    }

    /// TS2480: a `let`/`const` binding named `let`, anywhere in the pattern
    /// (tsc's checkGrammarNameInLetOrConstDeclarations).
    fn let_as_binding_name(&mut self, pattern: &Pat) {
        match &pattern.kind {
            PatKind::Ident(name) => {
                if name == "let"
                    && !self.diagnostics.iter().any(|diagnostic| {
                        diagnostic.code == 2480 && diagnostic.span == Some(pattern.span)
                    })
                {
                    self.diagnostics
                        .push(crate::diagnostics::error_let_as_declaration_name(
                            pattern.span,
                        ));
                }
            }
            PatKind::Array(elements) => {
                for element in elements.iter().flatten() {
                    match element {
                        ArrayPatElem::Pat(pattern) | ArrayPatElem::Rest(pattern) => {
                            self.let_as_binding_name(pattern);
                        }
                    }
                }
            }
            PatKind::Object(properties) => {
                for property in properties {
                    match property {
                        ObjPatProp::KeyValue(_, value) => self.let_as_binding_name(value),
                        ObjPatProp::Rest(pattern) => self.let_as_binding_name(pattern),
                        ObjPatProp::Shorthand(name, span)
                        | ObjPatProp::ShorthandAssign(name, _, span) => {
                            if name == "let" {
                                self.diagnostics
                                    .push(crate::diagnostics::error_let_as_declaration_name(*span));
                            }
                        }
                    }
                }
            }
            PatKind::Assign(pattern, _) | PatKind::Rest(pattern) => {
                self.let_as_binding_name(pattern)
            }
        }
    }

    fn declaration_name(&mut self, name: &str, span: Span, strict: bool) {
        self.name(name, span, strict);
    }

    fn prop_expression(&mut self, property: &PropName, strict: bool) {
        if let PropName::Computed(expression, _) = property {
            self.expr(expression, strict);
        }
    }

    fn pattern(&mut self, pattern: &Pat, strict: bool) {
        match &pattern.kind {
            PatKind::Ident(name) => self.name(name, pattern.span, strict),
            PatKind::Array(elements) => {
                for element in elements.iter().flatten() {
                    match element {
                        ArrayPatElem::Pat(pattern) | ArrayPatElem::Rest(pattern) => {
                            self.pattern(pattern, strict);
                        }
                    }
                }
            }
            PatKind::Object(properties) => {
                for property in properties {
                    match property {
                        ObjPatProp::KeyValue(key, value) => {
                            self.prop_expression(key, strict);
                            self.pattern(value, strict);
                        }
                        ObjPatProp::Shorthand(name, span) => self.name(name, *span, strict),
                        ObjPatProp::Rest(pattern) => self.pattern(pattern, strict),
                        ObjPatProp::ShorthandAssign(name, initializer, span) => {
                            self.name(name, *span, strict);
                            self.expr(initializer, strict);
                        }
                    }
                }
            }
            PatKind::Assign(pattern, initializer) => {
                self.pattern(pattern, strict);
                self.expr(initializer, strict);
            }
            PatKind::Rest(pattern) => self.pattern(pattern, strict),
        }
    }

    fn type_params(&mut self, parameters: Option<&[TypeParam]>, strict: bool) {
        for parameter in parameters.unwrap_or_default() {
            self.name(&parameter.name, parameter.span, strict);
            if let Some(constraint) = &parameter.constraint {
                self.ty(constraint, strict);
            }
            if let Some(default) = &parameter.default {
                self.ty(default, strict);
            }
        }
    }

    fn eval_or_arguments_name(&mut self, name: &str, span: Span) {
        if !matches!(name, "eval" | "arguments") {
            return;
        }
        let diagnostic = if self.class_depth > 0 {
            crate::diagnostics::error_invalid_strict_name(name, span)
        } else {
            let mut diagnostic = crate::diagnostics::error_invalid_strict_mode_name(name, span);
            if self.external_module {
                diagnostic.code = 1215;
                diagnostic.message = format!(
                    "Invalid use of '{}'. Modules are automatically in strict mode.",
                    name
                );
            }
            diagnostic
        };
        if !self
            .diagnostics
            .iter()
            .any(|existing| existing.code == diagnostic.code && existing.span == diagnostic.span)
        {
            self.diagnostics.push(diagnostic);
        }
    }

    /// TS1100 checks the direct write target. Parentheses and assertions are
    /// not unwrapped by tsc's strict-mode binding checks.
    fn eval_or_arguments_write(&mut self, target: &Expr, strict: bool) {
        if strict {
            if let ExprKind::Ident(name) = &target.kind {
                self.eval_or_arguments_name(name, target.span);
            }
        }
    }

    /// Strict-mode bindings may not be named `eval` or `arguments`.
    /// Callers apply the ambient exemption for functions and parameters;
    /// variable declarations are checked even in ambient contexts.
    fn eval_or_arguments_binding(&mut self, pattern: &Pat) {
        match &pattern.kind {
            PatKind::Ident(name) => self.eval_or_arguments_name(name, pattern.span),
            PatKind::Array(elements) => {
                for element in elements.iter().flatten() {
                    match element {
                        ArrayPatElem::Pat(pattern) | ArrayPatElem::Rest(pattern) => {
                            self.eval_or_arguments_binding(pattern);
                        }
                    }
                }
            }
            PatKind::Object(properties) => {
                for property in properties {
                    match property {
                        ObjPatProp::KeyValue(_, pattern) | ObjPatProp::Rest(pattern) => {
                            self.eval_or_arguments_binding(pattern);
                        }
                        ObjPatProp::Shorthand(name, span)
                        | ObjPatProp::ShorthandAssign(name, _, span) => {
                            self.eval_or_arguments_name(name, *span);
                        }
                    }
                }
            }
            PatKind::Assign(pattern, _) | PatKind::Rest(pattern) => {
                self.eval_or_arguments_binding(pattern);
            }
        }
    }

    fn params(&mut self, parameters: &[Param], strict: bool) {
        for parameter in parameters {
            if strict && self.ambient_depth == 0 {
                self.eval_or_arguments_binding(&parameter.name);
            }
            self.pattern(&parameter.name, strict);
            if let Some(annotation) = &parameter.type_ann {
                self.ty(annotation, strict);
            }
            if let Some(initializer) = &parameter.initializer {
                self.expr(initializer, strict);
            }
            for decorator in &parameter.decorators {
                self.expr(decorator, strict);
            }
        }
    }

    fn function(&mut self, function: &FnDecl, inherited_strict: bool) {
        if let (Some(name), Some(span)) = (&function.name, function.name_span) {
            self.name(name, span, inherited_strict);
            // A directive in this function's body does not make its name
            // strict: tsc binds the name in the enclosing context.
            if inherited_strict && self.ambient_depth == 0 {
                self.eval_or_arguments_name(name, span);
            }
        }
        for decorator in &function.decorators {
            self.expr(decorator, inherited_strict);
        }
        self.type_params(function.type_params.as_deref(), inherited_strict);
        self.params(&function.params, inherited_strict);
        if let Some(return_type) = &function.return_type {
            self.ty(return_type, inherited_strict);
        }
        if let Some(body) = &function.body {
            let body_strict = inherited_strict || self.has_use_strict(body);
            let saved_generator = self.current_function_is_generator;
            self.current_function_is_generator = function.is_generator;
            self.statements(body, body_strict);
            self.current_function_is_generator = saved_generator;
        }
    }

    fn arrow(&mut self, arrow: &ArrowFn, inherited_strict: bool) {
        self.type_params(arrow.type_params.as_deref(), inherited_strict);
        self.params(&arrow.params, inherited_strict);
        if let Some(return_type) = &arrow.return_type {
            self.ty(return_type, inherited_strict);
        }
        match &arrow.body {
            ArrowBody::Expr(expression) => {
                let saved_generator = self.current_function_is_generator;
                self.current_function_is_generator = false;
                self.expr(expression, inherited_strict);
                self.current_function_is_generator = saved_generator;
            }
            ArrowBody::Block(statements) => {
                let body_strict = inherited_strict || self.has_use_strict(statements);
                let saved_generator = self.current_function_is_generator;
                self.current_function_is_generator = false;
                self.statements(statements, body_strict);
                self.current_function_is_generator = saved_generator;
            }
        }
    }

    fn signature(
        &mut self,
        type_params: Option<&[TypeParam]>,
        params: &[Param],
        return_type: Option<&TypeNode>,
        strict: bool,
    ) {
        self.type_params(type_params, strict);
        self.params(params, strict);
        if let Some(return_type) = return_type {
            self.ty(return_type, strict);
        }
    }

    fn type_member(&mut self, member: &TypeMember, strict: bool) {
        match &member.kind {
            TypeMemberKind::PropertySig(property) => {
                self.prop_expression(&property.name, strict);
                if let Some(annotation) = &property.type_ann {
                    self.ty(annotation, strict);
                }
            }
            TypeMemberKind::MethodSig(method) => {
                self.prop_expression(&method.name, strict);
                self.signature(
                    method.type_params.as_deref(),
                    &method.params,
                    method.return_type.as_ref(),
                    strict,
                );
            }
            TypeMemberKind::CallSig(signature) => self.signature(
                signature.type_params.as_deref(),
                &signature.params,
                signature.return_type.as_ref(),
                strict,
            ),
            TypeMemberKind::ConstructSig(signature) => self.signature(
                signature.type_params.as_deref(),
                &signature.params,
                signature.return_type.as_ref(),
                strict,
            ),
            TypeMemberKind::IndexSig(signature) => {
                self.params(&signature.params, strict);
                if let Some(annotation) = &signature.type_ann {
                    self.ty(annotation, strict);
                }
            }
            TypeMemberKind::GetAccessorSig(accessor) | TypeMemberKind::SetAccessorSig(accessor) => {
                self.prop_expression(&accessor.name, strict);
                self.signature(
                    None,
                    &accessor.params,
                    accessor.return_type.as_ref(),
                    strict,
                );
            }
        }
    }

    fn ty(&mut self, node: &TypeNode, strict: bool) {
        match &node.kind {
            TypeNodeKind::Reference(reference) => {
                self.expr(&reference.name, strict);
                for argument in reference.type_args.as_deref().unwrap_or_default() {
                    self.ty(argument, strict);
                }
            }
            TypeNodeKind::Array(inner)
            | TypeNodeKind::Keyof(inner)
            | TypeNodeKind::Unique(inner)
            | TypeNodeKind::Readonly(inner)
            | TypeNodeKind::Paren(inner)
            | TypeNodeKind::Rest(inner)
            | TypeNodeKind::Optional(inner)
            | TypeNodeKind::TypeOperator(_, inner)
            | TypeNodeKind::JSDocNullable(Some(inner)) => self.ty(inner, strict),
            TypeNodeKind::Tuple(elements) => {
                for element in elements {
                    self.ty(&element.type_node, strict);
                }
            }
            TypeNodeKind::Union(types) | TypeNodeKind::Intersection(types) => {
                for node in types {
                    self.ty(node, strict);
                }
            }
            TypeNodeKind::Function(function) | TypeNodeKind::Constructor(function) => self
                .signature(
                    function.type_params.as_deref(),
                    &function.params,
                    Some(&function.return_type),
                    strict,
                ),
            TypeNodeKind::TypeLit(members) => {
                for member in members {
                    self.type_member(member, strict);
                }
            }
            TypeNodeKind::Conditional(conditional) => {
                self.ty(&conditional.check, strict);
                self.ty(&conditional.extends, strict);
                self.ty(&conditional.true_type, strict);
                self.ty(&conditional.false_type, strict);
            }
            TypeNodeKind::Mapped(mapped) => {
                self.name(&mapped.type_param.name, mapped.type_param.span, strict);
                if let Some(constraint) = &mapped.type_param.constraint {
                    self.ty(constraint, strict);
                }
                if let Some(default) = &mapped.type_param.default {
                    self.ty(default, strict);
                }
                if let Some(name_type) = &mapped.name_type {
                    self.ty(name_type, strict);
                }
                if let Some(annotation) = &mapped.type_ann {
                    self.ty(annotation, strict);
                }
            }
            TypeNodeKind::IndexedAccess(object, index) => {
                self.ty(object, strict);
                self.ty(index, strict);
            }
            TypeNodeKind::TypeQuery(expression) => self.expr(expression, strict),
            TypeNodeKind::Infer(name, constraint) => {
                self.name(name, node.span, strict);
                if let Some(constraint) = constraint {
                    self.ty(constraint, strict);
                }
            }
            TypeNodeKind::TemplateLit(template) => {
                for ty in &template.types {
                    self.ty(ty, strict);
                }
            }
            TypeNodeKind::ImportType(import_type) => {
                if let Some(qualifier) = &import_type.qualifier {
                    self.expr(qualifier, strict);
                }
                for argument in import_type.type_args.as_deref().unwrap_or_default() {
                    self.ty(argument, strict);
                }
            }
            TypeNodeKind::Predicate(predicate) => {
                self.name(&predicate.param_name, node.span, strict);
                if let Some(annotation) = &predicate.type_ann {
                    self.ty(annotation, strict);
                }
            }
            TypeNodeKind::NamedTupleMember(member) => {
                self.name(&member.name, node.span, strict);
                self.ty(&member.type_node, strict);
            }
            TypeNodeKind::Keyword(_)
            | TypeNodeKind::This
            | TypeNodeKind::JSDocNullable(None)
            | TypeNodeKind::Literal(_) => {}
        }
    }

    fn object_function(
        &mut self,
        type_params: Option<&[TypeParam]>,
        params: &[Param],
        return_type: Option<&TypeNode>,
        body: &[Stmt],
        inherited_strict: bool,
        is_generator: bool,
    ) {
        self.signature(type_params, params, return_type, inherited_strict);
        let body_strict = inherited_strict || self.has_use_strict(body);
        let saved_generator = self.current_function_is_generator;
        self.current_function_is_generator = is_generator;
        self.statements(body, body_strict);
        self.current_function_is_generator = saved_generator;
    }

    fn class(&mut self, class: &ClassDecl) {
        let ambient = class.modifiers & MOD_DECLARE != 0;
        self.ambient_depth += u32::from(ambient);
        self.class_depth += 1;
        if let (Some(name), Some(span)) = (&class.name, class.name_span) {
            self.name(name, span, true);
        }
        for decorator in &class.decorators {
            self.expr(decorator, true);
        }
        self.type_params(class.type_params.as_deref(), true);
        if let Some(base) = &class.extends {
            self.expr(base, true);
        }
        for argument in class.extends_type_args.as_deref().unwrap_or_default() {
            self.ty(argument, true);
        }
        for interface in &class.implements {
            self.ty(interface, true);
        }
        for member in &class.members {
            match &member.kind {
                ClassMemberKind::Property(property) => {
                    let ambient = property.modifiers & MOD_DECLARE != 0;
                    self.ambient_depth += u32::from(ambient);
                    self.prop_expression(&property.name, true);
                    for decorator in &property.decorators {
                        self.expr(decorator, true);
                    }
                    if let Some(annotation) = &property.type_ann {
                        self.ty(annotation, true);
                    }
                    if let Some(initializer) = &property.initializer {
                        self.expr(initializer, true);
                    }
                    self.ambient_depth -= u32::from(ambient);
                }
                ClassMemberKind::Method(method) => {
                    self.prop_expression(&method.name, true);
                    for decorator in &method.decorators {
                        self.expr(decorator, true);
                    }
                    self.object_function(
                        method.type_params.as_deref(),
                        &method.params,
                        method.return_type.as_ref(),
                        method.body.as_deref().unwrap_or_default(),
                        true,
                        method.is_generator,
                    );
                }
                ClassMemberKind::Constructor(constructor) => {
                    for decorator in &constructor.decorators {
                        self.expr(decorator, true);
                    }
                    self.object_function(
                        None,
                        &constructor.params,
                        None,
                        constructor.body.as_deref().unwrap_or_default(),
                        true,
                        false,
                    );
                }
                ClassMemberKind::GetAccessor(accessor) | ClassMemberKind::SetAccessor(accessor) => {
                    self.prop_expression(&accessor.name, true);
                    for decorator in &accessor.decorators {
                        self.expr(decorator, true);
                    }
                    self.object_function(
                        accessor.type_params.as_deref(),
                        &accessor.params,
                        accessor.return_type.as_ref(),
                        accessor.body.as_deref().unwrap_or_default(),
                        true,
                        false,
                    );
                }
                ClassMemberKind::IndexSignature(signature) => {
                    self.params(&signature.params, true);
                    if let Some(annotation) = &signature.type_ann {
                        self.ty(annotation, true);
                    }
                }
                ClassMemberKind::StaticBlock(statements) => {
                    let saved_generator = self.current_function_is_generator;
                    self.current_function_is_generator = false;
                    self.statements(statements, true);
                    self.current_function_is_generator = saved_generator;
                }
                ClassMemberKind::SemicolonClassElement => {}
            }
        }
        self.class_depth -= 1;
        self.ambient_depth -= u32::from(ambient);
    }

    fn expr(&mut self, expression: &Expr, strict: bool) {
        match &expression.kind {
            ExprKind::Ident(name) => {
                if !(self.current_function_is_generator && name.as_str() == "yield") {
                    self.name(name, expression.span, strict);
                }
            }
            ExprKind::Template(template) => {
                for expression in &template.exprs {
                    self.expr(expression, strict);
                }
            }
            ExprKind::TaggedTemplate(tagged) => {
                self.expr(&tagged.tag, strict);
                for argument in tagged.type_args.as_deref().unwrap_or_default() {
                    self.ty(argument, strict);
                }
                for expression in &tagged.quasi.exprs {
                    self.expr(expression, strict);
                }
            }
            ExprKind::ArrayLit(elements) => {
                for element in elements.iter().flatten() {
                    self.expr(element, strict);
                }
            }
            ExprKind::ObjectLit(properties) => {
                for property in properties {
                    match property {
                        ObjLitProp::Property(property) => {
                            self.prop_expression(&property.key, strict);
                            self.expr(&property.value, strict);
                        }
                        ObjLitProp::Shorthand(name, span) => self.name(name, *span, strict),
                        ObjLitProp::ShorthandDefault(name, initializer, span) => {
                            self.name(name, *span, strict);
                            self.expr(initializer, strict);
                        }
                        ObjLitProp::Spread(expression, _) => self.expr(expression, strict),
                        ObjLitProp::Method(method) => {
                            self.prop_expression(&method.name, strict);
                            self.object_function(
                                method.type_params.as_deref(),
                                &method.params,
                                method.return_type.as_ref(),
                                &method.body,
                                strict,
                                method.is_generator,
                            );
                        }
                        ObjLitProp::Get(accessor) | ObjLitProp::Set(accessor) => {
                            self.prop_expression(&accessor.name, strict);
                            self.object_function(
                                None,
                                &accessor.params,
                                accessor.return_type.as_ref(),
                                &accessor.body,
                                strict,
                                false,
                            );
                        }
                    }
                }
            }
            ExprKind::FnExpr(function) => self.function(function, strict),
            ExprKind::Arrow(arrow) => self.arrow(arrow, strict),
            ExprKind::ClassExpr(class) => self.class(class),
            ExprKind::Call(call) => {
                self.expr(&call.callee, strict);
                for argument in call.type_args.as_deref().unwrap_or_default() {
                    self.ty(argument, strict);
                }
                for argument in &call.args {
                    self.expr(argument, strict);
                }
            }
            ExprKind::New(new) => {
                self.expr(&new.callee, strict);
                for argument in new.type_args.as_deref().unwrap_or_default() {
                    self.ty(argument, strict);
                }
                for argument in new.args.as_deref().unwrap_or_default() {
                    self.expr(argument, strict);
                }
            }
            ExprKind::Member(member) => self.expr(&member.object, strict),
            ExprKind::ElemAccess(access) => {
                self.expr(&access.object, strict);
                self.expr(&access.index, strict);
            }
            ExprKind::Cond(conditional) => {
                self.expr(&conditional.test, strict);
                self.expr(&conditional.consequent, strict);
                self.expr(&conditional.alternate, strict);
            }
            ExprKind::Binary(binary) => {
                self.expr(&binary.left, strict);
                self.expr(&binary.right, strict);
            }
            ExprKind::Unary(unary) => self.expr(&unary.argument, strict),
            ExprKind::Update(update) => {
                self.eval_or_arguments_write(&update.argument, strict);
                self.expr(&update.argument, strict);
            }
            ExprKind::Paren(inner)
            | ExprKind::NonNull(inner)
            | ExprKind::Spread(inner)
            | ExprKind::Await(inner)
            | ExprKind::Delete(inner)
            | ExprKind::Typeof(inner)
            | ExprKind::Void(inner) => self.expr(inner, strict),
            ExprKind::TypeAssertion(assertion) => {
                self.ty(&assertion.type_node, strict);
                self.expr(&assertion.expr, strict);
            }
            ExprKind::As(assertion) => {
                self.expr(&assertion.expr, strict);
                self.ty(&assertion.type_node, strict);
            }
            ExprKind::Satisfies(assertion) => {
                self.expr(&assertion.expr, strict);
                self.ty(&assertion.type_node, strict);
            }
            ExprKind::Instantiation(instantiation) => {
                self.expr(&instantiation.expr, strict);
                for argument in &instantiation.type_args {
                    self.ty(argument, strict);
                }
            }
            ExprKind::Yield(_, argument) => {
                if !self.current_function_is_generator && argument.is_none() {
                    self.name(
                        "yield",
                        Span::new(expression.span.start, expression.span.start + 5),
                        strict,
                    );
                }
                if let Some(argument) = argument {
                    self.expr(argument, strict);
                }
            }
            ExprKind::Assign(assignment) => {
                self.eval_or_arguments_write(&assignment.left, strict);
                self.expr(&assignment.left, strict);
                self.expr(&assignment.right, strict);
            }
            ExprKind::Comma(expressions) => {
                for expression in expressions {
                    self.expr(expression, strict);
                }
            }
            ExprKind::JsxElement(element) => {
                for attribute in &element.attributes {
                    self.jsx_attribute(attribute, strict);
                }
                for child in &element.children {
                    self.jsx_child(child, strict);
                }
            }
            ExprKind::JsxSelfClosing(element) => {
                for attribute in &element.attributes {
                    self.jsx_attribute(attribute, strict);
                }
            }
            ExprKind::JsxFragment(fragment) => {
                for child in &fragment.children {
                    self.jsx_child(child, strict);
                }
            }
            ExprKind::NumLit(_)
            | ExprKind::BigIntLit(_)
            | ExprKind::StrLit(_)
            | ExprKind::BoolLit(_)
            | ExprKind::NullLit
            | ExprKind::RegexpLit(_)
            | ExprKind::NoSubstTemplate(_)
            | ExprKind::This
            | ExprKind::Super
            | ExprKind::MetaProp(_)
            | ExprKind::Omitted => {}
        }
    }

    fn jsx_attribute(&mut self, attribute: &JsxAttribute, strict: bool) {
        match attribute {
            JsxAttribute::Normal {
                value: Some(value), ..
            } => self.expr(value, strict),
            JsxAttribute::Spread(expression, _) => self.expr(expression, strict),
            JsxAttribute::Normal { value: None, .. } => {}
        }
    }

    fn jsx_child(&mut self, child: &JsxChild, strict: bool) {
        match child {
            JsxChild::Element(expression) => self.expr(expression, strict),
            JsxChild::Expression(Some(expression), _) => self.expr(expression, strict),
            JsxChild::Fragment(fragment) => {
                for child in &fragment.children {
                    self.jsx_child(child, strict);
                }
            }
            JsxChild::Text(_, _) | JsxChild::Expression(None, _) => {}
        }
    }

    fn variable(&mut self, variable: &VarStmt, strict: bool) {
        let block_scoped = matches!(
            variable.kind,
            VarKind::Let | VarKind::Const | VarKind::Using | VarKind::AwaitUsing
        );
        for declaration in &variable.declarations {
            if strict {
                self.eval_or_arguments_binding(&declaration.name);
            }
            self.pattern(&declaration.name, strict);
            if block_scoped {
                self.let_as_binding_name(&declaration.name);
            }
            if let Some(annotation) = &declaration.type_ann {
                self.ty(annotation, strict);
            }
            if let Some(initializer) = &declaration.init {
                self.expr(initializer, strict);
            }
        }
    }

    fn for_left(&mut self, left: &ForInOfLeft, strict: bool) {
        match left {
            ForInOfLeft::Var(variable) => self.variable(variable, strict),
            ForInOfLeft::Pat(pattern) => self.pattern(pattern, strict),
            ForInOfLeft::Expr(expression) => self.expr(expression, strict),
        }
    }

    fn module(&mut self, module: &ModuleDecl, strict: bool) {
        if let (ModuleName::Ident(name), Some(span)) = (&module.name, module.name_span) {
            self.declaration_name(name, span, strict);
        }
        match &module.body {
            Some(ModuleBody::Block(statements)) => {
                let body_strict = strict || self.has_use_strict(statements);
                self.statements(statements, body_strict);
            }
            Some(ModuleBody::Module(module)) => self.module(module, strict),
            None => {}
        }
    }

    fn statement(&mut self, statement: &Stmt, strict: bool) {
        let ambient = match &statement.kind {
            StmtKind::Var(declaration) => declaration.modifiers & MOD_DECLARE != 0,
            StmtKind::FnDecl(declaration) => declaration.modifiers & MOD_DECLARE != 0,
            StmtKind::ModuleDecl(declaration) => declaration.modifiers & MOD_DECLARE != 0,
            StmtKind::EnumDecl(declaration) => declaration.modifiers & MOD_DECLARE != 0,
            StmtKind::InterfaceDecl(declaration) => declaration.modifiers & MOD_DECLARE != 0,
            StmtKind::TypeAlias(declaration) => declaration.modifiers & MOD_DECLARE != 0,
            _ => false,
        };
        self.ambient_depth += u32::from(ambient);
        match &statement.kind {
            StmtKind::Var(variable) => self.variable(variable, strict),
            StmtKind::Expr(expression) | StmtKind::Throw(expression) => {
                self.expr(expression, strict);
            }
            StmtKind::Return(expression) => {
                if let Some(expression) = expression {
                    self.expr(expression, strict);
                }
            }
            StmtKind::If(statement) => {
                self.expr(&statement.test, strict);
                self.statement(&statement.consequent, strict);
                if let Some(alternate) = &statement.alternate {
                    self.statement(alternate, strict);
                }
            }
            StmtKind::While(statement) => {
                self.expr(&statement.test, strict);
                self.statement(&statement.body, strict);
            }
            StmtKind::DoWhile(statement) => {
                self.statement(&statement.body, strict);
                self.expr(&statement.test, strict);
            }
            StmtKind::For(statement) => {
                if let Some(initializer) = &statement.init {
                    match initializer {
                        ForInit::Var(variable) => self.variable(variable, strict),
                        ForInit::Expr(expression) => self.expr(expression, strict),
                    }
                }
                if let Some(test) = &statement.test {
                    self.expr(test, strict);
                }
                if let Some(update) = &statement.update {
                    self.expr(update, strict);
                }
                self.statement(&statement.body, strict);
            }
            StmtKind::ForIn(statement) => {
                self.for_left(&statement.left, strict);
                self.expr(&statement.right, strict);
                self.statement(&statement.body, strict);
            }
            StmtKind::ForOf(statement) => {
                self.for_left(&statement.left, strict);
                self.expr(&statement.right, strict);
                self.statement(&statement.body, strict);
            }
            StmtKind::Switch(statement) => {
                self.expr(&statement.discriminant, strict);
                for case in &statement.cases {
                    if let Some(test) = &case.test {
                        self.expr(test, strict);
                    }
                    self.statements(&case.consequent, strict);
                }
            }
            StmtKind::Try(statement) => {
                self.statements(&statement.block, strict);
                if let Some(handler) = &statement.handler {
                    if let Some(pattern) = &handler.param {
                        if strict {
                            self.eval_or_arguments_binding(pattern);
                        }
                        self.pattern(pattern, strict);
                    }
                    if let Some(annotation) = &handler.param_type {
                        self.ty(annotation, strict);
                    }
                    self.statements(&handler.body, strict);
                }
                if let Some(finalizer) = &statement.finalizer {
                    self.statements(finalizer, strict);
                }
            }
            StmtKind::Block(statements) => self.statements(statements, strict),
            StmtKind::FnDecl(function) => self.function(function, strict),
            StmtKind::ClassDecl(class) => self.class(class),
            StmtKind::InterfaceDecl(interface) => {
                if let Some(span) = interface.name_span {
                    // `interface public {}` is rejected under the ES2015
                    // grammar even when strict mode is explicitly disabled.
                    let strict =
                        strict || (self.es2015_or_later && interface.name.as_str() == "public");
                    self.declaration_name(&interface.name, span, strict);
                }
                self.type_params(interface.type_params.as_deref(), strict);
                for base in &interface.extends {
                    self.ty(base, strict);
                }
                for member in &interface.members {
                    self.type_member(member, strict);
                }
            }
            StmtKind::TypeAlias(alias) => {
                if let Some(span) = alias.name_span {
                    self.declaration_name(&alias.name, span, strict);
                }
                self.type_params(alias.type_params.as_deref(), strict);
                self.ty(&alias.type_ann, strict);
            }
            StmtKind::EnumDecl(enumeration) => {
                if let Some(span) = enumeration.name_span {
                    self.declaration_name(&enumeration.name, span, strict);
                }
                for member in &enumeration.members {
                    self.prop_expression(&member.name, strict);
                    if let Some(initializer) = &member.initializer {
                        self.expr(initializer, strict);
                    }
                }
            }
            StmtKind::ModuleDecl(module) => self.module(module, strict),
            StmtKind::ImportEquals(import) => self.expr(&import.module_ref, strict),
            StmtKind::Export(export) => match &export.kind {
                ExportDeclKind::Decl(statement) | ExportDeclKind::DefaultDecl(statement) => {
                    self.statement(statement, strict);
                }
                ExportDeclKind::Default(expression) => self.expr(expression, strict),
                ExportDeclKind::Named { .. } | ExportDeclKind::All { .. } => {}
            },
            StmtKind::ExportAssign(expression) => self.expr(expression, strict),
            StmtKind::Labeled(statement) => self.statement(&statement.body, strict),
            StmtKind::With(statement) => {
                self.expr(&statement.object, strict);
                self.statement(&statement.body, strict);
            }
            StmtKind::Import(_)
            | StmtKind::Break(_)
            | StmtKind::Continue(_)
            | StmtKind::Empty
            | StmtKind::Debugger => {}
        }
        self.ambient_depth -= u32::from(ambient);
    }

    fn statements(&mut self, statements: &[Stmt], strict: bool) {
        for statement in statements {
            self.statement(statement, strict);
        }
    }
}

impl TypeChecker {
    pub(crate) fn check_strict_reserved_words(&mut self, file: &SourceFile) {
        if !file.diagnostics.is_empty() && !self.current_file_is_js() {
            return;
        }
        let mut check = ReservedWordCheck::new(&self.compiler_options, &file.text);
        check.ambient_depth = u32::from(self.current_file_is_declaration());
        check.external_module = self.file_is_module_flag;
        let strict = !self.current_file_is_declaration()
            && self
                .compiler_options
                .always_strict
                .unwrap_or(self.compiler_options.strict != Some(false))
            || check.has_use_strict(&file.statements)
            || check.external_module;
        check.statements(&file.statements, strict);
        self.diagnostics.extend(check.diagnostics);
    }
}
