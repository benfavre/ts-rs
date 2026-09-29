use super::*;

#[derive(Clone, Copy)]
struct SignatureView<'a> {
    params: &'a [Param],
    type_params: Option<&'a [TypeParam]>,
    return_type: Option<&'a TypeNode>,
    body: Option<&'a [Stmt]>,
    span: Span,
    is_async: bool,
    constructor: bool,
}

impl TypeChecker {
    pub(crate) fn check_function_overload_compatibility(&mut self, statements: &[Stmt]) {
        if self.current_file_is_declaration() || self.ambient_depth > 0 {
            return;
        }
        let mut groups: HashMap<&str, Vec<SignatureView<'_>>> = HashMap::new();
        for statement in statements {
            let mut statement = statement;
            while let StmtKind::Export(export) = &statement.kind {
                match &export.kind {
                    ExportDeclKind::Decl(inner) | ExportDeclKind::DefaultDecl(inner) => {
                        statement = inner
                    }
                    _ => break,
                }
            }
            let StmtKind::FnDecl(function) = &statement.kind else {
                continue;
            };
            let Some(name) = function.name.as_deref() else {
                continue;
            };
            groups.entry(name).or_default().push(SignatureView {
                params: &function.params,
                type_params: function.type_params.as_deref(),
                return_type: function.return_type.as_ref(),
                body: function.body.as_deref(),
                span: function.name_span.unwrap_or(function.span),
                is_async: function.is_async,
                constructor: false,
            });
        }
        for signatures in groups.values() {
            self.check_overload_group(signatures, false);
        }
    }

    pub(crate) fn check_class_overload_compatibility(&mut self, members: &[ClassMember]) {
        if self.current_file_is_declaration() || self.ambient_depth > 0 {
            return;
        }
        let mut groups: HashMap<(bool, bool, String), Vec<SignatureView<'_>>> = HashMap::new();
        for member in members {
            let (key, signature) = match &member.kind {
                ClassMemberKind::Method(method) if method.modifiers & MOD_ABSTRACT == 0 => {
                    let name = match &method.name {
                        PropName::Ident(name, _)
                        | PropName::String(name, _)
                        | PropName::Private(name, _) => Some(name.to_string()),
                        PropName::Number(name, _) => {
                            Some(Self::canonical_numeric_property_name(name))
                        }
                        PropName::Computed(expression, _) => {
                            self.computed_property_name(expression)
                        }
                    };
                    let Some(name) = name else {
                        continue;
                    };
                    (
                        (false, method.modifiers & MOD_STATIC != 0, name),
                        SignatureView {
                            params: &method.params,
                            type_params: method.type_params.as_deref(),
                            return_type: method.return_type.as_ref(),
                            body: method.body.as_deref(),
                            span: method.name.span(),
                            is_async: method.is_async,
                            constructor: false,
                        },
                    )
                }
                ClassMemberKind::Constructor(constructor) => {
                    // TypeScript includes modifiers in constructor diagnostics.
                    // The parser already knows the keyword's end; rescanning the
                    // whole body here adds work even to non-overloaded classes.
                    let span = Span::new(member.span.start, constructor.keyword_span.end);
                    (
                        (true, false, String::new()),
                        SignatureView {
                            params: &constructor.params,
                            type_params: None,
                            return_type: None,
                            body: constructor.body.as_deref(),
                            span,
                            is_async: false,
                            constructor: true,
                        },
                    )
                }
                _ => continue,
            };
            groups.entry(key).or_default().push(signature);
        }
        for signatures in groups.values() {
            self.check_overload_group(signatures, true);
        }
    }

