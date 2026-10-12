use super::*;

impl TypeChecker {
    pub(super) fn check_for_of_assignment_pattern(
        &mut self,
        pattern: &Pat,
        element: &Type,
        flows: &mut Vec<(String, Type)>,
    ) {
        self.check_for_of_assignment_pattern_types(
            pattern,
            element,
            std::slice::from_ref(element),
            flows,
        );
    }

    fn check_for_of_assignment_pattern_types(
        &mut self,
        pattern: &Pat,
        element: &Type,
        sources: &[Type],
        flows: &mut Vec<(String, Type)>,
    ) {
        match &pattern.kind {
            PatKind::Ident(name) => self.check_for_of_assignment_expression_types(
                &Expr {
                    kind: ExprKind::Ident(name.clone()),
                    span: pattern.span,
                },
                element,
                sources,
                flows,
            ),
            PatKind::Array(elements) => {
                self.check_array_pattern_iterable(pattern, element);
                for (index, item) in elements.iter().enumerate() {
                    match item {
                        Some(ArrayPatElem::Pat(nested)) => {
                            let ty = self.extract_element_type(element, index);
                            let projected: Vec<_> = sources
                                .iter()
                                .map(|source| self.extract_element_type(source, index))
                                .collect();
                            self.check_for_of_assignment_pattern_types(
                                nested, &ty, &projected, flows,
                            );
                        }
                        Some(ArrayPatElem::Rest(nested)) => {
                            let ty = Self::array_rest_binding_type(element, index);
                            let projected: Vec<_> = sources
                                .iter()
                                .map(|source| Self::array_rest_binding_type(source, index))
                                .collect();
                            self.check_for_of_assignment_pattern_types(
                                nested, &ty, &projected, flows,
                            );
                        }
                        None => {}
                    }
                }
            }
            PatKind::Object(properties) => {
                self.check_binding_pattern_access(pattern, element);
                let consumed = self.object_pattern_consumed_names(properties);
                for property in properties {
                    match property {
                        ObjPatProp::KeyValue(key, nested) => {
                            self.check_property_name_expression(key);
                            let ty = self
                                .pattern_property_name(key)
                                .and_then(|name| self.extract_member_type_opt(element, &name))
                                .unwrap_or_else(|| {
                                    if matches!(element, Type::Any | Type::Error) {
                                        Type::Any
                                    } else {
                                        Type::Undefined
                                    }
                                });
                            let projected: Vec<_> = sources
                                .iter()
                                .map(|source| {
                                    self.pattern_property_name(key)
                                        .and_then(|name| {
                                            self.extract_member_type_opt(source, &name)
                                        })
                                        .unwrap_or(Type::Any)
                                })
                                .collect();
                            self.check_for_of_assignment_pattern_types(
                                nested, &ty, &projected, flows,
                            );
                        }
                        ObjPatProp::Shorthand(name, span)
                        | ObjPatProp::ShorthandAssign(name, _, span) => {
                            let span = Span::new(span.start, span.start + name.len() as u32);
                            let known = self.lookup_var(name).is_some();
                            if !known {
                                self.check_shorthand_name_exists(name, span);
                            }
                            let ty = self.extract_member_type_opt(element, name);
                            let projected: Vec<_> = sources
                                .iter()
                                .map(|source| {
                                    self.extract_member_type_opt(source, name)
                                        .unwrap_or(Type::Any)
                                })
                                .collect();
                            let mut checked_sources = projected.clone();
                            let ty = if let ObjPatProp::ShorthandAssign(_, default, _) = property {
                                let before = self.diagnostics.len();
                                self.check_expr(default);
                                if let ExprKind::Ident(default_name) = &default.kind {
                                    for diagnostic in &mut self.diagnostics[before..] {
                                        if matches!(diagnostic.code, 2304 | 2552)
                                            && diagnostic.span == Some(default.span)
                                        {
                                            diagnostic.code = 18004;
                                            diagnostic.message = format!("No value exists in scope for the shorthand property '{default_name}'. Either declare one or provide an initializer.");
                                            diagnostic.related = None;
                                        }
                                    }
                                }
                                checked_sources = self.for_of_default_sources(&projected, default);
                                self.binding_default_type(ty, default, false)
                            } else {
                                ty.unwrap_or(Type::Any)
                            };
                            if !known {
                                continue;
                            }
                            self.check_for_of_assignment_expression_types(
                                &Expr {
                                    kind: ExprKind::Ident(name.clone()),
                                    span,
                                },
                                &ty,
                                &checked_sources,
                                flows,
                            );
                        }
                        ObjPatProp::Rest(nested) => {
                            let ty = self.object_rest_binding_type(element, &consumed);
                            let projected: Vec<_> = sources
                                .iter()
                                .map(|source| self.object_rest_binding_type(source, &consumed))
                                .collect();
                            self.check_for_of_assignment_pattern_types(
                                nested, &ty, &projected, flows,
                            );
                        }
                    }
                }
            }
            PatKind::Assign(nested, default) => {
                self.check_expr(default);
                let ty = self.binding_default_type(Some(element.clone()), default, false);
                let sources = self.for_of_default_sources(sources, default);
                self.check_for_of_assignment_pattern_types(nested, &ty, &sources, flows);
            }
            PatKind::Rest(nested) => {
                self.check_for_of_assignment_pattern_types(nested, element, sources, flows)
            }
        }
    }

