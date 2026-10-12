use super::*;

#[derive(Clone, Copy)]
pub(super) enum DecoratorSite {
    Class,
    Property,
    Parameter,
    Method,
    AutoAccessor,
}

pub(super) struct DecoratorTarget {
    arguments: Vec<Type>,
    return_type: Type,
}

struct DecoratorArgumentError {
    source: Type,
    target: Type,
    is_this: bool,
}

#[derive(Clone)]
pub(super) struct DecoratorSignatureNotes {
    parameters: Vec<RelatedDiagnostic>,
    name_span: Option<Span>,
}

impl DecoratorSite {
    fn diagnostic(self) -> (u32, &'static str) {
        match self {
            Self::Class => (1238, "class"),
            Self::Property | Self::AutoAccessor => (1240, "property"),
            Self::Parameter => (1239, "parameter"),
            Self::Method => (1241, "method"),
        }
    }

    fn runtime_argument_count(self, legacy: bool) -> usize {
        if !legacy {
            return 2;
        }
        match self {
            Self::Class => 1,
            Self::Property => 2,
            Self::Parameter | Self::Method | Self::AutoAccessor => 3,
        }
    }

    fn argument_count(self, legacy: bool, parameter_count: usize) -> usize {
        if !legacy {
            return parameter_count.clamp(1, 2);
        }
        match self {
            Self::Class => 1,
            Self::Property => 2,
            Self::Parameter | Self::AutoAccessor => 3,
            Self::Method if parameter_count > 2 => 3,
            Self::Method => 2,
        }
    }
}

impl TypeChecker {
    pub(super) fn legacy_class_decorator_target(
        &self,
        class: &ClassDecl,
    ) -> Option<DecoratorTarget> {
        if self.compiler_options.experimental_decorators != Some(true)
            || class.decorators.is_empty()
        {
            return None;
        }
        let name = self.namespace_owned_type_name(class.name.as_deref()?);
        let constructor = Type::TypeReference(format!("typeof {name}"), Arc::from([]));
        Some(DecoratorTarget {
            arguments: vec![constructor.clone()],
            return_type: Type::Union(vec![Type::Void, constructor].into()),
        })
    }

    pub(super) fn legacy_member_decorator_target(
        &self,
        member: &ClassMember,
        parameter_index: Option<usize>,
    ) -> Option<DecoratorTarget> {
        if self.compiler_options.experimental_decorators != Some(true) {
            return None;
        }
        if parameter_index.is_none() {
            let decorators = match &member.kind {
                ClassMemberKind::Property(p) => &p.decorators,
                ClassMemberKind::Method(m) => &m.decorators,
                ClassMemberKind::GetAccessor(a) | ClassMemberKind::SetAccessor(a) => &a.decorators,
                _ => return None,
            };
            if decorators.is_empty() {
                return None;
            }
        }
        let class = self.namespace_owned_type_name(self.enclosing_class_names.last()?);
        let info = self.class_info.get(&class)?;
        let constructor = matches!(member.kind, ClassMemberKind::Constructor(_));
        let is_static = constructor || Self::class_member_is_static(member);
        let target = if is_static {
            Type::TypeReference(format!("typeof {class}"), Arc::from([]))
        } else {
            Type::TypeReference(
                class,
                info.type_params
                    .iter()
                    .cloned()
                    .map(Type::TypeParameter)
                    .collect(),
            )
        };
        let name = match &member.kind {
            ClassMemberKind::Property(p) => Some(&p.name),
            ClassMemberKind::Method(m) => Some(&m.name),
            ClassMemberKind::GetAccessor(a) | ClassMemberKind::SetAccessor(a) => Some(&a.name),
            _ => None,
        };
        let key = match name {
            Some(PropName::Computed(expression, _)) => {
                let ty = self.infer_expr_type(expression);
                if matches!(ty, Type::Symbol | Type::UniqueSymbol(_)) {
                    ty
                } else {
                    Type::String
                }
            }
            Some(name) => self
                .pattern_property_name(name)
                .map(Type::StringLiteral)
                .unwrap_or(Type::String),
            None => Type::Undefined,
        };
        let mut arguments = vec![target, key];
        if let Some(index) = parameter_index {
            arguments.push(Type::NumberLiteral(index.to_string()));
            return Some(DecoratorTarget {
                arguments,
                return_type: Type::Void,
            });
        }
        let property = matches!(member.kind, ClassMemberKind::Property(_));
        let accessor =
            matches!(&member.kind, ClassMemberKind::Property(p) if p.modifiers & MOD_ACCESSOR != 0);
        let return_type = if !property || accessor {
            let member_name = name.and_then(|name| self.pattern_property_name(name));
            let properties = if is_static {
                &info.static_properties
            } else {
                &info.instance_properties
            };
            let methods = if is_static {
                &info.static_methods
            } else {
                &info.instance_methods
            };
            let overloads = if is_static {
                &info.overloaded_static_methods
            } else {
                &info.overloaded_methods
            };
            let value = member_name
                .as_ref()
                .and_then(|name| {
                    overloads
                        .get(name)
                        .cloned()
                        .or_else(|| {
                            properties
                                .iter()
                                .find(|(n, _)| n == name)
                                .map(|(_, ty)| ty.clone())
                        })
                        .or_else(|| {
                            methods
                                .iter()
                                .find(|(n, _)| n == name)
                                .map(|(_, ty)| Type::Function(ty.clone()))
                        })
                })
                .unwrap_or(Type::Any);
            let descriptor =
                Type::TypeReference("TypedPropertyDescriptor".into(), vec![value].into());
            arguments.push(descriptor.clone());
            if property {
                Type::Void
            } else {
                Type::Union(vec![Type::Void, descriptor].into())
            }
        } else {
            Type::Void
        };
        Some(DecoratorTarget {
            arguments,
            return_type,
        })
    }

