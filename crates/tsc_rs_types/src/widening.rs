use super::*;

/// Widening is a property of an inferred occurrence, not of the intrinsic
/// null/undefined type. An explicit annotation or assertion makes that same
/// intrinsic type regular. Keep origins separately from the interned types.
#[derive(Clone)]
pub(super) struct WideningInfo {
    ty: Type,
    contains: bool,
    shape: WideningShape,
}

#[derive(Clone)]
enum WideningShape {
    Leaf,
    Alternatives(Vec<Arc<WideningInfo>>),
    Array(Arc<WideningInfo>),
    Tuple(Vec<Arc<WideningInfo>>),
    Object(Vec<WideningProperty>),
}

type WideningProperty = (String, Option<(String, Span)>, Arc<WideningInfo>);

#[derive(Clone)]
pub(super) struct ParameterWidening {
    pattern: Pat,
    initializer: Option<Arc<WideningInfo>>,
}

impl WideningInfo {
    fn leaf(ty: Type, contains: bool) -> Arc<Self> {
        Arc::new(Self {
            ty,
            contains,
            shape: WideningShape::Leaf,
        })
    }
}

impl TypeChecker {
    /// A computed name with a nonliteral key contributes an index signature,
    /// rather than a property named "[computed]". Keep the raw value types so
    /// declaration widening can still distinguish null/undefined origins.
    pub(super) fn object_literal_index_type(&self, expression: &Expr, ty: Type) -> Type {
        let ExprKind::ObjectLit(properties) = &expression.kind else {
            return ty;
        };
        let Type::ObjectType(mut info) = ty.clone() else {
            return ty;
        };
        let mut index_key = None;
        let mut values = Vec::new();
        for property in properties {
            let ObjLitProp::Property(property) = property else {
                continue;
            };
            let PropName::Computed(key, _) = &property.key else {
                continue;
            };
            if self.computed_property_name(key).is_some() {
                continue;
            }
            let key_type = self.infer_expr_type(key);
            if !matches!(key_type, Type::String | Type::Number | Type::Symbol) {
                return ty;
            }
            if index_key
                .as_ref()
                .is_some_and(|previous| *previous != key_type)
            {
                return ty;
            }
            index_key = Some(key_type);
            values.push(self.infer_expr_type(&property.value));
        }
        let Some(key) = index_key else {
            return ty;
        };
        info.properties.retain(|(name, _)| name != "[computed]");
        for (name, value) in &info.properties {
            if matches!(key, Type::String)
                || matches!(key, Type::Number) && name.parse::<f64>().is_ok()
            {
                values.push(Type::clone(value));
            }
        }
        info.index_signature = Some((Arc::new(key), Arc::new(self.array_literal_union(values))));
        info.index_signature_name = Some("x".into());
        Type::ObjectType(info)
    }

    fn widening_enabled(&self) -> bool {
        self.no_implicit_any && !self.strict_null_checks
    }

    fn widening_info(&self, expression: &Expr) -> Arc<WideningInfo> {
        self.widening_types
            .get(&(expression.span.start, expression.span.end))
            .cloned()
            .unwrap_or_else(|| WideningInfo::leaf(self.infer_expr_type(expression), false))
    }

    fn widening_alternatives(
        &self,
        entries: Vec<Arc<WideningInfo>>,
        ty: Type,
    ) -> Arc<WideningInfo> {
        let entries = entries
            .iter()
            .filter(|entry| {
                !entry.contains
                    || !entries
                        .iter()
                        .any(|other| !other.contains && self.is_assignable_to(&entry.ty, &other.ty))
            })
            .cloned()
            .collect::<Vec<_>>();
        Arc::new(WideningInfo {
            contains: !matches!(ty, Type::Any | Type::Error)
                && entries.iter().any(|entry| entry.contains),
            ty,
            shape: WideningShape::Alternatives(entries),
        })
    }

