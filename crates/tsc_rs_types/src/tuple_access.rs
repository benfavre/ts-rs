use super::*;

#[derive(Clone)]
pub(super) struct TupleBindingMetadata {
    pub(super) display: String,
    readonly: bool,
    pub(super) children: Vec<Option<TupleBindingMetadata>>,
}

/// A numeric property name uses JavaScript's canonical number spelling.
/// In particular, the string "-0" is not the property reached by numeric -0.
fn numeric_key(index: &Type) -> Option<String> {
    match index {
        Type::NumberLiteral(key) => Some(TypeChecker::canonical_numeric_property_name(key)),
        Type::StringLiteral(key)
            if key == "NaN"
                || key.parse::<f64>().is_ok()
                    && TypeChecker::canonical_numeric_property_name(key) == *key =>
        {
            Some(key.clone())
        }
        _ => None,
    }
}

fn elements(ty: &Type) -> Option<&[Type]> {
    match ty {
        Type::Tuple(items) => Some(items),
        Type::Readonly(inner) => elements(inner),
        _ => None,
    }
}

fn array_element(ty: &Type) -> Option<&Type> {
    match ty {
        Type::Array(element) => Some(element),
        Type::Readonly(inner) => array_element(inner),
        _ => None,
    }
}

fn element_value(ty: &Type, strict: bool) -> Type {
    match ty {
        Type::Optional(inner) if strict => {
            Type::flatten_union(vec![Type::clone(inner), Type::Undefined])
        }
        Type::Optional(inner) => Type::clone(inner),
        Type::Rest(inner) => match inner.as_ref() {
            Type::Array(value) => Type::clone(value),
            Type::Tuple(items) => Type::flatten_union(
                items
                    .iter()
                    .map(|item| element_value(item, strict))
                    .collect(),
            ),
            other => Type::IndexedAccess(Arc::new(other.clone()), Arc::new(Type::Number)),
        },
        other => other.clone(),
    }
}

fn combine(values: Vec<Type>, writing: bool) -> Type {
    if writing {
        let result = simplify_intersection(values);
        if TypeChecker::is_disjoint_primitive_intersection(&result) {
            Type::Never
        } else {
            result
        }
    } else {
        Type::flatten_union(values)
    }
}

/// Shared by expression reads and indexed-access types. Rest/optional wrappers
/// describe tuple slots; they must not escape as the value of an indexed read.
pub(crate) fn indexed_type(
    receiver: &Type,
    index: &Type,
    strict: bool,
    unchecked: bool,
    writing: bool,
) -> Option<Type> {
    if let Type::Union(keys) = index {
        return keys
            .iter()
            .map(|key| indexed_type(receiver, key, strict, unchecked, writing))
            .collect::<Option<Vec<_>>>()
            .map(|values| combine(values, writing));
    }
    if let Type::Readonly(inner) = receiver {
        return indexed_type(inner, index, strict, unchecked, writing);
    }
    if let Type::Array(element) = receiver {
        if matches!(index, Type::StringLiteral(key) if key == "length") {
            return Some(Type::Number);
        }
        return (numeric_key(index).is_some() || matches!(index, Type::Number | Type::Any)).then(
            || {
                if unchecked && !writing {
                    Type::flatten_union(vec![Type::clone(element), Type::Undefined])
                } else {
                    Type::clone(element)
                }
            },
        );
    }
    if let Type::Union(members) = receiver {
        // An array constituent supplies a numeric index signature for the
        // entire union. Tuple-only unions instead retain their known slots.
        let has_array = members.iter().any(|ty| array_element(ty).is_some());
        let negative = numeric_key(index)
            .is_some_and(|key| key.parse::<f64>().is_ok_and(|number| number < 0.0));
        let numeric = numeric_key(index).is_some() || matches!(index, Type::Number | Type::Any);
        if has_array && !numeric {
            return None;
        }
        return members
            .iter()
            .map(|member| match member {
                Type::Array(value) => Some(if unchecked && !writing {
                    Type::flatten_union(vec![Type::clone(value), Type::Undefined])
                } else {
                    Type::clone(value)
                }),
                _ => indexed_type(
                    member,
                    if has_array || negative {
                        &Type::Number
                    } else {
                        index
                    },
                    strict,
                    unchecked,
                    writing,
                ),
            })
            .collect::<Option<Vec<_>>>()
            // A union receiver exposes the union of its property types, even
            // for a write. A union of possible keys still intersects above.
            .map(Type::flatten_union);
    }
    let items = elements(receiver)?;
    let rest = items.iter().position(|ty| matches!(ty, Type::Rest(_)));
    if matches!(index, Type::StringLiteral(key) if key == "length") {
        return Some(if rest.is_some() {
            Type::Number
        } else {
            let required = items
                .iter()
                .filter(|item| !matches!(item, Type::Optional(_)))
                .count();
            Type::flatten_union(
                (required..=items.len())
                    .map(|length| Type::NumberLiteral(length.to_string()))
                    .collect(),
            )
        });
    }
    let key = numeric_key(index);
    let number = key.as_ref().and_then(|key| key.parse::<f64>().ok());
    let dynamic = matches!(index, Type::Number | Type::Any);
    if !dynamic && number.is_none() {
        return None;
    }
    let number = number.unwrap_or(f64::NAN);
    let position = key.as_ref().and_then(|key| key.parse::<usize>().ok());
    if let Some(position) = position {
        if position < rest.unwrap_or(items.len()) {
            return Some(element_value(&items[position], strict));
        }
    }
    if !dynamic && !number.is_nan() && rest.is_none() {
        return Some(Type::Undefined);
    }
    let start = if !dynamic && number >= 0.0 {
        rest.unwrap_or(0)
    } else {
        0
    };
    let values = items[start..]
        .iter()
        .map(|item| element_value(item, strict))
        .collect();
    let value = Type::flatten_union(values);
    let fixed_count = items
        .iter()
        .filter(|ty| !matches!(ty, Type::Rest(_)))
        .count();
    Some(
        if unchecked && !writing && (dynamic || number < 0.0 || number >= fixed_count as f64) {
            Type::flatten_union(vec![value, Type::Undefined])
        } else {
            value
        },
    )
}