    fn check_legacy_decorator_call(
        &mut self,
        decorator: &Expr,
        signatures: &[FunctionType],
        site: DecoratorSite,
        target: &DecoratorTarget,
    ) {
        fn literal_parameter(ty: &Type) -> bool {
            match ty {
                Type::StringLiteral(_)
                | Type::NumberLiteral(_)
                | Type::BooleanLiteral(_)
                | Type::BigIntLiteral(_) => true,
                Type::Union(parts) => parts.iter().any(literal_parameter),
                _ => false,
            }
        }
        let specialized =
            |sig: &FunctionType| sig.params.iter().any(|(_, ty)| literal_parameter(ty));
        let ordered = signatures
            .iter()
            .enumerate()
            .filter(|(_, sig)| specialized(sig))
            .chain(
                signatures
                    .iter()
                    .enumerate()
                    .filter(|(_, sig)| !specialized(sig)),
            );
        let mut candidates = Vec::new();
        let mut selected = None;
        let mut recovery = None;
        for (signature_index, signature) in ordered {
            let count = site.argument_count(
                true,
                signature
                    .params
                    .iter()
                    .filter(|(name, _)| name != "this")
                    .count(),
            );
            let instantiated =
                self.instantiate_decorator_signature(signature, &target.arguments[..count]);
            recovery = Some(instantiated.clone());
            let params = Self::relation_params(&instantiated.params);
            let required = params
                .iter()
                .filter(|(name, ty)| {
                    !name.starts_with('?')
                        && !name.starts_with("...")
                        && !matches!(ty, Type::Optional(_) | Type::Rest(_))
                })
                .count();
            let rest = params
                .iter()
                .any(|(name, ty)| name.starts_with("...") || matches!(ty, Type::Rest(_)));
            if count < required || (!rest && count > params.len()) {
                continue;
            }
            let failure = self.decorator_argument_error(&instantiated, &target.arguments[..count]);
            if let Some(failure) = failure {
                candidates.push((signature_index, instantiated, failure));
            } else {
                selected = Some(instantiated);
                break;
            }
        }
        if selected.is_none() && !candidates.is_empty() {
            self.report_decorator_argument_errors(decorator, site, &candidates);
        }
        // An unresolved overload set recovers as an error signature (any),
        // whereas a single signature retains its instantiated result.
        let Some(signature) =
            selected.or_else(|| (signatures.len() == 1).then_some(recovery).flatten())
        else {
            return;
        };
        let returned = Type::clone(&signature.return_type);
        if self.decorator_return_assignable(&returned, &target.return_type) {
            return;
        }
        let (code, mut message) = if matches!(
            site,
            DecoratorSite::Property | DecoratorSite::AutoAccessor | DecoratorSite::Parameter
        ) {
            (
                1271,
                format!(
                    "Decorator function return type is '{}' but is expected to be 'void' or 'any'.",
                    returned.display_string_single_line()
                ),
            )
        } else {
            (
                1270,
                format!(
                    "Decorator function return type '{}' is not assignable to type '{}'.",
                    returned.display_string_single_line(),
                    target.return_type.display_string_single_line()
                ),
            )
        };
        let mut related = None;
        let elaboration_target = match &target.return_type {
            Type::Union(parts) => parts.iter().find(|part| {
                !matches!(part, Type::Void)
                    && self.assignability_elaboration(&returned, part).is_some()
            }),
            _ => None,
        };
        if let Some(part) = elaboration_target {
            let detail = self.leaf_reason_text(&returned, part);
            message.push_str("\n  ");
            message.push_str(&detail.replace('\n', "\n  "));
            related = self.last_leaf_related.take().map(|note| vec![note]);
        } else if let Some((detail, note)) =
            self.assignability_elaboration(&returned, &target.return_type)
        {
            message.push_str("\n  ");
            message.push_str(&detail.replace('\n', "\n  "));
            related = note.map(|note| vec![note]);
        }
        self.diagnostics.push(Diagnostic {
            code,
            message,
            category: DiagnosticCategory::Error,
            file_name: None,
            span: Some(self.decorator_expression_span(decorator)),
            related,
        });
    }