    fn for_of_default_sources(&self, sources: &[Type], default: &Expr) -> Vec<Type> {
        // Check the initializer first, then the incoming value without
        // undefined. Combining them would change both diagnostic wording and order.
        let mut result = vec![self.binding_default_type(None, default, false)];
        result.extend(sources.iter().map(|source| match source {
            Type::Optional(inner) => Type::clone(inner),
            Type::Undefined => Type::Never,
            ty => self.remove_undefined_only(ty),
        }));
        result
    }

    pub(super) fn check_for_of_assignment_expression(
        &mut self,
        expression: &Expr,
        element: &Type,
        flows: &mut Vec<(String, Type)>,
    ) {
        self.check_for_of_assignment_expression_types(
            expression,
            element,
            std::slice::from_ref(element),
            flows,
        );
    }

    fn check_for_of_assignment_expression_types(
        &mut self,
        expression: &Expr,
        element: &Type,
        sources: &[Type],
        flows: &mut Vec<(String, Type)>,
    ) {
        let reference = Self::skip_reference_wrappers(expression);
        let readonly = self.report_readonly_property_write(expression);
        let invalid_binding = self.report_invalid_assignment_target(expression);
        let saved_target = self.assignment_target_span;
        let saved_declared = self.assign_lhs_declared;
        self.assignment_target_span = Some(reference.span);
        self.assign_lhs_declared = true;
        let checked = self.check_expr(expression);
        self.assignment_target_span = saved_target;
        self.assign_lhs_declared = saved_declared;
        let mut target = checked.clone();
        let path = match &reference.kind {
            ExprKind::Ident(name) => {
                self.uninitialized_vars.remove(name.as_str());
                let Some(declared) = self.lookup_declared_var(name) else {
                    return;
                };
                target = declared;
                Some(name.to_string())
            }
            ExprKind::Member(member) => {
                target = self.setter_write_type(member).unwrap_or(target);
                Self::member_path(reference)
            }
            ExprKind::ElemAccess(access) => {
                if let ExprKind::StrLit(key) = &access.index.kind {
                    target = self
                        .setter_write_type_of(&access.object, key)
                        .unwrap_or(target);
                }
                self.known_element_path(reference)
                    .or_else(|| Self::element_path(reference))
            }
            _ => None,
        };
        self.record_expr_type(reference.span.start, &target);
        self.record_expression_type_span(reference.span, &target);
        if readonly || invalid_binding {
            return;
        }
        if Self::reference_is_optional_chain(reference) {
            self.diagnostics.push(Diagnostic {
                code: 2781,
                message: "The left-hand side of a 'for...of' statement may not be an optional property access.".into(),
                category: DiagnosticCategory::Error, file_name: None,
                span: Some(expression.span), related: None,
            });
            return;
        }
        if !matches!(target, Type::Any | Type::Error) {
            let mut invalid = false;
            for source in sources {
                if !self.is_assignable_to(source, &target) {
                    self.push_not_assignable(source, &target, reference.span);
                    invalid = true;
                }
            }
            if invalid {
                return;
            }
        }
        // An unresolved property is recovered as any. Do not retain a flow
        // entry that would bypass checking a later access to that property.
        if matches!(checked, Type::Any | Type::Error) {
            return;
        }
        let Some(path) = path else {
            return;
        };
        let before = self
            .lookup_narrowed(&path)
            .or_else(|| self.lookup_var(&path).cloned())
            .unwrap_or_else(|| checked.clone());
        let read_type = if matches!(reference.kind, ExprKind::Ident(_)) {
            &target
        } else {
            &checked
        };
        let assigned = self.assignment_reduced_type(read_type, element);
        if !flows.iter().any(|(existing, _)| *existing == path) {
            flows.push((path.clone(), before));
        }
        self.narrow_var(&path, assigned);
    }

    pub(super) fn assignment_reduced_type(&self, target: &Type, element: &Type) -> Type {
        if let Type::Union(members) = target {
            let fits = |member: &Type| match element {
                Type::Union(parts) => parts.iter().any(|part| self.is_assignable_to(part, member)),
                ty => self.is_assignable_to(ty, member),
            };
            let kept: Vec<_> = members
                .iter()
                .filter(|member| fits(member))
                .cloned()
                .collect();
            if kept.is_empty() {
                target.clone()
            } else {
                Type::flatten_union(kept)
            }
        } else {
            target.clone()
        }
    }
}