    pub(super) fn record_widening_expression(&mut self, expression: &Expr, ty: &Type) {
        if !self.widening_enabled() {
            return;
        }
        let info = match &expression.kind {
            ExprKind::NullLit => WideningInfo::leaf(ty.clone(), true),
            ExprKind::Ident(name)
                if name == "undefined"
                    && matches!(ty, Type::Undefined)
                    && !self.non_global_scope_declares_name(name)
                    && !self.function_parameter_name_is_active(name)
                    && !self.local_value_names.contains(name.as_str())
                    && !self.user_global_value_names.contains(name.as_str()) =>
            {
                WideningInfo::leaf(ty.clone(), true)
            }
            ExprKind::Unary(unary) if unary.op == UnaryOp::Void => {
                WideningInfo::leaf(ty.clone(), true)
            }
            ExprKind::Void(_) => WideningInfo::leaf(ty.clone(), true),
            ExprKind::Paren(inner) | ExprKind::NonNull(inner) => self.widening_info(inner),
            ExprKind::Satisfies(assertion) => self.widening_info(&assertion.expr),
            ExprKind::As(assertion) if Self::is_const_assertion_expr(expression) => {
                self.const_asserted_widening_info(&assertion.expr, ty)
            }
            ExprKind::TypeAssertion(assertion) if Self::is_const_assertion_expr(expression) => {
                self.const_asserted_widening_info(&assertion.expr, ty)
            }
            ExprKind::Cond(condition) => self.widening_alternatives(
                vec![
                    self.widening_info(&condition.consequent),
                    self.widening_info(&condition.alternate),
                ],
                ty.clone(),
            ),
            ExprKind::Assign(assignment) if assignment.op == AssignOp::Assign => {
                self.widening_info(&assignment.right)
            }
            ExprKind::ArrayLit(elements) => {
                let mut entries = elements
                    .iter()
                    .map(|element| match element {
                        Some(element) => self.widening_info(element),
                        None => WideningInfo::leaf(Type::Undefined, true),
                    })
                    .collect::<Vec<_>>();
                if matches!(ty, Type::Tuple(_)) {
                    let info = Arc::new(WideningInfo {
                        ty: ty.clone(),
                        contains: entries.iter().any(|entry| entry.contains),
                        shape: WideningShape::Tuple(entries),
                    });
                    self.widening_types
                        .insert((expression.span.start, expression.span.end), info);
                    return;
                }
                if elements.is_empty() {
                    entries.push(WideningInfo::leaf(Type::Undefined, true));
                }
                let element_type = match ty {
                    Type::Array(element) => Type::clone(element),
                    _ => Type::Any,
                };
                let element = self.widening_alternatives(entries, element_type);
                Arc::new(WideningInfo {
                    ty: ty.clone(),
                    contains: element.contains,
                    shape: WideningShape::Array(element),
                })
            }
            ExprKind::ObjectLit(properties) if !matches!(ty, Type::Any | Type::Error) => {
                let mut entries: Vec<WideningProperty> = Vec::new();
                let mut contains = false;
                for property in properties {
                    match property {
                        ObjLitProp::Property(property) => {
                            let key = self.prop_name_to_string(&property.key);
                            let info = self.widening_info(&property.value);
                            contains |= info.contains;
                            let name_span = property.key.span();
                            let written = self
                                .current_source
                                .as_deref()
                                .and_then(|source| {
                                    source.get(name_span.start as usize..name_span.end as usize)
                                })
                                .unwrap_or(&key)
                                .to_string();
                            let mut origin = Some((
                                written,
                                Span::new(name_span.start, property.value.span.end),
                            ));
                            if let PropName::Computed(key, _) = &property.key {
                                if self.computed_property_name(key).is_none() {
                                    origin = None;
                                }
                            }
                            entries.retain(|(existing, _, _)| *existing != key);
                            entries.push((key, origin, info));
                        }
                        ObjLitProp::Spread(expression, _) => {
                            let spread = self.widening_info(expression);
                            let properties = match &spread.ty {
                                Type::ObjectType(info) => Some(info.properties.clone()),
                                Type::TypeReference(name, args) => self
                                    .resolve_type_reference_to_object(name, args)
                                    .and_then(|ty| {
                                        if let Type::ObjectType(info) = ty {
                                            Some(info.properties.clone())
                                        } else {
                                            None
                                        }
                                    }),
                                _ => None,
                            };
                            if let Some(properties) = properties {
                                for (name, property) in properties {
                                    let optional = matches!(property.as_ref(), Type::Optional(_));
                                    if !optional {
                                        entries.retain(|(existing, _, _)| *existing != name);
                                        entries.push((
                                            name,
                                            None,
                                            WideningInfo::leaf(Type::clone(&property), false),
                                        ));
                                    } else if let Some((_, origin, entry)) =
                                        entries.iter_mut().find(|(key, _, _)| *key == name)
                                    {
                                        *origin = None;
                                        *entry = WideningInfo::leaf(
                                            self.extract_member_type_opt(ty, &name)
                                                .unwrap_or_else(|| Type::clone(&property)),
                                            false,
                                        );
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                }
                Arc::new(WideningInfo {
                    ty: ty.clone(),
                    contains,
                    shape: WideningShape::Object(entries),
                })
            }
            _ => WideningInfo::leaf(ty.clone(), false),
        };
        self.widening_types
            .insert((expression.span.start, expression.span.end), info);
    }

    fn const_asserted_widening_info(&self, expression: &Expr, ty: &Type) -> Arc<WideningInfo> {
        if let ExprKind::Paren(inner) = &expression.kind {
            return self.const_asserted_widening_info(inner, ty);
        }
        let shape = match ty {
            Type::Readonly(inner) => inner.as_ref(),
            other => other,
        };
        if let (ExprKind::ArrayLit(elements), Type::Tuple(types)) = (&expression.kind, shape) {
            let entries = elements
                .iter()
                .zip(types.iter())
                .map(|(expression, ty)| match expression {
                    Some(expression) => self.const_asserted_widening_info(expression, ty),
                    None => WideningInfo::leaf(Type::Undefined, true),
                })
                .collect::<Vec<_>>();
            return Arc::new(WideningInfo {
                ty: ty.clone(),
                contains: entries.iter().any(|entry| entry.contains),
                shape: WideningShape::Tuple(entries),
            });
        }
        let inner = self.widening_info(expression);
        if let (ExprKind::ObjectLit(properties), Type::ObjectType(object)) = (&expression.kind, ty)
        {
            if let WideningShape::Object(entries) = &inner.shape {
                let mut entries = entries.clone();
                for property in properties {
                    let ObjLitProp::Property(property) = property else {
                        continue;
                    };
                    let key = self.prop_name_to_string(&property.key);
                    if let (Some((_, _, entry)), Some((_, property_type))) = (
                        entries.iter_mut().find(|(name, _, _)| *name == key),
                        object.properties.iter().find(|(name, _)| *name == key),
                    ) {
                        *entry = self.const_asserted_widening_info(&property.value, property_type);
                    }
                }
                return Arc::new(WideningInfo {
                    ty: ty.clone(),
                    contains: inner.contains || entries.iter().any(|(_, _, entry)| entry.contains),
                    shape: WideningShape::Object(entries),
                });
            }
        }
        inner
    }

    fn report_widening_properties(&mut self, info: &WideningInfo) -> bool {
        if !info.contains {
            return false;
        }
        match &info.shape {
            WideningShape::Array(element) => self.report_widening_properties(element),
            WideningShape::Alternatives(entries) | WideningShape::Tuple(entries) => entries
                .iter()
                .any(|entry| self.report_widening_properties(entry)),
            WideningShape::Object(properties) => {
                let mut reported = false;
                for (_, origin, property) in properties {
                    if !property.contains {
                        continue;
                    }
                    let nested = self.report_widening_properties(property);
                    reported |= nested;
                    if !nested {
                        if let Some((name, span)) = origin {
                            let ty = Self::widened_info_type(property).display_string_single_line();
                            self.push_widening_diagnostic(7018, format!("Object literal's property '{name}' implicitly has an '{ty}' type."), *span);
                            reported = true;
                        }
                    }
                }
                reported
            }
            WideningShape::Leaf => false,
        }
    }

    fn push_widening_diagnostic(&mut self, code: u32, message: String, span: Span) {
        if self
            .reported_duplicate_spans
            .insert((code, span.start, span.end))
        {
            self.diagnostics.push(Diagnostic {
                code,
                message,
                category: DiagnosticCategory::Error,
                file_name: None,
                span: Some(span),
                related: None,
            });
        }
    }

    pub(super) fn report_widening_declaration(
        &mut self,
        expression: &Expr,
        code: u32,
        name: &str,
        span: Span,
    ) {
        if !self.widening_enabled() {
            return;
        }
        let info = self.widening_info(expression);
        self.report_widening_info(&info, code, name, span);
    }

    pub(super) fn defer_parameter_widening(&mut self, parameter: &Param) {
        if !self.widening_enabled() || parameter.type_ann.is_some() {
            return;
        }
        self.widening_parameters.insert(
            (parameter.span.start, parameter.span.end),
            ParameterWidening {
                pattern: parameter.name.clone(),
                initializer: parameter
                    .initializer
                    .as_deref()
                    .map(|expr| self.widening_info(expr)),
            },
        );
    }

    pub(super) fn mark_contextual_widening_parameter(&mut self, parameter: &Param) {
        if self.widening_enabled() {
            self.contextual_widening_parameters
                .insert((parameter.span.start, parameter.span.end));
        }
    }

    pub(super) fn finish_parameter_widening(&mut self) {
        for ((start, end), parameter) in std::mem::take(&mut self.widening_parameters) {
            if !self.contextual_widening_parameters.contains(&(start, end)) {
                if let PatKind::Ident(name) = &parameter.pattern.kind {
                    if let Some(info) = parameter.initializer {
                        self.report_widening_info(&info, 7006, name, Span::new(start, end));
                    }
                } else {
                    self.report_binding_widening(&parameter.pattern, parameter.initializer);
                }
            }
        }
    }

    fn report_widening_info(&mut self, info: &WideningInfo, code: u32, name: &str, span: Span) {
        if !info.contains || self.report_widening_properties(info) {
            return;
        }
        let ty = Self::widened_info_type(info).display_string_single_line();
        let message = match code {
            7006 => format!("Parameter '{name}' implicitly has an '{ty}' type."),
            7008 => format!("Member '{name}' implicitly has an '{ty}' type."),
            7010 => format!("'{name}', which lacks return-type annotation, implicitly has an '{ty}' return type."),
            7011 => format!("Function expression, which lacks return-type annotation, implicitly has an '{ty}' return type."),
            7031 => format!("Binding element '{name}' implicitly has an '{ty}' type."),
            _ => format!("Variable '{name}' implicitly has an '{ty}' type."),
        };
        self.push_widening_diagnostic(code, message, span);
    }

    pub(super) fn report_variable_widening(
        &mut self,
        declaration: &VarDeclarator,
        variables: &VarStmt,
    ) {
        if !self.widening_enabled()
            || declaration.type_ann.is_some()
            || self.is_auto_array_declaration(declaration, variables.modifiers)
        {
            return;
        }
        let Some(initializer) = declaration.init.as_deref() else {
            self.report_binding_widening(&declaration.name, None);
            return;
        };
        let info = self.widening_info(initializer);
        let mut unwrapped = initializer;
        while let ExprKind::Paren(inner) = &unwrapped.kind {
            unwrapped = inner;
        }
        let auto_scalar = matches!(unwrapped.kind, ExprKind::NullLit)
            || matches!(&unwrapped.kind, ExprKind::Ident(name) if name == "undefined");
        if variables.kind != VarKind::Const
            && variables.modifiers & (MOD_EXPORT | MOD_DECLARE) == 0
            && auto_scalar
            && matches!(info.ty, Type::Null | Type::Undefined)
        {
            return;
        }
        match &declaration.name.kind {
            PatKind::Ident(name) => {
                self.report_widening_info(&info, 7005, name, declaration.name.span)
            }
            _ => self.report_binding_widening(&declaration.name, Some(info)),
        }
    }

    fn binding_widening_property(
        &self,
        info: &WideningInfo,
        key: &str,
    ) -> Option<Arc<WideningInfo>> {
        if let WideningShape::Alternatives(entries) = &info.shape {
            return self.combine_binding_widening(
                entries
                    .iter()
                    .filter_map(|entry| self.binding_widening_property(entry, key))
                    .collect(),
            );
        }
        if let Ok(index) = key.parse::<usize>() {
            if matches!(
                info.shape,
                WideningShape::Tuple(_) | WideningShape::Array(_)
            ) {
                return self.binding_widening_element(info, index);
            }
        }
        if let WideningShape::Object(properties) = &info.shape {
            if let Some((_, _, property)) = properties.iter().find(|(name, _, _)| name == key) {
                return Some(property.clone());
            }
        }
        self.extract_member_type_opt(&info.ty, key)
            .map(|ty| WideningInfo::leaf(ty, false))
    }

    fn binding_widening_element(
        &self,
        info: &WideningInfo,
        index: usize,
    ) -> Option<Arc<WideningInfo>> {
        if let WideningShape::Alternatives(entries) = &info.shape {
            return self.combine_binding_widening(
                entries
                    .iter()
                    .filter_map(|entry| self.binding_widening_element(entry, index))
                    .collect(),
            );
        }
        if let WideningShape::Tuple(entries) = &info.shape {
            return entries.get(index).cloned();
        }
        let ty = match &info.ty {
            Type::Readonly(inner) => inner.as_ref(),
            ty => ty,
        };
        match ty {
            Type::Array(element) => {
                if let WideningShape::Array(entry) = &info.shape {
                    Some(entry.clone())
                } else {
                    Some(WideningInfo::leaf(Type::clone(element), false))
                }
            }
            Type::Tuple(elements) => elements
                .get(index)
                .cloned()
                .map(|ty| WideningInfo::leaf(ty, false)),
            _ => Some(WideningInfo::leaf(
                self.extract_element_type(ty, index),
                false,
            )),
        }
    }

    fn binding_widening_rest(&self, info: &WideningInfo, start: usize) -> Arc<WideningInfo> {
        if let WideningShape::Alternatives(entries) = &info.shape {
            return self
                .combine_binding_widening(
                    entries
                        .iter()
                        .map(|entry| self.binding_widening_rest(entry, start))
                        .collect(),
                )
                .unwrap_or_else(|| WideningInfo::leaf(Type::Never, false));
        }
        let ty = match &info.ty {
            Type::Readonly(inner) => inner.as_ref(),
            ty => ty,
        };
        let ty = self.array_rest_binding_type(ty, start);
        if let WideningShape::Tuple(entries) = &info.shape {
            let entries: Vec<_> = entries.iter().skip(start).cloned().collect();
            Arc::new(WideningInfo {
                ty,
                contains: entries.iter().any(|entry| entry.contains),
                shape: WideningShape::Tuple(entries),
            })
        } else {
            Arc::new(WideningInfo { ty, ..info.clone() })
        }
    }

    fn binding_widening_default(
        &self,
        projected: Option<Arc<WideningInfo>>,
        expression: &Expr,
    ) -> Arc<WideningInfo> {
        let initializer = self.widening_info(expression);
        let Some(projected) = projected else {
            return initializer;
        };
        if matches!(projected.ty, Type::Undefined) {
            return initializer;
        }
        let ty = self.array_literal_union(vec![projected.ty.clone(), initializer.ty.clone()]);
        self.widening_alternatives(vec![projected, initializer], ty)
    }

    fn combine_binding_widening(
        &self,
        entries: Vec<Arc<WideningInfo>>,
    ) -> Option<Arc<WideningInfo>> {
        if entries.is_empty() {
            return None;
        }
        let ty = self.array_literal_union(entries.iter().map(|entry| entry.ty.clone()).collect());
        Some(self.widening_alternatives(entries, ty))
    }

    /// Project the source occurrence through the binding pattern before
    /// reporting. A nested pattern can consume a nullable member directly,
    /// whereas an identifier binding the whole object reports its property.
    fn report_binding_widening(&mut self, pattern: &Pat, info: Option<Arc<WideningInfo>>) {
        match &pattern.kind {
            PatKind::Ident(name) => {
                if let Some(info) = info {
                    self.report_widening_info(&info, 7031, name, pattern.span);
                }
            }
            PatKind::Array(elements) => {
                for (index, element) in elements.iter().enumerate() {
                    match element {
                        Some(ArrayPatElem::Pat(pattern)) => {
                            let projected = info
                                .as_deref()
                                .and_then(|info| self.binding_widening_element(info, index));
                            self.report_binding_widening(pattern, projected);
                        }
                        Some(ArrayPatElem::Rest(pattern)) => {
                            let rest = info
                                .as_deref()
                                .map(|info| self.binding_widening_rest(info, index));
                            self.report_binding_widening(pattern, rest);
                        }
                        None => {}
                    }
                }
            }
            PatKind::Object(properties) => {
                for property in properties {
                    match property {
                        ObjPatProp::KeyValue(key, pattern) => {
                            let Some(key) = self.pattern_property_name(key) else {
                                continue;
                            };
                            let projected = info
                                .as_deref()
                                .and_then(|info| self.binding_widening_property(info, &key));
                            self.report_binding_widening(pattern, projected);
                        }
                        ObjPatProp::Shorthand(name, span)
                        | ObjPatProp::ShorthandAssign(name, _, span) => {
                            let projected = info
                                .as_deref()
                                .and_then(|info| self.binding_widening_property(info, name));
                            let projected =
                                if let ObjPatProp::ShorthandAssign(_, initializer, _) = property {
                                    Some(self.binding_widening_default(projected, initializer))
                                } else {
                                    projected
                                };
                            if let Some(info) = projected {
                                self.report_widening_info(&info, 7031, name, *span);
                            }
                        }
                        // Object rest produces a new regular object shape.
                        ObjPatProp::Rest(_) => {}
                    }
                }
            }
            PatKind::Assign(pattern, initializer) => {
                let projected = self.binding_widening_default(info, initializer);
                self.report_binding_widening(pattern, Some(projected));
            }
            PatKind::Rest(pattern) => self.report_binding_widening(pattern, info),
        }
    }

    pub(super) fn collect_widening_return(&mut self, expression: &Expr) {
        if self.widening_enabled() && self.return_type_stack.last().is_some_and(Option::is_none) {
            let info = self.widening_info(expression);
            self.widening_returns
                .entry(self.return_type_stack.len())
                .or_default()
                .push(info);
        }
    }

    pub(super) fn finish_widening_function(&mut self, name: Option<&str>, span: Span) {
        let Some(entries) = self.widening_returns.remove(&self.return_type_stack.len()) else {
            return;
        };
        let ty = Type::flatten_union(entries.iter().map(|entry| entry.ty.clone()).collect());
        let info = self.widening_alternatives(entries, ty);
        self.report_widening_info(
            &info,
            if name.is_some() { 7010 } else { 7011 },
            name.unwrap_or(""),
            span,
        );
    }

    pub(super) fn finish_contextual_widening_function(
        &mut self,
        name: Option<&str>,
        span: Span,
        contextual: Option<&Type>,
    ) {
        if contextual.is_some_and(|ty| {
            matches!(ty, Type::Function(signature)
            if !self.contains_unresolved_type_param(&signature.return_type))
        }) {
            self.widening_returns.remove(&self.return_type_stack.len());
        } else {
            self.finish_widening_function(name, span);
        }
    }

    fn widened_info_type(info: &WideningInfo) -> Type {
        match &info.shape {
            WideningShape::Array(element) => {
                return Type::Array(Arc::new(Self::widened_info_type(element)))
            }
            WideningShape::Tuple(entries) => {
                let (ty, readonly) = match &info.ty {
                    Type::Readonly(inner) => (inner.as_ref(), true),
                    ty => (ty, false),
                };
                if let Type::Tuple(types) = ty {
                    let elements = types
                        .iter()
                        .enumerate()
                        .map(|(index, ty)| match entries.get(index) {
                            Some(entry)
                                if entry.contains
                                    || matches!(entry.shape, WideningShape::Tuple(_)) =>
                            {
                                Self::widened_info_type(entry)
                            }
                            _ => ty.clone(),
                        })
                        .collect();
                    let tuple = Type::Tuple(elements);
                    return if readonly {
                        Type::Readonly(Arc::new(tuple))
                    } else {
                        tuple
                    };
                }
            }
            WideningShape::Alternatives(entries)
                if entries.len() == 1
                    && (entries[0].ty == info.ty
                        || matches!(&entries[0].ty, Type::Readonly(inner) if inner.as_ref() == &info.ty)) =>
            {
                return Self::widened_info_type(&entries[0]);
            }
            WideningShape::Alternatives(entries) => {
                if let Type::Union(members) = &info.ty {
                    return Type::flatten_union(
                        members
                            .iter()
                            .map(|member| {
                                entries
                                    .iter()
                                    .find(|entry| entry.ty == *member
                                        || matches!(&entry.ty, Type::Readonly(inner) if inner.as_ref() == member))
                                    .map(|entry| Self::widened_info_type(entry))
                                    .unwrap_or_else(|| Self::widen_diagnostic_type(member))
                            })
                            .collect(),
                    );
                }
            }
            _ => {}
        }
        Self::widen_diagnostic_type(&info.ty)
    }

    fn widen_diagnostic_type(ty: &Type) -> Type {
        match ty {
            Type::Readonly(inner) => Type::Readonly(Arc::new(Self::widen_diagnostic_type(inner))),
            Type::ObjectType(info) if info.index_signature.is_some() => {
                let Type::ObjectType(mut widened) = Self::widen_nested_literals(ty) else {
                    unreachable!();
                };
                widened.index_signature = info.index_signature.as_ref().map(|(key, value)| {
                    (key.clone(), Arc::new(Self::widen_diagnostic_type(value)))
                });
                Type::ObjectType(widened)
            }
            Type::Union(members)
                if members
                    .iter()
                    .all(|member| matches!(member, Type::Null | Type::Undefined)) =>
            {
                Type::Any
            }
            _ => Self::widen_nested_literals(ty),
        }
    }
}