    fn instantiate_decorator_signature(
        &self,
        signature: &FunctionType,
        arguments: &[Type],
    ) -> FunctionType {
        let params = Self::relation_params(&signature.params);
        let mut inferred =
            self.infer_type_arguments_for_call(arguments, &params, &signature.type_params);
        Self::apply_type_param_defaults(signature, &mut inferred);
        for (index, parameter) in signature.type_params.iter().enumerate() {
            inferred.entry(parameter.clone()).or_insert(Type::Unknown);
            if let Some(Some(constraint)) = signature.type_param_constraints.get(index) {
                let constraint = Self::substitute(constraint, &inferred);
                if !self.decorator_argument_assignable(&inferred[parameter], &constraint) {
                    inferred.insert(parameter.clone(), constraint);
                }
            }
        }
        FunctionType {
            source: signature.source.clone(),
            params: signature
                .params
                .iter()
                .map(|(name, ty)| (name.clone(), Self::substitute(ty, &inferred)))
                .collect(),
            return_type: Arc::new(Self::substitute(&signature.return_type, &inferred)),
            type_params: vec![],
            type_param_defaults: vec![],
            type_param_constraints: vec![],
            type_predicate: signature.type_predicate.clone(),
        }
    }

    fn decorator_argument_error(
        &self,
        signature: &FunctionType,
        arguments: &[Type],
    ) -> Option<DecoratorArgumentError> {
        if let Some((_, this_type)) = signature.params.iter().find(|(name, _)| name == "this") {
            if !self.decorator_argument_assignable(&Type::Void, this_type) {
                return Some(DecoratorArgumentError {
                    source: Type::Void,
                    target: this_type.clone(),
                    is_this: true,
                });
            }
        }
        let params = Self::relation_params(&signature.params);
        for (index, argument) in arguments.iter().enumerate() {
            let (name, parameter) = params
                .get(index)
                .or_else(|| params.last().filter(|(name, _)| name.starts_with("...")))?;
            let parameter = if name.starts_with("...") {
                match parameter {
                    Type::Array(element) | Type::Rest(element) => Type::clone(element),
                    other => other.clone(),
                }
            } else {
                Self::optional_param_type(name, parameter.clone())
            };
            if !self.decorator_argument_assignable(argument, &parameter) {
                return Some(DecoratorArgumentError {
                    source: argument.clone(),
                    target: parameter,
                    is_this: false,
                });
            }
        }
        None
    }

    fn compact_decorator_function_type(&self, ty: &Type) -> Option<Type> {
        let Type::TypeReference(name, _) = ty else {
            return None;
        };
        if name != "Function"
            || self.interface_info.contains_key(name)
            || self.type_aliases.contains_key(name)
            || self.class_info.contains_key(name)
        {
            return None;
        }
        let library = stdlib::stdlib_member_availability(&self.compiler_options)?;
        if !library.declares_global_type("Function") {
            return None;
        }
        let mut properties: Vec<_> = [
            "apply",
            "call",
            "bind",
            "prototype",
            "length",
            "arguments",
            "caller",
            "name",
        ]
        .into_iter()
        .filter(|name| library.declares_inherited_member("Function", name) == Some(true))
        .filter_map(|name| {
            Self::function_intrinsic_property_type(name).map(|ty| (name.to_string(), Arc::new(ty)))
        })
        .collect();
        if library.declares_member("SymbolConstructor", "hasInstance") == Some(true) {
            properties.push((
                "[Symbol.hasInstance]".into(),
                Arc::new(Type::Function(FunctionType {
                    source: Default::default(),
                    params: vec![("value".into(), Type::Any)],
                    return_type: Arc::new(Type::Boolean),
                    type_params: vec![],
                    type_param_defaults: vec![],
                    type_param_constraints: vec![],
                    type_predicate: None,
                })),
            ));
        }
        Some(Type::ObjectType(ObjectTypeInfo::new(ObjectTypeData {
            properties,
            ..ObjectTypeData::default()
        })))
    }