    fn check_overload_group(&mut self, signatures: &[SignatureView<'_>], method: bool) {
        let Some(implementation) = signatures.iter().find(|signature| signature.body.is_some())
        else {
            return;
        };
        if !signatures.iter().any(|signature| signature.body.is_none()) {
            return;
        }
        let (source, source_this) = self.resolve_compatibility_signature(*implementation);
        let strict_parameters = !method
            && self
                .compiler_options
                .strict_function_types
                .or(self.compiler_options.strict)
                .unwrap_or(false);
        for overload in signatures
            .iter()
            .filter(|signature| signature.body.is_none())
        {
            let (target, target_this) = self.resolve_compatibility_signature(*overload);
            let compatible = self.overload_signature_compatible(
                &source,
                &target,
                strict_parameters,
                true,
                false,
            ) && match (&source_this, &target_this) {
                (Some(source), Some(target)) => {
                    self.is_assignable_to(target, source)
                        || !strict_parameters && self.is_assignable_to(source, target)
                }
                _ => true,
            };
            if compatible {
                continue;
            }
            if !self
                .diagnostics
                .iter()
                .any(|d| d.code == 2394 && d.span == Some(overload.span))
            {
                self.diagnostics.push(Diagnostic {
                    code: 2394,
                    message: "This overload signature is not compatible with its implementation signature.".into(),
                    category: DiagnosticCategory::Error, file_name: None, span: Some(overload.span),
                    related: Some(vec![RelatedDiagnostic {
                        code: 2750, message: "The implementation signature is declared here.".into(),
                        file_name: self.current_file_name.clone(), span: Some(implementation.span),
                    }]),
                });
            }
            break;
        }
    }

    fn resolve_compatibility_signature(
        &self,
        signature: SignatureView<'_>,
    ) -> (ConstructorType, Option<Type>) {
        let substitutions: HashMap<_, _> = signature
            .type_params
            .unwrap_or(&[])
            .iter()
            .map(|parameter| (parameter.name.clone(), Type::Any))
            .collect();
        let mut bindings = Vec::new();
        let mut params = Vec::new();
        let mut this_type = None;
        for param in signature.params {
            let ty = param
                .type_ann
                .as_ref()
                .map(|node| self.resolve_type_node(node))
                .or_else(|| {
                    param
                        .initializer
                        .as_ref()
                        .map(|init| self.widen_type(&self.infer_expr_type(init)))
                })
                .unwrap_or(Type::Any);
            let ty = Self::substitute(&ty, &substitutions);
            let name = match &param.name.kind {
                PatKind::Ident(name) => name.as_str(),
                _ => "_",
            };
            bindings.push((name.to_string(), ty.clone()));
            if name == "this" {
                this_type = Some(ty);
                continue;
            }
            let name = if param.dotdotdot {
                format!("...{name}")
            } else if param.optional || param.initializer.is_some() {
                format!("?{name}")
            } else {
                name.to_string()
            };
            let ty = self.declared_optional_param_type(param, ty);
            params.push((name, ty));
        }
        let return_type = if signature.constructor {
            Type::Any
        } else if let Some(annotation) = signature.return_type {
            self.resolve_type_node(annotation)
        } else if let Some(body) = signature.body {
            let inferred = self.compatibility_return_type(body, &mut bindings);
            if signature.is_async {
                Self::wrap_async_inferred_return(inferred)
            } else {
                inferred
            }
        } else {
            Type::Any
        };
        (
            ConstructorType {
                is_abstract: false,
                params,
                return_type: Arc::new(Self::substitute(&return_type, &substitutions)),
                type_params: Vec::new(),
                type_param_constraints: Vec::new(),
                type_param_defaults: Vec::new(),
            },
            this_type,
        )
    }

    fn overload_signature_compatible(
        &self,
        source: &ConstructorType,
        target: &ConstructorType,
        strict: bool,
        bivariant_return: bool,
        callback: bool,
    ) -> bool {
        if !matches!(target.return_type.as_ref(), Type::Void)
            && (!bivariant_return
                || !self.is_assignable_to(&target.return_type, &source.return_type))
            && !self.is_assignable_to(&source.return_type, &target.return_type)
        {
            return false;
        }
        if constructor_max_count(target)
            .is_some_and(|count| constructor_required_count(source) > count)
        {
            return false;
        }
        let compared = constructor_max_count(source)
            .unwrap_or(source.params.len())
            .max(constructor_max_count(target).unwrap_or(target.params.len()));
        for index in 0..compared {
            let (Some(source), Some(target)) = (
                constructor_parameter_at(source, index),
                constructor_parameter_at(target, index),
            ) else {
                continue;
            };
            // Callback parameters have a dedicated comparison: reverse their
            // signatures, always compare callback parameters contravariantly,
            // and allow bivariant callback returns only in non-strict mode.
            if !callback {
                if let (
                    Some((source_callback, source_nulls)),
                    Some((target_callback, target_nulls)),
                ) = (
                    self.compatibility_callback(&source),
                    self.compatibility_callback(&target),
                ) {
                    if source_nulls == target_nulls {
                        if !self.overload_signature_compatible(
                            &target_callback,
                            &source_callback,
                            true,
                            !strict,
                            true,
                        ) {
                            return false;
                        }
                        continue;
                    }
                }
            }
            if !self.is_assignable_to(&target, &source)
                && (strict || !self.is_assignable_to(&source, &target))
            {
                return false;
            }
        }
        true
    }

    fn compatibility_callback(&self, ty: &Type) -> Option<(ConstructorType, u8)> {
        let mut ty = ty.clone();
        let mut nulls = 0;
        for _ in 0..16 {
            let function = match &ty {
                Type::Function(function) => function,
                Type::ObjectType(object) if object.call_signatures.len() == 1 => {
                    &object.call_signatures[0]
                }
                Type::Union(parts) => {
                    let mut concrete = Vec::new();
                    for part in parts.iter() {
                        match part {
                            Type::Undefined => nulls |= 1,
                            Type::Null => nulls |= 2,
                            _ => concrete.push(part.clone()),
                        }
                    }
                    if concrete.len() != 1 {
                        return None;
                    }
                    ty = concrete.pop().unwrap();
                    continue;
                }
                Type::TypeReference(name, arguments) => {
                    ty = self.resolve_type_reference_to_object(name, arguments)?;
                    continue;
                }
                _ => return None,
            };
            if function.type_predicate.is_some() {
                return None;
            }
            return Some((
                ConstructorType {
                    is_abstract: false,
                    params: function.params.clone(),
                    return_type: function.return_type.clone(),
                    type_params: function.type_params.clone(),
                    type_param_constraints: function.type_param_constraints.clone(),
                    type_param_defaults: function.type_param_defaults.clone(),
                },
                nulls,
            ));
        }
        None
    }

    fn compatibility_return_type(&self, body: &[Stmt], bindings: &mut Vec<(String, Type)>) -> Type {
        let mut work: Vec<_> = body.iter().rev().collect();
        let mut returns = Vec::new();
        let mut bare_return = false;
        while let Some(statement) = work.pop() {
            match &statement.kind {
                StmtKind::Return(Some(expression)) => returns.push(
                    self.widen_type(
                        &self
                            .local_initializer_type_from_bindings(expression, bindings)
                            .unwrap_or_else(|| self.infer_expr_type(expression)),
                    ),
                ),
                StmtKind::Return(None) => bare_return = true,
                StmtKind::Var(variable) => {
                    for declaration in &variable.declarations {
                        if let PatKind::Ident(name) = &declaration.name.kind {
                            let ty = declaration
                                .type_ann
                                .as_ref()
                                .map(|node| self.resolve_type_node(node))
                                .or_else(|| {
                                    declaration.init.as_ref().map(|init| {
                                        self.local_initializer_type_from_bindings(init, bindings)
                                            .unwrap_or_else(|| self.infer_expr_type(init))
                                    })
                                })
                                .unwrap_or(Type::Any);
                            bindings.push((name.to_string(), ty));
                        }
                    }
                }
                StmtKind::Block(body) => work.extend(body.iter().rev()),
                StmtKind::If(statement) => {
                    if let Some(alternate) = &statement.alternate {
                        work.push(alternate);
                    }
                    work.push(&statement.consequent);
                }
                StmtKind::Try(statement) => {
                    if let Some(finalizer) = &statement.finalizer {
                        work.extend(finalizer.iter().rev());
                    }
                    if let Some(handler) = &statement.handler {
                        work.extend(handler.body.iter().rev());
                    }
                    work.extend(statement.block.iter().rev());
                }
                StmtKind::Switch(statement) => {
                    for case in statement.cases.iter().rev() {
                        work.extend(case.consequent.iter().rev());
                    }
                }
                StmtKind::While(statement) => work.push(&statement.body),
                StmtKind::DoWhile(statement) => work.push(&statement.body),
                StmtKind::For(statement) => work.push(&statement.body),
                StmtKind::ForIn(statement) => work.push(&statement.body),
                StmtKind::ForOf(statement) => work.push(&statement.body),
                StmtKind::Labeled(statement) => work.push(&statement.body),
                StmtKind::With(statement) => work.push(&statement.body),
                _ => {}
            }
        }
        if returns.is_empty() {
            return Type::Void;
        }
        if bare_return
            && self
                .compiler_options
                .strict_null_checks
                .or(self.compiler_options.strict)
                .unwrap_or(false)
        {
            returns.push(Type::Undefined);
        }
        Type::flatten_union(returns)
    }
}
