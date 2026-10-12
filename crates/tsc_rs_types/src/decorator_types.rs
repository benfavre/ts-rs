use super::*;

#[derive(Clone, Copy)]
pub(super) enum DecoratorSite {
    Class,
    Property,
    Parameter,
    Method,
    AutoAccessor,
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
                .push(notes);
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
                            .push(notes);
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
                    return overloads.get(signature)?.get(provided).cloned();
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
            }
        }
    }
}