    fn decorator_argument_assignable(&self, source: &Type, target: &Type) -> bool {
        if let Type::Union(parts) = source {
            return parts
                .iter()
                .all(|part| self.decorator_argument_assignable(part, target));
        }
        if matches!(target, Type::TypeReference(name, _) if name == "Object"
            && !self.interface_info.contains_key(name) && !self.type_aliases.contains_key(name) && !self.class_info.contains_key(name))
            && self.direct_constructor_signatures(source).is_some()
        {
            return true;
        }
        if let Type::Union(parts) = target {
            return parts
                .iter()
                .any(|part| self.decorator_argument_assignable(source, part));
        }
        let source = if self.direct_constructor_signatures(target).is_some() {
            source.clone()
        } else {
            self.decorator_relation_type(source)
        };
        let target = if let Some(function) = self.compact_decorator_function_type(target) {
            if matches!(&source, Type::Function(_) | Type::Constructor(_))
                || matches!(&source, Type::ObjectType(info) if !info.call_signatures.is_empty() || !info.construct_signatures.is_empty())
            {
                return true;
            }
            if let Type::TypeReference(name, _) = &source {
                if self.class_extends_function(name)
                    || self.interface_extends_function(name, &mut HashSet::new())
                {
                    return true;
                }
            }
            function
        } else {
            target.clone()
        };
        use std::sync::atomic::Ordering::Relaxed;
        let previous = self.strict_nullish_relation.swap(true, Relaxed);
        let assignable = self.is_assignable_to(&source, &target);
        self.strict_nullish_relation.store(previous, Relaxed);
        assignable
    }

    fn decorator_argument_detail(
        &mut self,
        failure: &DecoratorArgumentError,
    ) -> (String, Option<RelatedDiagnostic>) {
        let source = &failure.source;
        let target = &failure.target;
        let shown_target =
            Self::strip_nullable_union_target(source, target).unwrap_or_else(|| target.clone());
        let mut message = if failure.is_this {
            format!("The 'this' context of type '{}' is not assignable to method's 'this' of type '{}'.", source.display_string_single_line(), target.display_string_single_line())
        } else {
            format!(
                "Argument of type '{}' is not assignable to parameter of type '{}'.",
                self.elaboration_display_type(source, &shown_target)
                    .display_string_single_line(),
                shown_target.display_string_single_line()
            )
        };
        let elaboration_target = self
            .compact_decorator_function_type(target)
            .unwrap_or_else(|| target.clone());
        let elaboration_source = if self.direct_constructor_signatures(target).is_some() {
            source.clone()
        } else {
            self.decorator_relation_type(source)
        };
        let elaboration = self.assignability_elaboration(&elaboration_source, &elaboration_target);
        let mut related = None;
        if let Some((detail, note)) = elaboration {
            let detail = if elaboration_target != *target {
                detail.replace(
                    &elaboration_target.display_string_single_line(),
                    &target.display_string_single_line(),
                )
            } else {
                detail
            };
            let detail = if elaboration_source != *source {
                detail.replace(
                    &elaboration_source.display_string_single_line(),
                    &source.display_string_single_line(),
                )
            } else {
                detail
            };
            message.push_str("\n  ");
            message.push_str(&detail.replace('\n', "\n  "));
            related = note;
        }
        (message, related)
    }

    fn report_decorator_argument_errors(
        &mut self,
        decorator: &Expr,
        site: DecoratorSite,
        candidates: &[(usize, FunctionType, DecoratorArgumentError)],
    ) {
        let mut related = Vec::new();
        let detail = if candidates.len() == 1 {
            let (detail, note) = self.decorator_argument_detail(&candidates[0].2);
            related.extend(note);
            detail
        } else {
            let mut message = String::from("No overload matches this call.");
            let shown = if candidates.len() > 3 {
                &candidates[candidates.len() - 1..]
            } else {
                candidates
            };
            for (index, (signature_index, signature, failure)) in shown.iter().enumerate() {
                if candidates.len() > 3 {
                    message.push_str("\n  The last overload gave the following error.");
                    if let Some(note) =
                        self.decorator_last_overload_note(decorator, *signature_index)
                    {
                        related.push(note);
                    }
                } else {
                    message.push_str(&format!(
                        "\n  Overload {} of {}, '{}', gave the following error.",
                        index + 1,
                        candidates.len(),
                        self.signature_text(&[], &signature.params, &signature.return_type, false)
                    ));
                }
                let (detail, note) = self.decorator_argument_detail(failure);
                message.push_str("\n    ");
                message.push_str(&detail.replace('\n', "\n    "));
                related.extend(note);
            }
            message
        };
        let span = if candidates.iter().all(|(_, _, failure)| failure.is_this) {
            decorator.span
        } else {
            self.decorator_expression_span(decorator)
        };
        let mut diagnostic = self.decorator_signature_diagnostic(site, &detail, span);
        if !related.is_empty() {
            diagnostic.related = Some(related);
        }
        self.diagnostics.push(diagnostic);
    }