impl TypeChecker {
    pub(super) fn binding_tuple_metadata(
        &self,
        annotation: Option<&TypeNode>,
        initializer: Option<&Expr>,
    ) -> Option<TupleBindingMetadata> {
        annotation
            .and_then(|annotation| self.tuple_annotation_metadata(annotation))
            .or_else(|| {
                initializer.and_then(|initializer| self.tuple_expression_metadata(initializer))
            })
    }

    fn tuple_annotation_display(&self, annotation: &TypeNode) -> Option<String> {
        match &annotation.kind {
            TypeNodeKind::Paren(inner) => self.tuple_annotation_display(inner),
            TypeNodeKind::Readonly(inner)
            | TypeNodeKind::TypeOperator(TypeOperatorKind::Readonly, inner) => self
                .tuple_annotation_display(inner)
                .map(|display| format!("readonly {display}")),
            TypeNodeKind::Tuple(items) if items.iter().any(|item| item.label.is_some()) => {
                let mut shown = Vec::new();
                for item in items {
                    let value = self.resolve_type_node(&item.type_node);
                    let value = if item.optional && self.strict_null_checks {
                        Type::flatten_union(vec![value, Type::Undefined])
                    } else {
                        value
                    };
                    let value = value.display_string_single_line();
                    shown.push(match &item.label {
                        Some(label) => format!(
                            "{}{}{}: {value}",
                            if item.dotdotdot { "..." } else { "" },
                            label,
                            if item.optional { "?" } else { "" }
                        ),
                        None => format!(
                            "{}{}{}",
                            if item.dotdotdot { "..." } else { "" },
                            value,
                            if item.optional { "?" } else { "" }
                        ),
                    });
                }
                Some(format!("[{}]", shown.join(", ")))
            }
            TypeNodeKind::Tuple(_) => self
                .array_or_tuple_type(&self.resolve_type_node(annotation), 0)
                .map(|shape| self.tuple_diagnostic_name(&shape)),
            _ => None,
        }
    }