    fn decorator_last_overload_note(
        &self,
        decorator: &Expr,
        signature: usize,
    ) -> Option<RelatedDiagnostic> {
        let mut expression = decorator;
        while let ExprKind::Paren(inner) = &expression.kind {
            expression = inner;
        }
        let ExprKind::Ident(name) = &expression.kind else {
            return None;
        };
        if self.non_global_scope_declares_name(name) {
            return None;
        }
        let span = self
            .decorator_parameter_notes
            .get(&(name.to_string(), 0))?
            .get(signature)?
            .name_span?;
        Some(RelatedDiagnostic {
            code: 2771,
            message: "The last overload is declared here.".into(),
            file_name: self.current_file_name.clone(),
            span: Some(span),
        })
    }

    fn decorator_relation_type(&self, ty: &Type) -> Type {
        match ty {
            Type::Union(parts) => Type::Union(
                parts
                    .iter()
                    .map(|part| self.decorator_relation_type(part))
                    .collect(),
            ),
            Type::TypeReference(name, _) if name.starts_with("typeof ") => self
                .inference_constructor_shape(ty)
                .unwrap_or_else(|| ty.clone()),
            _ => ty.clone(),
        }
    }

    fn decorator_return_assignable(&self, source: &Type, target: &Type) -> bool {
        if source == target || matches!(source, Type::Any | Type::Error | Type::Never) {
            return true;
        }
        if let Type::Union(parts) = source {
            return parts
                .iter()
                .all(|part| self.decorator_return_assignable(part, target));
        }
        if let Type::Union(parts) = target {
            return parts
                .iter()
                .any(|part| self.decorator_return_assignable(source, part));
        }
        // Replacement classes must be constructable, including when an
        // instance and its constructor share the same class identity.
        if self.direct_constructor_signatures(target).is_some()
            && self.direct_constructor_signatures(source).is_none()
            && !matches!(source, Type::Undefined | Type::Null)
        {
            return false;
        }
        let shape = |ty: &Type| match ty {
            Type::TypeReference(name, args) => self
                .resolve_type_reference_to_object(name, args)
                .unwrap_or_else(|| ty.clone()),
            other => other.clone(),
        };
        let target_shape = shape(target);
        if crate::assign::is_weak_object(&target_shape) {
            let source_shape = shape(source);
            if let (Type::ObjectType(source), Type::ObjectType(target)) =
                (&source_shape, &target_shape)
            {
                let nonempty = !source.properties.is_empty()
                    || !source.call_signatures.is_empty()
                    || !source.construct_signatures.is_empty();
                if nonempty
                    && !source.properties.iter().any(|(name, _)| {
                        target.properties.iter().any(|(other, _)| {
                            name.trim_start_matches('?') == other.trim_start_matches('?')
                        })
                    })
                {
                    return false;
                }
            }
        }
        self.is_assignable_to(source, target)
    }

    pub(super) fn prepare_decorator_parameter_notes(&mut self, file: &SourceFile) {
        self.decorator_parameter_notes.clear();
        if !file.text.contains('@') {
            return;
        }
        for stmt in &file.statements {
            let StmtKind::FnDecl(function) = &Self::unwrap_export_stmt(stmt).kind else {
                continue;
            };
            let Some(name) = &function.name else {
                continue;
            };
            let notes = self.decorator_notes_for_parameters(&function.params);
            self.decorator_parameter_notes
                .entry((name.clone(), 0))
                .or_default()
                .push(DecoratorSignatureNotes {
                    parameters: notes,
                    name_span: function.name_span,
                });
            let mut returned = function.return_type.as_ref();
            let mut calls = 1;
            while let Some(node) = returned {
                match &node.kind {
                    TypeNodeKind::Paren(inner) => returned = Some(inner),
                    TypeNodeKind::Function(signature) => {
                        let notes = self.decorator_notes_for_parameters(&signature.params);
                        self.decorator_parameter_notes
                            .entry((name.clone(), calls))
                            .or_default()
                            .push(DecoratorSignatureNotes {
                                parameters: notes,
                                name_span: None,
                            });
                        calls += 1;
                        returned = Some(&signature.return_type);
                    }
                    _ => break,
                }
            }
        }
    }

    fn decorator_notes_for_parameters(
        &self,
        params: &[Param],
    ) -> Vec<tsc_rs_ast::RelatedDiagnostic> {
        params
            .iter()
            .filter(|param| !matches!(&param.name.kind, PatKind::Ident(name) if name == "this"))
            .map(|param| {
                let name = match &param.name.kind {
                    PatKind::Ident(name) => name.as_str(),
                    _ => "",
                };
                let mut note = self.missing_argument_note(name, param.span);
                if param.dotdotdot {
                    note.code = 6212;
                    note.message =
                        format!("Arguments for the rest parameter '{name}' were not provided.");
                }
                note
            })
            .collect()
    }

    fn decorator_missing_argument_note(
        &self,
        decorator: &Expr,
        signature: usize,
        provided: usize,
    ) -> Option<tsc_rs_ast::RelatedDiagnostic> {
        let mut expression = decorator;
        let mut calls = 0;
        loop {
            match &expression.kind {
                ExprKind::Paren(inner) => expression = inner,
                ExprKind::Call(call) => {
                    calls += 1;
                    expression = &call.callee;
                }
                ExprKind::Ident(name) if !self.non_global_scope_declares_name(name) => {
                    let overloads = self
                        .decorator_parameter_notes
                        .get(&(name.to_string(), calls))?;
                    // The returned function's overload index does not identify
                    // which overload of its factory was selected.
                    if calls > 0 && overloads.len() != 1 {
                        return None;
                    }
                    return overloads.get(signature)?.parameters.get(provided).cloned();
                }
                ExprKind::Arrow(arrow) if calls == 0 => {
                    return self
                        .decorator_notes_for_parameters(&arrow.params)
                        .get(provided)
                        .cloned();
                }
                ExprKind::FnExpr(function) if calls == 0 => {
                    return self
                        .decorator_notes_for_parameters(&function.params)
                        .get(provided)
                        .cloned();
                }
                _ => return None,
            }
        }
    }

    fn decorator_expression_span(&self, decorator: &Expr) -> Span {
        let mut start = decorator.span.start;
        if let Some(source) = self.current_source.as_deref() {
            if source.as_bytes().get(start as usize) == Some(&b'@') {
                start += 1;
            }
            if let Some(text) = source.get(start as usize..decorator.span.end as usize) {
                let mut scanner = tsc_rs_scanner::TsScanner::new(text);
                scanner.scan();
                start += scanner.token_pos() as u32;
            }
        }
        Span::new(start, decorator.span.end)
    }

    fn decorator_signature_diagnostic(
        &self,
        site: DecoratorSite,
        detail: &str,
        span: Span,
    ) -> Diagnostic {
        let (code, kind) = site.diagnostic();
        Diagnostic {
            code,
            message: format!("Unable to resolve signature of {kind} decorator when called as an expression.\n  {}", detail.replace('\n', "\n  ")),
            category: DiagnosticCategory::Error,
            file_name: None,
            span: Some(span),
            related: None,
        }
    }

    fn decorator_has_construct_signature(&self, ty: &Type, depth: usize) -> bool {
        if depth > 12 {
            return false;
        }
        match ty {
            Type::Constructor(_) => true,
            Type::ObjectType(info) => !info.construct_signatures.is_empty(),
            Type::TypeReference(name, args) => {
                name.strip_prefix("typeof ")
                    .is_some_and(|name| self.class_info.contains_key(name))
                    || self
                        .resolve_type_reference_to_object(name, args)
                        .filter(|resolved| resolved != ty)
                        .is_some_and(|resolved| {
                            self.decorator_has_construct_signature(&resolved, depth + 1)
                        })
            }
            Type::Typeof(name) => {
                self.class_info.contains_key(name)
                    || self
                        .lookup_var(name)
                        .filter(|resolved| *resolved != ty)
                        .is_some_and(|resolved| {
                            self.decorator_has_construct_signature(resolved, depth + 1)
                        })
            }
            Type::Intersection(parts) | Type::Union(parts) => parts
                .iter()
                .any(|part| self.decorator_has_construct_signature(part, depth + 1)),
            _ => false,
        }
    }