    fn tuple_annotation_metadata(&self, annotation: &TypeNode) -> Option<TupleBindingMetadata> {
        let display = self.tuple_annotation_display(annotation)?;
        fn readonly(node: &TypeNode) -> bool {
            match &node.kind {
                TypeNodeKind::Paren(inner) => readonly(inner),
                TypeNodeKind::Readonly(_)
                | TypeNodeKind::TypeOperator(TypeOperatorKind::Readonly, _) => true,
                _ => false,
            }
        }
        fn tuple_items(node: &TypeNode) -> &[TupleElement] {
            match &node.kind {
                TypeNodeKind::Tuple(items) => items,
                TypeNodeKind::Paren(inner)
                | TypeNodeKind::Readonly(inner)
                | TypeNodeKind::TypeOperator(TypeOperatorKind::Readonly, inner) => {
                    tuple_items(inner)
                }
                _ => &[],
            }
        }
        Some(TupleBindingMetadata {
            display,
            readonly: readonly(annotation),
            children: tuple_items(annotation)
                .iter()
                .map(|item| self.tuple_annotation_metadata(&item.type_node))
                .collect(),
        })
    }

    /// Preserve labels and readonly declaration syntax alongside the structural
    /// tuple. Lexical ownership prevents a shadowing binding borrowing metadata.
    pub(super) fn mark_tuple_annotation(&mut self, name: &str, annotation: &TypeNode) {
        if let Some(metadata) = self.tuple_annotation_metadata(annotation) {
            self.scopes[self.current_scope]
                .tuple_metadata
                .insert(name.to_owned(), metadata);
        }
    }

    pub(super) fn mark_tuple_initializer(&mut self, declaration: &VarDeclarator) {
        if declaration.type_ann.is_some() {
            return;
        }
        let (PatKind::Ident(name), Some(initializer)) =
            (&declaration.name.kind, declaration.init.as_deref())
        else {
            return;
        };
        let metadata = if Self::is_const_assertion_expr(initializer) {
            self.lookup_var(name)
                .filter(|ty| elements(ty).is_some())
                .map(|ty| TupleBindingMetadata {
                    display: self.tuple_diagnostic_name(ty),
                    readonly: matches!(ty, Type::Readonly(_)),
                    children: Vec::new(),
                })
        } else {
            self.tuple_expression_metadata(initializer)
        };
        if let Some(metadata) = metadata {
            self.scopes[self.current_scope]
                .tuple_metadata
                .insert(name.to_string(), metadata);
        }
    }

    fn tuple_expression_metadata(&self, expression: &Expr) -> Option<TupleBindingMetadata> {
        match &expression.kind {
            ExprKind::Ident(name) => self.scopes[self.nearest_binding_scope(name)?]
                .tuple_metadata
                .get(name.as_str())
                .cloned(),
            ExprKind::Paren(inner) | ExprKind::NonNull(inner) => {
                self.tuple_expression_metadata(inner)
            }
            ExprKind::Satisfies(assertion) => self.tuple_expression_metadata(&assertion.expr),
            ExprKind::As(assertion) => self.tuple_annotation_metadata(&assertion.type_node),
            ExprKind::TypeAssertion(assertion) => {
                self.tuple_annotation_metadata(&assertion.type_node)
            }
            _ => None,
        }
    }

    pub(super) fn is_readonly_tuple_expression(&self, expression: &Expr) -> bool {
        self.tuple_expression_metadata(expression)
            .is_some_and(|metadata| metadata.readonly)
    }