    fn check_noncallable_decorator(&mut self, decorator: &Expr, ty: &Type, site: DecoratorSite) {
        let mut apparent = ty.clone();
        for _ in 0..12 {
            let Some(resolved) = self
                .typeparam_constraint_apparent(&apparent)
                .or_else(|| self.resolve_type_for_assignability(&apparent))
            else {
                break;
            };
            if resolved == apparent {
                break;
            }
            apparent = resolved;
        }
        if matches!(apparent, Type::Any | Type::Error) {
            return;
        }
        if let Type::TypeReference(name, _) = &apparent {
            if !name.starts_with("typeof ") && !self.type_name_is_known(name) {
                return;
            }
        }
        // A value assignable to Function without call or construct signatures
        // is an untyped call. In particular, a user-supplied empty Function
        // interface under noLib accepts more values than the standard library.
        if !matches!(apparent, Type::Union(_) | Type::Never)
            && !self.decorator_has_construct_signature(&apparent, 0)
        {
            if let Type::TypeReference(name, _) = &apparent {
                if self.class_extends_function(name)
                    || self.interface_extends_function(name, &mut HashSet::new())
                {
                    return;
                }
            }
            let function = self
                .resolve_type_reference_to_object("Function", &[])
                .or_else(|| {
                    let library = stdlib::stdlib_member_availability(&self.compiler_options)?;
                    if !library.declares_global_type("Function") {
                        return None;
                    }
                    // Standalone checking uses the compact builtin table, so
                    // Function need not have a registered interface. Compare
                    // against its members rather than an unresolved reference,
                    // whose error recovery would also accept unknown and {}.
                    let properties = [
                        "apply",
                        "call",
                        "bind",
                        "prototype",
                        "length",
                        "arguments",
                        "caller",
                        "name",
                        "toString",
                    ]
                    .into_iter()
                    .filter(|name| {
                        library.declares_inherited_member("Function", name) == Some(true)
                    })
                    .filter_map(|name| {
                        Self::function_intrinsic_property_type(name)
                            .or_else(|| self.builtins.lookup_instance_property("Object", name))
                            .map(|ty| (name.to_string(), Arc::new(ty)))
                    })
                    .collect();
                    Some(Type::ObjectType(ObjectTypeInfo::new(ObjectTypeData {
                        properties,
                        ..ObjectTypeData::default()
                    })))
                })
                .unwrap_or_else(|| Type::TypeReference("Function".into(), Arc::from([])));
            if self.is_assignable_to(&apparent, &function) {
                return;
            }
        }
        let detail = if let Type::Union(parts) = &apparent {
            let callable = parts
                .iter()
                .any(|part| !self.decorator_call_signatures(part, 0).is_empty());
            if callable {
                let missing = parts
                    .iter()
                    .find(|part| self.decorator_call_signatures(part, 0).is_empty());
                format!("Not all constituents of type '{}' are callable.\n  Type '{}' has no call signatures.", ty.display_string_single_line(), missing.unwrap_or(ty).display_string_single_line())
            } else {
                format!(
                    "No constituent of type '{}' is callable.",
                    ty.display_string_single_line()
                )
            }
        } else {
            let apparent = match &apparent {
                Type::Number | Type::NumberLiteral(_) => "Number".into(),
                Type::String | Type::StringLiteral(_) => "String".into(),
                Type::Boolean | Type::BooleanLiteral(_) => "Boolean".into(),
                Type::BigInt | Type::BigIntLiteral(_) => "BigInt".into(),
                Type::Symbol | Type::UniqueSymbol(_) => "Symbol".into(),
                _ => apparent.display_string_single_line(),
            };
            format!("Type '{apparent}' has no call signatures.")
        };
        self.diagnostics.push(self.decorator_signature_diagnostic(
            site,
            &format!(
                "This expression is not callable.\n  {}",
                detail.replace('\n', "\n  ")
            ),
            self.decorator_expression_span(decorator),
        ));
    }

    fn check_decorator_arity(
        &mut self,
        decorator: &Expr,
        signatures: &[FunctionType],
        site: DecoratorSite,
        legacy: bool,
    ) {
        let mut minimum = usize::MAX;
        let mut maximum = 0;
        let mut any_rest = false;
        let mut closest = 0;
        for (index, signature) in signatures.iter().enumerate() {
            let params = Self::relation_params(&signature.params);
            let min = params
                .iter()
                .filter(|(name, ty)| {
                    !name.starts_with('?')
                        && !name.starts_with("...")
                        && !matches!(ty, Type::Optional(_) | Type::Rest(_))
                })
                .count();
            let rest = params
                .iter()
                .any(|(name, ty)| name.starts_with("...") || matches!(ty, Type::Rest(_)));
            let positional = signature
                .params
                .iter()
                .filter(|(name, _)| name != "this")
                .count();
            let provided =
                site.argument_count(legacy, if legacy { positional } else { params.len() });
            if min <= provided && (rest || provided <= params.len()) {
                return;
            }
            if min < minimum {
                minimum = min;
                closest = index;
            }
            maximum = maximum.max(params.len());
            any_rest |= rest;
        }
        let provided = site.runtime_argument_count(legacy);
        let detail = if minimum < provided && provided < maximum {
            let counts: Vec<_> = signatures
                .iter()
                .map(|sig| Self::relation_params(&sig.params).len())
                .collect();
            let below = counts
                .iter()
                .filter(|count| **count < provided)
                .max()
                .copied()
                .unwrap_or(minimum);
            let above = counts
                .iter()
                .filter(|count| **count > provided)
                .min()
                .copied()
                .unwrap_or(maximum);
            format!("No overload expects {provided} arguments, but overloads do exist that expect either {below} or {above} arguments.")
        } else {
            let range = if any_rest || minimum == maximum {
                minimum.to_string()
            } else {
                format!("{minimum}-{maximum}")
            };
            let at_least = if any_rest { "at least " } else { "" };
            format!("The runtime will invoke the decorator with {provided} arguments, but the decorator expects {at_least}{range}.")
        };
        let span = if provided < minimum || (minimum < provided && provided < maximum) {
            decorator.span
        } else {
            self.decorator_expression_span(decorator)
        };
        let mut diagnostic = self.decorator_signature_diagnostic(site, &detail, span);
        if provided < minimum {
            if let Some(note) = self.decorator_missing_argument_note(decorator, closest, provided) {
                diagnostic.related = Some(vec![note]);
            }
        }
        self.diagnostics.push(diagnostic);
    }

    fn decorator_call_signatures(&self, ty: &Type, depth: usize) -> Vec<FunctionType> {
        if depth > 12 {
            return Vec::new();
        }
        match ty {
            Type::Function(signature) => vec![signature.clone()],
            Type::ObjectType(object) => object.call_signatures.clone(),
            Type::Intersection(parts) => parts
                .iter()
                .flat_map(|part| self.decorator_call_signatures(part, depth + 1))
                .collect(),
            Type::Union(parts) => {
                let mut signatures = Vec::new();
                for part in parts.iter() {
                    if !self.strict_null_checks && matches!(part, Type::Null | Type::Undefined) {
                        continue;
                    }
                    let part_signatures = self.decorator_call_signatures(part, depth + 1);
                    if part_signatures.is_empty() {
                        return Vec::new();
                    }
                    signatures.extend(part_signatures);
                }
                signatures
            }
            Type::Readonly(inner) | Type::Instance(inner) => {
                self.decorator_call_signatures(inner, depth + 1)
            }
            Type::Optional(inner) if !self.strict_null_checks => {
                self.decorator_call_signatures(inner, depth + 1)
            }
            Type::Typeof(name) => self
                .lookup_var(name)
                .filter(|resolved| *resolved != ty)
                .map(|resolved| self.decorator_call_signatures(resolved, depth + 1))
                .unwrap_or_default(),
            Type::TypeReference(..) | Type::TypeParameter(_) => self
                .typeparam_constraint_apparent(ty)
                .or_else(|| self.resolve_type_for_assignability(ty))
                .or_else(|| match ty {
                    Type::TypeReference(name, args) => {
                        self.resolve_type_reference_to_object(name, args)
                    }
                    _ => None,
                })
                .filter(|resolved| resolved != ty)
                .map(|resolved| self.decorator_call_signatures(&resolved, depth + 1))
                .unwrap_or_default(),
            _ => Vec::new(),
        }
    }

    /// Legacy method/accessor decorators use TypedPropertyDescriptor only
    /// after resolution reaches a call signature. Untyped decorators and
    /// uncalled factories never request that global type.
    pub(super) fn check_decorators_at_site(
        &mut self,
        decorators: &[Expr],
        valid: bool,
        site: DecoratorSite,
        target: Option<&DecoratorTarget>,
    ) {
        if !valid {
            self.check_decorators_with_validity(decorators, valid);
            return;
        }
        let legacy = self.compiler_options.experimental_decorators == Some(true);
        for decorator in decorators {
            let ty = self.check_decorator_expression(decorator);
            let signatures = self.decorator_call_signatures(&ty, 0);
            if signatures.is_empty() {
                self.check_noncallable_decorator(decorator, &ty, site);
                continue;
            }
            let uncalled_factory = !matches!(decorator.kind, ExprKind::Paren(_))
                && signatures.iter().all(|signature| {
                    let parameters: Vec<_> = signature
                        .params
                        .iter()
                        .filter(|(name, _)| name != "this")
                        .collect();
                    let argument_count = site.argument_count(legacy, parameters.len());
                    parameters.len() < argument_count
                        && parameters.iter().all(|(name, ty)| {
                            !name.starts_with("...")
                                && !matches!(ty, Type::Rest(_))
                                && (name.starts_with('?') || matches!(ty, Type::Optional(_)))
                        })
                });
            if uncalled_factory {
                let source = self.current_source.as_deref().unwrap_or("");
                let text = source
                    .get(decorator.span.start as usize..decorator.span.end as usize)
                    .unwrap_or("");
                let expression = text.strip_prefix('@').unwrap_or(text);
                let mut scanner = tsc_rs_scanner::TsScanner::new(expression);
                scanner.scan();
                let expression = expression.get(scanner.token_pos()..).unwrap_or(expression);
                self.diagnostics.push(Diagnostic {
                    code: 1329,
                    message: format!("'{expression}' accepts too few arguments to be used as a decorator here. Did you mean to call it first and write '@{expression}()'?"),
                    category: DiagnosticCategory::Error,
                    file_name: None,
                    span: Some(decorator.span),
                    related: None,
                });
            } else {
                if legacy && matches!(site, DecoratorSite::Method | DecoratorSite::AutoAccessor) {
                    self.require_global_type("TypedPropertyDescriptor");
                }
                self.check_decorator_arity(decorator, &signatures, site, legacy);
                if legacy {
                    if let Some(target) = target {
                        self.check_legacy_decorator_call(decorator, &signatures, site, target);
                    }
                }
            }
        }
    }
}