    /// Tuple slots are named readonly properties; the rest of the numeric
    /// domain uses the tuple's readonly index signature. Keep these separate:
    /// an out-of-bounds write still has its ordinary type and bounds errors.
    pub(super) fn report_readonly_sequence_write(
        &mut self,
        target: &Expr,
        deleting: bool,
    ) -> Option<bool> {
        let target = Self::skip_reference_wrappers(target);
        let (object, index, key_span) = match &target.kind {
            ExprKind::ElemAccess(access) => (
                access.object.as_ref(),
                self.infer_expr_type(&access.index),
                access.index.span,
            ),
            ExprKind::Member(member) => (
                member.object.as_ref(),
                Type::StringLiteral(member.property.to_string()),
                Span::new(
                    target.span.end - member.property.len() as u32,
                    target.span.end,
                ),
            ),
            _ => return None,
        };
        let receiver = self.infer_expr_type(object);
        let shape = self.array_or_tuple_type(&self.remove_null_undefined(&receiver), 0)?;
        let metadata = self.tuple_expression_metadata(object);
        let syntax_readonly = metadata.as_ref().is_some_and(|metadata| metadata.readonly);
        let members = match &shape {
            Type::Union(members) => members.as_ref(),
            _ => std::slice::from_ref(&shape),
        };
        let readonly = members
            .iter()
            .filter(|member| syntax_readonly || matches!(member, Type::Readonly(_)))
            .collect::<Vec<_>>();
        if readonly.is_empty() {
            return None;
        }
        let mut fixed = false;
        let mut index_signature = false;
        for (index, _) in self.element_index_keys(&index) {
            let key = numeric_key(&index);
            let length = matches!(&index, Type::StringLiteral(key) if key == "length");
            if length
                || members.iter().filter_map(elements).any(|items| {
                    key.as_ref().is_some_and(|key| {
                        key.parse::<usize>().is_ok_and(|index| {
                            index
                                < items
                                    .iter()
                                    .position(|item| matches!(item, Type::Rest(_)))
                                    .unwrap_or(items.len())
                        })
                    })
                })
            {
                fixed = true;
                if !deleting {
                    self.diagnostics.push(error_cannot_assign_readonly(
                        if length {
                            "length"
                        } else {
                            key.as_deref().unwrap()
                        },
                        key_span,
                    ));
                }
            } else if (key.is_some() || matches!(index, Type::Number | Type::Any))
                && !(key
                    .as_ref()
                    .is_some_and(|key| key.parse::<f64>().is_ok_and(|value| value < 0.0))
                    && !matches!(shape, Type::Union(_))
                    && members.iter().all(|member| {
                        elements(member).is_some_and(|items| {
                            !items.iter().any(|item| matches!(item, Type::Rest(_)))
                        })
                    }))
            {
                index_signature = true;
            }
        }
        if deleting && fixed && !index_signature {
            self.diagnostics
                .push(crate::diagnostics::error_delete_operand(2704, target.span));
        }
        if index_signature {
            let shown = metadata
                .map(|metadata| metadata.display)
                .unwrap_or_else(|| {
                    self.tuple_diagnostic_name(
                        &self
                            .declared_type_param_constraint(&receiver)
                            .unwrap_or(receiver),
                    )
                });
            self.diagnostics.push(Diagnostic {
                code: 2542,
                message: format!("Index signature in type '{shown}' only permits reading."),
                category: DiagnosticCategory::Error,
                file_name: None,
                span: Some(target.span),
                related: None,
            });
        }
        Some(fixed)
    }

    pub(super) fn check_tuple_element_access(
        &mut self,
        access: &ElemAccessExpr,
        receiver: &Type,
        index: &Type,
        writing: bool,
        unchecked: bool,
    ) -> Option<Type> {
        let metadata = self.tuple_expression_metadata(&access.object);
        let non_nullish = self.remove_null_undefined(receiver);
        let nullable = access.optional && non_nullish != *receiver;
        let receiver = if nullable { &non_nullish } else { receiver };
        let result = self.tuple_element_access(
            receiver,
            index,
            access.index.span,
            writing,
            unchecked,
            metadata.as_ref().map(|metadata| metadata.display.as_str()),
        )?;
        Some(if nullable && self.strict_null_checks && !writing {
            Type::flatten_union(vec![result, Type::Undefined])
        } else {
            result
        })
    }

    pub(super) fn array_or_tuple_type(&self, receiver: &Type, depth: usize) -> Option<Type> {
        if depth >= 16 {
            return None;
        }
        match receiver {
            Type::Array(_) => Some(receiver.clone()),
            Type::Tuple(items) => {
                let mut expanded = Vec::new();
                for item in items.iter() {
                    if let Type::Rest(inner) = item {
                        if let Some(Type::Tuple(rest)) = self.array_or_tuple_type(inner, depth + 1)
                        {
                            expanded.extend(rest.iter().cloned());
                            continue;
                        }
                        let resolved = self.simplify_type(inner);
                        if matches!(resolved, Type::Array(_)) {
                            expanded.push(Type::Rest(Arc::new(resolved)));
                            continue;
                        }
                    }
                    expanded.push(item.clone());
                }
                Some(Type::Tuple(expanded.into()))
            }
            Type::Readonly(inner) => self
                .array_or_tuple_type(inner, depth + 1)
                .map(|ty| Type::Readonly(Arc::new(ty))),
            Type::TypeReference(name, args)
                if matches!(name.as_str(), "Array" | "ReadonlyArray") && args.len() == 1 =>
            {
                let array = Type::Array(Arc::new(args[0].clone()));
                Some(if name == "ReadonlyArray" {
                    Type::Readonly(Arc::new(array))
                } else {
                    array
                })
            }
            Type::Union(members) => members
                .iter()
                .map(|member| match member {
                    Type::Array(_) => Some(member.clone()),
                    _ => self.array_or_tuple_type(member, depth + 1),
                })
                .collect::<Option<Vec<_>>>()
                .map(Type::flatten_union),
            _ => {
                if let Some(constraint) = self.declared_type_param_constraint(receiver) {
                    if constraint != *receiver {
                        return self.array_or_tuple_type(&constraint, depth + 1);
                    }
                }
                if let Type::TypeReference(name, args) = receiver {
                    if let Some(alias) = self.expand_type_alias_once(name, args) {
                        if alias != *receiver {
                            return self.array_or_tuple_type(&alias, depth + 1);
                        }
                    }
                }
                None
            }
        }
    }

    fn tuple_diagnostic_name(&self, ty: &Type) -> String {
        match ty {
            Type::Tuple(items) => format!(
                "[{}]",
                items
                    .iter()
                    .map(|item| match item {
                        Type::Optional(inner) if self.strict_null_checks => format!(
                            "({})?",
                            Type::flatten_union(vec![Type::clone(inner), Type::Undefined])
                                .display_string_single_line()
                        ),
                        other => self.tuple_diagnostic_name(other),
                    })
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Type::Readonly(inner) => format!("readonly {}", self.tuple_diagnostic_name(inner)),
            Type::Union(members) => members
                .iter()
                .map(|ty| self.tuple_diagnostic_name(ty))
                .collect::<Vec<_>>()
                .join(" | "),
            _ => ty.display_string_single_line(),
        }
    }

    pub(super) fn tuple_element_access(
        &mut self,
        receiver: &Type,
        index: &Type,
        span: Span,
        writing: bool,
        unchecked: bool,
        display: Option<&str>,
    ) -> Option<Type> {
        let shape = self.array_or_tuple_type(receiver, 0)?;
        let index = self
            .declared_type_param_constraint(index)
            .unwrap_or_else(|| index.clone());
        let value = match indexed_type(&shape, &index, self.strict_null_checks, unchecked, writing)
        {
            Some(value) => value,
            None if matches!(&index, Type::StringLiteral(key) if numeric_key(&index).is_none() && stdlib::stdlib_member_availability(&self.compiler_options).is_some_and(|lib| lib.declares_member("Array", key) == Some(false))) =>
            {
                if self.no_implicit_any {
                    self.diagnostics.push(Diagnostic { code: 7015, message: "Element implicitly has an 'any' type because index expression is not of type 'number'.".into(), category: DiagnosticCategory::Error, file_name: None, span: Some(span), related: None });
                }
                return Some(Type::Any);
            }
            None => return None,
        };
        let shown = if let Some(display) = display {
            display.to_owned()
        } else if self.declared_type_param_constraint(receiver).is_some()
            || matches!(receiver, Type::Tuple(_))
        {
            self.tuple_diagnostic_name(&shape)
        } else {
            self.tuple_diagnostic_name(receiver)
        };
        for (key, _) in self.element_index_keys(&index) {
            let Some(key) = numeric_key(&key) else {
                continue;
            };
            let missing = |ty: &Type| {
                elements(ty).is_some_and(|items| {
                    !items.iter().any(|item| matches!(item, Type::Rest(_)))
                        && !key.parse::<usize>().is_ok_and(|index| index < items.len())
                })
            };
            let diagnostic = if let Type::Union(members) = &shape {
                members.iter().all(missing).then(|| {
                    (
                        2339,
                        format!("Property '{key}' does not exist on type '{shown}'."),
                    )
                })
            } else if missing(&shape) {
                if key.parse::<f64>().is_ok_and(|number| number < 0.0) {
                    Some((
                        2514,
                        "A tuple type cannot be indexed with a negative value.".into(),
                    ))
                } else {
                    Some((
                        2493,
                        format!(
                            "Tuple type '{shown}' of length '{}' has no element at index '{key}'.",
                            elements(&shape)?.len()
                        ),
                    ))
                }
            } else {
                None
            };
            if let Some((code, message)) = diagnostic {
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
        Some(value)
    }

    pub(super) fn check_tuple_indexed_access_type(&mut self, object: &TypeNode, index: &TypeNode) {
        let object_type = self.resolve_type_node(object);
        let index_type = self.resolve_type_node(index);
        let display = self.tuple_annotation_display(object);
        self.tuple_element_access(
            &object_type,
            &index_type,
            index.span,
            false,
            false,
            display.as_deref(),
        );
    }
}
